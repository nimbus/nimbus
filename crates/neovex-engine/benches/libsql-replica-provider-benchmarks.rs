use std::env;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use neovex_core::{
    DocumentId, Error as NeovexError, FieldSchema, FieldType, Filter, FilterOp, IndexDefinition,
    OrderBy, OrderDirection, Query, TableName, TableSchema, TenantId,
};
use neovex_engine::{
    ControlPlaneConfig, EmbeddedProviderKind, PersistenceDialect, PersistenceTopology, PoolConfig,
    ProviderCredentials, Service, ServicePersistenceConfig, TenantProviderConfig,
    TenantRoutingConfig,
};
use neovex_storage::{LibsqlReplicaProvider, LibsqlReplicaProviderConfig};
use serde_json::json;

type BenchResult<T> = Result<T, Box<dyn std::error::Error>>;

#[path = "provider_bench/common.rs"]
mod common;

use common::*;

#[path = "libsql_replica_provider_benchmarks/report.rs"]
mod report;
#[path = "libsql_replica_provider_benchmarks/suite.rs"]
mod suite;
#[path = "libsql_replica_provider_benchmarks/support.rs"]
mod support;

use report::render_markdown;
use suite::run_suite;
use support::*;

const STEADY_STATE_WARMUP_ROUNDS: usize = 2;
const STEADY_STATE_MEASURE_ROUNDS: usize = 10;
const COLD_START_WARMUP_ROUNDS: usize = 1;
const COLD_START_MEASURE_ROUNDS: usize = 8;
const OPERATIONAL_WARMUP_ROUNDS: usize = 1;
const OPERATIONAL_MEASURE_ROUNDS: usize = 10;

const CRUD_DOCUMENTS: usize = 24;
const POINT_READ_DOCUMENTS: usize = 500;
const POINT_READ_BATCH_SIZE: usize = 100;
const INDEXED_QUERY_DOCUMENTS: usize = 1_000;
const INDEXED_QUERY_BATCH_SIZE: usize = 12;
const MIXED_LOAD_TENANTS: usize = 2;
const MIXED_LOAD_OPS_PER_TENANT: usize = 40;
const MIXED_LOAD_OPERATION_TIMEOUT_SECS: u64 = 20;
const MIXED_LOAD_SAMPLE_TIMEOUT_SECS: u64 = 120;
const BENCHMARK_QUIESCE_TIMEOUT_SECS: u64 = 10;
const PEER_CATCH_UP_TIMEOUT_SECS: u64 = 6;
const PEER_CATCH_UP_POLL_INTERVAL_MS: u64 = 25;

const LIBSQL_URL_ENV: &str = "NEOVEX_LIBSQL_URL";
const LIBSQL_AUTH_TOKEN_ENV: &str = "NEOVEX_LIBSQL_AUTH_TOKEN";
const LIBSQL_ADMIN_URL_ENV: &str = "NEOVEX_LIBSQL_ADMIN_URL";
const LIBSQL_ADMIN_AUTH_HEADER_ENV: &str = "NEOVEX_LIBSQL_ADMIN_AUTH_HEADER";

static BENCH_COUNTER: AtomicU64 = AtomicU64::new(1);
static REPLICA_CLEANUP_QUEUE: OnceLock<StdMutex<Vec<LibsqlReplicaProviderConfig>>> =
    OnceLock::new();

#[tokio::main(flavor = "multi_thread")]
async fn main() -> BenchResult<()> {
    let config = BenchmarkConfig::from_args()?;
    let environment = BenchmarkEnvironment::new(&config);
    let report = run_suite(&config, &environment).await?;
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
    primary_url: String,
    auth_token: Option<String>,
    admin_api_url: String,
    admin_auth_header: Option<String>,
}

impl BenchmarkConfig {
    fn from_args() -> BenchResult<Self> {
        let mut markdown_output = None;
        let mut workload_filter = None;
        let mut primary_url = env::var(LIBSQL_URL_ENV).ok();
        let mut auth_token = env::var(LIBSQL_AUTH_TOKEN_ENV).ok();
        let mut admin_api_url = env::var(LIBSQL_ADMIN_URL_ENV).ok();
        let mut admin_auth_header = env::var(LIBSQL_ADMIN_AUTH_HEADER_ENV).ok();
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
                "--libsql-url" => {
                    let Some(url) = args.next() else {
                        return Err("expected a URL after --libsql-url".into());
                    };
                    primary_url = Some(url);
                }
                "--libsql-auth-token" => {
                    let Some(token) = args.next() else {
                        return Err("expected a token after --libsql-auth-token".into());
                    };
                    auth_token = Some(token);
                }
                "--libsql-admin-url" => {
                    let Some(url) = args.next() else {
                        return Err("expected a URL after --libsql-admin-url".into());
                    };
                    admin_api_url = Some(url);
                }
                "--libsql-admin-auth-header" => {
                    let Some(header) = args.next() else {
                        return Err("expected a header after --libsql-admin-auth-header".into());
                    };
                    admin_auth_header = Some(header);
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }

        let Some(primary_url) = primary_url else {
            return Err(format!(
                "set {LIBSQL_URL_ENV} or pass --libsql-url for the benchmark target"
            )
            .into());
        };
        let Some(admin_api_url) = admin_api_url else {
            return Err(format!(
                "set {LIBSQL_ADMIN_URL_ENV} or pass --libsql-admin-url for the benchmark target"
            )
            .into());
        };

        Ok(Self {
            markdown_output,
            workload_filter,
            primary_url,
            auth_token,
            admin_api_url,
            admin_auth_header,
        })
    }
}

fn print_usage() {
    println!(
        "Usage: cargo bench -p neovex-engine --bench libsql-replica-provider-benchmarks -- [--markdown <path>] [--workload <slug>] [--libsql-url <url>] [--libsql-auth-token <token>] [--libsql-admin-url <url>] [--libsql-admin-auth-header <header>]"
    );
}

struct BenchmarkEnvironment {
    primary_url: String,
    auth_token: Option<String>,
    admin_api_url: String,
    admin_auth_header: Option<String>,
}

impl BenchmarkEnvironment {
    fn new(config: &BenchmarkConfig) -> Self {
        Self {
            primary_url: config.primary_url.clone(),
            auth_token: config.auth_token.clone(),
            admin_api_url: config.admin_api_url.clone(),
            admin_auth_header: config.admin_auth_header.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkloadKind {
    CrudThroughput,
    PointReadLatency,
    IndexedQueryLatency,
    CompositeIndexedQueryLatency,
    MixedMultiTenantLoad,
    BarrierRefreshLatency,
    PeerCatchUpLatency,
}

impl WorkloadKind {
    fn parse(value: &str) -> BenchResult<Self> {
        match value {
            "crud" => Ok(Self::CrudThroughput),
            "point-read" => Ok(Self::PointReadLatency),
            "indexed-query" => Ok(Self::IndexedQueryLatency),
            "composite-indexed-query" => Ok(Self::CompositeIndexedQueryLatency),
            "mixed-load" => Ok(Self::MixedMultiTenantLoad),
            "barrier-refresh" => Ok(Self::BarrierRefreshLatency),
            "peer-catch-up" => Ok(Self::PeerCatchUpLatency),
            _ => Err(format!("unknown workload slug: {value}").into()),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::CrudThroughput => "document CRUD throughput",
            Self::PointReadLatency => "point read latency",
            Self::IndexedQueryLatency => "indexed query latency",
            Self::CompositeIndexedQueryLatency => "composite indexed query latency",
            Self::MixedMultiTenantLoad => "concurrent multi-tenant mixed read/write load",
            Self::BarrierRefreshLatency => "same-service barrier refresh latency",
            Self::PeerCatchUpLatency => "peer catch-up / delegated-write visibility latency",
        }
    }

    fn notes(self) -> &'static str {
        match self {
            Self::CrudThroughput => {
                "async insert + update + delete through the canonical service mutation path"
            }
            Self::PointReadLatency => "batched async `get_document_async` over seeded documents",
            Self::IndexedQueryLatency => {
                "single-field `status` equality query through the planner-selected index path"
            }
            Self::CompositeIndexedQueryLatency => {
                "three-field composite index query with exact-prefix + range filters"
            }
            Self::MixedMultiTenantLoad => {
                "concurrent per-tenant mix of point reads, indexed queries, inserts, and updates"
            }
            Self::BarrierRefreshLatency => {
                "time from a committed replica-backed write returning to the first same-service read completing against a refreshed derivative cache"
            }
            Self::PeerCatchUpLatency => {
                "time from a delegated write on one replica-backed service to visibility on a second service through poll-driven catch-up"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchmarkLane {
    SteadyState,
    ColdStart,
    ReplicaOperational,
}

impl BenchmarkLane {
    fn label(self) -> &'static str {
        match self {
            Self::SteadyState => "Steady-State",
            Self::ColdStart => "Cold-Start",
            Self::ReplicaOperational => "Replica-Operational",
        }
    }

    fn notes(self) -> &'static str {
        match self {
            Self::SteadyState => "reuses warmed services and alternates backend order every round",
            Self::ColdStart => {
                "times a fresh service/runtime open plus the first representative execution"
            }
            Self::ReplicaOperational => {
                "reuses warmed replica-backed services and measures the explicit refresh/catch-up drills that define semantic freshness for this provider family"
            }
        }
    }

    fn warmup_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_STEADY_WARMUP_ROUNDS",
                STEADY_STATE_WARMUP_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_COLD_WARMUP_ROUNDS",
                COLD_START_WARMUP_ROUNDS,
            ),
            Self::ReplicaOperational => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_OPERATIONAL_WARMUP_ROUNDS",
                OPERATIONAL_WARMUP_ROUNDS,
            ),
        }
    }

    fn measure_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_STEADY_MEASURE_ROUNDS",
                STEADY_STATE_MEASURE_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_COLD_MEASURE_ROUNDS",
                COLD_START_MEASURE_ROUNDS,
            ),
            Self::ReplicaOperational => read_round_override(
                "NEOVEX_LIBSQL_REPLICA_BENCH_OPERATIONAL_MEASURE_ROUNDS",
                OPERATIONAL_MEASURE_ROUNDS,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeasuredBackend {
    Sqlite,
    LibsqlReplica,
}

impl MeasuredBackend {
    fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::LibsqlReplica => "libsql replica",
        }
    }
}

#[derive(Debug, Default)]
struct BenchmarkReport {
    measurements: Vec<WorkloadMeasurement>,
}

impl BenchmarkReport {
    fn push_measurement(
        &mut self,
        workload: WorkloadKind,
        lane: BenchmarkLane,
        backend: MeasuredBackend,
        operations_per_sample: u64,
        samples: Vec<Duration>,
    ) {
        self.measurements.push(WorkloadMeasurement {
            workload,
            lane,
            backend,
            operations_per_sample,
            samples,
        });
    }
}

#[derive(Debug, Clone)]
struct WorkloadMeasurement {
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backend: MeasuredBackend,
    operations_per_sample: u64,
    samples: Vec<Duration>,
}

impl WorkloadMeasurement {
    fn stats(&self) -> SampleStats {
        SampleStats::from_samples(&self.samples, self.operations_per_sample)
    }
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
struct TenantFixture {
    resource: LiveResource,
    service: Arc<Service>,
    tenant_id: TenantId,
}

#[derive(Clone)]
struct PointReadFixture {
    tenant: TenantFixture,
    ids: Vec<DocumentId>,
}

#[derive(Clone)]
struct QueryFixture {
    tenant: TenantFixture,
    query: Query,
}

#[derive(Clone)]
struct MixedLoadFixture {
    resource: LiveResource,
    service: Arc<Service>,
    tenant_states: Vec<TenantState>,
}

#[derive(Clone)]
struct PointReadSeed {
    resource: SeedResource,
    tenant_id: TenantId,
    ids: Vec<DocumentId>,
}

#[derive(Clone)]
struct QuerySeed {
    resource: SeedResource,
    tenant_id: TenantId,
    query: Query,
}

#[derive(Clone)]
struct MixedLoadSeed {
    resource: SeedResource,
    tenant_states: Vec<TenantState>,
}

#[derive(Clone)]
struct TenantState {
    tenant_id: TenantId,
    ids: Vec<DocumentId>,
}

struct PeerCatchUpFixture {
    creator_resource: LiveResource,
    creator_service: Arc<Service>,
    opener_resource: LiveResource,
    opener_service: Arc<Service>,
    tenant_id: TenantId,
}

#[derive(Clone)]
enum LiveResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
        data_dir: PathBuf,
    },
    LibsqlReplica {
        control_dir: Arc<BenchDir>,
        replica_cache_dir: Arc<BenchDir>,
        provider_config: LibsqlReplicaProviderConfig,
    },
}

#[derive(Clone)]
enum SeedResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
        data_dir: PathBuf,
    },
    LibsqlReplica {
        provider_config: LibsqlReplicaProviderConfig,
    },
}

enum ReopenedResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
    },
    LibsqlReplica {
        control_dir: Arc<BenchDir>,
        replica_cache_dir: Arc<BenchDir>,
    },
}

#[derive(Debug)]
struct BenchDir {
    path: PathBuf,
}

impl BenchDir {
    fn new(label: &str) -> BenchResult<Self> {
        let counter = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
        let path = env::temp_dir().join(format!(
            "neovex-libsql-replica-bench-{label}-{}-{counter}",
            std::process::id()
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

async fn benchmark_crud_throughput(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::CrudThroughput, async {
        let sqlite_fixture =
            create_crud_fixture("crud-steady", "crud", MeasuredBackend::Sqlite, environment)
                .await?;
        let replica_fixture = create_crud_fixture(
            "crud-steady",
            "crud",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let (sqlite_steady, replica_steady) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let replica_fixture = replica_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::LibsqlReplica => replica_fixture,
                    };
                    let started = Instant::now();
                    exercise_crud_sample(&fixture.service, &fixture.tenant_id, CRUD_DOCUMENTS)
                        .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .resource
            .cleanup(
                sqlite_fixture.service.clone(),
                "CRUD steady-state sqlite teardown",
            )
            .await?;
        replica_fixture
            .resource
            .cleanup(
                replica_fixture.service.clone(),
                "CRUD steady-state libsql-replica teardown",
            )
            .await?;

        let (sqlite_cold, replica_cold) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| async move {
                let fixture =
                    create_crud_fixture("crud-cold", "crud", backend, environment).await?;
                let started = Instant::now();
                exercise_crud_sample(&fixture.service, &fixture.tenant_id, CRUD_DOCUMENTS).await?;
                let elapsed = started.elapsed();
                fixture
                    .resource
                    .cleanup(fixture.service.clone(), "CRUD cold-start teardown")
                    .await?;
                Ok(elapsed)
            },
        )
        .await?;

        record_contrast_measurements(
            report,
            WorkloadKind::CrudThroughput,
            BenchmarkLane::SteadyState,
            (CRUD_DOCUMENTS * 3) as u64,
            sqlite_steady,
            replica_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            (CRUD_DOCUMENTS * 3) as u64,
            sqlite_cold,
            replica_cold,
        );
        Ok(())
    })
    .await
}

async fn benchmark_point_read_latency(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::PointReadLatency, async {
        let sqlite_fixture = create_point_read_fixture(
            "point-read-steady",
            "point-read",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let replica_fixture = create_point_read_fixture(
            "point-read-steady",
            "point-read",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let (sqlite_steady, replica_steady) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let replica_fixture = replica_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::LibsqlReplica => replica_fixture,
                    };
                    let started = Instant::now();
                    exercise_point_read_sample(
                        &fixture.tenant.service,
                        &fixture.tenant.tenant_id,
                        &fixture.ids,
                        POINT_READ_BATCH_SIZE,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .tenant
            .resource
            .cleanup(
                sqlite_fixture.tenant.service.clone(),
                "point-read steady-state sqlite teardown",
            )
            .await?;
        replica_fixture
            .tenant
            .resource
            .cleanup(
                replica_fixture.tenant.service.clone(),
                "point-read steady-state libsql-replica teardown",
            )
            .await?;

        let sqlite_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-cold-seed",
                "point-read",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await?,
            "point-read sqlite seed freeze",
        )
        .await?;
        let replica_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-cold-seed",
                "point-read",
                MeasuredBackend::LibsqlReplica,
                environment,
            )
            .await?,
            "point-read libsql-replica seed freeze",
        )
        .await?;
        let (sqlite_cold, replica_cold) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let replica_seed = replica_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::LibsqlReplica => replica_seed,
                    };
                    let (service, resource) = seed
                        .resource
                        .reopen_service("point-read-cold", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_point_read_sample(
                        &service,
                        &seed.tenant_id,
                        &seed.ids,
                        POINT_READ_BATCH_SIZE,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    resource
                        .cleanup(service, "point-read cold-start teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        replica_seed.resource.cleanup_seed().await?;

        record_contrast_measurements(
            report,
            WorkloadKind::PointReadLatency,
            BenchmarkLane::SteadyState,
            POINT_READ_BATCH_SIZE as u64,
            sqlite_steady,
            replica_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            POINT_READ_BATCH_SIZE as u64,
            sqlite_cold,
            replica_cold,
        );
        Ok(())
    })
    .await
}

async fn benchmark_indexed_query_latency(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_query_latency(
        WorkloadKind::IndexedQueryLatency,
        QueryFixtureKind::Indexed,
        environment,
        report,
    )
    .await
}

async fn benchmark_composite_indexed_query_latency(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_query_latency(
        WorkloadKind::CompositeIndexedQueryLatency,
        QueryFixtureKind::Composite,
        environment,
        report,
    )
    .await
}

#[derive(Clone, Copy)]
enum QueryFixtureKind {
    Indexed,
    Composite,
}

async fn benchmark_query_latency(
    workload: WorkloadKind,
    query_kind: QueryFixtureKind,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(workload, async move {
        let sqlite_fixture = create_query_fixture(
            query_kind,
            "query-steady",
            "query",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let replica_fixture = create_query_fixture(
            query_kind,
            "query-steady",
            "query",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let (sqlite_steady, replica_steady) = measure_two_backends_async(
            workload,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let replica_fixture = replica_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::LibsqlReplica => replica_fixture,
                    };
                    let started = Instant::now();
                    exercise_query_sample(
                        &fixture.tenant.service,
                        &fixture.tenant.tenant_id,
                        &fixture.query,
                        INDEXED_QUERY_BATCH_SIZE,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .tenant
            .resource
            .cleanup(
                sqlite_fixture.tenant.service.clone(),
                "query steady-state sqlite teardown",
            )
            .await?;
        replica_fixture
            .tenant
            .resource
            .cleanup(
                replica_fixture.tenant.service.clone(),
                "query steady-state libsql-replica teardown",
            )
            .await?;

        let sqlite_seed = freeze_query_seed(
            create_query_fixture(
                query_kind,
                "query-cold-seed",
                "query",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await?,
            "query sqlite seed freeze",
        )
        .await?;
        let replica_seed = freeze_query_seed(
            create_query_fixture(
                query_kind,
                "query-cold-seed",
                "query",
                MeasuredBackend::LibsqlReplica,
                environment,
            )
            .await?,
            "query libsql-replica seed freeze",
        )
        .await?;
        let (sqlite_cold, replica_cold) = measure_two_backends_async(
            workload,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let replica_seed = replica_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::LibsqlReplica => replica_seed,
                    };
                    let (service, resource) = seed
                        .resource
                        .reopen_service("query-cold", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_query_sample(
                        &service,
                        &seed.tenant_id,
                        &seed.query,
                        INDEXED_QUERY_BATCH_SIZE,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    resource
                        .cleanup(service, "query cold-start teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        replica_seed.resource.cleanup_seed().await?;

        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::SteadyState,
            INDEXED_QUERY_BATCH_SIZE as u64,
            sqlite_steady,
            replica_steady,
        );
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::ColdStart,
            INDEXED_QUERY_BATCH_SIZE as u64,
            sqlite_cold,
            replica_cold,
        );
        Ok(())
    })
    .await
}

async fn create_query_fixture(
    kind: QueryFixtureKind,
    label: &'static str,
    tenant_label: &'static str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<QueryFixture> {
    match kind {
        QueryFixtureKind::Indexed => {
            create_indexed_query_fixture(label, tenant_label, backend, environment).await
        }
        QueryFixtureKind::Composite => {
            create_composite_query_fixture(label, tenant_label, backend, environment).await
        }
    }
}

async fn benchmark_mixed_multi_tenant_load(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::MixedMultiTenantLoad, async {
        let sqlite_fixture =
            create_mixed_load_fixture("mixed-load-steady", MeasuredBackend::Sqlite, environment)
                .await?;
        let replica_fixture = create_mixed_load_fixture(
            "mixed-load-steady",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let (sqlite_steady, replica_steady) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let replica_fixture = replica_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::LibsqlReplica => replica_fixture,
                    };
                    let started = Instant::now();
                    run_mixed_load_sample(
                        "mixed-load steady-state sample",
                        exercise_mixed_load_sample(
                            &fixture.service,
                            &fixture.tenant_states,
                            MIXED_LOAD_TENANTS,
                            MIXED_LOAD_OPS_PER_TENANT,
                        ),
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .resource
            .cleanup(
                sqlite_fixture.service.clone(),
                "mixed-load steady-state sqlite teardown",
            )
            .await?;
        replica_fixture
            .resource
            .cleanup(
                replica_fixture.service.clone(),
                "mixed-load steady-state libsql-replica teardown",
            )
            .await?;

        let sqlite_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture("mixed-load-cold-seed", MeasuredBackend::Sqlite, environment)
                .await?,
            "mixed-load sqlite seed freeze",
        )
        .await?;
        let replica_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-cold-seed",
                MeasuredBackend::LibsqlReplica,
                environment,
            )
            .await?,
            "mixed-load libsql-replica seed freeze",
        )
        .await?;
        let (sqlite_cold, replica_cold) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::LibsqlReplica],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let replica_seed = replica_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::LibsqlReplica => replica_seed,
                    };
                    let (service, resource) = seed
                        .resource
                        .reopen_service("mixed-load-cold", backend, environment)
                        .await?;
                    let started = Instant::now();
                    run_mixed_load_sample(
                        "mixed-load cold-start sample",
                        exercise_mixed_load_sample(
                            &service,
                            &seed.tenant_states,
                            MIXED_LOAD_TENANTS,
                            MIXED_LOAD_OPS_PER_TENANT,
                        ),
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    resource
                        .cleanup(service, "mixed-load cold-start teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        replica_seed.resource.cleanup_seed().await?;

        record_contrast_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            (MIXED_LOAD_TENANTS * MIXED_LOAD_OPS_PER_TENANT) as u64,
            sqlite_steady,
            replica_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::ColdStart,
            (MIXED_LOAD_TENANTS * MIXED_LOAD_OPS_PER_TENANT) as u64,
            sqlite_cold,
            replica_cold,
        );
        Ok(())
    })
    .await
}

async fn benchmark_barrier_refresh_latency(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::BarrierRefreshLatency, async {
        let fixture = create_tenant_service(
            "barrier-refresh",
            "barrier-refresh",
            MeasuredBackend::LibsqlReplica,
            environment,
        )
        .await?;
        let samples = measure_single_backend_async(
            WorkloadKind::BarrierRefreshLatency,
            BenchmarkLane::ReplicaOperational,
            || {
                let fixture = fixture.clone();
                async move {
                    let created_id = fixture
                        .service
                        .insert_document_async(
                            fixture.tenant_id.clone(),
                            tasks_table(),
                            serde_json::Map::from_iter([
                                ("status".to_string(), json!("open")),
                                (
                                    "title".to_string(),
                                    json!(format!(
                                        "barrier-{}",
                                        BENCH_COUNTER.fetch_add(1, Ordering::SeqCst)
                                    )),
                                ),
                            ]),
                        )
                        .await?;
                    let started = Instant::now();
                    let document = fixture
                        .service
                        .get_document_async(fixture.tenant_id.clone(), tasks_table(), created_id)
                        .await?;
                    black_box(document);
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        fixture
            .resource
            .cleanup(
                fixture.service.clone(),
                "barrier-refresh libsql-replica teardown",
            )
            .await?;
        report.push_measurement(
            WorkloadKind::BarrierRefreshLatency,
            BenchmarkLane::ReplicaOperational,
            MeasuredBackend::LibsqlReplica,
            1,
            samples,
        );
        Ok(())
    })
    .await
}

async fn benchmark_peer_catch_up_latency(
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::PeerCatchUpLatency, async {
        let fixture = create_peer_catch_up_fixture("peer-catch-up", environment).await?;
        let samples = measure_single_backend_async(
            WorkloadKind::PeerCatchUpLatency,
            BenchmarkLane::ReplicaOperational,
            || {
                let fixture = fixture.clone();
                async move { exercise_peer_catch_up_sample(&fixture).await }
            },
        )
        .await?;
        fixture
            .cleanup("peer-catch-up libsql-replica teardown")
            .await?;
        report.push_measurement(
            WorkloadKind::PeerCatchUpLatency,
            BenchmarkLane::ReplicaOperational,
            MeasuredBackend::LibsqlReplica,
            1,
            samples,
        );
        Ok(())
    })
    .await
}

async fn create_tenant_service(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<TenantFixture> {
    match backend {
        MeasuredBackend::Sqlite => {
            let bench_dir = Arc::new(BenchDir::new(&format!("{label}-sqlite"))?);
            let data_dir = bench_dir.path().to_path_buf();
            let service = Arc::new(
                Service::new_with_persistence_config(ServicePersistenceConfig::embedded(
                    &data_dir,
                    EmbeddedProviderKind::Sqlite,
                ))
                .await?,
            );
            let tenant_id = benchmark_tenant_id(tenant_label)?;
            service.create_tenant_async(tenant_id.clone()).await?;
            Ok(TenantFixture {
                resource: LiveResource::Sqlite {
                    bench_dir,
                    data_dir,
                },
                service,
                tenant_id,
            })
        }
        MeasuredBackend::LibsqlReplica => {
            let control_dir = Arc::new(BenchDir::new(&format!("{label}-replica-control"))?);
            let replica_cache_dir = Arc::new(BenchDir::new(&format!("{label}-replica-cache"))?);
            let provider_config =
                benchmark_libsql_provider_config(label, environment, replica_cache_dir.path());
            let service = Arc::new(
                Service::new_with_persistence_config(libsql_replica_service_config(
                    control_dir.path(),
                    &provider_config,
                ))
                .await?,
            );
            let tenant_id = benchmark_tenant_id(tenant_label)?;
            service.create_tenant_async(tenant_id.clone()).await?;
            Ok(TenantFixture {
                resource: LiveResource::LibsqlReplica {
                    control_dir,
                    replica_cache_dir,
                    provider_config,
                },
                service,
                tenant_id,
            })
        }
    }
}

async fn create_crud_fixture(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<TenantFixture> {
    create_tenant_service(label, tenant_label, backend, environment).await
}

async fn create_point_read_fixture(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<PointReadFixture> {
    let tenant = create_tenant_service(label, tenant_label, backend, environment).await?;
    let mut ids = Vec::with_capacity(POINT_READ_DOCUMENTS);
    for rank in 0..POINT_READ_DOCUMENTS {
        ids.push(
            tenant
                .service
                .insert_document_async(
                    tenant.tenant_id.clone(),
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
    Ok(PointReadFixture { tenant, ids })
}

async fn create_indexed_query_fixture(
    label: &'static str,
    tenant_label: &'static str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<QueryFixture> {
    let tenant = create_tenant_service(label, tenant_label, backend, environment).await?;
    tenant
        .service
        .set_table_schema_async(tenant.tenant_id.clone(), single_field_schema())
        .await?;
    for rank in 0..INDEXED_QUERY_DOCUMENTS {
        tenant
            .service
            .insert_document_async(
                tenant.tenant_id.clone(),
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
        tenant,
        query: Query {
            table: tasks_table(),
            filters: vec![filter("status", FilterOp::Eq, json!("open"))],
            order: None,
            limit: None,
        },
    })
}

async fn create_composite_query_fixture(
    label: &'static str,
    tenant_label: &'static str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<QueryFixture> {
    let tenant = create_tenant_service(label, tenant_label, backend, environment).await?;
    tenant
        .service
        .set_table_schema_async(tenant.tenant_id.clone(), composite_schema())
        .await?;
    for rank in 0..INDEXED_QUERY_DOCUMENTS {
        let team = if rank % 2 == 0 { "alpha" } else { "beta" };
        let status = if rank % 3 == 0 { "open" } else { "done" };
        tenant
            .service
            .insert_document_async(
                tenant.tenant_id.clone(),
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
        tenant,
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

async fn create_mixed_load_fixture(
    label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<MixedLoadFixture> {
    let (resource, service) = match backend {
        MeasuredBackend::Sqlite => {
            let bench_dir = Arc::new(BenchDir::new(&format!("{label}-sqlite"))?);
            let data_dir = bench_dir.path().to_path_buf();
            let service = Arc::new(
                Service::new_with_persistence_config(ServicePersistenceConfig::embedded(
                    &data_dir,
                    EmbeddedProviderKind::Sqlite,
                ))
                .await?,
            );
            (
                LiveResource::Sqlite {
                    bench_dir,
                    data_dir,
                },
                service,
            )
        }
        MeasuredBackend::LibsqlReplica => {
            let control_dir = Arc::new(BenchDir::new(&format!("{label}-replica-control"))?);
            let replica_cache_dir = Arc::new(BenchDir::new(&format!("{label}-replica-cache"))?);
            let provider_config =
                benchmark_libsql_provider_config(label, environment, replica_cache_dir.path());
            let service = Arc::new(
                Service::new_with_persistence_config(libsql_replica_service_config(
                    control_dir.path(),
                    &provider_config,
                ))
                .await?,
            );
            (
                LiveResource::LibsqlReplica {
                    control_dir,
                    replica_cache_dir,
                    provider_config,
                },
                service,
            )
        }
    };

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
        resource,
        service,
        tenant_states,
    })
}

async fn create_peer_catch_up_fixture(
    label: &str,
    environment: &BenchmarkEnvironment,
) -> BenchResult<PeerCatchUpFixture> {
    let suffix = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let base_slug = slugify_label(label, 12);
    let metadata_namespace = format!("nvx_{}_{}_{suffix:x}", base_slug, std::process::id());
    let tenant_namespace_prefix = format!("t_{}_{}_{suffix:x}_", base_slug, std::process::id());

    let creator_control = Arc::new(BenchDir::new(&format!("{label}-creator-control"))?);
    let creator_cache = Arc::new(BenchDir::new(&format!("{label}-creator-cache"))?);
    let opener_control = Arc::new(BenchDir::new(&format!("{label}-opener-control"))?);
    let opener_cache = Arc::new(BenchDir::new(&format!("{label}-opener-cache"))?);

    let creator_provider_config = LibsqlReplicaProviderConfig {
        primary_url: environment.primary_url.clone(),
        auth_token: environment.auth_token.clone(),
        admin_api_url: environment.admin_api_url.clone(),
        admin_auth_header: environment.admin_auth_header.clone(),
        metadata_namespace: metadata_namespace.clone(),
        tenant_namespace_prefix: tenant_namespace_prefix.clone(),
        replica_cache_dir: creator_cache.path().to_path_buf(),
    };
    let opener_provider_config = LibsqlReplicaProviderConfig {
        replica_cache_dir: opener_cache.path().to_path_buf(),
        ..creator_provider_config.clone()
    };

    let creator_service = Arc::new(
        Service::new_with_persistence_config(libsql_replica_service_config(
            creator_control.path(),
            &creator_provider_config,
        ))
        .await?,
    );
    let opener_service = Arc::new(
        Service::new_with_persistence_config(libsql_replica_service_config(
            opener_control.path(),
            &opener_provider_config,
        ))
        .await?,
    );

    let tenant_id = benchmark_tenant_id("peer-catch-up")?;
    creator_service
        .create_tenant_async(tenant_id.clone())
        .await?;
    creator_service
        .set_table_schema_async(tenant_id.clone(), single_field_schema())
        .await?;
    opener_service
        .ensure_tenant_exists_async(tenant_id.clone())
        .await?;
    let _ = opener_service.get_schema_async(tenant_id.clone()).await?;

    Ok(PeerCatchUpFixture {
        creator_resource: LiveResource::LibsqlReplica {
            control_dir: creator_control,
            replica_cache_dir: creator_cache,
            provider_config: creator_provider_config,
        },
        creator_service,
        opener_resource: LiveResource::LibsqlReplica {
            control_dir: opener_control,
            replica_cache_dir: opener_cache,
            provider_config: opener_provider_config,
        },
        opener_service,
        tenant_id,
    })
}

async fn freeze_point_read_seed(
    fixture: PointReadFixture,
    context: &str,
) -> BenchResult<PointReadSeed> {
    let PointReadFixture { tenant, ids } = fixture;
    quiesce_service(&tenant.service, context).await?;
    drop(tenant.service);
    Ok(PointReadSeed {
        resource: tenant.resource.into_seed_resource(),
        tenant_id: tenant.tenant_id,
        ids,
    })
}

async fn freeze_query_seed(fixture: QueryFixture, context: &str) -> BenchResult<QuerySeed> {
    let QueryFixture { tenant, query } = fixture;
    quiesce_service(&tenant.service, context).await?;
    drop(tenant.service);
    Ok(QuerySeed {
        resource: tenant.resource.into_seed_resource(),
        tenant_id: tenant.tenant_id,
        query,
    })
}

async fn freeze_mixed_load_seed(
    fixture: MixedLoadFixture,
    context: &str,
) -> BenchResult<MixedLoadSeed> {
    let MixedLoadFixture {
        resource,
        service,
        tenant_states,
    } = fixture;
    quiesce_service(&service, context).await?;
    drop(service);
    Ok(MixedLoadSeed {
        resource: resource.into_seed_resource(),
        tenant_states,
    })
}

impl LiveResource {
    async fn cleanup(&self, service: Arc<Service>, context: &str) -> BenchResult<()> {
        quiesce_service(&service, context).await?;
        drop(service);
        match self {
            Self::Sqlite {
                bench_dir,
                data_dir,
            } => {
                black_box(bench_dir.path());
                black_box(data_dir.as_os_str());
            }
            Self::LibsqlReplica {
                control_dir,
                replica_cache_dir,
                provider_config,
            } => {
                black_box(control_dir.path());
                black_box(replica_cache_dir.path());
                register_libsql_replica_cleanup(provider_config);
            }
        }
        Ok(())
    }

    fn into_seed_resource(self) -> SeedResource {
        match self {
            Self::Sqlite {
                bench_dir,
                data_dir,
            } => SeedResource::Sqlite {
                bench_dir,
                data_dir,
            },
            Self::LibsqlReplica {
                provider_config, ..
            } => SeedResource::LibsqlReplica { provider_config },
        }
    }
}

impl SeedResource {
    async fn reopen_service(
        &self,
        label: &str,
        backend: MeasuredBackend,
        environment: &BenchmarkEnvironment,
    ) -> BenchResult<(Arc<Service>, ReopenedResource)> {
        match self {
            Self::Sqlite { data_dir, .. } => {
                let cloned = Arc::new(BenchDir::new(&format!(
                    "{label}-{}",
                    backend.label().replace(' ', "-")
                ))?);
                copy_dir_all(data_dir, cloned.path())?;
                let service = Arc::new(
                    Service::new_with_persistence_config(ServicePersistenceConfig::embedded(
                        cloned.path(),
                        EmbeddedProviderKind::Sqlite,
                    ))
                    .await?,
                );
                Ok((service, ReopenedResource::Sqlite { bench_dir: cloned }))
            }
            Self::LibsqlReplica { provider_config } => {
                let control_dir = Arc::new(BenchDir::new(&format!("{label}-replica-control"))?);
                let replica_cache_dir = Arc::new(BenchDir::new(&format!("{label}-replica-cache"))?);
                let mut reopened_config = provider_config.clone();
                reopened_config.primary_url = environment.primary_url.clone();
                reopened_config.auth_token = environment.auth_token.clone();
                reopened_config.admin_api_url = environment.admin_api_url.clone();
                reopened_config.admin_auth_header = environment.admin_auth_header.clone();
                reopened_config.replica_cache_dir = replica_cache_dir.path().to_path_buf();
                let service = Arc::new(
                    Service::new_with_persistence_config(libsql_replica_service_config(
                        control_dir.path(),
                        &reopened_config,
                    ))
                    .await?,
                );
                Ok((
                    service,
                    ReopenedResource::LibsqlReplica {
                        control_dir,
                        replica_cache_dir,
                    },
                ))
            }
        }
    }

    async fn cleanup_seed(&self) -> BenchResult<()> {
        match self {
            Self::Sqlite {
                bench_dir,
                data_dir,
            } => {
                black_box(bench_dir.path());
                black_box(data_dir.as_os_str());
            }
            Self::LibsqlReplica { provider_config } => {
                register_libsql_replica_cleanup(provider_config);
            }
        }
        Ok(())
    }
}

impl ReopenedResource {
    async fn cleanup(self, service: Arc<Service>, context: &str) -> BenchResult<()> {
        quiesce_service(&service, context).await?;
        drop(service);
        match self {
            Self::Sqlite { bench_dir } => {
                drop(bench_dir);
            }
            Self::LibsqlReplica {
                control_dir,
                replica_cache_dir,
            } => {
                drop(control_dir);
                drop(replica_cache_dir);
            }
        }
        Ok(())
    }
}

impl Clone for PeerCatchUpFixture {
    fn clone(&self) -> Self {
        Self {
            creator_resource: self.creator_resource.clone(),
            creator_service: self.creator_service.clone(),
            opener_resource: self.opener_resource.clone(),
            opener_service: self.opener_service.clone(),
            tenant_id: self.tenant_id.clone(),
        }
    }
}

impl PeerCatchUpFixture {
    async fn cleanup(self, context: &str) -> BenchResult<()> {
        self.creator_resource
            .cleanup(self.creator_service.clone(), &format!("{context} creator"))
            .await?;
        self.opener_resource
            .cleanup(self.opener_service.clone(), &format!("{context} opener"))
            .await?;
        Ok(())
    }
}

async fn exercise_crud_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    document_count: usize,
) -> BenchResult<()> {
    let mut ids = Vec::with_capacity(document_count);
    for rank in 0..document_count {
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
                serde_json::Map::from_iter([("rank".to_string(), json!(rank + document_count))]),
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
    batch_size: usize,
) -> BenchResult<()> {
    for step in 0..batch_size {
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

async fn exercise_mixed_load_sample(
    service: &Arc<Service>,
    tenant_states: &[TenantState],
    tenant_limit: usize,
    ops_per_tenant: usize,
) -> BenchResult<()> {
    let selected = tenant_states
        .iter()
        .take(tenant_limit)
        .cloned()
        .collect::<Vec<_>>();
    let mut handles = Vec::with_capacity(selected.len());
    for (task_index, state) in selected.into_iter().enumerate() {
        let service = service.clone();
        handles.push(tokio::spawn(async move {
            let query = Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                order: None,
                limit: Some(25),
            };
            for step in 0..ops_per_tenant {
                let id = state.ids[step % state.ids.len()];
                match step % 4 {
                    0 => {
                        let document = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            service.get_document_async(state.tenant_id.clone(), tasks_table(), id),
                        )
                        .await
                        .map_err(|_| {
                            NeovexError::Internal(format!(
                                "mixed-load point read timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                        black_box(document);
                    }
                    1 => {
                        let documents = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            service.query_documents_async(state.tenant_id.clone(), query.clone()),
                        )
                        .await
                        .map_err(|_| {
                            NeovexError::Internal(format!(
                                "mixed-load indexed query timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                        black_box(documents);
                    }
                    2 => {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            service.insert_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                serde_json::Map::from_iter([
                                    ("status".to_string(), json!("open")),
                                    (
                                        "rank".to_string(),
                                        json!(task_index * ops_per_tenant + step),
                                    ),
                                    (
                                        "title".to_string(),
                                        json!(format!("tenant-{task_index}-insert-{step}")),
                                    ),
                                ]),
                            ),
                        )
                        .await
                        .map_err(|_| {
                            NeovexError::Internal(format!(
                                "mixed-load insert timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                    }
                    _ => {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            service.update_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                id,
                                serde_json::Map::from_iter([(
                                    "rank".to_string(),
                                    json!(step + ops_per_tenant),
                                )]),
                            ),
                        )
                        .await
                        .map_err(|_| {
                            NeovexError::Internal(format!(
                                "mixed-load update timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                    }
                }
            }
            Ok::<(), NeovexError>(())
        }));
    }
    for handle in handles {
        handle.await??;
    }
    Ok(())
}

async fn run_mixed_load_sample<F>(context: &str, future: F) -> BenchResult<()>
where
    F: std::future::Future<Output = BenchResult<()>>,
{
    tokio::time::timeout(Duration::from_secs(MIXED_LOAD_SAMPLE_TIMEOUT_SECS), future)
        .await
        .map_err(|_| -> Box<dyn std::error::Error> {
            format!("{context} exceeded {MIXED_LOAD_SAMPLE_TIMEOUT_SECS}s").into()
        })?
}

async fn exercise_peer_catch_up_sample(fixture: &PeerCatchUpFixture) -> BenchResult<Duration> {
    let inserted_id = fixture
        .creator_service
        .insert_document_async(
            fixture.tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("open")),
                (
                    "title".to_string(),
                    json!(format!(
                        "peer-catch-up-{}",
                        BENCH_COUNTER.fetch_add(1, Ordering::SeqCst)
                    )),
                ),
            ]),
        )
        .await?;
    let started = Instant::now();
    loop {
        match fixture
            .opener_service
            .get_document_async(fixture.tenant_id.clone(), tasks_table(), inserted_id)
            .await
        {
            Ok(document) => {
                black_box(document);
                return Ok(started.elapsed());
            }
            Err(NeovexError::DocumentNotFound(_)) => {}
            Err(error) => return Err(Box::new(error)),
        }
        if started.elapsed() >= Duration::from_secs(PEER_CATCH_UP_TIMEOUT_SECS) {
            return Err(format!(
                "peer catch-up did not surface the delegated write within {}s",
                PEER_CATCH_UP_TIMEOUT_SECS
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(PEER_CATCH_UP_POLL_INTERVAL_MS)).await;
    }
}
