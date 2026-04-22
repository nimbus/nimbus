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
    ControlPlaneConfig, EmbeddedProviderKind, LocalEncryptionConfig, LocalKeyProviderConfig,
    MasterKeyFileConfig, PersistenceDialect, PersistenceTopology, PoolConfig, ProviderCredentials,
    Service, ServicePersistenceConfig, TenantProviderConfig, TenantRoutingConfig,
};
use neovex_storage::{LibsqlReplicaProvider, LibsqlReplicaProviderConfig};
use serde_json::json;

type BenchResult<T> = Result<T, Box<dyn std::error::Error>>;

#[path = "provider_bench/common.rs"]
mod common;

#[path = "libsql_replica_provider_benchmarks/config.rs"]
mod config;
#[path = "libsql_replica_provider_benchmarks/fixtures.rs"]
mod fixtures;
#[path = "libsql_replica_provider_benchmarks/models.rs"]
mod models;
#[path = "libsql_replica_provider_benchmarks/report.rs"]
mod report;
#[path = "libsql_replica_provider_benchmarks/scenarios.rs"]
mod scenarios;
#[path = "libsql_replica_provider_benchmarks/suite.rs"]
mod suite;
#[path = "libsql_replica_provider_benchmarks/support.rs"]
mod support;
#[path = "libsql_replica_provider_benchmarks/workloads.rs"]
mod workloads;

use config::{BenchmarkConfig, BenchmarkEnvironment};
use report::render_markdown;
use suite::run_suite;
use support::configure_local_cache_encryption;

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
    configure_local_cache_encryption(config.local_cache_encryption)?;
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
