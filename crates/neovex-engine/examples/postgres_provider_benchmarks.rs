use std::env;
use std::fs;
use std::hint::black_box;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use neovex_core::{
    DocumentId, FieldSchema, FieldType, Filter, FilterOp, IndexDefinition, OrderBy, OrderDirection,
    Query, SequenceNumber, TableName, TableSchema, TenantId,
};
use neovex_engine::{
    ControlPlaneConfig, EmbeddedProviderKind, PersistenceDialect, PersistenceTopology, PoolConfig,
    ProviderCredentials, Service, ServicePersistenceConfig, SubscriptionRegistration,
    SubscriptionUpdate, TenantProviderConfig, TenantRoutingConfig,
};
use neovex_storage::{PostgresProvider, PostgresProviderConfig};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_postgres::{Config as PostgresConfig, NoTls, config::Host};

type BenchResult<T> = Result<T, Box<dyn std::error::Error>>;

const STEADY_STATE_WARMUP_ROUNDS: usize = 2;
const STEADY_STATE_MEASURE_ROUNDS: usize = 10;
const COLD_START_WARMUP_ROUNDS: usize = 1;
const COLD_START_MEASURE_ROUNDS: usize = 8;
const RTT_WARMUP_ROUNDS: usize = 1;
const RTT_MEASURE_ROUNDS: usize = 4;

const CRUD_DOCUMENTS: usize = 300;
const CRUD_RTT_DOCUMENTS: usize = 8;
const POINT_READ_DOCUMENTS: usize = 2_000;
const POINT_READ_BATCH_SIZE: usize = 200;
const POINT_READ_RTT_BATCH_SIZE: usize = 8;
const INDEXED_QUERY_DOCUMENTS: usize = 4_000;
const INDEXED_QUERY_BATCH_SIZE: usize = 24;
const INDEXED_QUERY_RTT_BATCH_SIZE: usize = 4;
const JOURNAL_DOCUMENTS: usize = 1_000;
const JOURNAL_STREAM_LIMIT: usize = 256;
const SUBSCRIPTION_FANOUT_COUNT: usize = 24;
const MIXED_LOAD_TENANTS: usize = 4;
const MIXED_LOAD_OPS_PER_TENANT: usize = 120;
const MIXED_LOAD_RTT_TENANTS: usize = 1;
const MIXED_LOAD_RTT_OPS_PER_TENANT: usize = 8;
const MIXED_LOAD_OPERATION_TIMEOUT_SECS: u64 = 15;
const POOL_PRESSURE_SAMPLES: usize = 8;
const POOL_PRESSURE_TASKS: usize = MIXED_LOAD_TENANTS;
const POOL_PRESSURE_MAX_CONNECTIONS: usize = 2;
const POOL_PRESSURE_SAMPLE_INTERVAL_MS: u64 = 10;
const DEFAULT_RTT_DELAY_MS: u64 = 5;
const BENCHMARK_QUIESCE_TIMEOUT_SECS: u64 = 5;
const BENCH_POSTGRES_URL_ENV: &str = "NEOVEX_BENCH_POSTGRES_URL";
const TEST_POSTGRES_URL_ENV: &str = "NEOVEX_TEST_POSTGRES_URL";

static BENCH_COUNTER: AtomicU64 = AtomicU64::new(1);
static POSTGRES_CLEANUP_QUEUE: OnceLock<StdMutex<Vec<PostgresProviderConfig>>> = OnceLock::new();

#[tokio::main(flavor = "multi_thread")]
async fn main() -> BenchResult<()> {
    let config = BenchmarkConfig::from_args()?;
    let environment = BenchmarkEnvironment::new(&config).await?;
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
    postgres_url: String,
    rtt_delay: Duration,
}

impl BenchmarkConfig {
    fn from_args() -> BenchResult<Self> {
        let mut markdown_output = None;
        let mut workload_filter = None;
        let mut postgres_url = env::var(BENCH_POSTGRES_URL_ENV)
            .ok()
            .or_else(|| env::var(TEST_POSTGRES_URL_ENV).ok());
        let mut rtt_delay = Duration::from_millis(read_u64_override(
            "NEOVEX_BENCH_RTT_DELAY_MS",
            DEFAULT_RTT_DELAY_MS,
        ));
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
                "--postgres-url" => {
                    let Some(url) = args.next() else {
                        return Err("expected a connection string after --postgres-url".into());
                    };
                    postgres_url = Some(url);
                }
                "--rtt-ms" => {
                    let Some(value) = args.next() else {
                        return Err("expected a delay after --rtt-ms".into());
                    };
                    let millis = value.parse::<u64>()?;
                    if millis == 0 {
                        return Err("RTT delay must be greater than 0 ms".into());
                    }
                    rtt_delay = Duration::from_millis(millis);
                }
                "--help" | "-h" => {
                    print_usage();
                    std::process::exit(0);
                }
                _ => return Err(format!("unknown argument: {arg}").into()),
            }
        }

        let Some(postgres_url) = postgres_url else {
            return Err(format!(
                "set {BENCH_POSTGRES_URL_ENV} or pass --postgres-url for the benchmark target"
            )
            .into());
        };

        Ok(Self {
            markdown_output,
            workload_filter,
            postgres_url,
            rtt_delay,
        })
    }
}

fn print_usage() {
    println!(
        "Usage: cargo run -p neovex-engine --release --example postgres_provider_benchmarks -- [--markdown <path>] [--workload <slug>] [--postgres-url <connection-string>] [--rtt-ms <delay>]"
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
    SubscriptionBootstrapCatchupLatency,
    SubscriptionFanoutLatency,
    MixedMultiTenantLoad,
    TenantLifecycleLatency,
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
            "subscription-bootstrap" => Ok(Self::SubscriptionBootstrapCatchupLatency),
            "subscription-fanout" => Ok(Self::SubscriptionFanoutLatency),
            "mixed-load" => Ok(Self::MixedMultiTenantLoad),
            "tenant-lifecycle" => Ok(Self::TenantLifecycleLatency),
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
            Self::SubscriptionBootstrapCatchupLatency => {
                "subscription bootstrap plus catch-up latency"
            }
            Self::SubscriptionFanoutLatency => "subscription fan-out latency",
            Self::MixedMultiTenantLoad => "concurrent multi-tenant mixed read/write load",
            Self::TenantLifecycleLatency => "tenant create/open/delete latency",
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
            Self::DurableJournalStreamLatency => {
                "async `stream_durable_journal_async` from cursor 0 with a fixed page limit"
            }
            Self::DurableJournalBootstrapLatency => {
                "async `export_durable_journal_bootstrap_async` on a seeded tenant"
            }
            Self::SubscriptionBootstrapCatchupLatency => {
                "single subscription bootstrap followed by one durable matching update"
            }
            Self::SubscriptionFanoutLatency => {
                "time from one durable matching write to delivery across all active subscriptions"
            }
            Self::MixedMultiTenantLoad => {
                "concurrent per-tenant mix of point reads, indexed queries, inserts, and updates"
            }
            Self::TenantLifecycleLatency => {
                "create a tenant, verify it opens from a peer service when the topology allows it, then delete it cleanly"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BenchmarkLane {
    SteadyState,
    ColdStart,
    RttSensitive,
}

impl BenchmarkLane {
    fn label(self) -> &'static str {
        match self {
            Self::SteadyState => "Steady-State",
            Self::ColdStart => "Cold-Start",
            Self::RttSensitive => "RTT-Sensitive",
        }
    }

    fn notes(self) -> &'static str {
        match self {
            Self::SteadyState => "reuses warmed services and alternates backend order every round",
            Self::ColdStart => {
                "times a fresh service/runtime open plus the first representative execution"
            }
            Self::RttSensitive => {
                "compares Postgres loopback against the same service path through a local injected-latency TCP proxy"
            }
        }
    }

    fn warmup_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_PG_BENCH_STEADY_WARMUP_ROUNDS",
                STEADY_STATE_WARMUP_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NEOVEX_PG_BENCH_COLD_WARMUP_ROUNDS",
                COLD_START_WARMUP_ROUNDS,
            ),
            Self::RttSensitive => {
                read_round_override("NEOVEX_PG_BENCH_RTT_WARMUP_ROUNDS", RTT_WARMUP_ROUNDS)
            }
        }
    }

    fn measure_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_PG_BENCH_STEADY_MEASURE_ROUNDS",
                STEADY_STATE_MEASURE_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NEOVEX_PG_BENCH_COLD_MEASURE_ROUNDS",
                COLD_START_MEASURE_ROUNDS,
            ),
            Self::RttSensitive => {
                read_round_override("NEOVEX_PG_BENCH_RTT_MEASURE_ROUNDS", RTT_MEASURE_ROUNDS)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeasuredBackend {
    Sqlite,
    PostgresLoopback,
    PostgresInjectedRtt,
}

impl MeasuredBackend {
    fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::PostgresLoopback => "postgres (loopback)",
            Self::PostgresInjectedRtt => "postgres (injected RTT)",
        }
    }
}

#[derive(Debug, Default)]
struct BenchmarkReport {
    measurements: Vec<WorkloadMeasurement>,
    pool_pressure: Option<PoolPressureObservation>,
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

#[derive(Debug, Clone)]
struct PoolPressureObservation {
    sample_count: usize,
    max_backends_observed: i64,
    mean_sample_latency: Duration,
    median_sample_latency: Duration,
    p95_sample_latency: Duration,
    configured_max_connections: usize,
    concurrent_tasks: usize,
}

#[derive(Clone)]
struct TenantFixture {
    resource: LiveResource,
    service: Arc<Service>,
    tenant_id: TenantId,
}

#[derive(Clone)]
struct QueryFixture {
    tenant: TenantFixture,
    query: Query,
}

#[derive(Clone)]
struct PointReadFixture {
    tenant: TenantFixture,
    ids: Vec<DocumentId>,
}

struct SubscriptionFixture {
    tenant: TenantFixture,
    registrations: Vec<SubscriptionRegistration>,
    receivers: Vec<mpsc::Receiver<SubscriptionUpdate>>,
}

#[derive(Clone)]
struct MixedLoadFixture {
    resource: LiveResource,
    service: Arc<Service>,
    tenant_states: Vec<TenantState>,
}

#[derive(Clone)]
struct TenantLifecycleFixture {
    creator_resource: LiveResource,
    creator_service: Arc<Service>,
    opener_resource: LiveResource,
    opener_service: Arc<Service>,
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
struct JournalSeed {
    resource: SeedResource,
    tenant_id: TenantId,
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

#[derive(Clone)]
enum LiveResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
        data_dir: PathBuf,
    },
    Postgres {
        control_dir: Arc<BenchDir>,
        provider_config: PostgresProviderConfig,
    },
}

#[derive(Clone)]
enum SeedResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
        data_dir: PathBuf,
    },
    Postgres {
        provider_config: PostgresProviderConfig,
    },
}

enum ReopenedResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
    },
    Postgres {
        control_dir: Arc<BenchDir>,
        provider_config: PostgresProviderConfig,
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
            "neovex-postgres-bench-{label}-{}-{counter}",
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

struct BenchmarkEnvironment {
    loopback_connection_string: String,
    injected_rtt_connection_string: String,
    _proxy: LatencyProxy,
}

impl BenchmarkEnvironment {
    async fn new(config: &BenchmarkConfig) -> BenchResult<Self> {
        let proxy = LatencyProxy::new(config.postgres_url.as_str(), config.rtt_delay).await?;
        Ok(Self {
            loopback_connection_string: config.postgres_url.clone(),
            injected_rtt_connection_string: proxy.connection_string().to_string(),
            _proxy: proxy,
        })
    }

    fn connection_string(&self, backend: MeasuredBackend) -> Option<&str> {
        match backend {
            MeasuredBackend::Sqlite => None,
            MeasuredBackend::PostgresLoopback => Some(self.loopback_connection_string.as_str()),
            MeasuredBackend::PostgresInjectedRtt => {
                Some(self.injected_rtt_connection_string.as_str())
            }
        }
    }
}

struct LatencyProxy {
    connection_string: String,
    shutdown_tx: watch::Sender<bool>,
    accept_task: JoinHandle<()>,
}

impl LatencyProxy {
    async fn new(connection_string: &str, one_way_delay: Duration) -> BenchResult<Self> {
        let (target_host, target_port, proxied_connection_string) =
            proxied_connection_string(connection_string)?;
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let local_addr = listener.local_addr()?;
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let accept_task = tokio::spawn(run_latency_proxy(
            listener,
            target_host,
            target_port,
            one_way_delay,
            shutdown_rx,
        ));
        let connection_string = rewrite_connection_string_host_port(
            proxied_connection_string.as_str(),
            IpAddr::from([127, 0, 0, 1]),
            local_addr.port(),
        )?;
        Ok(Self {
            connection_string,
            shutdown_tx,
            accept_task,
        })
    }

    fn connection_string(&self) -> &str {
        &self.connection_string
    }
}

impl Drop for LatencyProxy {
    fn drop(&mut self) {
        let _ = self.shutdown_tx.send(true);
        self.accept_task.abort();
    }
}

async fn run_latency_proxy(
    listener: TcpListener,
    target_host: String,
    target_port: u16,
    one_way_delay: Duration,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    loop {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                if changed.is_ok() && *shutdown_rx.borrow() {
                    break;
                }
            }
            accepted = listener.accept() => {
                let Ok((client_stream, _)) = accepted else {
                    break;
                };
                let target = format!("{target_host}:{target_port}");
                tokio::spawn(async move {
                    let Ok(server_stream) = TcpStream::connect(target).await else {
                        return;
                    };
                    let (mut client_read, mut client_write) = client_stream.into_split();
                    let (mut server_read, mut server_write) = server_stream.into_split();
                    let upstream = tokio::spawn(async move {
                        let _ = copy_with_delay(&mut client_read, &mut server_write, one_way_delay).await;
                    });
                    let downstream = tokio::spawn(async move {
                        let _ = copy_with_delay(&mut server_read, &mut client_write, one_way_delay).await;
                    });
                    let _ = upstream.await;
                    let _ = downstream.await;
                });
            }
        }
    }
}

async fn copy_with_delay<R, W>(
    reader: &mut R,
    writer: &mut W,
    delay: Duration,
) -> std::io::Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            writer.shutdown().await?;
            return Ok(());
        }
        tokio::time::sleep(delay).await;
        writer.write_all(&buffer[..read]).await?;
        writer.flush().await?;
    }
}

async fn run_suite(
    config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
) -> BenchResult<BenchmarkReport> {
    let mut report = BenchmarkReport::default();
    if should_run_workload(config, WorkloadKind::CrudThroughput) {
        benchmark_crud_throughput(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::PointReadLatency) {
        benchmark_point_read_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::IndexedQueryLatency) {
        benchmark_indexed_query_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::CompositeIndexedQueryLatency) {
        benchmark_composite_indexed_query_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::DurableJournalStreamLatency) {
        benchmark_durable_journal_stream_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::DurableJournalBootstrapLatency) {
        benchmark_durable_journal_bootstrap_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::SubscriptionBootstrapCatchupLatency) {
        benchmark_subscription_bootstrap_catchup_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::SubscriptionFanoutLatency) {
        benchmark_subscription_fanout_latency(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::MixedMultiTenantLoad) {
        benchmark_mixed_multi_tenant_load(config, environment, &mut report).await?;
    }
    if should_run_workload(config, WorkloadKind::TenantLifecycleLatency) {
        benchmark_tenant_lifecycle_latency(config, environment, &mut report).await?;
    }
    report.pool_pressure = Some(observe_pool_pressure(environment).await?);
    cleanup_registered_postgres_providers().await;
    Ok(report)
}

fn should_run_workload(config: &BenchmarkConfig, workload: WorkloadKind) -> bool {
    config
        .workload_filter
        .is_none_or(|selected| selected == workload)
}

async fn benchmark_crud_throughput(
    config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::CrudThroughput, async {
        let sqlite_fixture =
            create_crud_fixture("crud-steady", "crud", MeasuredBackend::Sqlite, environment)
                .await?;
        let postgres_fixture = create_crud_fixture(
            "crud-steady",
            "crud",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
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
        postgres_fixture
            .resource
            .cleanup(
                postgres_fixture.service.clone(),
                "CRUD steady-state postgres teardown",
            )
            .await?;

        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
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

        let loopback_fixture = create_crud_fixture(
            "crud-rtt-loopback",
            "crud-rtt",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let rtt_fixture = create_crud_fixture(
            "crud-rtt-injected",
            "crud-rtt",
            MeasuredBackend::PostgresInjectedRtt,
            environment,
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::CrudThroughput,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_fixture = loopback_fixture.clone();
                let rtt_fixture = rtt_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_fixture,
                        MeasuredBackend::PostgresInjectedRtt => rtt_fixture,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_crud_sample(&fixture.service, &fixture.tenant_id, CRUD_RTT_DOCUMENTS)
                        .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        loopback_fixture
            .resource
            .cleanup(
                loopback_fixture.service.clone(),
                "CRUD RTT loopback teardown",
            )
            .await?;
        rtt_fixture
            .resource
            .cleanup(rtt_fixture.service.clone(), "CRUD RTT injected teardown")
            .await?;

        let operations_per_sample = u64::try_from(CRUD_DOCUMENTS * 3)?;
        let rtt_operations_per_sample = u64::try_from(CRUD_RTT_DOCUMENTS * 3)?;
        record_contrast_measurements(
            report,
            WorkloadKind::CrudThroughput,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::CrudThroughput,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::CrudThroughput,
            rtt_operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        let _ = config;
        Ok(())
    })
    .await
}

async fn benchmark_point_read_latency(
    _config: &BenchmarkConfig,
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
        let postgres_fixture = create_point_read_fixture(
            "point-read-steady",
            "point-read",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
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
        postgres_fixture
            .tenant
            .resource
            .cleanup(
                postgres_fixture.tenant.service.clone(),
                "point-read steady-state postgres teardown",
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
            "point-read cold-start sqlite seed freeze",
        )
        .await?;
        let postgres_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-cold-seed",
                "point-read",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "point-read cold-start postgres seed freeze",
        )
        .await?;
        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let postgres_seed = postgres_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::PostgresLoopback => postgres_seed,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("point-read-cold-sample", backend, environment)
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
                    reopened_resource
                        .cleanup(service, "point-read cold-start reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        postgres_seed.resource.cleanup_seed().await?;

        let loopback_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-rtt-loopback-seed",
                "point-read-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "point-read RTT loopback seed freeze",
        )
        .await?;
        let rtt_seed = freeze_point_read_seed(
            create_point_read_fixture(
                "point-read-rtt-injected-seed",
                "point-read-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "point-read RTT injected seed freeze",
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::PointReadLatency,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_seed,
                        MeasuredBackend::PostgresInjectedRtt => rtt_seed,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("point-read-rtt-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_point_read_sample(
                        &service,
                        &seed.tenant_id,
                        &seed.ids,
                        POINT_READ_RTT_BATCH_SIZE,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "point-read RTT reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        loopback_seed.resource.cleanup_seed().await?;
        rtt_seed.resource.cleanup_seed().await?;

        let operations_per_sample = u64::try_from(POINT_READ_BATCH_SIZE)?;
        let rtt_operations_per_sample = u64::try_from(POINT_READ_RTT_BATCH_SIZE)?;
        record_contrast_measurements(
            report,
            WorkloadKind::PointReadLatency,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::PointReadLatency,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::PointReadLatency,
            rtt_operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}

async fn benchmark_indexed_query_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_query_workload(
        report,
        environment,
        WorkloadKind::IndexedQueryLatency,
        || async move {
            create_indexed_query_fixture(
                "indexed-query",
                "indexed-query",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await
        },
        || async move {
            create_indexed_query_fixture(
                "indexed-query",
                "indexed-query",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_indexed_query_fixture(
                "indexed-query-rtt-loopback-seed",
                "indexed-query-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_indexed_query_fixture(
                "indexed-query-rtt-injected-seed",
                "indexed-query-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
    )
    .await
}

async fn benchmark_composite_indexed_query_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_query_workload(
        report,
        environment,
        WorkloadKind::CompositeIndexedQueryLatency,
        || async move {
            create_composite_query_fixture(
                "composite-query",
                "composite-query",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await
        },
        || async move {
            create_composite_query_fixture(
                "composite-query",
                "composite-query",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_composite_query_fixture(
                "composite-query-rtt-loopback-seed",
                "composite-query-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
        || async move {
            create_composite_query_fixture(
                "composite-query-rtt-injected-seed",
                "composite-query-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await
        },
    )
    .await
}

async fn benchmark_query_workload<F1, F2, F3, F4, Fut1, Fut2, Fut3, Fut4>(
    report: &mut BenchmarkReport,
    environment: &BenchmarkEnvironment,
    workload: WorkloadKind,
    sqlite_builder: F1,
    postgres_builder: F2,
    loopback_rtt_builder: F3,
    injected_rtt_builder: F4,
) -> BenchResult<()>
where
    F1: Fn() -> Fut1,
    F2: Fn() -> Fut2,
    F3: Fn() -> Fut3,
    F4: Fn() -> Fut4,
    Fut1: std::future::Future<Output = BenchResult<QueryFixture>>,
    Fut2: std::future::Future<Output = BenchResult<QueryFixture>>,
    Fut3: std::future::Future<Output = BenchResult<QueryFixture>>,
    Fut4: std::future::Future<Output = BenchResult<QueryFixture>>,
{
    run_workload(workload, async move {
        let sqlite_fixture = sqlite_builder().await?;
        let postgres_fixture = postgres_builder().await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            workload,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
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
        postgres_fixture
            .tenant
            .resource
            .cleanup(
                postgres_fixture.tenant.service.clone(),
                "query steady-state postgres teardown",
            )
            .await?;

        let sqlite_seed =
            freeze_query_seed(sqlite_builder().await?, "query cold-start sqlite seed").await?;
        let postgres_seed =
            freeze_query_seed(postgres_builder().await?, "query cold-start postgres seed").await?;
        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            workload,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let postgres_seed = postgres_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::PostgresLoopback => postgres_seed,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("query-cold-sample", backend, environment)
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
                    reopened_resource
                        .cleanup(service, "query cold-start reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        postgres_seed.resource.cleanup_seed().await?;

        let loopback_seed =
            freeze_query_seed(loopback_rtt_builder().await?, "query RTT loopback seed").await?;
        let rtt_seed =
            freeze_query_seed(injected_rtt_builder().await?, "query RTT injected seed").await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            workload,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_seed,
                        MeasuredBackend::PostgresInjectedRtt => rtt_seed,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("query-rtt-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_query_sample(
                        &service,
                        &seed.tenant_id,
                        &seed.query,
                        INDEXED_QUERY_RTT_BATCH_SIZE,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "query RTT reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        loopback_seed.resource.cleanup_seed().await?;
        rtt_seed.resource.cleanup_seed().await?;

        let operations_per_sample = u64::try_from(INDEXED_QUERY_BATCH_SIZE)?;
        let rtt_operations_per_sample = u64::try_from(INDEXED_QUERY_RTT_BATCH_SIZE)?;
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            workload,
            rtt_operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}

async fn benchmark_durable_journal_stream_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_journal_workload(
        report,
        environment,
        WorkloadKind::DurableJournalStreamLatency,
    )
    .await
}

async fn benchmark_durable_journal_bootstrap_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    benchmark_journal_workload(
        report,
        environment,
        WorkloadKind::DurableJournalBootstrapLatency,
    )
    .await
}

async fn benchmark_journal_workload(
    report: &mut BenchmarkReport,
    environment: &BenchmarkEnvironment,
    workload: WorkloadKind,
) -> BenchResult<()> {
    run_workload(workload, async move {
        let sqlite_fixture = create_journal_fixture(
            "journal-steady",
            "journal",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let postgres_fixture = create_journal_fixture(
            "journal-steady",
            "journal",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            workload,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_journal_workload_sample(
                        workload,
                        &fixture.tenant.service,
                        &fixture.tenant.tenant_id,
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
                "journal steady-state sqlite teardown",
            )
            .await?;
        postgres_fixture
            .tenant
            .resource
            .cleanup(
                postgres_fixture.tenant.service.clone(),
                "journal steady-state postgres teardown",
            )
            .await?;

        let sqlite_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-cold-seed",
                "journal",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await?,
            "journal cold-start sqlite seed",
        )
        .await?;
        let postgres_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-cold-seed",
                "journal",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "journal cold-start postgres seed",
        )
        .await?;
        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            workload,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let postgres_seed = postgres_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::PostgresLoopback => postgres_seed,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("journal-cold-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_journal_workload_sample(workload, &service, &seed.tenant_id).await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "journal cold-start reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        postgres_seed.resource.cleanup_seed().await?;

        let loopback_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-rtt-loopback-seed",
                "journal-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "journal RTT loopback seed freeze",
        )
        .await?;
        let rtt_seed = freeze_journal_seed(
            create_journal_fixture(
                "journal-rtt-injected-seed",
                "journal-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "journal RTT injected seed freeze",
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            workload,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_seed,
                        MeasuredBackend::PostgresInjectedRtt => rtt_seed,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("journal-rtt-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_journal_workload_sample(workload, &service, &seed.tenant_id).await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "journal RTT reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        loopback_seed.resource.cleanup_seed().await?;
        rtt_seed.resource.cleanup_seed().await?;

        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::SteadyState,
            1,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            workload,
            BenchmarkLane::ColdStart,
            1,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            workload,
            1,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}

async fn benchmark_subscription_bootstrap_catchup_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::SubscriptionBootstrapCatchupLatency, async {
        let sqlite_fixture = create_tenant_service(
            "subscription-bootstrap-steady",
            "subscription-bootstrap",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let postgres_fixture = create_tenant_service(
            "subscription-bootstrap-steady",
            "subscription-bootstrap",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_subscription_bootstrap_catchup_sample(
                        &fixture.service,
                        &fixture.tenant_id,
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
                "subscription bootstrap steady-state sqlite teardown",
            )
            .await?;
        postgres_fixture
            .resource
            .cleanup(
                postgres_fixture.service.clone(),
                "subscription bootstrap steady-state postgres teardown",
            )
            .await?;

        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| async move {
                let fixture = create_tenant_service(
                    "subscription-bootstrap-cold",
                    "subscription-bootstrap",
                    backend,
                    environment,
                )
                .await?;
                let started = Instant::now();
                exercise_subscription_bootstrap_catchup_sample(
                    &fixture.service,
                    &fixture.tenant_id,
                )
                .await?;
                let elapsed = started.elapsed();
                fixture
                    .resource
                    .cleanup(
                        fixture.service.clone(),
                        "subscription bootstrap cold-start teardown",
                    )
                    .await?;
                Ok(elapsed)
            },
        )
        .await?;

        let loopback_fixture = create_tenant_service(
            "subscription-bootstrap-rtt",
            "subscription-bootstrap-rtt",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let rtt_fixture = create_tenant_service(
            "subscription-bootstrap-rtt",
            "subscription-bootstrap-rtt",
            MeasuredBackend::PostgresInjectedRtt,
            environment,
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_fixture = loopback_fixture.clone();
                let rtt_fixture = rtt_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_fixture,
                        MeasuredBackend::PostgresInjectedRtt => rtt_fixture,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_subscription_bootstrap_catchup_sample(
                        &fixture.service,
                        &fixture.tenant_id,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        loopback_fixture
            .resource
            .cleanup(
                loopback_fixture.service.clone(),
                "subscription bootstrap RTT loopback teardown",
            )
            .await?;
        rtt_fixture
            .resource
            .cleanup(
                rtt_fixture.service.clone(),
                "subscription bootstrap RTT injected teardown",
            )
            .await?;

        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::SteadyState,
            1,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            BenchmarkLane::ColdStart,
            1,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::SubscriptionBootstrapCatchupLatency,
            1,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}

async fn benchmark_subscription_fanout_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::SubscriptionFanoutLatency, async {
        let sqlite_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-steady",
                "subscription-fanout",
                MeasuredBackend::Sqlite,
                environment,
            )
            .await?,
        ));
        let postgres_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-steady",
                "subscription-fanout",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
        ));
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let mut fixture = fixture.lock().await;
                    let service = fixture.tenant.service.clone();
                    let tenant_id = fixture.tenant.tenant_id.clone();
                    let started = Instant::now();
                    exercise_subscription_fanout_sample(
                        &service,
                        &tenant_id,
                        &mut fixture.receivers,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        {
            let fixture = sqlite_fixture.lock().await;
            black_box(fixture.registrations.len());
            fixture
                .tenant
                .resource
                .cleanup(
                    fixture.tenant.service.clone(),
                    "subscription fanout steady-state sqlite teardown",
                )
                .await?;
        }
        {
            let fixture = postgres_fixture.lock().await;
            black_box(fixture.registrations.len());
            fixture
                .tenant
                .resource
                .cleanup(
                    fixture.tenant.service.clone(),
                    "subscription fanout steady-state postgres teardown",
                )
                .await?;
        }

        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| async move {
                let fixture = create_subscription_fixture(
                    "subscription-fanout-cold",
                    "subscription-fanout",
                    backend,
                    environment,
                )
                .await?;
                let mut receivers = fixture.receivers;
                let registrations = fixture.registrations;
                let started = Instant::now();
                exercise_subscription_fanout_sample(
                    &fixture.tenant.service,
                    &fixture.tenant.tenant_id,
                    &mut receivers,
                )
                .await?;
                let elapsed = started.elapsed();
                drop(registrations);
                fixture
                    .tenant
                    .resource
                    .cleanup(
                        fixture.tenant.service.clone(),
                        "subscription fanout cold-start teardown",
                    )
                    .await?;
                Ok(elapsed)
            },
        )
        .await?;

        let loopback_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-rtt",
                "subscription-fanout-rtt",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
        ));
        let rtt_fixture = Arc::new(Mutex::new(
            create_subscription_fixture(
                "subscription-fanout-rtt",
                "subscription-fanout-rtt",
                MeasuredBackend::PostgresInjectedRtt,
                environment,
            )
            .await?,
        ));
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_fixture = loopback_fixture.clone();
                let rtt_fixture = rtt_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_fixture,
                        MeasuredBackend::PostgresInjectedRtt => rtt_fixture,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let mut fixture = fixture.lock().await;
                    let service = fixture.tenant.service.clone();
                    let tenant_id = fixture.tenant.tenant_id.clone();
                    let started = Instant::now();
                    exercise_subscription_fanout_sample(
                        &service,
                        &tenant_id,
                        &mut fixture.receivers,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        {
            let fixture = loopback_fixture.lock().await;
            black_box(fixture.registrations.len());
            fixture
                .tenant
                .resource
                .cleanup(
                    fixture.tenant.service.clone(),
                    "subscription fanout RTT loopback teardown",
                )
                .await?;
        }
        {
            let fixture = rtt_fixture.lock().await;
            black_box(fixture.registrations.len());
            fixture
                .tenant
                .resource
                .cleanup(
                    fixture.tenant.service.clone(),
                    "subscription fanout RTT injected teardown",
                )
                .await?;
        }

        let operations_per_sample = u64::try_from(SUBSCRIPTION_FANOUT_COUNT)?;
        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::SubscriptionFanoutLatency,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::SubscriptionFanoutLatency,
            operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}

async fn benchmark_mixed_multi_tenant_load(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::MixedMultiTenantLoad, async {
        let sqlite_fixture =
            create_mixed_load_fixture("mixed-load-steady", MeasuredBackend::Sqlite, environment)
                .await?;
        let postgres_fixture = create_mixed_load_fixture(
            "mixed-load-steady",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_mixed_load_sample(
                        &fixture.service,
                        &fixture.tenant_states,
                        MIXED_LOAD_TENANTS,
                        MIXED_LOAD_OPS_PER_TENANT,
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
        postgres_fixture
            .resource
            .cleanup(
                postgres_fixture.service.clone(),
                "mixed-load steady-state postgres teardown",
            )
            .await?;

        let sqlite_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture("mixed-load-cold-seed", MeasuredBackend::Sqlite, environment)
                .await?,
            "mixed-load cold-start sqlite seed",
        )
        .await?;
        let postgres_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-cold-seed",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "mixed-load cold-start postgres seed",
        )
        .await?;
        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_seed = sqlite_seed.clone();
                let postgres_seed = postgres_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::Sqlite => sqlite_seed,
                        MeasuredBackend::PostgresLoopback => postgres_seed,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("mixed-load-cold-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_mixed_load_sample(
                        &service,
                        &seed.tenant_states,
                        MIXED_LOAD_TENANTS,
                        MIXED_LOAD_OPS_PER_TENANT,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "mixed-load cold-start reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        sqlite_seed.resource.cleanup_seed().await?;
        postgres_seed.resource.cleanup_seed().await?;

        let loopback_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-rtt-loopback-seed",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "mixed-load RTT loopback seed freeze",
        )
        .await?;
        let rtt_seed = freeze_mixed_load_seed(
            create_mixed_load_fixture(
                "mixed-load-rtt-injected-seed",
                MeasuredBackend::PostgresLoopback,
                environment,
            )
            .await?,
            "mixed-load RTT injected seed freeze",
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_seed = loopback_seed.clone();
                let rtt_seed = rtt_seed.clone();
                async move {
                    let seed = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_seed,
                        MeasuredBackend::PostgresInjectedRtt => rtt_seed,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let (service, reopened_resource) = seed
                        .resource
                        .reopen_service("mixed-load-rtt-sample", backend, environment)
                        .await?;
                    let started = Instant::now();
                    exercise_mixed_load_sample(
                        &service,
                        &seed.tenant_states,
                        MIXED_LOAD_RTT_TENANTS,
                        MIXED_LOAD_RTT_OPS_PER_TENANT,
                    )
                    .await?;
                    let elapsed = started.elapsed();
                    reopened_resource
                        .cleanup(service, "mixed-load RTT reopened teardown")
                        .await?;
                    Ok(elapsed)
                }
            },
        )
        .await?;
        loopback_seed.resource.cleanup_seed().await?;
        rtt_seed.resource.cleanup_seed().await?;

        let operations_per_sample = u64::try_from(MIXED_LOAD_TENANTS * MIXED_LOAD_OPS_PER_TENANT)?;
        let rtt_operations_per_sample =
            u64::try_from(MIXED_LOAD_RTT_TENANTS * MIXED_LOAD_RTT_OPS_PER_TENANT)?;
        record_contrast_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            operations_per_sample,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::ColdStart,
            operations_per_sample,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            rtt_operations_per_sample,
            postgres_loopback_rtt,
            postgres_injected_rtt,
        );
        Ok(())
    })
    .await
}

async fn benchmark_tenant_lifecycle_latency(
    _config: &BenchmarkConfig,
    environment: &BenchmarkEnvironment,
    report: &mut BenchmarkReport,
) -> BenchResult<()> {
    run_workload(WorkloadKind::TenantLifecycleLatency, async {
        let sqlite_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle",
            MeasuredBackend::Sqlite,
            environment,
        )
        .await?;
        let postgres_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let (sqlite_steady, postgres_steady) = measure_two_backends_async(
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::SteadyState,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| {
                let sqlite_fixture = sqlite_fixture.clone();
                let postgres_fixture = postgres_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::Sqlite => sqlite_fixture,
                        MeasuredBackend::PostgresLoopback => postgres_fixture,
                        MeasuredBackend::PostgresInjectedRtt => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_tenant_lifecycle_sample(
                        &fixture.creator_service,
                        &fixture.opener_service,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        sqlite_fixture
            .cleanup("tenant-lifecycle steady-state sqlite teardown")
            .await?;
        postgres_fixture
            .cleanup("tenant-lifecycle steady-state postgres teardown")
            .await?;

        let (sqlite_cold, postgres_cold) = measure_two_backends_async(
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::ColdStart,
            [MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
            |backend| async move {
                let fixture =
                    create_tenant_lifecycle_fixture("tenant-lifecycle-cold", backend, environment)
                        .await?;
                let started = Instant::now();
                exercise_tenant_lifecycle_sample(&fixture.creator_service, &fixture.opener_service)
                    .await?;
                let elapsed = started.elapsed();
                fixture
                    .cleanup("tenant-lifecycle cold-start teardown")
                    .await?;
                Ok(elapsed)
            },
        )
        .await?;
        let loopback_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle-rtt",
            MeasuredBackend::PostgresLoopback,
            environment,
        )
        .await?;
        let rtt_fixture = create_tenant_lifecycle_fixture(
            "tenant-lifecycle-rtt",
            MeasuredBackend::PostgresInjectedRtt,
            environment,
        )
        .await?;
        let (postgres_loopback_rtt, postgres_injected_rtt) = measure_two_backends_async(
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::RttSensitive,
            [
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
            |backend| {
                let loopback_fixture = loopback_fixture.clone();
                let rtt_fixture = rtt_fixture.clone();
                async move {
                    let fixture = match backend {
                        MeasuredBackend::PostgresLoopback => loopback_fixture,
                        MeasuredBackend::PostgresInjectedRtt => rtt_fixture,
                        MeasuredBackend::Sqlite => unreachable!(),
                    };
                    let started = Instant::now();
                    exercise_tenant_lifecycle_sample(
                        &fixture.creator_service,
                        &fixture.opener_service,
                    )
                    .await?;
                    Ok(started.elapsed())
                }
            },
        )
        .await?;
        loopback_fixture
            .cleanup("tenant-lifecycle RTT loopback teardown")
            .await?;
        rtt_fixture
            .cleanup("tenant-lifecycle RTT injected teardown")
            .await?;

        record_contrast_measurements(
            report,
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::SteadyState,
            3,
            sqlite_steady,
            postgres_steady,
        );
        record_contrast_measurements(
            report,
            WorkloadKind::TenantLifecycleLatency,
            BenchmarkLane::ColdStart,
            3,
            sqlite_cold,
            postgres_cold,
        );
        record_rtt_measurements(
            report,
            WorkloadKind::TenantLifecycleLatency,
            3,
            postgres_loopback_rtt,
            postgres_injected_rtt,
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
        MeasuredBackend::PostgresLoopback | MeasuredBackend::PostgresInjectedRtt => {
            let control_dir = Arc::new(BenchDir::new(&format!(
                "{label}-{}",
                backend.label().replace([' ', '(', ')'], "-")
            ))?);
            let provider_config = benchmark_postgres_provider_config(
                label,
                environment
                    .connection_string(backend)
                    .expect("postgres backend should have connection string"),
                Some(1),
                Some(4),
            )?;
            let service = Arc::new(
                Service::new_with_persistence_config(postgres_service_config(
                    control_dir.path(),
                    &provider_config,
                ))
                .await?,
            );
            let tenant_id = benchmark_tenant_id(tenant_label)?;
            service.create_tenant_async(tenant_id.clone()).await?;
            Ok(TenantFixture {
                resource: LiveResource::Postgres {
                    control_dir,
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
    label: &str,
    tenant_label: &str,
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
    label: &str,
    tenant_label: &str,
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

async fn create_journal_fixture(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<QueryFixture> {
    let tenant = create_tenant_service(label, tenant_label, backend, environment).await?;
    for rank in 0..JOURNAL_DOCUMENTS {
        tenant
            .service
            .insert_document_async(
                tenant.tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("open")),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .await?;
    }
    Ok(QueryFixture {
        tenant,
        query: query_for_all(),
    })
}

async fn create_subscription_fixture(
    label: &str,
    tenant_label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<SubscriptionFixture> {
    let tenant = create_tenant_service(label, tenant_label, backend, environment).await?;
    let (registrations, receivers) =
        register_subscription_receivers(&tenant.service, &tenant.tenant_id).await?;
    Ok(SubscriptionFixture {
        tenant,
        registrations,
        receivers,
    })
}

async fn create_mixed_load_fixture(
    label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<MixedLoadFixture> {
    let resource_service = match backend {
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
        MeasuredBackend::PostgresLoopback | MeasuredBackend::PostgresInjectedRtt => {
            let control_dir = Arc::new(BenchDir::new(&format!(
                "{label}-{}",
                backend.label().replace([' ', '(', ')'], "-")
            ))?);
            let provider_config = benchmark_postgres_provider_config(
                label,
                environment
                    .connection_string(backend)
                    .expect("postgres backend should have connection string"),
                Some(1),
                Some(4),
            )?;
            let service = Arc::new(
                Service::new_with_persistence_config(postgres_service_config(
                    control_dir.path(),
                    &provider_config,
                ))
                .await?,
            );
            (
                LiveResource::Postgres {
                    control_dir,
                    provider_config,
                },
                service,
            )
        }
    };
    let (resource, service) = resource_service;
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

async fn create_tenant_lifecycle_fixture(
    label: &str,
    backend: MeasuredBackend,
    environment: &BenchmarkEnvironment,
) -> BenchResult<TenantLifecycleFixture> {
    match backend {
        MeasuredBackend::Sqlite => {
            let bench_dir = Arc::new(BenchDir::new(&format!("{label}-sqlite"))?);
            let data_dir = bench_dir.path().to_path_buf();
            let creator_service = Arc::new(
                Service::new_with_persistence_config(ServicePersistenceConfig::embedded(
                    &data_dir,
                    EmbeddedProviderKind::Sqlite,
                ))
                .await?,
            );
            Ok(TenantLifecycleFixture {
                creator_resource: LiveResource::Sqlite {
                    bench_dir: bench_dir.clone(),
                    data_dir: data_dir.clone(),
                },
                creator_service: creator_service.clone(),
                opener_resource: LiveResource::Sqlite {
                    bench_dir,
                    data_dir,
                },
                opener_service: creator_service,
            })
        }
        MeasuredBackend::PostgresLoopback | MeasuredBackend::PostgresInjectedRtt => {
            let creator_control = Arc::new(BenchDir::new(&format!(
                "{label}-creator-{}",
                backend.label().replace([' ', '(', ')'], "-")
            ))?);
            let opener_control = Arc::new(BenchDir::new(&format!(
                "{label}-opener-{}",
                backend.label().replace([' ', '(', ')'], "-")
            ))?);
            let provider_config = benchmark_postgres_provider_config(
                label,
                environment
                    .connection_string(backend)
                    .expect("postgres backend should have connection string"),
                Some(1),
                Some(4),
            )?;
            let creator_service = Arc::new(
                Service::new_with_persistence_config(postgres_service_config(
                    creator_control.path(),
                    &provider_config,
                ))
                .await?,
            );
            let opener_service = Arc::new(
                Service::new_with_persistence_config(postgres_service_config(
                    opener_control.path(),
                    &provider_config,
                ))
                .await?,
            );
            Ok(TenantLifecycleFixture {
                creator_resource: LiveResource::Postgres {
                    control_dir: creator_control,
                    provider_config: provider_config.clone(),
                },
                creator_service,
                opener_resource: LiveResource::Postgres {
                    control_dir: opener_control,
                    provider_config: provider_config.clone(),
                },
                opener_service,
            })
        }
    }
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

async fn freeze_journal_seed(fixture: QueryFixture, context: &str) -> BenchResult<JournalSeed> {
    let QueryFixture { tenant, .. } = fixture;
    quiesce_service(&tenant.service, context).await?;
    drop(tenant.service);
    Ok(JournalSeed {
        resource: tenant.resource.into_seed_resource(),
        tenant_id: tenant.tenant_id,
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
            Self::Postgres {
                control_dir,
                provider_config,
            } => {
                black_box(control_dir.path());
                terminate_benchmark_postgres_connections(provider_config).await?;
                register_postgres_cleanup(provider_config);
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
            Self::Postgres {
                provider_config, ..
            } => SeedResource::Postgres { provider_config },
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
                    backend.label().replace([' ', '(', ')'], "-")
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
            Self::Postgres { provider_config } => {
                let control_dir = Arc::new(BenchDir::new(&format!(
                    "{label}-{}",
                    backend.label().replace([' ', '(', ')'], "-")
                ))?);
                let mut reopened_config = provider_config.clone();
                reopened_config.connection_string = environment
                    .connection_string(backend)
                    .expect("postgres backend should have connection string")
                    .to_string();
                let service = Arc::new(
                    Service::new_with_persistence_config(postgres_service_config(
                        control_dir.path(),
                        &reopened_config,
                    ))
                    .await?,
                );
                Ok((
                    service,
                    ReopenedResource::Postgres {
                        control_dir,
                        provider_config: reopened_config,
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
            Self::Postgres { provider_config } => {
                terminate_benchmark_postgres_connections(provider_config).await?;
                register_postgres_cleanup(provider_config);
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
            Self::Postgres {
                control_dir,
                provider_config,
            } => {
                terminate_benchmark_postgres_connections(&provider_config).await?;
                drop(control_dir);
            }
        }
        Ok(())
    }
}

impl TenantLifecycleFixture {
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

async fn exercise_journal_workload_sample(
    workload: WorkloadKind,
    service: &Arc<Service>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    match workload {
        WorkloadKind::DurableJournalStreamLatency => {
            exercise_journal_stream_sample(service, tenant_id).await
        }
        WorkloadKind::DurableJournalBootstrapLatency => {
            exercise_journal_bootstrap_sample(service, tenant_id).await
        }
        _ => Err(format!("invalid journal workload: {}", workload.label()).into()),
    }
}

async fn exercise_subscription_bootstrap_catchup_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    let token = format!("bootstrap-{}", BENCH_COUNTER.fetch_add(1, Ordering::SeqCst));
    let query = Query {
        table: tasks_table(),
        filters: vec![filter("topic", FilterOp::Eq, json!(token.clone()))],
        order: None,
        limit: None,
    };
    let (sender, mut receiver) = mpsc::channel(8);
    let registration = service
        .subscribe_async(tenant_id.clone(), query, token.clone(), sender)
        .await?;
    let bootstrap = receiver
        .recv()
        .await
        .ok_or("subscription bootstrap should arrive")?;
    black_box(bootstrap);
    let _ = service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([
                ("topic".to_string(), json!(token)),
                ("title".to_string(), json!("catchup")),
            ]),
        )
        .await?;
    let update = receiver
        .recv()
        .await
        .ok_or("subscription catch-up should arrive")?;
    black_box(update);
    drop(registration);
    Ok(())
}

async fn register_subscription_receivers(
    service: &Arc<Service>,
    tenant_id: &TenantId,
) -> BenchResult<(
    Vec<SubscriptionRegistration>,
    Vec<mpsc::Receiver<SubscriptionUpdate>>,
)> {
    let query = query_for_all();
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
                    BENCH_COUNTER.fetch_add(1, Ordering::SeqCst)
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
                            neovex_core::Error::Internal(format!(
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
                            neovex_core::Error::Internal(format!(
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
                            neovex_core::Error::Internal(format!(
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
                            neovex_core::Error::Internal(format!(
                                "mixed-load update timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
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

async fn exercise_tenant_lifecycle_sample(
    creator_service: &Arc<Service>,
    opener_service: &Arc<Service>,
) -> BenchResult<()> {
    let suffix = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let tenant_id = TenantId::new(format!("bench-tenant-lifecycle-{suffix}"))?;
    creator_service
        .create_tenant_async(tenant_id.clone())
        .await?;
    opener_service
        .ensure_tenant_exists_async(tenant_id.clone())
        .await?;
    creator_service.delete_tenant_async(tenant_id).await?;
    Ok(())
}

async fn observe_pool_pressure(
    environment: &BenchmarkEnvironment,
) -> BenchResult<PoolPressureObservation> {
    let provider_config = benchmark_postgres_provider_config(
        "pool-pressure",
        environment.loopback_connection_string.as_str(),
        Some(1),
        Some(POOL_PRESSURE_MAX_CONNECTIONS),
    )?;
    let control_dir = Arc::new(BenchDir::new("pool-pressure")?);
    let service = Arc::new(
        Service::new_with_persistence_config(postgres_service_config(
            control_dir.path(),
            &provider_config,
        ))
        .await?,
    );
    let application_name = provider_config.derived_pool_application_name()?;
    let (client, connection) =
        tokio_postgres::connect(provider_config.connection_string.as_str(), NoTls).await?;
    let connection_task = tokio::spawn(async move {
        let _ = connection.await;
    });

    let seeded_fixture = create_pool_pressure_fixture(
        service.clone(),
        LiveResource::Postgres {
            control_dir: control_dir.clone(),
            provider_config: provider_config.clone(),
        },
    )
    .await?;
    let (stop_tx, stop_rx) = watch::channel(false);
    let sampler = tokio::spawn(sample_pool_backends(client, application_name, stop_rx));

    let mut samples = Vec::with_capacity(POOL_PRESSURE_SAMPLES);
    for _ in 0..POOL_PRESSURE_SAMPLES {
        let started = Instant::now();
        exercise_pool_pressure_read_sample(
            &seeded_fixture.tenant.service,
            &seeded_fixture.tenant.tenant_id,
            &seeded_fixture.ids,
            POOL_PRESSURE_TASKS,
        )
        .await?;
        samples.push(started.elapsed());
    }

    let _ = stop_tx.send(true);
    let max_backends_observed = sampler
        .await
        .map_err(|error| format!("pool-pressure sampler join failed: {error}"))?
        .map_err(|error| format!("pool-pressure sampler failed: {error}"))?;
    connection_task.abort();
    seeded_fixture
        .tenant
        .resource
        .cleanup(
            seeded_fixture.tenant.service.clone(),
            "pool-pressure observation teardown",
        )
        .await?;

    let stats = SampleStats::from_samples(&samples, 1);
    Ok(PoolPressureObservation {
        sample_count: samples.len(),
        max_backends_observed,
        mean_sample_latency: stats.mean_per_operation,
        median_sample_latency: stats.median_per_operation,
        p95_sample_latency: stats.p95_per_operation,
        configured_max_connections: POOL_PRESSURE_MAX_CONNECTIONS,
        concurrent_tasks: POOL_PRESSURE_TASKS,
    })
}

async fn sample_pool_backends(
    client: tokio_postgres::Client,
    application_name: String,
    mut stop_rx: watch::Receiver<bool>,
) -> Result<i64, neovex_core::Error> {
    let mut max_observed = 0_i64;
    loop {
        if *stop_rx.borrow() {
            break;
        }
        let row = client
            .query_one(
                "SELECT COUNT(*) FROM pg_stat_activity WHERE application_name = $1",
                &[&application_name],
            )
            .await
            .map_err(|error| neovex_core::Error::Internal(error.to_string()))?;
        let count = row.get::<_, i64>(0);
        max_observed = max_observed.max(count);
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(POOL_PRESSURE_SAMPLE_INTERVAL_MS)) => {}
        }
    }
    Ok(max_observed)
}

async fn create_pool_pressure_fixture(
    service: Arc<Service>,
    resource: LiveResource,
) -> BenchResult<PointReadFixture> {
    let tenant_id = TenantId::new("pool-pressure-tenant")?;
    service.create_tenant_async(tenant_id.clone()).await?;
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
                        ("title".to_string(), json!(format!("pool-task-{rank}"))),
                    ]),
                )
                .await?,
        );
    }
    Ok(PointReadFixture {
        tenant: TenantFixture {
            resource,
            service,
            tenant_id,
        },
        ids,
    })
}

async fn exercise_pool_pressure_read_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    ids: &[DocumentId],
    parallel_tasks: usize,
) -> BenchResult<()> {
    let mut handles = Vec::with_capacity(parallel_tasks);
    for task_index in 0..parallel_tasks {
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let ids = ids.to_vec();
        handles.push(tokio::spawn(async move {
            for step in 0..POINT_READ_BATCH_SIZE {
                let id = ids[(task_index * 17 + step) % ids.len()];
                let document = service
                    .get_document_async(tenant_id.clone(), tasks_table(), id)
                    .await?;
                black_box(document);
            }
            Ok::<(), neovex_core::Error>(())
        }));
    }
    for handle in handles {
        handle.await??;
    }
    Ok(())
}

async fn run_workload<Fut>(workload: WorkloadKind, run: Fut) -> BenchResult<()>
where
    Fut: std::future::Future<Output = BenchResult<()>>,
{
    eprintln!("starting {}", workload.label());
    let started = Instant::now();
    let result = run.await;
    eprintln!("finished {} in {:?}", workload.label(), started.elapsed());
    result
}

async fn measure_two_backends_async<B, F, Fut>(
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backends: [B; 2],
    mut run_sample: F,
) -> BenchResult<(Vec<Duration>, Vec<Duration>)>
where
    B: Copy + Eq,
    F: FnMut(B) -> Fut,
    Fut: std::future::Future<Output = BenchResult<Duration>>,
{
    eprintln!("  starting {} lane", lane.label().to_lowercase());
    let started = Instant::now();
    let total_rounds = lane.warmup_rounds() + lane.measure_rounds();
    let mut first = Vec::new();
    let mut second = Vec::new();
    for round in 0..total_rounds {
        let order = if round.is_multiple_of(2) {
            backends
        } else {
            [backends[1], backends[0]]
        };
        for backend in order {
            let sample = run_sample(backend).await?;
            if round >= lane.warmup_rounds() {
                if backend == backends[0] {
                    first.push(sample);
                } else {
                    second.push(sample);
                }
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
    Ok((first, second))
}

async fn quiesce_service(service: &Arc<Service>, context: &str) -> BenchResult<()> {
    match tokio::time::timeout(
        Duration::from_secs(BENCHMARK_QUIESCE_TIMEOUT_SECS),
        service.quiesce(),
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(_) => {
            eprintln!(
                "graceful service quiesce timed out during {context}; falling back to drop-based benchmark teardown"
            );
            Ok(())
        }
    }
}

fn record_contrast_measurements(
    report: &mut BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    operations_per_sample: u64,
    sqlite: Vec<Duration>,
    postgres_loopback: Vec<Duration>,
) {
    report.push_measurement(
        workload,
        lane,
        MeasuredBackend::Sqlite,
        operations_per_sample,
        sqlite,
    );
    report.push_measurement(
        workload,
        lane,
        MeasuredBackend::PostgresLoopback,
        operations_per_sample,
        postgres_loopback,
    );
}

fn record_rtt_measurements(
    report: &mut BenchmarkReport,
    workload: WorkloadKind,
    operations_per_sample: u64,
    postgres_loopback: Vec<Duration>,
    postgres_injected_rtt: Vec<Duration>,
) {
    report.push_measurement(
        workload,
        BenchmarkLane::RttSensitive,
        MeasuredBackend::PostgresLoopback,
        operations_per_sample,
        postgres_loopback,
    );
    report.push_measurement(
        workload,
        BenchmarkLane::RttSensitive,
        MeasuredBackend::PostgresInjectedRtt,
        operations_per_sample,
        postgres_injected_rtt,
    );
}

fn render_markdown(config: &BenchmarkConfig, report: &BenchmarkReport) -> String {
    let workloads = [
        WorkloadKind::CrudThroughput,
        WorkloadKind::PointReadLatency,
        WorkloadKind::IndexedQueryLatency,
        WorkloadKind::CompositeIndexedQueryLatency,
        WorkloadKind::DurableJournalStreamLatency,
        WorkloadKind::DurableJournalBootstrapLatency,
        WorkloadKind::SubscriptionBootstrapCatchupLatency,
        WorkloadKind::SubscriptionFanoutLatency,
        WorkloadKind::MixedMultiTenantLoad,
        WorkloadKind::TenantLifecycleLatency,
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
    markdown.push_str("# Postgres Provider Benchmark Report\n\n");
    markdown.push_str("Generated with:\n\n");
    markdown.push_str("```bash\n");
    markdown.push_str(
        "NEOVEX_BENCH_POSTGRES_URL='<connection-string>' make bench-postgres-provider REPORT=docs/research/postgres-provider-benchmark-report.md\n",
    );
    markdown.push_str("```\n\n");
    markdown.push_str("## Methodology\n\n");
    markdown.push_str(&format!(
        "- steady-state lane compares `sqlite` against `postgres (loopback)` with alternating backend order\n- cold-start lane compares `sqlite` against `postgres (loopback)` and includes fresh service open plus the first representative execution\n- RTT-sensitive lane compares `postgres (loopback)` against `postgres (injected RTT)` using a local TCP proxy that delays each forwarded chunk by `{}`\n- RTT-sensitive lanes use reduced representative sample sizes documented below so network sensitivity stays measurable without turning the readiness gate into an hours-long run\n- steady-state warmup rounds: `{}`; steady-state measured rounds: `{}`\n- cold-start warmup rounds: `{}`; cold-start measured rounds: `{}`\n- RTT warmup rounds: `{}`; RTT measured rounds: `{}`\n- 95% confidence intervals use a two-sided Student-t interval on mean per-operation latency\n",
        format_duration(config.rtt_delay),
        BenchmarkLane::SteadyState.warmup_rounds(),
        BenchmarkLane::SteadyState.measure_rounds(),
        BenchmarkLane::ColdStart.warmup_rounds(),
        BenchmarkLane::ColdStart.measure_rounds(),
        BenchmarkLane::RttSensitive.warmup_rounds(),
        BenchmarkLane::RttSensitive.measure_rounds(),
    ));
    markdown.push('\n');
    markdown.push_str("## Configuration\n\n");
    markdown.push_str(&format!(
        "- CRUD documents per steady/cold sample: `{CRUD_DOCUMENTS}`; RTT sample: `{CRUD_RTT_DOCUMENTS}`\n- point reads per steady/cold sample: `{POINT_READ_BATCH_SIZE}` over `{POINT_READ_DOCUMENTS}` seeded documents; RTT sample: `{POINT_READ_RTT_BATCH_SIZE}`\n- indexed queries per steady/cold sample: `{INDEXED_QUERY_BATCH_SIZE}` over `{INDEXED_QUERY_DOCUMENTS}` seeded documents; RTT sample: `{INDEXED_QUERY_RTT_BATCH_SIZE}`\n- journal dataset size: `{JOURNAL_DOCUMENTS}` writes with stream page limit `{JOURNAL_STREAM_LIMIT}`\n- subscription fan-out count: `{SUBSCRIPTION_FANOUT_COUNT}`\n- mixed-load steady/cold sample: `{MIXED_LOAD_TENANTS}` tenants with `{MIXED_LOAD_OPS_PER_TENANT}` ops per tenant; RTT sample: `{MIXED_LOAD_RTT_TENANTS}` tenants with `{MIXED_LOAD_RTT_OPS_PER_TENANT}` ops per tenant\n- standard Postgres pool config for benchmark fixtures: `min_connections=1`, `max_connections=4`\n- pool-pressure observation: `min_connections=1`, `max_connections={POOL_PRESSURE_MAX_CONNECTIONS}`, `{POOL_PRESSURE_TASKS}` concurrent workers running pure point reads\n- notification model assumption: one additional Postgres listener connection per live service process, outside the measured pool\n- control-plane assumption: tenant persistence may be Postgres-backed while the global usage/control path remains local redb\n",
    ));
    if workloads.contains(&WorkloadKind::TenantLifecycleLatency) {
        markdown.push_str(
            "- tenant-lifecycle sqlite contrast uses same-service open verification because the embedded redb control plane is single-open within one process; the Postgres lane uses a distinct peer service\n",
        );
    }
    if let Some(path) = &config.markdown_output {
        markdown.push_str(&format!("- report path: `{}`\n", path.display()));
    }
    if let Some(workload) = config.workload_filter {
        markdown.push_str(&format!("- workload filter: `{}`\n", workload.label()));
    }
    markdown.push('\n');

    if !workloads.is_empty() {
        let mut overall_postgres_wins = 0;
        let mut overall_sqlite_wins = 0;
        markdown.push_str("## SQLite Contrast Scorecard\n\n");
        markdown.push_str(
            "Winner is determined by higher median ops/s, which is equivalent here to lower median per-op latency.\n\n",
        );
        for lane in [BenchmarkLane::SteadyState, BenchmarkLane::ColdStart] {
            let mut postgres_wins = 0;
            let mut sqlite_wins = 0;
            markdown.push_str(&format!("### {} summary\n\n", lane.label()));
            markdown.push_str("| Workload | Postgres vs sqlite | Winner |\n");
            markdown.push_str("| --- | ---: | --- |\n");
            for workload in &workloads {
                let sqlite = measurement_for(report, *workload, lane, MeasuredBackend::Sqlite);
                let postgres =
                    measurement_for(report, *workload, lane, MeasuredBackend::PostgresLoopback);
                let ratio = postgres.stats().median_operations_per_second
                    / sqlite.stats().median_operations_per_second;
                let winner = if ratio > 1.0 {
                    postgres_wins += 1;
                    overall_postgres_wins += 1;
                    "postgres"
                } else if ratio < 1.0 {
                    sqlite_wins += 1;
                    overall_sqlite_wins += 1;
                    "sqlite"
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
                "| Total lanes won | postgres {}, sqlite {} | {} |\n\n",
                postgres_wins,
                sqlite_wins,
                overall_contrast_winner_label(postgres_wins, sqlite_wins)
            ));
        }
        markdown.push_str("### Overall total\n\n");
        markdown.push_str("| Scope | Postgres lanes won | sqlite lanes won | Overall winner |\n");
        markdown.push_str("| --- | ---: | ---: | --- |\n");
        markdown.push_str(&format!(
            "| Loopback contrast lanes | {} | {} | {} |\n\n",
            overall_postgres_wins,
            overall_sqlite_wins,
            overall_contrast_winner_label(overall_postgres_wins, overall_sqlite_wins)
        ));
    }

    markdown.push_str("## RTT Sensitivity Scorecard\n\n");
    markdown.push_str("| Workload | Injected RTT vs loopback latency | Interpretation |\n");
    markdown.push_str("| --- | ---: | --- |\n");
    for workload in &workloads {
        let loopback = measurement_for(
            report,
            *workload,
            BenchmarkLane::RttSensitive,
            MeasuredBackend::PostgresLoopback,
        );
        let injected = measurement_for(
            report,
            *workload,
            BenchmarkLane::RttSensitive,
            MeasuredBackend::PostgresInjectedRtt,
        );
        let inflation = injected.stats().median_per_operation.as_secs_f64()
            / loopback
                .stats()
                .median_per_operation
                .as_secs_f64()
                .max(f64::MIN_POSITIVE);
        markdown.push_str(&format!(
            "| {} | {:.2}x | {} |\n",
            workload.label(),
            inflation,
            if inflation > 1.0 {
                "higher is worse; this is the steady-state sensitivity to non-zero RTT"
            } else {
                "at or below parity in this proxy setup"
            }
        ));
    }
    markdown.push('\n');

    for workload in workloads {
        markdown.push_str(&format!("## {}\n\n", workload.label()));
        markdown.push_str(&format!("{}\n\n", workload.notes()));
        render_lane_table(
            &mut markdown,
            report,
            workload,
            BenchmarkLane::SteadyState,
            &[MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
        );
        render_lane_table(
            &mut markdown,
            report,
            workload,
            BenchmarkLane::ColdStart,
            &[MeasuredBackend::Sqlite, MeasuredBackend::PostgresLoopback],
        );
        render_lane_table(
            &mut markdown,
            report,
            workload,
            BenchmarkLane::RttSensitive,
            &[
                MeasuredBackend::PostgresLoopback,
                MeasuredBackend::PostgresInjectedRtt,
            ],
        );
    }

    if let Some(pool_pressure) = &report.pool_pressure {
        markdown.push_str("## Pool Pressure Observation\n\n");
        markdown.push_str(
            "This observation intentionally constrains the Postgres provider pool to expose head-of-line behavior and verify that active pooled backends remain bounded.\n\n",
        );
        markdown.push_str("| Samples | Max pooled backends observed | Configured max connections | Concurrent workers | Median sample latency | P95 sample latency | Mean sample latency |\n");
        markdown.push_str("| ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {} |\n\n",
            pool_pressure.sample_count,
            pool_pressure.max_backends_observed,
            pool_pressure.configured_max_connections,
            pool_pressure.concurrent_tasks,
            format_duration(pool_pressure.median_sample_latency),
            format_duration(pool_pressure.p95_sample_latency),
            format_duration(pool_pressure.mean_sample_latency),
        ));
        if let Some(steady_mixed) = maybe_measurement_for(
            report,
            WorkloadKind::MixedMultiTenantLoad,
            BenchmarkLane::SteadyState,
            MeasuredBackend::PostgresLoopback,
        ) {
            let inflation = pool_pressure.median_sample_latency.as_secs_f64()
                / steady_mixed
                    .stats()
                    .median_per_operation
                    .as_secs_f64()
                    .max(f64::MIN_POSITIVE);
            markdown.push_str(&format!(
                "Relative to the unconstrained steady-state Postgres mixed-load lane, the bounded-pool observation shows `{:.2}x` higher median end-to-end sample latency while pooled backend count remained capped at `{}`.\n\n",
                inflation,
                pool_pressure.max_backends_observed,
            ));
        }
    }

    markdown.push_str("## Operator Assumptions\n\n");
    markdown.push_str(
        "- Postgres tenant persistence is benchmarked with the global usage/control path still local and redb-backed.\n- The service-path benchmark includes provider-owned pooling, typed construction, scheduler/journal semantics, and the provider hint-listener wake path, but notifications remain wake hints rather than the authoritative journal contract.\n- Companion operational drills for reconnect recovery, restart recovery, transient backend termination, unloaded-tenant scheduler wake, and tenant cleanup are covered by focused storage/engine verification and recorded in `/Users/jack/src/github.com/agentstation/neovex/docs/plans/archive/postgres-storage-provider-plan.md`.\n",
    );

    markdown
}

fn render_lane_table(
    markdown: &mut String,
    report: &BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backends: &[MeasuredBackend],
) {
    markdown.push_str(&format!("### {} lane\n\n", lane.label()));
    markdown.push_str(&format!("{}\n\n", lane.notes()));
    markdown.push_str(
        "| Backend | Samples | Median per op | P95 per op | Mean per op | Stddev per op | CV | 95% CI of mean | Median ops/s |\n",
    );
    markdown.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- | ---: |\n");
    for backend in backends {
        let measurement = measurement_for(report, workload, lane, *backend);
        let stats = measurement.stats();
        markdown.push_str(&format!(
            "| {} | {} | {} | {} | {} | {} | {:.2}% | {} | {:.2} |\n",
            backend.label(),
            stats.sample_count,
            format_duration(stats.median_per_operation),
            format_duration(stats.p95_per_operation),
            format_duration(stats.mean_per_operation),
            format_duration(stats.stddev_per_operation),
            stats.cv_percent,
            format_confidence_interval(
                stats.ci95_low_per_operation,
                stats.ci95_high_per_operation,
            ),
            stats.median_operations_per_second,
        ));
    }
    markdown.push('\n');
    if backends.len() == 2 {
        let left = measurement_for(report, workload, lane, backends[0]);
        let right = measurement_for(report, workload, lane, backends[1]);
        let left_stats = left.stats();
        let right_stats = right.stats();
        markdown.push_str(&format!(
            "{} vs {} on the {} lane: `{:.2}x` median ops/s, `{:.2}x` median per-op latency\n\n",
            right.backend.label(),
            left.backend.label(),
            lane.label().to_lowercase(),
            right_stats.median_operations_per_second / left_stats.median_operations_per_second,
            duration_ratio(
                left_stats.median_per_operation,
                right_stats.median_per_operation
            ),
        ));
    }
}

fn measurement_for(
    report: &BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backend: MeasuredBackend,
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

fn maybe_measurement_for(
    report: &BenchmarkReport,
    workload: WorkloadKind,
    lane: BenchmarkLane,
    backend: MeasuredBackend,
) -> Option<&WorkloadMeasurement> {
    report.measurements.iter().find(|measurement| {
        measurement.workload == workload
            && measurement.lane == lane
            && measurement.backend == backend
    })
}

fn overall_contrast_winner_label(postgres_wins: usize, sqlite_wins: usize) -> &'static str {
    use std::cmp::Ordering::*;

    match postgres_wins.cmp(&sqlite_wins) {
        Greater => "postgres",
        Less => "sqlite",
        Equal => "tie",
    }
}

async fn cleanup_postgres_provider(config: &PostgresProviderConfig) -> BenchResult<()> {
    terminate_benchmark_postgres_connections(config).await?;
    PostgresProvider::connect(config.clone())
        .await?
        .drop_metadata_schema_for_test()
        .await?;
    Ok(())
}

async fn terminate_benchmark_postgres_connections(
    config: &PostgresProviderConfig,
) -> BenchResult<()> {
    let pool_application_name = config.derived_pool_application_name()?;
    let notification_application_name = config.derived_notification_channel_name()?;
    let (client, connection) =
        tokio_postgres::connect(config.connection_string.as_str(), NoTls).await?;
    let connection_task = tokio::spawn(async move {
        let _ = connection.await;
    });
    client
        .execute(
            "SELECT pg_terminate_backend(pid)
             FROM pg_stat_activity
             WHERE pid <> pg_backend_pid()
               AND (application_name = $1 OR application_name = $2)",
            &[&pool_application_name, &notification_application_name],
        )
        .await?;
    connection_task.abort();
    Ok(())
}

fn register_postgres_cleanup(config: &PostgresProviderConfig) {
    let queue = POSTGRES_CLEANUP_QUEUE.get_or_init(|| StdMutex::new(Vec::new()));
    queue
        .lock()
        .expect("cleanup queue lock should not be poisoned")
        .push(config.clone());
}

async fn cleanup_registered_postgres_providers() {
    let Some(queue) = POSTGRES_CLEANUP_QUEUE.get() else {
        return;
    };
    let mut drained = {
        let mut configs = queue
            .lock()
            .expect("cleanup queue lock should not be poisoned");
        std::mem::take(&mut *configs)
    };

    if drained.is_empty() {
        return;
    }

    drained.sort_by(|left, right| left.metadata_schema.cmp(&right.metadata_schema));
    drained.dedup_by(|left, right| left.metadata_schema == right.metadata_schema);

    tokio::time::sleep(Duration::from_millis(250)).await;
    for config in drained {
        if let Err(error) = cleanup_postgres_provider(&config).await {
            eprintln!(
                "warning: failed to drop benchmark metadata schema {}: {error}",
                config.metadata_schema
            );
        }
    }
}

fn postgres_service_config(
    control_dir: &Path,
    provider_config: &PostgresProviderConfig,
) -> ServicePersistenceConfig {
    ServicePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            dialect: PersistenceDialect::Postgres,
            topology: PersistenceTopology::ExternalPrimary,
            routing: TenantRoutingConfig::SchemaPerTenant {
                metadata_schema: provider_config.metadata_schema.clone(),
                tenant_schema_prefix: provider_config.tenant_schema_prefix.clone(),
            },
            pool: PoolConfig {
                min_connections: provider_config.min_connections,
                max_connections: provider_config.max_connections,
            },
            credentials: ProviderCredentials::ConnectionString(
                provider_config.connection_string.clone(),
            ),
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_dir),
    }
}

fn benchmark_postgres_provider_config(
    label: &str,
    connection_string: &str,
    min_connections: Option<usize>,
    max_connections: Option<usize>,
) -> BenchResult<PostgresProviderConfig> {
    let counter = BENCH_COUNTER.fetch_add(1, Ordering::SeqCst);
    let label_slug = label
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect::<String>()
        .to_lowercase();
    let metadata_schema = format!("nvx_{}_{}_{counter:x}", label_slug, std::process::id());
    let prefix_base = format!("t_{}_{}_{counter:x}_", label_slug, std::process::id());
    let tenant_schema_prefix = prefix_base.chars().take(24).collect::<String>();
    Ok(PostgresProviderConfig {
        connection_string: connection_string.to_string(),
        metadata_schema,
        tenant_schema_prefix,
        min_connections,
        max_connections,
    })
}

fn proxied_connection_string(connection_string: &str) -> BenchResult<(String, u16, String)> {
    let config = PostgresConfig::from_str(connection_string)?;
    let host = config
        .get_hosts()
        .first()
        .ok_or("Postgres benchmark connection string must specify an explicit TCP host")?;
    let host = match host {
        Host::Tcp(host) => host.clone(),
        #[cfg(unix)]
        Host::Unix(_) => return Err(
            "RTT-sensitive Postgres benchmarks require a TCP host; unix sockets are not supported"
                .into(),
        ),
    };
    let port = config.get_ports().first().copied().unwrap_or(5432);
    Ok((host, port, connection_string.to_string()))
}

fn rewrite_connection_string_host_port(
    connection_string: &str,
    host: IpAddr,
    port: u16,
) -> BenchResult<String> {
    let config = PostgresConfig::from_str(connection_string)?;
    let mut parts = Vec::new();
    if let Some(user) = config.get_user() {
        parts.push(format!("user={}", quote_connection_value(user)));
    }
    if let Some(password) = config.get_password() {
        parts.push(format!(
            "password={}",
            quote_connection_value(String::from_utf8_lossy(password).as_ref())
        ));
    }
    if let Some(dbname) = config.get_dbname() {
        parts.push(format!("dbname={}", quote_connection_value(dbname)));
    }
    if let Some(options) = config.get_options() {
        parts.push(format!("options={}", quote_connection_value(options)));
    }
    if let Some(application_name) = config.get_application_name() {
        parts.push(format!(
            "application_name={}",
            quote_connection_value(application_name)
        ));
    }
    parts.push(format!(
        "sslmode={}",
        match config.get_ssl_mode() {
            tokio_postgres::config::SslMode::Disable => "disable",
            tokio_postgres::config::SslMode::Prefer => "prefer",
            tokio_postgres::config::SslMode::Require => "require",
            _ => "prefer",
        }
    ));
    parts.push(format!("host={host}"));
    parts.push(format!("port={port}"));
    Ok(parts.join(" "))
}

fn quote_connection_value(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('\'', "\\'");
    format!("'{escaped}'")
}

fn benchmark_tenant_id(label: &str) -> BenchResult<TenantId> {
    Ok(TenantId::new(format!("bench-{label}"))?)
}

fn tasks_table() -> TableName {
    TableName::new("tasks").expect("static table name should be valid")
}

fn query_for_all() -> Query {
    Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: None,
        limit: None,
    }
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

fn read_round_override(env_key: &str, default: usize) -> usize {
    env::var(env_key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

fn read_u64_override(env_key: &str, default: u64) -> u64 {
    env::var(env_key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
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
