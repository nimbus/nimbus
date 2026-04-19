use std::env;
use std::fs;
use std::hint::black_box;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::time::{Duration, Instant};

use mysql_async::prelude::Queryable;
use mysql_async::{Opts, Pool};
use neovex_core::{
    DocumentId, FieldSchema, FieldType, Filter, FilterOp, IndexDefinition, OrderBy, OrderDirection,
    Query, SequenceNumber, TableName, TableSchema, TenantId,
};
use neovex_engine::{
    ControlPlaneConfig, EmbeddedProviderKind, PersistenceDialect, PersistenceTopology, PoolConfig,
    ProviderCredentials, Service, ServicePersistenceConfig, SubscriptionRegistration,
    SubscriptionUpdate, TenantProviderConfig, TenantRoutingConfig,
};
use neovex_storage::{MySqlProvider, MySqlProviderConfig};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{Mutex, mpsc, watch};
use tokio::task::JoinHandle;

type BenchResult<T> = Result<T, Box<dyn std::error::Error>>;

#[path = "provider_bench/common.rs"]
mod common;

use common::*;

#[path = "mysql_provider_benchmarks/report.rs"]
mod report;
#[path = "mysql_provider_benchmarks/scenarios.rs"]
mod scenarios;
#[path = "mysql_provider_benchmarks/suite.rs"]
mod suite;
#[path = "mysql_provider_benchmarks/support.rs"]
mod support;

use report::render_markdown;
use scenarios::*;
use suite::run_suite;
use support::*;

#[path = "mysql_provider_benchmarks/workloads.rs"]
mod workloads;

use workloads::*;

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
const BENCHMARK_QUIESCE_TIMEOUT_SECS: u64 = 30;
const BENCHMARK_MYSQL_CLEANUP_TIMEOUT_SECS: u64 = 30;
const MIXED_LOAD_SAMPLE_TIMEOUT_SECS: u64 = 120;
const BENCHMARK_REOPEN_TIMEOUT_SECS: u64 = 30;
const BENCH_MYSQL_URL_ENV: &str = "NEOVEX_BENCH_MYSQL_URL";
const MYSQL_URL_ENV: &str = "NEOVEX_MYSQL_URL";

static BENCH_COUNTER: AtomicU64 = AtomicU64::new(1);
static MYSQL_CLEANUP_QUEUE: OnceLock<StdMutex<Vec<MySqlProviderConfig>>> = OnceLock::new();

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
    mysql_url: String,
    rtt_delay: Duration,
}

impl BenchmarkConfig {
    fn from_args() -> BenchResult<Self> {
        let mut markdown_output = None;
        let mut workload_filter = None;
        let mut mysql_url = env::var(MYSQL_URL_ENV)
            .ok()
            .or_else(|| env::var(BENCH_MYSQL_URL_ENV).ok());
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
                "--mysql-url" => {
                    let Some(url) = args.next() else {
                        return Err("expected a connection string after --mysql-url".into());
                    };
                    mysql_url = Some(url);
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

        let Some(mysql_url) = mysql_url else {
            return Err(format!(
                "set {MYSQL_URL_ENV} or pass --mysql-url for the benchmark target"
            )
            .into());
        };

        Ok(Self {
            markdown_output,
            workload_filter,
            mysql_url,
            rtt_delay,
        })
    }
}

fn print_usage() {
    println!(
        "Usage: cargo bench -p neovex-engine --bench mysql-provider-benchmarks -- [--markdown <path>] [--workload <slug>] [--mysql-url <connection-string>] [--rtt-ms <delay>]"
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
                "compares MySQL loopback against the same service path through a local injected-latency TCP proxy"
            }
        }
    }

    fn warmup_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_MYSQL_BENCH_STEADY_WARMUP_ROUNDS",
                STEADY_STATE_WARMUP_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NEOVEX_MYSQL_BENCH_COLD_WARMUP_ROUNDS",
                COLD_START_WARMUP_ROUNDS,
            ),
            Self::RttSensitive => {
                read_round_override("NEOVEX_MYSQL_BENCH_RTT_WARMUP_ROUNDS", RTT_WARMUP_ROUNDS)
            }
        }
    }

    fn measure_rounds(self) -> usize {
        match self {
            Self::SteadyState => read_round_override(
                "NEOVEX_MYSQL_BENCH_STEADY_MEASURE_ROUNDS",
                STEADY_STATE_MEASURE_ROUNDS,
            ),
            Self::ColdStart => read_round_override(
                "NEOVEX_MYSQL_BENCH_COLD_MEASURE_ROUNDS",
                COLD_START_MEASURE_ROUNDS,
            ),
            Self::RttSensitive => {
                read_round_override("NEOVEX_MYSQL_BENCH_RTT_MEASURE_ROUNDS", RTT_MEASURE_ROUNDS)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MeasuredBackend {
    Sqlite,
    MySqlLoopback,
    MySqlInjectedRtt,
}

impl MeasuredBackend {
    fn label(self) -> &'static str {
        match self {
            Self::Sqlite => "sqlite",
            Self::MySqlLoopback => "mysql (loopback)",
            Self::MySqlInjectedRtt => "mysql (injected RTT)",
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
    max_active_threads_observed: i64,
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
    MySql {
        control_dir: Arc<BenchDir>,
        provider_config: MySqlProviderConfig,
    },
}

#[derive(Clone)]
enum SeedResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
        data_dir: PathBuf,
    },
    MySql {
        provider_config: MySqlProviderConfig,
    },
}

enum ReopenedResource {
    Sqlite {
        bench_dir: Arc<BenchDir>,
    },
    MySql {
        control_dir: Arc<BenchDir>,
        provider_config: MySqlProviderConfig,
    },
}
