use std::env;
use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use nimbus_core::{
    DocumentId, FieldSchema, FieldType, Filter, FilterOp, IndexDefinition, OrderBy, OrderDirection,
    Query, SequenceNumber, TableName, TableSchema, TenantId,
};
use nimbus_engine::{
    EmbeddedProviderKind, LocalEncryptionConfig, LocalKeyProviderConfig, MasterKeyFileConfig,
    Service, ServicePersistenceConfig, SubscriptionRegistration, SubscriptionUpdate,
};
use nimbus_storage::{
    LocalKeySubject, ManifestCipher, MasterKeyFileProvider, resolve_database_encryption_key,
    sqlite_index_scan_composite_range_query_sql, sqlite_index_scan_prefix_query_sql,
};
use rusqlite::{Connection, params};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};

type BenchResult<T> = Result<T, Box<dyn std::error::Error>>;

#[path = "provider_bench/common.rs"]
mod common;

#[path = "embedded_provider_benchmarks/config.rs"]
mod config;
#[path = "embedded_provider_benchmarks/fixtures.rs"]
mod fixtures;
#[path = "embedded_provider_benchmarks/models.rs"]
mod models;
#[path = "embedded_provider_benchmarks/report.rs"]
mod report;
#[path = "embedded_provider_benchmarks/scenarios.rs"]
mod scenarios;
#[path = "embedded_provider_benchmarks/suite.rs"]
mod suite;
#[path = "embedded_provider_benchmarks/support.rs"]
mod support;
#[path = "embedded_provider_benchmarks/workloads.rs"]
mod workloads;

use config::BenchmarkConfig;
use report::render_markdown;
use suite::run_suite;
use support::configure_encryption_mode;

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
    configure_encryption_mode(config.encryption_mode)?;
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
