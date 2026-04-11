use std::env;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use neovex_core::{
    DocumentId, FieldSchema, FieldType, Filter, FilterOp, IndexDefinition, OrderBy, OrderDirection,
    Query, SequenceNumber, TableName, TableSchema, TenantId,
};
use neovex_engine::{EmbeddedProviderKind, Service, SubscriptionRegistration, SubscriptionUpdate};
use neovex_storage::{
    sqlite_index_scan_composite_range_query_sql, sqlite_index_scan_prefix_query_sql,
};
use rusqlite::{Connection, params};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};

type BenchResult<T> = Result<T, Box<dyn std::error::Error>>;

const STEADY_STATE_WARMUP_ROUNDS: usize = 2;
const STEADY_STATE_MEASURE_ROUNDS: usize = 12;
const COLD_START_WARMUP_ROUNDS: usize = 1;
const COLD_START_MEASURE_ROUNDS: usize = 10;

const CRUD_DOCUMENTS: usize = 300;
const POINT_READ_DOCUMENTS: usize = 2_000;
const POINT_READ_BATCH_SIZE: usize = 200;
const INDEXED_QUERY_DOCUMENTS: usize = 4_000;
const INDEXED_QUERY_BATCH_SIZE: usize = 24;
const JOURNAL_DOCUMENTS: usize = 1_000;
const JOURNAL_STREAM_LIMIT: usize = 256;
const SUBSCRIPTION_FANOUT_COUNT: usize = 24;
const MIXED_LOAD_TENANTS: usize = 4;
const MIXED_LOAD_OPS_PER_TENANT: usize = 120;
const QUIESCE_TIMEOUT_SECS: u64 = 120;

static BENCH_DIR_COUNTER: AtomicU64 = AtomicU64::new(1);

#[tokio::main(flavor = "current_thread")]
async fn main() -> BenchResult<()> {
    let config = BenchmarkConfig::from_args()?;
    let report = run_suite(&config).await?;
    let markdown = render_markdown(&config, &report);
    if let Some(path) = config.markdown_output.as_deref() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, markdown.as_bytes())?;
    }
    print!("{markdown}");
    Ok(())
}

#[derive(Debug, Clone)]
struct BenchmarkConfig {
    markdown_output: Option<PathBuf>,
    workload_filter: Option<WorkloadKind>,
}

impl BenchmarkConfig {
    fn from_args() -> BenchResult<Self> {
        let mut markdown_output = None;
        let mut workload_filter = None;
        let mut args = env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--markdown" => {
                    let Some(path) = args.next() else {
                        return Err("expected a path after --markdown".into());
                    };
                    markdown_output = Some(PathBuf::from(path));
                }
                "--workload" => {
                    let Some(workload) = args.next() else {
                        return Err("expected a workload after --workload".into());
                    };
                    workload_filter = Some(WorkloadKind::parse(workload.as_str())?);
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ => {
                    return Err(format!("unknown argument: {arg}").into());
                }
            }
        }
        Ok(Self {
            markdown_output,
            workload_filter,
        })
    }
}

fn print_usage() {
    println!(
        "Usage: cargo bench -p neovex-engine --bench embedded-provider-benchmarks -- [--markdown <path>] [--workload <slug>]"
    );
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkloadKind {
    CrudThroughput,
    PointReadLatency,
    IndexedQueryLatency,
    CompositeIndexedQueryLatency,
    DurableJournalStreamLatency,
    DurableJournalBootstrapLatency,
    SubscriptionFanoutLatency,
    MixedMultiTenantLoad,
}

impl WorkloadKind {
    fn parse(value: &str) -> BenchResult<Self> {
        match value {
            "crud" => Ok(Self::CrudThroughput),
            "point-read" => Ok(Self::PointReadLatency),
            "indexed-query" => Ok(Self::IndexedQueryLatency),
            "composite-indexed-query" => Ok(Self::CompositeIndexedQueryLatency),
            "journal-stream" => Ok(Self::DurableJournalStreamLatency),
            "journal-bootstrap" => Ok(Self::DurableJournalBootstrapLatency),
            "subscription-fanout" => Ok(Self::SubscriptionFanoutLatency),
            "mixed-load" => Ok(Self::MixedMultiTenantLoad),
            _ => Err(format!("unknown workload slug: {value}").into()),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CrudThroughput => "document CRUD throughput",
            Self::PointReadLatency => "point read latency",
            Self::IndexedQueryLatency => "indexed query latency",
            Self::CompositeIndexedQueryLatency => "composite indexed query latency",
            Self::DurableJournalStreamLatency => "durable journal stream latency",
            Self::DurableJournalBootstrapLatency => "durable journal bootstrap latency",
            Self::SubscriptionFanoutLatency => "subscription fan-out latency",
            Self::MixedMultiTenantLoad => "concurrent multi-tenant mixed read/write load",
        }
    }

    fn notes(self) -> &'static str {
        match self {
            Self::CrudThroughput => {
                "async insert + update + delete through the Service mutation path"
            }
            Self::PointReadLatency => "batched async `get_document_async` over preseeded documents",
            Self::IndexedQueryLatency => {
                "single-field `status` equality query through planner-selected index path"
            }
            Self::CompositeIndexedQueryLatency => {
                "three-field composite index query with exact-prefix + range filters"
            }
            Self::DurableJournalStreamLatency => {
                "async `stream_durable_journal_async` from cursor 0 with a fixed page limit"
            }
            Self::DurableJournalBootstrapLatency => {
                "async `export_durable_journal_bootstrap_async` on a seeded tenant"
            }
            Self::SubscriptionFanoutLatency => {
                "time from one matching write to receipt of updates across all active subscriptions"
            }
            Self::MixedMultiTenantLoad => {
                "concurrent per-tenant mix of point reads, indexed queries, inserts, and updates"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchmarkLane {
    SteadyState,
    ColdStart,
}

impl BenchmarkLane {
    fn label(self) -> &'static str {
        match self {
            Self::SteadyState => "Steady-State",
            Self::ColdStart => "Cold-Start",
        }
    }

    fn notes(self) -> &'static str {
        match self {
            Self::SteadyState => {
                "reuses preseeded services and alternates backend order on every round so both backends are measured under the same warmed process"
            }
            Self::ColdStart => {
                "measures a fresh service/runtime plus the first representative workload execution; read-heavy lanes seed their dataset first and then time a reopen plus the first execution so startup cost is visible without letting seed writes dominate the result"
            }
        }
    }

    fn warmup_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_BENCH_STEADY_WARMUP_ROUNDS",
                STEADY_STATE_WARMUP_ROUNDS,
            ),
            Self::ColdStart => {
                read_round_override("NEOVEX_BENCH_COLD_WARMUP_ROUNDS", COLD_START_WARMUP_ROUNDS)
            }
        }
    }

    fn measure_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_BENCH_STEADY_MEASURE_ROUNDS",
                STEADY_STATE_MEASURE_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NEOVEX_BENCH_COLD_MEASURE_ROUNDS",
                COLD_START_MEASURE_ROUNDS,
            ),
        }
    }
}

#[derive(Debug, Default)]
struct BenchmarkReport {
    measurements: Vec<WorkloadMeasurement>,
    sqlite_query_plans: Vec<SqliteQueryPlan>,
}

impl BenchmarkReport {
    fn extend(&mut self, outcome: WorkloadOutcome) {
        self.measurements.extend(outcome.measurements);
        self.sqlite_query_plans.extend(outcome.sqlite_query_plans);
    }
}

#[derive(Debug, Default)]
struct WorkloadOutcome {
    measurements: Vec<WorkloadMeasurement>,
    sqlite_query_plans: Vec<SqliteQueryPlan>,
}

impl WorkloadOutcome {
    fn push_measurements(
        &mut self,
        workload: WorkloadKind,
        lane: BenchmarkLane,
        operations_per_sample: u64,
        samples: BackendSamples,
    ) {
        self.measurements
            .extend(samples.into_measurements(workload, lane, operations_per_sample));
    }
}

#[derive(Debug, Clone)]
struct WorkloadMeasurement {
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backend: EmbeddedProviderKind,
    operations_per_sample: u64,
    samples: Vec<Duration>,
}

impl WorkloadMeasurement {
    fn stats(&self) -> SampleStats {
        SampleStats::from_samples(&self.samples, self.operations_per_sample)
    }
}

#[derive(Debug, Clone)]
struct SqliteQueryPlan {
    workload: WorkloadKind,
    statement: String,
    detail_rows: Vec<String>,
}

#[derive(Debug, Clone)]
struct SampleStats {
    sample_count: usize,
    mean_per_operation: Duration,
    median_per_operation: Duration,
    p95_per_operation: Duration,
    stddev_per_operation: Duration,
    ci95_low_per_operation: Duration,
    ci95_high_per_operation: Duration,
    cv_percent: f64,
    median_operations_per_second: f64,
}

impl SampleStats {
    fn from_samples(samples: &[Duration], operations_per_sample: u64) -> Self {
        assert!(!samples.is_empty(), "benchmark samples should not be empty");
        let ops = operations_per_sample.max(1) as f64;
        let mut per_operation_nanos = samples
            .iter()
            .map(|sample| sample.as_secs_f64() * 1_000_000_000.0 / ops)
            .collect::<Vec<_>>();
        per_operation_nanos.sort_by(f64::total_cmp);

        let sample_count = per_operation_nanos.len();
        let mean_nanos = per_operation_nanos.iter().sum::<f64>() / sample_count as f64;
        let median_nanos = median_f64(&per_operation_nanos);
        let p95_index = ((sample_count - 1) * 95) / 100;
        let p95_nanos = per_operation_nanos[p95_index];
        let variance = if sample_count > 1 {
            per_operation_nanos
                .iter()
                .map(|sample| (sample - mean_nanos).powi(2))
                .sum::<f64>()
                / (sample_count - 1) as f64
        } else {
            0.0
        };
        let stddev_nanos = variance.sqrt();
        let sem = if sample_count > 1 {
            stddev_nanos / (sample_count as f64).sqrt()
        } else {
            0.0
        };
        let ci_radius = student_t_critical_95(sample_count) * sem;
        let mean_per_operation = duration_from_nanos_f64(mean_nanos);
        let median_per_operation = duration_from_nanos_f64(median_nanos);
        let p95_per_operation = duration_from_nanos_f64(p95_nanos);
        Self {
            sample_count,
            mean_per_operation,
            median_per_operation,
            p95_per_operation,
            stddev_per_operation: duration_from_nanos_f64(stddev_nanos),
            ci95_low_per_operation: duration_from_nanos_f64((mean_nanos - ci_radius).max(0.0)),
            ci95_high_per_operation: duration_from_nanos_f64(mean_nanos + ci_radius),
            cv_percent: if mean_nanos <= f64::EPSILON {
                0.0
            } else {
                (stddev_nanos / mean_nanos) * 100.0
            },
            median_operations_per_second: if median_per_operation.is_zero() {
                f64::INFINITY
            } else {
                median_per_operation.as_secs_f64().recip()
            },
        }
    }
}

#[derive(Clone)]
struct BackendPair<T> {
    redb: T,
    sqlite: T,
}

impl<T> BackendPair<T> {
    fn get(&self, backend: EmbeddedProviderKind) -> &T {
        match backend {
            EmbeddedProviderKind::Redb => &self.redb,
            EmbeddedProviderKind::Sqlite => &self.sqlite,
        }
    }
}

#[derive(Default)]
struct BackendSamples {
    redb: Vec<Duration>,
    sqlite: Vec<Duration>,
}

impl BackendSamples {
    fn push(&mut self, backend: EmbeddedProviderKind, sample: Duration) {
        match backend {
            EmbeddedProviderKind::Redb => self.redb.push(sample),
            EmbeddedProviderKind::Sqlite => self.sqlite.push(sample),
        }
    }

    fn into_measurements(
        self,
        workload: WorkloadKind,
        lane: BenchmarkLane,
        operations_per_sample: u64,
    ) -> Vec<WorkloadMeasurement> {
        vec![
            WorkloadMeasurement {
                workload,
                lane,
                backend: EmbeddedProviderKind::Redb,
                operations_per_sample,
                samples: self.redb,
            },
            WorkloadMeasurement {
                workload,
                lane,
                backend: EmbeddedProviderKind::Sqlite,
                operations_per_sample,
                samples: self.sqlite,
            },
        ]
    }
}

#[derive(Clone)]
struct CrudFixture {
    _bench_dir: Arc<BenchDir>,
    service: Arc<Service>,
    tenant_id: TenantId,
}

#[derive(Clone)]
struct PointReadFixture {
    bench_dir: Arc<BenchDir>,
    data_dir: PathBuf,
    service: Arc<Service>,
    tenant_id: TenantId,
    ids: Vec<DocumentId>,
}

#[derive(Clone)]
struct QueryFixture {
    bench_dir: Arc<BenchDir>,
    data_dir: PathBuf,
    service: Arc<Service>,
    tenant_id: TenantId,
    query: Query,
    tenant_path: PathBuf,
}

#[derive(Clone)]
struct JournalFixture {
    bench_dir: Arc<BenchDir>,
    data_dir: PathBuf,
    service: Arc<Service>,
    tenant_id: TenantId,
}

struct SubscriptionFixture {
    bench_dir: Arc<BenchDir>,
    data_dir: PathBuf,
    service: Arc<Service>,
    tenant_id: TenantId,
    registrations: Vec<SubscriptionRegistration>,
    receivers: Vec<mpsc::Receiver<SubscriptionUpdate>>,
}

#[derive(Clone)]
struct MixedLoadFixture {
    bench_dir: Arc<BenchDir>,
    data_dir: PathBuf,
    service: Arc<Service>,
    tenant_states: Vec<TenantState>,
}

#[derive(Clone)]
struct PointReadSeed {
    _bench_dir: Arc<BenchDir>,
    data_dir: PathBuf,
    tenant_id: TenantId,
    ids: Vec<DocumentId>,
}

#[derive(Clone)]
struct QuerySeed {
    _bench_dir: Arc<BenchDir>,
    data_dir: PathBuf,
    tenant_id: TenantId,
    query: Query,
}

#[derive(Clone)]
struct JournalSeed {
    _bench_dir: Arc<BenchDir>,
    data_dir: PathBuf,
    tenant_id: TenantId,
}

#[derive(Clone)]
struct MixedLoadSeed {
    _bench_dir: Arc<BenchDir>,
    data_dir: PathBuf,
    tenant_states: Vec<TenantState>,
}

async fn run_suite(_config: &BenchmarkConfig) -> BenchResult<BenchmarkReport> {
    let mut report = BenchmarkReport::default();
    if should_run_workload(_config, WorkloadKind::CrudThroughput) {
        report.extend(run_workload(WorkloadKind::CrudThroughput, benchmark_crud_throughput).await?);
    }
    if should_run_workload(_config, WorkloadKind::PointReadLatency) {
        report.extend(
            run_workload(WorkloadKind::PointReadLatency, benchmark_point_read_latency).await?,
        );
    }
    if should_run_workload(_config, WorkloadKind::IndexedQueryLatency) {
        report.extend(
            run_workload(
                WorkloadKind::IndexedQueryLatency,
                benchmark_indexed_query_latency,
            )
            .await?,
        );
    }
    if should_run_workload(_config, WorkloadKind::CompositeIndexedQueryLatency) {
        report.extend(
            run_workload(
                WorkloadKind::CompositeIndexedQueryLatency,
                benchmark_composite_indexed_query_latency,
            )
            .await?,
        );
    }
    if should_run_workload(_config, WorkloadKind::DurableJournalStreamLatency) {
        report.extend(
            run_workload(
                WorkloadKind::DurableJournalStreamLatency,
                benchmark_durable_journal_stream_latency,
            )
            .await?,
        );
    }
    if should_run_workload(_config, WorkloadKind::DurableJournalBootstrapLatency) {
        report.extend(
            run_workload(
                WorkloadKind::DurableJournalBootstrapLatency,
                benchmark_durable_journal_bootstrap_latency,
            )
            .await?,
        );
    }
    if should_run_workload(_config, WorkloadKind::SubscriptionFanoutLatency) {
        report.extend(
            run_workload(
                WorkloadKind::SubscriptionFanoutLatency,
                benchmark_subscription_fanout_latency,
            )
            .await?,
        );
    }
    if should_run_workload(_config, WorkloadKind::MixedMultiTenantLoad) {
        report.extend(
            run_workload(
                WorkloadKind::MixedMultiTenantLoad,
                benchmark_mixed_multi_tenant_load,
            )
            .await?,
        );
    }
    Ok(report)
}

fn should_run_workload(config: &BenchmarkConfig, workload: WorkloadKind) -> bool {
    config
        .workload_filter
        .is_none_or(|selected| selected == workload)
}

async fn run_workload<F, Fut>(workload: WorkloadKind, run: F) -> BenchResult<WorkloadOutcome>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = BenchResult<WorkloadOutcome>>,
{
    eprintln!("starting {}", workload.label());
    let started = Instant::now();
    let outcome = run().await?;
    eprintln!("finished {} in {:?}", workload.label(), started.elapsed());
    Ok(outcome)
}

async fn benchmark_crud_throughput() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_crud_fixture("crud-steady", "crud", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::CrudThroughput,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_crud_sample(&fixture.service, &fixture.tenant_id).await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "CRUD steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "CRUD steady-state sqlite teardown",
    )
    .await?;

    let cold_samples = measure_backend_pair_async(
        WorkloadKind::CrudThroughput,
        BenchmarkLane::ColdStart,
        |backend| async move {
            let bench_dir = Arc::new(BenchDir::new("crud-cold", backend)?);
            let data_dir = bench_dir.path().to_path_buf();
            let tenant_id = benchmark_tenant_id("crud")?;
            let started = Instant::now();
            let service = Arc::new(Service::new_with_embedded_provider(&data_dir, backend)?);
            service.create_tenant_async(tenant_id.clone()).await?;
            exercise_crud_sample(&service, &tenant_id).await?;
            let elapsed = started.elapsed();
            quiesce_service(&service, "CRUD cold-start sample teardown").await?;
            Ok(elapsed)
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(CRUD_DOCUMENTS * 3)?;
    outcome.push_measurements(
        WorkloadKind::CrudThroughput,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::CrudThroughput,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    Ok(outcome)
}

async fn benchmark_point_read_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_point_read_fixture("point-read-steady", "point-read", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::PointReadLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_point_read_sample(&fixture.service, &fixture.tenant_id, &fixture.ids)
                    .await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "point-read steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "point-read steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_point_read_seed(
            create_point_read_fixture("point-read-cold-seed", "point-read", backend).await?,
            "point-read cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::PointReadLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "point-read-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_point_read_sample(&reopened, &seed.tenant_id, &seed.ids).await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "point-read cold-start reopened teardown").await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(POINT_READ_BATCH_SIZE)?;
    outcome.push_measurements(
        WorkloadKind::PointReadLatency,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::PointReadLatency,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    Ok(outcome)
}

async fn benchmark_indexed_query_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_indexed_query_fixture("indexed-query-steady", "indexed-query", backend).await
    })
    .await?;
    let sqlite_statement =
        sqlite_index_scan_prefix_query_sql(&["status"], 1).expect("indexed query SQL should build");
    let sqlite_plan = SqliteQueryPlan {
        workload: WorkloadKind::IndexedQueryLatency,
        statement: sqlite_statement.clone(),
        detail_rows: capture_sqlite_query_plan(
            &steady_fixtures.sqlite.tenant_path,
            sqlite_statement.as_str(),
            params![tasks_table().as_str(), "open"],
        )?,
    };
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::IndexedQueryLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_query_sample(
                    &fixture.service,
                    &fixture.tenant_id,
                    &fixture.query,
                    INDEXED_QUERY_BATCH_SIZE,
                )
                .await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "indexed-query steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "indexed-query steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_query_seed(
            create_indexed_query_fixture("indexed-query-cold-seed", "indexed-query", backend)
                .await?,
            "indexed-query cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::IndexedQueryLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "indexed-query-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_query_sample(
                    &reopened,
                    &seed.tenant_id,
                    &seed.query,
                    INDEXED_QUERY_BATCH_SIZE,
                )
                .await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "indexed-query cold-start reopened teardown").await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(INDEXED_QUERY_BATCH_SIZE)?;
    outcome.push_measurements(
        WorkloadKind::IndexedQueryLatency,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::IndexedQueryLatency,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    outcome.sqlite_query_plans.push(sqlite_plan);
    Ok(outcome)
}

async fn benchmark_composite_indexed_query_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_composite_query_fixture("composite-query-steady", "composite-query", backend).await
    })
    .await?;
    let sqlite_statement = sqlite_index_scan_composite_range_query_sql(
        &["team", "status", "rank"],
        2,
        true,
        true,
        true,
        false,
    )
    .expect("composite indexed query SQL should build");
    let sqlite_plan = SqliteQueryPlan {
        workload: WorkloadKind::CompositeIndexedQueryLatency,
        statement: sqlite_statement.clone(),
        detail_rows: capture_sqlite_query_plan(
            &steady_fixtures.sqlite.tenant_path,
            sqlite_statement.as_str(),
            params![tasks_table().as_str(), "alpha", "open", 500_i64, 2_500_i64],
        )?,
    };
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::CompositeIndexedQueryLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_query_sample(
                    &fixture.service,
                    &fixture.tenant_id,
                    &fixture.query,
                    INDEXED_QUERY_BATCH_SIZE,
                )
                .await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "composite indexed-query steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "composite indexed-query steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_query_seed(
            create_composite_query_fixture("composite-query-cold-seed", "composite-query", backend)
                .await?,
            "composite indexed-query cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::CompositeIndexedQueryLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "composite-query-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_query_sample(
                    &reopened,
                    &seed.tenant_id,
                    &seed.query,
                    INDEXED_QUERY_BATCH_SIZE,
                )
                .await?;
                let elapsed = started.elapsed();
                quiesce_service(
                    &reopened,
                    "composite indexed-query cold-start reopened teardown",
                )
                .await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(INDEXED_QUERY_BATCH_SIZE)?;
    outcome.push_measurements(
        WorkloadKind::CompositeIndexedQueryLatency,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::CompositeIndexedQueryLatency,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    outcome.sqlite_query_plans.push(sqlite_plan);
    Ok(outcome)
}

async fn benchmark_durable_journal_stream_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_journal_fixture("journal-stream-steady", "journal-stream", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::DurableJournalStreamLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_journal_stream_sample(&fixture.service, &fixture.tenant_id).await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "journal-stream steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "journal-stream steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_journal_seed(
            create_journal_fixture("journal-stream-cold-seed", "journal-stream", backend).await?,
            "journal-stream cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::DurableJournalStreamLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "journal-stream-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_journal_stream_sample(&reopened, &seed.tenant_id).await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "journal-stream cold-start reopened teardown").await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    outcome.push_measurements(
        WorkloadKind::DurableJournalStreamLatency,
        BenchmarkLane::SteadyState,
        1,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::DurableJournalStreamLatency,
        BenchmarkLane::ColdStart,
        1,
        cold_samples,
    );
    Ok(outcome)
}

async fn benchmark_durable_journal_bootstrap_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_journal_fixture("journal-bootstrap-steady", "journal-bootstrap", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::DurableJournalBootstrapLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_journal_bootstrap_sample(&fixture.service, &fixture.tenant_id).await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "journal-bootstrap steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "journal-bootstrap steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_journal_seed(
            create_journal_fixture("journal-bootstrap-cold-seed", "journal-bootstrap", backend)
                .await?,
            "journal-bootstrap cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::DurableJournalBootstrapLatency,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir = clone_seeded_data_dir(
                    &seed.data_dir,
                    "journal-bootstrap-cold-sample",
                    backend,
                )?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_journal_bootstrap_sample(&reopened, &seed.tenant_id).await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "journal-bootstrap cold-start reopened teardown")
                    .await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    outcome.push_measurements(
        WorkloadKind::DurableJournalBootstrapLatency,
        BenchmarkLane::SteadyState,
        1,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::DurableJournalBootstrapLatency,
        BenchmarkLane::ColdStart,
        1,
        cold_samples,
    );
    Ok(outcome)
}

async fn benchmark_subscription_fanout_latency() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        Ok(Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-steady",
                "subscription-fanout",
                backend,
            )
            .await?,
        )))
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::SubscriptionFanoutLatency,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let mut fixture = fixture.lock().await;
                let service = fixture.service.clone();
                let tenant_id = fixture.tenant_id.clone();
                let started = Instant::now();
                exercise_subscription_fanout_sample(&service, &tenant_id, &mut fixture.receivers)
                    .await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    {
        let fixture = steady_fixtures.redb.lock().await;
        black_box(fixture.registrations.len());
        black_box(fixture.data_dir.as_os_str());
        black_box(fixture.bench_dir.path());
        quiesce_service(&fixture.service, "subscription steady-state redb teardown").await?;
    }
    {
        let fixture = steady_fixtures.sqlite.lock().await;
        black_box(fixture.registrations.len());
        black_box(fixture.data_dir.as_os_str());
        black_box(fixture.bench_dir.path());
        quiesce_service(
            &fixture.service,
            "subscription steady-state sqlite teardown",
        )
        .await?;
    }

    let cold_samples = measure_backend_pair_async(
        WorkloadKind::SubscriptionFanoutLatency,
        BenchmarkLane::ColdStart,
        |backend| async move {
            let bench_dir = Arc::new(BenchDir::new("subscription-fanout-cold", backend)?);
            let data_dir = bench_dir.path().to_path_buf();
            let tenant_id = benchmark_tenant_id("subscription-fanout")?;
            let started = Instant::now();
            let service = Arc::new(Service::new_with_embedded_provider(&data_dir, backend)?);
            service.create_tenant_async(tenant_id.clone()).await?;
            seed_subscription_fixture(&service, &tenant_id).await?;
            let (registrations, mut receivers) =
                register_subscription_receivers(&service, &tenant_id).await?;
            exercise_subscription_fanout_sample(&service, &tenant_id, &mut receivers).await?;
            let elapsed = started.elapsed();
            drop(registrations);
            quiesce_service(&service, "subscription cold-start teardown").await?;
            drop(bench_dir);
            Ok(elapsed)
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(SUBSCRIPTION_FANOUT_COUNT)?;
    outcome.push_measurements(
        WorkloadKind::SubscriptionFanoutLatency,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::SubscriptionFanoutLatency,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    Ok(outcome)
}

async fn benchmark_mixed_multi_tenant_load() -> BenchResult<WorkloadOutcome> {
    let steady_fixtures = build_backend_pair_async(|backend| async move {
        create_mixed_load_fixture("mixed-load-steady", backend).await
    })
    .await?;
    let steady_samples = measure_backend_pair_async(
        WorkloadKind::MixedMultiTenantLoad,
        BenchmarkLane::SteadyState,
        |backend| {
            let fixture = steady_fixtures.get(backend).clone();
            async move {
                let started = Instant::now();
                exercise_mixed_load_sample(&fixture.service, &fixture.tenant_states).await?;
                Ok(started.elapsed())
            }
        },
    )
    .await?;
    quiesce_service(
        &steady_fixtures.redb.service,
        "mixed-load steady-state redb teardown",
    )
    .await?;
    quiesce_service(
        &steady_fixtures.sqlite.service,
        "mixed-load steady-state sqlite teardown",
    )
    .await?;

    let cold_seeds = build_backend_pair_async(|backend| async move {
        freeze_mixed_load_seed(
            create_mixed_load_fixture("mixed-load-cold-seed", backend).await?,
            "mixed-load cold-start seed freeze",
        )
        .await
    })
    .await?;
    let cold_samples = measure_backend_pair_async(
        WorkloadKind::MixedMultiTenantLoad,
        BenchmarkLane::ColdStart,
        |backend| {
            let seed = cold_seeds.get(backend).clone();
            async move {
                let sample_dir =
                    clone_seeded_data_dir(&seed.data_dir, "mixed-load-cold-sample", backend)?;
                let started = Instant::now();
                let reopened = Arc::new(Service::new_with_embedded_provider(
                    sample_dir.path(),
                    backend,
                )?);
                exercise_mixed_load_sample(&reopened, &seed.tenant_states).await?;
                let elapsed = started.elapsed();
                quiesce_service(&reopened, "mixed-load cold-start reopened teardown").await?;
                drop(sample_dir);
                Ok(elapsed)
            }
        },
    )
    .await?;

    let mut outcome = WorkloadOutcome::default();
    let operations_per_sample = u64::try_from(MIXED_LOAD_TENANTS * MIXED_LOAD_OPS_PER_TENANT)?;
    outcome.push_measurements(
        WorkloadKind::MixedMultiTenantLoad,
        BenchmarkLane::SteadyState,
        operations_per_sample,
        steady_samples,
    );
    outcome.push_measurements(
        WorkloadKind::MixedMultiTenantLoad,
        BenchmarkLane::ColdStart,
        operations_per_sample,
        cold_samples,
    );
    Ok(outcome)
}

async fn create_crud_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<CrudFixture> {
    let (bench_dir, _data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    Ok(CrudFixture {
        _bench_dir: bench_dir,
        service,
        tenant_id,
    })
}

async fn create_point_read_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<PointReadFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    let mut ids = Vec::with_capacity(POINT_READ_DOCUMENTS);
    for rank in 0..POINT_READ_DOCUMENTS {
        ids.push(
            service
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([
                        (
                            "status".to_string(),
                            json!(if rank % 2 == 0 { "open" } else { "done" }),
                        ),
                        ("rank".to_string(), json!(rank)),
                        ("title".to_string(), json!(format!("task-{rank:05}"))),
                    ]),
                )
                .await?,
        );
    }
    Ok(PointReadFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
        ids,
    })
}

async fn create_indexed_query_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<QueryFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    service
        .set_table_schema_async(tenant_id.clone(), single_field_schema())
        .await?;
    for rank in 0..INDEXED_QUERY_DOCUMENTS {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    (
                        "status".to_string(),
                        json!(if rank % 5 == 0 { "open" } else { "done" }),
                    ),
                    ("rank".to_string(), json!(rank)),
                    ("title".to_string(), json!(format!("task-{rank:05}"))),
                ]),
            )
            .await?;
    }
    Ok(QueryFixture {
        tenant_path: tenant_store_path(&data_dir, backend, &tenant_id),
        bench_dir,
        data_dir,
        service,
        tenant_id,
        query: Query {
            table: tasks_table(),
            filters: vec![filter("status", FilterOp::Eq, json!("open"))],
            order: None,
            limit: None,
        },
    })
}

async fn create_composite_query_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<QueryFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    service
        .set_table_schema_async(tenant_id.clone(), composite_schema())
        .await?;
    for rank in 0..INDEXED_QUERY_DOCUMENTS {
        let team = if rank % 2 == 0 { "alpha" } else { "beta" };
        let status = if rank % 3 == 0 { "open" } else { "done" };
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("team".to_string(), json!(team)),
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                    ("title".to_string(), json!(format!("task-{rank:05}"))),
                ]),
            )
            .await?;
    }
    Ok(QueryFixture {
        tenant_path: tenant_store_path(&data_dir, backend, &tenant_id),
        bench_dir,
        data_dir,
        service,
        tenant_id,
        query: Query {
            table: tasks_table(),
            filters: vec![
                filter("team", FilterOp::Eq, json!("alpha")),
                filter("status", FilterOp::Eq, json!("open")),
                filter("rank", FilterOp::Gte, json!(500)),
                filter("rank", FilterOp::Lt, json!(2_500)),
            ],
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
    })
}

async fn create_journal_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<JournalFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    for rank in 0..JOURNAL_DOCUMENTS {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("open")),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .await?;
    }
    Ok(JournalFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
    })
}

async fn create_subscription_fixture(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<SubscriptionFixture> {
    let (bench_dir, data_dir, service, tenant_id) =
        create_tenant_service(label, tenant_label, backend).await?;
    seed_subscription_fixture(&service, &tenant_id).await?;
    let (registrations, receivers) = register_subscription_receivers(&service, &tenant_id).await?;
    Ok(SubscriptionFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
        registrations,
        receivers,
    })
}

async fn create_mixed_load_fixture(
    label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<MixedLoadFixture> {
    let bench_dir = Arc::new(BenchDir::new(label, backend)?);
    let data_dir = bench_dir.path().to_path_buf();
    let service = Arc::new(Service::new_with_embedded_provider(&data_dir, backend)?);
    let mut tenant_states = Vec::with_capacity(MIXED_LOAD_TENANTS);
    for tenant_index in 0..MIXED_LOAD_TENANTS {
        let tenant_id = TenantId::new(format!("tenant-{tenant_index}"))?;
        service.create_tenant_async(tenant_id.clone()).await?;
        service
            .set_table_schema_async(tenant_id.clone(), single_field_schema())
            .await?;
        let mut ids = Vec::with_capacity(MIXED_LOAD_OPS_PER_TENANT);
        for rank in 0..MIXED_LOAD_OPS_PER_TENANT {
            ids.push(
                service
                    .insert_document_async(
                        tenant_id.clone(),
                        tasks_table(),
                        serde_json::Map::from_iter([
                            (
                                "status".to_string(),
                                json!(if rank % 2 == 0 { "open" } else { "done" }),
                            ),
                            ("rank".to_string(), json!(rank)),
                            (
                                "title".to_string(),
                                json!(format!("tenant-{tenant_index}-task-{rank}")),
                            ),
                        ]),
                    )
                    .await?,
            );
        }
        tenant_states.push(TenantState { tenant_id, ids });
    }
    Ok(MixedLoadFixture {
        bench_dir,
        data_dir,
        service,
        tenant_states,
    })
}

async fn create_tenant_service(
    label: &str,
    tenant_label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<(Arc<BenchDir>, PathBuf, Arc<Service>, TenantId)> {
    let bench_dir = Arc::new(BenchDir::new(label, backend)?);
    let data_dir = bench_dir.path().to_path_buf();
    let service = Arc::new(Service::new_with_embedded_provider(&data_dir, backend)?);
    let tenant_id = benchmark_tenant_id(tenant_label)?;
    service.create_tenant_async(tenant_id.clone()).await?;
    Ok((bench_dir, data_dir, service, tenant_id))
}

async fn freeze_point_read_seed(
    fixture: PointReadFixture,
    context: &str,
) -> BenchResult<PointReadSeed> {
    let PointReadFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
        ids,
    } = fixture;
    quiesce_service(&service, context).await?;
    drop(service);
    Ok(PointReadSeed {
        _bench_dir: bench_dir,
        data_dir,
        tenant_id,
        ids,
    })
}

async fn freeze_query_seed(fixture: QueryFixture, context: &str) -> BenchResult<QuerySeed> {
    let QueryFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
        query,
        ..
    } = fixture;
    quiesce_service(&service, context).await?;
    drop(service);
    Ok(QuerySeed {
        _bench_dir: bench_dir,
        data_dir,
        tenant_id,
        query,
    })
}

async fn freeze_journal_seed(fixture: JournalFixture, context: &str) -> BenchResult<JournalSeed> {
    let JournalFixture {
        bench_dir,
        data_dir,
        service,
        tenant_id,
    } = fixture;
    quiesce_service(&service, context).await?;
    drop(service);
    Ok(JournalSeed {
        _bench_dir: bench_dir,
        data_dir,
        tenant_id,
    })
}

async fn freeze_mixed_load_seed(
    fixture: MixedLoadFixture,
    context: &str,
) -> BenchResult<MixedLoadSeed> {
    let MixedLoadFixture {
        bench_dir,
        data_dir,
        service,
        tenant_states,
    } = fixture;
    quiesce_service(&service, context).await?;
    drop(service);
    Ok(MixedLoadSeed {
        _bench_dir: bench_dir,
        data_dir,
        tenant_states,
    })
}

async fn exercise_crud_sample(service: &Arc<Service>, tenant_id: &TenantId) -> BenchResult<()> {
    let mut ids = Vec::with_capacity(CRUD_DOCUMENTS);
    for rank in 0..CRUD_DOCUMENTS {
        ids.push(
            service
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([
                        ("status".to_string(), json!("open")),
                        ("rank".to_string(), json!(rank)),
                        ("title".to_string(), json!(format!("task-{rank:05}"))),
                    ]),
                )
                .await?,
        );
    }
    for (rank, id) in ids.iter().copied().enumerate() {
        let _ = service
            .update_document_async(
                tenant_id.clone(),
                tasks_table(),
                id,
                serde_json::Map::from_iter([("rank".to_string(), json!(rank + CRUD_DOCUMENTS))]),
            )
            .await?;
    }
    for id in ids {
        service
            .delete_document_async(tenant_id.clone(), tasks_table(), id)
            .await?;
    }
    Ok(())
}

async fn exercise_point_read_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    ids: &[DocumentId],
) -> BenchResult<()> {
    for step in 0..POINT_READ_BATCH_SIZE {
        let id = ids[(step * 17) % ids.len()];
        let document = service
            .get_document_async(tenant_id.clone(), tasks_table(), id)
            .await?;
        black_box(document);
    }
    Ok(())
}

async fn exercise_query_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    query: &Query,
    batch_size: usize,
) -> BenchResult<()> {
    for _ in 0..batch_size {
        let documents = service
            .query_documents_async(tenant_id.clone(), query.clone())
            .await?;
        black_box(documents);
    }
    Ok(())
}

async fn exercise_journal_stream_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    let page = service
        .stream_durable_journal_async(tenant_id.clone(), SequenceNumber(0), JOURNAL_STREAM_LIMIT)
        .await?;
    black_box(page);
    Ok(())
}

async fn exercise_journal_bootstrap_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    let bootstrap = service
        .export_durable_journal_bootstrap_async(tenant_id.clone())
        .await?;
    black_box(bootstrap);
    Ok(())
}

async fn seed_subscription_fixture(
    service: &Arc<Service>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("seed"))]),
        )
        .await?;
    Ok(())
}

async fn register_subscription_receivers(
    service: &Arc<Service>,
    tenant_id: &TenantId,
) -> BenchResult<(
    Vec<SubscriptionRegistration>,
    Vec<mpsc::Receiver<SubscriptionUpdate>>,
)> {
    let query = Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: None,
        limit: None,
    };
    let mut registrations = Vec::with_capacity(SUBSCRIPTION_FANOUT_COUNT);
    let mut receivers = Vec::with_capacity(SUBSCRIPTION_FANOUT_COUNT);
    for index in 0..SUBSCRIPTION_FANOUT_COUNT {
        let (sender, mut receiver) = mpsc::channel(8);
        let registration = service
            .subscribe_async(
                tenant_id.clone(),
                query.clone(),
                format!("fanout-{index}"),
                sender,
            )
            .await?;
        let initial = receiver
            .recv()
            .await
            .ok_or("subscription bootstrap should arrive")?;
        black_box(initial);
        registrations.push(registration);
        receivers.push(receiver);
    }
    Ok((registrations, receivers))
}

async fn exercise_subscription_fanout_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    receivers: &mut [mpsc::Receiver<SubscriptionUpdate>],
) -> BenchResult<()> {
    let _ = service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([(
                "title".to_string(),
                json!(format!(
                    "fanout-{}",
                    BENCH_DIR_COUNTER.fetch_add(1, Ordering::SeqCst)
                )),
            )]),
        )
        .await?;
    for receiver in receivers {
        let update = receiver
            .recv()
            .await
            .ok_or("subscription update should arrive")?;
        match update {
            SubscriptionUpdate::Result { .. } => {}
            SubscriptionUpdate::Error { message, .. } => {
                return Err(format!("unexpected subscription error: {message}").into());
            }
        }
    }
    Ok(())
}

async fn exercise_mixed_load_sample(
    service: &Arc<Service>,
    tenant_states: &[TenantState],
) -> BenchResult<()> {
    let mut handles = Vec::with_capacity(tenant_states.len());
    for (task_index, state) in tenant_states.iter().cloned().enumerate() {
        let service = service.clone();
        handles.push(tokio::spawn(async move {
            let query = Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                order: None,
                limit: Some(25),
            };
            for step in 0..MIXED_LOAD_OPS_PER_TENANT {
                let id = state.ids[step % state.ids.len()];
                match step % 4 {
                    0 => {
                        let document = service
                            .get_document_async(state.tenant_id.clone(), tasks_table(), id)
                            .await?;
                        black_box(document);
                    }
                    1 => {
                        let documents = service
                            .query_documents_async(state.tenant_id.clone(), query.clone())
                            .await?;
                        black_box(documents);
                    }
                    2 => {
                        let _ = service
                            .insert_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                serde_json::Map::from_iter([
                                    ("status".to_string(), json!("open")),
                                    (
                                        "rank".to_string(),
                                        json!(task_index * MIXED_LOAD_OPS_PER_TENANT + step),
                                    ),
                                    (
                                        "title".to_string(),
                                        json!(format!("tenant-{task_index}-insert-{step}")),
                                    ),
                                ]),
                            )
                            .await?;
                    }
                    _ => {
                        let _ = service
                            .update_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                id,
                                serde_json::Map::from_iter([(
                                    "rank".to_string(),
                                    json!(step + MIXED_LOAD_OPS_PER_TENANT),
                                )]),
                            )
                            .await?;
                    }
                }
            }
            Ok::<(), neovex_core::Error>(())
        }));
    }
    for handle in handles {
        handle.await??;
    }
    Ok(())
}

async fn build_backend_pair_async<T, F, Fut>(mut build: F) -> BenchResult<BackendPair<T>>
where
    F: FnMut(EmbeddedProviderKind) -> Fut,
    Fut: std::future::Future<Output = BenchResult<T>>,
{
    Ok(BackendPair {
        redb: build(EmbeddedProviderKind::Redb).await?,
        sqlite: build(EmbeddedProviderKind::Sqlite).await?,
    })
}

async fn measure_backend_pair_async<F, Fut>(
    workload: WorkloadKind,
    lane: BenchmarkLane,
    mut run_sample: F,
) -> BenchResult<BackendSamples>
where
    F: FnMut(EmbeddedProviderKind) -> Fut,
    Fut: std::future::Future<Output = BenchResult<Duration>>,
{
    eprintln!("  starting {} lane", lane.label().to_lowercase());
    let started = Instant::now();
    let mut samples = BackendSamples::default();
    let total_rounds = lane.warmup_rounds() + lane.measure_rounds();
    for round in 0..total_rounds {
        for backend in provider_order_for_round(round) {
            let sample = run_sample(backend).await?;
            if round >= lane.warmup_rounds() {
                samples.push(backend, sample);
            } else {
                black_box(sample);
            }
        }
    }
    eprintln!(
        "  finished {} lane for {} in {:?}",
        lane.label().to_lowercase(),
        workload.label(),
        started.elapsed()
    );
    Ok(samples)
}

async fn quiesce_service(service: &Arc<Service>, context: &str) -> BenchResult<()> {
    tokio::time::timeout(Duration::from_secs(QUIESCE_TIMEOUT_SECS), service.quiesce())
        .await
        .map_err(|_| format!("service quiesce timed out during {context}").into())
}

fn render_markdown(config: &BenchmarkConfig, report: &BenchmarkReport) -> String {
    let workloads = [
        WorkloadKind::CrudThroughput,
        WorkloadKind::PointReadLatency,
        WorkloadKind::IndexedQueryLatency,
        WorkloadKind::CompositeIndexedQueryLatency,
        WorkloadKind::DurableJournalStreamLatency,
        WorkloadKind::DurableJournalBootstrapLatency,
        WorkloadKind::SubscriptionFanoutLatency,
        WorkloadKind::MixedMultiTenantLoad,
    ]
    .into_iter()
    .filter(|workload| {
        report
            .measurements
            .iter()
            .any(|measurement| measurement.workload == *workload)
    })
    .collect::<Vec<_>>();
    let mut markdown = String::new();
    markdown.push_str("# SQLite Storage Backend Benchmark Report\n\n");
    markdown.push_str("Generated with:\n\n");
    markdown.push_str("```bash\n");
    markdown.push_str(
        "make bench-embedded-providers REPORT=docs/research/sqlite-storage-benchmark-report.md\n",
    );
    markdown.push_str("```\n\n");
    markdown.push_str("## Methodology\n\n");
    markdown.push_str(&format!(
        "- backend order alternates every round inside each workload and lane: round 1 runs `redb -> sqlite`, round 2 runs `sqlite -> redb`, then repeats\n- steady-state warmup rounds: `{STEADY_STATE_WARMUP_ROUNDS}`; steady-state measured rounds: `{STEADY_STATE_MEASURE_ROUNDS}`\n- cold-start warmup rounds: `{COLD_START_WARMUP_ROUNDS}`; cold-start measured rounds: `{COLD_START_MEASURE_ROUNDS}`\n- cold-start read/query/journal lanes seed one canonical on-disk dataset per backend, clone that dataset before each sample, and then time only the fresh open plus first representative execution\n- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency\n- subscription cold-start includes fresh subscription registration/bootstrap because subscriptions are in-memory and do not survive reopen\n"
    ));
    markdown.push('\n');
    markdown.push_str("## Configuration\n\n");
    markdown.push_str(&format!(
        "- CRUD documents per sample: `{CRUD_DOCUMENTS}`\n- point reads per sample: `{POINT_READ_BATCH_SIZE}` over `{POINT_READ_DOCUMENTS}` seeded documents\n- indexed queries per sample: `{INDEXED_QUERY_BATCH_SIZE}` over `{INDEXED_QUERY_DOCUMENTS}` seeded documents\n- journal dataset size: `{JOURNAL_DOCUMENTS}` writes with stream page limit `{JOURNAL_STREAM_LIMIT}`\n- subscription fan-out count: `{SUBSCRIPTION_FANOUT_COUNT}`\n- mixed-load tenants: `{MIXED_LOAD_TENANTS}` with `{MIXED_LOAD_OPS_PER_TENANT}` ops per tenant per sample\n",
    ));
    if let Some(path) = &config.markdown_output {
        markdown.push_str(&format!("- report path: `{}`\n", path.display()));
    }
    if let Some(workload) = config.workload_filter {
        markdown.push_str(&format!("- workload filter: `{}`\n", workload.label()));
    }
    markdown.push('\n');

    if !workloads.is_empty() {
        let mut overall_sqlite_wins = 0;
        let mut overall_redb_wins = 0;
        markdown.push_str("## Winner Scorecard\n\n");
        markdown.push_str(
            "Winner is determined by higher median ops/s, which is equivalent here to lower\nmedian per-op latency.\n\n",
        );

        for lane in [BenchmarkLane::SteadyState, BenchmarkLane::ColdStart] {
            let mut sqlite_wins = 0;
            let mut redb_wins = 0;
            markdown.push_str(&format!("### {} summary\n\n", lane.label()));
            markdown.push_str("| Workload | SQLite vs redb | Winner |\n");
            markdown.push_str("| --- | ---: | --- |\n");
            for workload in &workloads {
                let redb = measurement_for(report, *workload, lane, EmbeddedProviderKind::Redb);
                let sqlite = measurement_for(report, *workload, lane, EmbeddedProviderKind::Sqlite);
                let ratio = sqlite.stats().median_operations_per_second
                    / redb.stats().median_operations_per_second;
                let winner = if ratio > 1.0 {
                    sqlite_wins += 1;
                    overall_sqlite_wins += 1;
                    "sqlite"
                } else if ratio < 1.0 {
                    redb_wins += 1;
                    overall_redb_wins += 1;
                    "redb"
                } else {
                    "tie"
                };
                markdown.push_str(&format!(
                    "| {} | {:.2}x | {} |\n",
                    workload.label(),
                    ratio,
                    winner
                ));
            }
            markdown.push_str(&format!(
                "| Total lanes won | sqlite {}, redb {} | {} |\n\n",
                sqlite_wins,
                redb_wins,
                overall_winner_label(sqlite_wins, redb_wins)
            ));
        }

        markdown.push_str("### Overall total\n\n");
        markdown.push_str("| Scope | SQLite lanes won | redb lanes won | Overall winner |\n");
        markdown.push_str("| --- | ---: | ---: | --- |\n");
        markdown.push_str(&format!(
            "| All measured lanes | {} | {} | {} |\n\n",
            overall_sqlite_wins,
            overall_redb_wins,
            overall_winner_label(overall_sqlite_wins, overall_redb_wins)
        ));
    }

    for workload in workloads {
        markdown.push_str(&format!("## {}\n\n", workload.label()));
        markdown.push_str(&format!("{}\n\n", workload.notes()));
        for lane in [BenchmarkLane::SteadyState, BenchmarkLane::ColdStart] {
            let redb = measurement_for(report, workload, lane, EmbeddedProviderKind::Redb);
            let sqlite = measurement_for(report, workload, lane, EmbeddedProviderKind::Sqlite);
            let redb_stats = redb.stats();
            let sqlite_stats = sqlite.stats();
            markdown.push_str(&format!("### {} lane\n\n", lane.label()));
            markdown.push_str(&format!("{}\n\n", lane.notes()));
            markdown.push_str(
                "| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |\n",
            );
            markdown.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |\n");
            markdown.push_str(&format!(
                "| redb | {} | {} | {} | {} | {} | {:.2}% | {} | {:.2} |\n",
                redb_stats.sample_count,
                format_duration(redb_stats.median_per_operation),
                format_duration(redb_stats.p95_per_operation),
                format_duration(redb_stats.mean_per_operation),
                format_duration(redb_stats.stddev_per_operation),
                redb_stats.cv_percent,
                format_confidence_interval(
                    redb_stats.ci95_low_per_operation,
                    redb_stats.ci95_high_per_operation,
                ),
                redb_stats.median_operations_per_second,
            ));
            markdown.push_str(&format!(
                "| sqlite | {} | {} | {} | {} | {} | {:.2}% | {} | {:.2} |\n\n",
                sqlite_stats.sample_count,
                format_duration(sqlite_stats.median_per_operation),
                format_duration(sqlite_stats.p95_per_operation),
                format_duration(sqlite_stats.mean_per_operation),
                format_duration(sqlite_stats.stddev_per_operation),
                sqlite_stats.cv_percent,
                format_confidence_interval(
                    sqlite_stats.ci95_low_per_operation,
                    sqlite_stats.ci95_high_per_operation,
                ),
                sqlite_stats.median_operations_per_second,
            ));
            markdown.push_str(&format!(
                "SQLite vs redb on the {} lane: `{:.2}x` median ops/s, `{:.2}x` median per-op latency\n\n",
                lane.label().to_lowercase(),
                sqlite_stats.median_operations_per_second / redb_stats.median_operations_per_second,
                duration_ratio(
                    redb_stats.median_per_operation,
                    sqlite_stats.median_per_operation,
                ),
            ));
        }

        if let Some(plan) = report
            .sqlite_query_plans
            .iter()
            .find(|plan| plan.workload == workload)
        {
            markdown.push_str("### SQLite EXPLAIN QUERY PLAN\n\n");
            markdown.push_str(
                "Captured against the seeded SQLite benchmark dataset for this workload.\n\n",
            );
            markdown.push_str("```sql\n");
            markdown.push_str(plan.statement.trim());
            markdown.push_str("\n```\n\n");
            markdown.push_str("```text\n");
            for detail in &plan.detail_rows {
                markdown.push_str(detail);
                markdown.push('\n');
            }
            markdown.push_str("```\n\n");
        }
    }

    markdown
}

fn measurement_for(
    report: &BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backend: EmbeddedProviderKind,
) -> &WorkloadMeasurement {
    report
        .measurements
        .iter()
        .find(|measurement| {
            measurement.workload == workload
                && measurement.lane == lane
                && measurement.backend == backend
        })
        .expect("benchmark measurement should exist")
}

fn overall_winner_label(sqlite_wins: usize, redb_wins: usize) -> &'static str {
    use std::cmp::Ordering::*;

    match sqlite_wins.cmp(&redb_wins) {
        Greater => "sqlite",
        Less => "redb",
        Equal => "tie",
    }
}

fn capture_sqlite_query_plan<P>(
    sqlite_path: &Path,
    statement: &str,
    params: P,
) -> BenchResult<Vec<String>>
where
    P: rusqlite::Params,
{
    let conn = Connection::open(sqlite_path)?;
    let explain = format!("EXPLAIN QUERY PLAN {statement}");
    let mut stmt = conn.prepare(explain.as_str())?;
    let mut rows = stmt.query(params)?;
    let mut detail_rows = Vec::new();
    while let Some(row) = rows.next()? {
        let select_id = row.get::<_, i64>(0)?;
        let parent_id = row.get::<_, i64>(1)?;
        let order = row.get::<_, i64>(2)?;
        let detail = row.get::<_, String>(3)?;
        detail_rows.push(format!("{select_id} | {parent_id} | {order} | {detail}"));
    }
    Ok(detail_rows)
}

#[derive(Debug)]
struct BenchDir {
    path: PathBuf,
}

impl BenchDir {
    fn new(label: &str, backend: EmbeddedProviderKind) -> BenchResult<Self> {
        let counter = BENCH_DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = env::temp_dir().join(format!(
            "neovex-storage-bench-{}-{}-{}-{}",
            label,
            provider_label(backend),
            std::process::id(),
            counter
        ));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Debug, Clone)]
struct TenantState {
    tenant_id: TenantId,
    ids: Vec<DocumentId>,
}

fn provider_label(backend: EmbeddedProviderKind) -> &'static str {
    match backend {
        EmbeddedProviderKind::Redb => "redb",
        EmbeddedProviderKind::Sqlite => "sqlite",
    }
}

fn provider_order_for_round(round: usize) -> [EmbeddedProviderKind; 2] {
    if round.is_multiple_of(2) {
        [EmbeddedProviderKind::Redb, EmbeddedProviderKind::Sqlite]
    } else {
        [EmbeddedProviderKind::Sqlite, EmbeddedProviderKind::Redb]
    }
}

fn benchmark_tenant_id(label: &str) -> BenchResult<TenantId> {
    Ok(TenantId::new(format!("bench-{label}"))?)
}

fn read_round_override(env_key: &str, default: usize) -> usize {
    env::var(env_key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn clone_seeded_data_dir(
    source: &Path,
    label: &str,
    backend: EmbeddedProviderKind,
) -> BenchResult<BenchDir> {
    let cloned = BenchDir::new(label, backend)?;
    copy_dir_all(source, cloned.path())?;
    Ok(cloned)
}

fn copy_dir_all(source: &Path, destination: &Path) -> BenchResult<()> {
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            fs::create_dir_all(&destination_path)?;
            copy_dir_all(&source_path, &destination_path)?;
        } else {
            fs::copy(&source_path, &destination_path)?;
        }
    }
    Ok(())
}

fn tenant_store_path(
    data_dir: &Path,
    backend: EmbeddedProviderKind,
    tenant_id: &TenantId,
) -> PathBuf {
    data_dir.join(format!(
        "{}.{}",
        tenant_id.as_str(),
        backend.tenant_file_extension()
    ))
}

fn tasks_table() -> TableName {
    TableName::new("tasks").expect("static table name should be valid")
}

fn filter(field: &str, op: FilterOp, value: serde_json::Value) -> Filter {
    Filter {
        field: field.to_string(),
        op,
        value,
    }
}

fn single_field_schema() -> TableSchema {
    TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![IndexDefinition {
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    }
}

fn composite_schema() -> TableSchema {
    TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "team".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![IndexDefinition {
            name: "by_team_status_rank".to_string(),
            fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    }
}

fn duration_from_nanos_f64(nanos: f64) -> Duration {
    Duration::from_secs_f64((nanos.max(0.0)) / 1_000_000_000.0)
}

fn median_f64(sorted: &[f64]) -> f64 {
    let mid = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[mid - 1] + sorted[mid]) / 2.0
    } else {
        sorted[mid]
    }
}

fn student_t_critical_95(sample_count: usize) -> f64 {
    match sample_count.saturating_sub(1) {
        0 => 0.0,
        1 => 12.706,
        2 => 4.303,
        3 => 3.182,
        4 => 2.776,
        5 => 2.571,
        6 => 2.447,
        7 => 2.365,
        8 => 2.306,
        9 => 2.262,
        10 => 2.228,
        11 => 2.201,
        12 => 2.179,
        13 => 2.160,
        14 => 2.145,
        15 => 2.131,
        16 => 2.120,
        17 => 2.110,
        18 => 2.101,
        19 => 2.093,
        20 => 2.086,
        21 => 2.080,
        22 => 2.074,
        23 => 2.069,
        24 => 2.064,
        25 => 2.060,
        26 => 2.056,
        27 => 2.052,
        28 => 2.048,
        29 => 2.045,
        30 => 2.042,
        _ => 1.960,
    }
}

fn duration_ratio(baseline: Duration, candidate: Duration) -> f64 {
    candidate.as_secs_f64().max(f64::MIN_POSITIVE).recip() * baseline.as_secs_f64()
}

fn format_duration(duration: Duration) -> String {
    if duration.as_secs_f64() >= 1.0 {
        format!("{:.2} s", duration.as_secs_f64())
    } else if duration.as_millis() > 0 {
        format!("{:.2} ms", duration.as_secs_f64() * 1_000.0)
    } else if duration.as_micros() > 0 {
        format!("{:.2} us", duration.as_secs_f64() * 1_000_000.0)
    } else {
        format!("{:.2} ns", duration.as_secs_f64() * 1_000_000_000.0)
    }
}

fn format_confidence_interval(lower: Duration, upper: Duration) -> String {
    format!("{} - {}", format_duration(lower), format_duration(upper))
}
