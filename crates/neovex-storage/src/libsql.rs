use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::task::{Context, Poll};
use std::time::Instant;

use hyper::client::HttpConnector;
use hyper::client::connect::{Connected, Connection as HyperConnection};
use libsql::{Builder, Connection, Database, Transaction, TransactionBehavior};
use native_tls::TlsConnector as NativeTlsConnector;
use neovex_core::{
    CommitEntry, CronJob, Document, DocumentId, DurableMutationRecord, Error, Result, ScheduledJob,
    ScheduledJobResult, Schema, SequenceNumber, StorageErrorKind, TableName, TableSchema, TenantId,
    Timestamp, WriteOp, WriteOpType,
};
use reqwest::Client as HttpClient;
use reqwest::header::AUTHORIZATION;
use rusqlite::{Connection as LocalSqliteConnection, params};
use serde::Serialize;
use serde::de::DeserializeOwned;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::{Notify, Semaphore};
use tokio_native_tls::{TlsConnector as TokioTlsConnector, TlsStream};
use tower_service::Service;
use tracing::{debug, warn};

use crate::async_storage::{TenantReadStorage, TenantWriteOutcome, TenantWriteStorage};
use crate::commit_log::{deserialize_durable_record, serialize_commit, serialize_durable_record};
use crate::runtime_bridge::{bridge_tokio_runtime, bridge_tokio_runtime_local};
use crate::simulation::{Clock, FaultInjector, NoopFaultInjector, SystemClock};
use crate::sqlite::{
    SQLITE_INIT_SQL, SqliteReadSnapshot, SqliteTenantStore,
    rebuild_sqlite_indexes_from_loaded_schema,
};
use crate::store::{
    APPLIED_SEQUENCE_KEY, DurableJournalBootstrap, DurableJournalPage, JournalProgress,
    MAX_DURABLE_JOURNAL_STREAM_LIMIT, NEXT_SEQUENCE_KEY, ResolvedScheduleOp, ResolvedWrite,
    TenantWriteCommit,
};

mod provider;
mod read;
mod remote;
mod storage;
mod transport;
mod write;

use self::remote::{
    bootstrap_tenant_namespace, clear_tenant_namespace, drop_remote_namespace,
    ensure_remote_namespace_exists, fetch_remote_namespace_snapshot,
    materialize_snapshot_to_replica_cache, open_remote_database, query_remote_schema_rows,
    remove_sqlite_artifacts, tenant_namespace_has_foundation, tenant_namespace_name,
    validate_namespace_input,
};
pub use self::transport::{
    LibsqlTransportConnector, LibsqlTransportStream, libsql_transport_connector,
};

const LIBSQL_NAMESPACE_LIMIT: usize = 63;
const TARGET_TENANT_HASH_HEX_LEN: usize = 40;
const MIN_TENANT_HASH_HEX_LEN: usize = 16;
const LIBSQL_TENANT_READ_PARALLELISM: usize = 4;
const LIBSQL_TENANT_WRITE_PARALLELISM: usize = 1;
const LIBSQL_REPLICA_FILENAME: &str = "tenant.sqlite3";
const LIBSQL_DROP_TENANT_SQL: &str = r#"
DROP TABLE IF EXISTS documents;
DROP TABLE IF EXISTS schemas;
DROP TABLE IF EXISTS scheduled_jobs;
DROP TABLE IF EXISTS running_scheduled_jobs;
DROP TABLE IF EXISTS scheduled_job_results;
DROP TABLE IF EXISTS scheduled_job_executions;
DROP TABLE IF EXISTS cron_jobs;
DROP TABLE IF EXISTS commit_log;
DROP TABLE IF EXISTS metadata;
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibsqlReplicaBarrierPath {
    Unknown,
    AlreadyCurrentCache,
    WaitedForBackgroundRefresh,
    IncrementalCatchUp,
    FullSnapshotRebuild,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibsqlReplicaRefreshCause {
    Unknown,
    CommitBarrier,
    DurableJournalReplay,
    SchemaMismatch,
    SchemaWrite,
    BootstrapExport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LibsqlReplicaRefreshPath {
    Unknown,
    IncrementalCatchUp,
    FullSnapshotRebuild,
    IncrementalFallbackToSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibsqlReplicaFreshnessStats {
    pub required_sequence: SequenceNumber,
    pub local_durable_head: SequenceNumber,
    pub local_applied_sequence: SequenceNumber,
    pub refresh_needed: bool,
    pub refresh_requested: bool,
    pub refresh_inflight: bool,
    pub last_barrier_path: LibsqlReplicaBarrierPath,
    pub barrier_current_count: u64,
    pub barrier_waited_for_background_refresh_count: u64,
    pub barrier_incremental_catch_up_count: u64,
    pub barrier_full_snapshot_rebuild_count: u64,
    pub last_refresh_cause: LibsqlReplicaRefreshCause,
    pub last_refresh_path: LibsqlReplicaRefreshPath,
    pub incremental_refresh_count: u64,
    pub full_snapshot_refresh_count: u64,
    pub incremental_fallback_to_snapshot_count: u64,
    pub refresh_error_count: u64,
    pub last_refresh_duration_ms: u64,
    pub last_refresh_required_sequence: SequenceNumber,
    pub last_refresh_local_durable_head: SequenceNumber,
    pub last_refresh_applied_sequence: SequenceNumber,
    pub last_refresh_error: Option<String>,
}

struct LibsqlReplicaFreshnessMetrics {
    requested_refresh_cause: AtomicU8,
    refresh_attempt_path: AtomicU8,
    last_barrier_path: AtomicU8,
    barrier_current_count: AtomicU64,
    barrier_waited_for_background_refresh_count: AtomicU64,
    barrier_incremental_catch_up_count: AtomicU64,
    barrier_full_snapshot_rebuild_count: AtomicU64,
    last_refresh_cause: AtomicU8,
    last_refresh_path: AtomicU8,
    incremental_refresh_count: AtomicU64,
    full_snapshot_refresh_count: AtomicU64,
    incremental_fallback_to_snapshot_count: AtomicU64,
    refresh_error_count: AtomicU64,
    last_refresh_duration_ms: AtomicU64,
    last_refresh_required_sequence: AtomicU64,
    last_refresh_local_durable_head: AtomicU64,
    last_refresh_applied_sequence: AtomicU64,
    last_refresh_error: RwLock<Option<String>>,
}

struct ReplicaRefreshOutcome {
    path: LibsqlReplicaRefreshPath,
    progress: JournalProgress,
}

impl LibsqlReplicaBarrierPath {
    fn to_atomic(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::AlreadyCurrentCache => 1,
            Self::WaitedForBackgroundRefresh => 2,
            Self::IncrementalCatchUp => 3,
            Self::FullSnapshotRebuild => 4,
        }
    }

    fn from_atomic(value: u8) -> Self {
        match value {
            1 => Self::AlreadyCurrentCache,
            2 => Self::WaitedForBackgroundRefresh,
            3 => Self::IncrementalCatchUp,
            4 => Self::FullSnapshotRebuild,
            _ => Self::Unknown,
        }
    }

    fn from_refresh_path(path: LibsqlReplicaRefreshPath) -> Self {
        match path {
            LibsqlReplicaRefreshPath::IncrementalCatchUp => Self::IncrementalCatchUp,
            LibsqlReplicaRefreshPath::FullSnapshotRebuild
            | LibsqlReplicaRefreshPath::IncrementalFallbackToSnapshot => Self::FullSnapshotRebuild,
            LibsqlReplicaRefreshPath::Unknown => Self::Unknown,
        }
    }
}

impl LibsqlReplicaRefreshCause {
    fn to_atomic(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::CommitBarrier => 1,
            Self::DurableJournalReplay => 2,
            Self::SchemaMismatch => 3,
            Self::SchemaWrite => 4,
            Self::BootstrapExport => 5,
        }
    }

    fn from_atomic(value: u8) -> Self {
        match value {
            1 => Self::CommitBarrier,
            2 => Self::DurableJournalReplay,
            3 => Self::SchemaMismatch,
            4 => Self::SchemaWrite,
            5 => Self::BootstrapExport,
            _ => Self::Unknown,
        }
    }
}

impl LibsqlReplicaRefreshPath {
    fn to_atomic(self) -> u8 {
        match self {
            Self::Unknown => 0,
            Self::IncrementalCatchUp => 1,
            Self::FullSnapshotRebuild => 2,
            Self::IncrementalFallbackToSnapshot => 3,
        }
    }

    fn from_atomic(value: u8) -> Self {
        match value {
            1 => Self::IncrementalCatchUp,
            2 => Self::FullSnapshotRebuild,
            3 => Self::IncrementalFallbackToSnapshot,
            _ => Self::Unknown,
        }
    }
}

impl LibsqlReplicaFreshnessMetrics {
    fn new() -> Self {
        Self {
            requested_refresh_cause: AtomicU8::new(LibsqlReplicaRefreshCause::Unknown.to_atomic()),
            refresh_attempt_path: AtomicU8::new(LibsqlReplicaRefreshPath::Unknown.to_atomic()),
            last_barrier_path: AtomicU8::new(LibsqlReplicaBarrierPath::Unknown.to_atomic()),
            barrier_current_count: AtomicU64::new(0),
            barrier_waited_for_background_refresh_count: AtomicU64::new(0),
            barrier_incremental_catch_up_count: AtomicU64::new(0),
            barrier_full_snapshot_rebuild_count: AtomicU64::new(0),
            last_refresh_cause: AtomicU8::new(LibsqlReplicaRefreshCause::Unknown.to_atomic()),
            last_refresh_path: AtomicU8::new(LibsqlReplicaRefreshPath::Unknown.to_atomic()),
            incremental_refresh_count: AtomicU64::new(0),
            full_snapshot_refresh_count: AtomicU64::new(0),
            incremental_fallback_to_snapshot_count: AtomicU64::new(0),
            refresh_error_count: AtomicU64::new(0),
            last_refresh_duration_ms: AtomicU64::new(0),
            last_refresh_required_sequence: AtomicU64::new(0),
            last_refresh_local_durable_head: AtomicU64::new(0),
            last_refresh_applied_sequence: AtomicU64::new(0),
            last_refresh_error: RwLock::new(None),
        }
    }

    fn note_refresh_request(&self, cause: LibsqlReplicaRefreshCause) {
        self.requested_refresh_cause
            .store(cause.to_atomic(), Ordering::Release);
    }

    fn requested_refresh_cause(&self) -> LibsqlReplicaRefreshCause {
        LibsqlReplicaRefreshCause::from_atomic(self.requested_refresh_cause.load(Ordering::Acquire))
    }

    fn note_refresh_attempt_path(&self, path: LibsqlReplicaRefreshPath) {
        self.refresh_attempt_path
            .store(path.to_atomic(), Ordering::Release);
    }

    fn refresh_attempt_path(&self) -> LibsqlReplicaRefreshPath {
        LibsqlReplicaRefreshPath::from_atomic(self.refresh_attempt_path.load(Ordering::Acquire))
    }

    fn record_barrier_path(&self, path: LibsqlReplicaBarrierPath) {
        self.last_barrier_path
            .store(path.to_atomic(), Ordering::Release);
        match path {
            LibsqlReplicaBarrierPath::AlreadyCurrentCache => {
                self.barrier_current_count.fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaBarrierPath::WaitedForBackgroundRefresh => {
                self.barrier_waited_for_background_refresh_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaBarrierPath::IncrementalCatchUp => {
                self.barrier_incremental_catch_up_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaBarrierPath::FullSnapshotRebuild => {
                self.barrier_full_snapshot_rebuild_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaBarrierPath::Unknown => {}
        }
    }

    fn record_refresh_success(
        &self,
        cause: LibsqlReplicaRefreshCause,
        outcome: &ReplicaRefreshOutcome,
        duration_ms: u64,
        required_sequence: SequenceNumber,
    ) {
        self.last_refresh_cause
            .store(cause.to_atomic(), Ordering::Release);
        self.last_refresh_path
            .store(outcome.path.to_atomic(), Ordering::Release);
        self.last_refresh_duration_ms
            .store(duration_ms, Ordering::Release);
        self.last_refresh_required_sequence
            .store(required_sequence.0, Ordering::Release);
        self.last_refresh_local_durable_head
            .store(outcome.progress.durable_head.0, Ordering::Release);
        self.last_refresh_applied_sequence
            .store(outcome.progress.applied_head.0, Ordering::Release);
        match outcome.path {
            LibsqlReplicaRefreshPath::IncrementalCatchUp => {
                self.incremental_refresh_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaRefreshPath::FullSnapshotRebuild => {
                self.full_snapshot_refresh_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaRefreshPath::IncrementalFallbackToSnapshot => {
                self.incremental_fallback_to_snapshot_count
                    .fetch_add(1, Ordering::Relaxed);
            }
            LibsqlReplicaRefreshPath::Unknown => {}
        }
        if let Ok(mut guard) = self.last_refresh_error.write() {
            *guard = None;
        }
    }

    fn record_refresh_error(
        &self,
        cause: LibsqlReplicaRefreshCause,
        path: LibsqlReplicaRefreshPath,
        duration_ms: u64,
        required_sequence: SequenceNumber,
        local_progress: JournalProgress,
        error: &Error,
    ) {
        self.last_refresh_cause
            .store(cause.to_atomic(), Ordering::Release);
        self.last_refresh_path
            .store(path.to_atomic(), Ordering::Release);
        self.last_refresh_duration_ms
            .store(duration_ms, Ordering::Release);
        self.last_refresh_required_sequence
            .store(required_sequence.0, Ordering::Release);
        self.last_refresh_local_durable_head
            .store(local_progress.durable_head.0, Ordering::Release);
        self.last_refresh_applied_sequence
            .store(local_progress.applied_head.0, Ordering::Release);
        self.refresh_error_count.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut guard) = self.last_refresh_error.write() {
            *guard = Some(error.to_string());
        }
    }

    fn snapshot(
        &self,
        required_sequence: SequenceNumber,
        local_progress: JournalProgress,
        refresh_needed: bool,
        refresh_requested: bool,
        refresh_inflight: bool,
    ) -> LibsqlReplicaFreshnessStats {
        let last_refresh_error = self
            .last_refresh_error
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_else(|_| Some("libsql replica refresh error lock poisoned".to_string()));
        LibsqlReplicaFreshnessStats {
            required_sequence,
            local_durable_head: local_progress.durable_head,
            local_applied_sequence: local_progress.applied_head,
            refresh_needed,
            refresh_requested,
            refresh_inflight,
            last_barrier_path: LibsqlReplicaBarrierPath::from_atomic(
                self.last_barrier_path.load(Ordering::Acquire),
            ),
            barrier_current_count: self.barrier_current_count.load(Ordering::Relaxed),
            barrier_waited_for_background_refresh_count: self
                .barrier_waited_for_background_refresh_count
                .load(Ordering::Relaxed),
            barrier_incremental_catch_up_count: self
                .barrier_incremental_catch_up_count
                .load(Ordering::Relaxed),
            barrier_full_snapshot_rebuild_count: self
                .barrier_full_snapshot_rebuild_count
                .load(Ordering::Relaxed),
            last_refresh_cause: LibsqlReplicaRefreshCause::from_atomic(
                self.last_refresh_cause.load(Ordering::Acquire),
            ),
            last_refresh_path: LibsqlReplicaRefreshPath::from_atomic(
                self.last_refresh_path.load(Ordering::Acquire),
            ),
            incremental_refresh_count: self.incremental_refresh_count.load(Ordering::Relaxed),
            full_snapshot_refresh_count: self.full_snapshot_refresh_count.load(Ordering::Relaxed),
            incremental_fallback_to_snapshot_count: self
                .incremental_fallback_to_snapshot_count
                .load(Ordering::Relaxed),
            refresh_error_count: self.refresh_error_count.load(Ordering::Relaxed),
            last_refresh_duration_ms: self.last_refresh_duration_ms.load(Ordering::Acquire),
            last_refresh_required_sequence: SequenceNumber(
                self.last_refresh_required_sequence.load(Ordering::Acquire),
            ),
            last_refresh_local_durable_head: SequenceNumber(
                self.last_refresh_local_durable_head.load(Ordering::Acquire),
            ),
            last_refresh_applied_sequence: SequenceNumber(
                self.last_refresh_applied_sequence.load(Ordering::Acquire),
            ),
            last_refresh_error,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibsqlReplicaProviderConfig {
    pub primary_url: String,
    pub auth_token: Option<String>,
    pub admin_api_url: String,
    pub admin_auth_header: Option<String>,
    pub metadata_namespace: String,
    pub tenant_namespace_prefix: String,
    pub replica_cache_dir: PathBuf,
}

impl LibsqlReplicaProviderConfig {
    pub fn new(
        primary_url: impl Into<String>,
        admin_api_url: impl Into<String>,
        replica_cache_dir: impl Into<PathBuf>,
    ) -> Self {
        Self {
            primary_url: primary_url.into(),
            auth_token: None,
            admin_api_url: admin_api_url.into(),
            admin_auth_header: None,
            metadata_namespace: "neovex_provider".to_string(),
            tenant_namespace_prefix: "tenant_".to_string(),
            replica_cache_dir: replica_cache_dir.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibsqlReplicaTenantRegistration {
    pub tenant_id: TenantId,
    pub namespace: String,
}

#[derive(Clone)]
pub struct LibsqlReplicaProvider {
    primary_url: String,
    auth_token: Option<String>,
    admin_api_url: String,
    admin_auth_header: Option<String>,
    metadata_namespace: String,
    tenant_namespace_prefix: String,
    replica_cache_dir: PathBuf,
    runtime_handle: TokioRuntimeHandle,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    tenant_read_parallelism: usize,
    metadata_database: Arc<Database>,
}

pub struct OpenedLibsqlReplicaTenant {
    pub store: Arc<LibsqlReplicaTenantStore>,
    pub read_storage: Arc<LibsqlReplicaTenantStorage>,
    tenant_id: TenantId,
    namespace: String,
    replica_path: PathBuf,
    primary_url: String,
}

#[derive(Clone)]
pub struct LibsqlReplicaTenantStore {
    provider: LibsqlReplicaProvider,
    tenant_id: TenantId,
    namespace: String,
    remote_database: Arc<Database>,
    active_cache: Arc<RwLock<ReplicaCacheHandle>>,
    retired_caches: Arc<Mutex<Vec<ReplicaCacheHandle>>>,
    next_cache_generation: Arc<AtomicU64>,
    refresh_needed: Arc<AtomicBool>,
    refresh_requested: Arc<AtomicBool>,
    refresh_inflight: Arc<AtomicBool>,
    refresh_complete: Arc<Notify>,
    required_cache_sequence: Arc<AtomicU64>,
    freshness_metrics: Arc<LibsqlReplicaFreshnessMetrics>,
}

#[derive(Clone)]
struct ReplicaCacheHandle {
    store: Arc<SqliteTenantStore>,
    replica_path: PathBuf,
}

#[derive(Clone)]
pub struct LibsqlReplicaTenantStorage {
    store: Arc<LibsqlReplicaTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
    write_executor: LibsqlReplicaBlockingWriteExecutor,
}

pub struct LibsqlReplicaWriteTransaction {
    store: LibsqlReplicaTenantStore,
    tx: Option<Transaction>,
    commit_writes: Vec<WriteOp>,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
    refresh_cache_after_commit: bool,
}

#[derive(Clone)]
struct LibsqlReplicaBlockingWriteExecutor {
    store: Arc<LibsqlReplicaTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
}

impl LibsqlReplicaTenantStore {
    fn new(
        provider: LibsqlReplicaProvider,
        tenant_id: TenantId,
        namespace: String,
        remote_database: Arc<Database>,
        local_store: Arc<SqliteTenantStore>,
        replica_path: PathBuf,
    ) -> Self {
        let initial_applied = local_store
            .applied_sequence()
            .map(|sequence| sequence.0)
            .unwrap_or(0);
        Self {
            provider,
            tenant_id,
            namespace,
            remote_database,
            active_cache: Arc::new(RwLock::new(ReplicaCacheHandle {
                store: local_store,
                replica_path,
            })),
            retired_caches: Arc::new(Mutex::new(Vec::new())),
            next_cache_generation: Arc::new(AtomicU64::new(1)),
            refresh_needed: Arc::new(AtomicBool::new(false)),
            refresh_requested: Arc::new(AtomicBool::new(false)),
            refresh_inflight: Arc::new(AtomicBool::new(false)),
            refresh_complete: Arc::new(Notify::new()),
            required_cache_sequence: Arc::new(AtomicU64::new(initial_applied)),
            freshness_metrics: Arc::new(LibsqlReplicaFreshnessMetrics::new()),
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn replica_path(&self) -> Result<PathBuf> {
        Ok(self.active_cache_handle()?.replica_path.clone())
    }

    pub fn primary_url(&self) -> &str {
        &self.provider.primary_url
    }

    pub fn check_fault(&self, point: crate::FaultPoint) -> Result<()> {
        self.provider.fault_injector.check(point)
    }

    pub fn now(&self) -> Timestamp {
        self.provider.clock.now()
    }

    fn remote_connection(&self) -> Result<Connection> {
        self.remote_database.connect().map_err(map_libsql_error)
    }

    fn active_cache_handle(&self) -> Result<ReplicaCacheHandle> {
        self.active_cache
            .read()
            .map_err(|_| Error::Internal("libsql replica cache lock poisoned".to_string()))
            .map(|guard| guard.clone())
    }

    fn active_cache_store(&self) -> Result<Arc<SqliteTenantStore>> {
        Ok(self.active_cache_handle()?.store)
    }

    fn current_query_cache_store(&self) -> Result<Arc<SqliteTenantStore>> {
        self.ensure_local_cache_current()?;
        self.active_cache_store()
    }

    fn ensure_local_cache_current(&self) -> Result<()> {
        if self.local_cache_satisfies_requirements()? {
            self.freshness_metrics
                .record_barrier_path(LibsqlReplicaBarrierPath::AlreadyCurrentCache);
            return Ok(());
        }
        self.schedule_background_refresh();
        self.wait_for_background_refresh()?;
        if self.local_cache_satisfies_requirements()? {
            self.freshness_metrics
                .record_barrier_path(LibsqlReplicaBarrierPath::WaitedForBackgroundRefresh);
            return Ok(());
        }
        let outcome = self.refresh_local_cache()?;
        self.freshness_metrics
            .record_barrier_path(LibsqlReplicaBarrierPath::from_refresh_path(outcome.path));
        Ok(())
    }

    fn note_required_cache_sequence_with_cause(
        &self,
        sequence: SequenceNumber,
        cause: LibsqlReplicaRefreshCause,
    ) {
        let mut current = self.required_cache_sequence.load(Ordering::Acquire);
        while sequence.0 > current {
            match self.required_cache_sequence.compare_exchange(
                current,
                sequence.0,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => break,
                Err(updated) => current = updated,
            }
        }
        self.freshness_metrics.note_refresh_request(cause);
        self.schedule_background_refresh();
    }

    fn refresh_local_cache(&self) -> Result<ReplicaRefreshOutcome> {
        let cause = self.freshness_metrics.requested_refresh_cause();
        let required_sequence =
            SequenceNumber(self.required_cache_sequence.load(Ordering::Acquire));
        let local_progress = self.active_cache_store()?.journal_progress()?;
        let started = Instant::now();
        let refresh_result = if !self.refresh_needed.load(Ordering::Acquire) {
            self.freshness_metrics
                .note_refresh_attempt_path(LibsqlReplicaRefreshPath::IncrementalCatchUp);
            self.catch_up_local_cache_from_remote_durable_journal()
        } else {
            self.freshness_metrics
                .note_refresh_attempt_path(LibsqlReplicaRefreshPath::FullSnapshotRebuild);
            self.refresh_local_cache_from_snapshot()
        };
        let duration_ms = started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64;
        match refresh_result {
            Ok(outcome) => {
                self.freshness_metrics.record_refresh_success(
                    cause,
                    &outcome,
                    duration_ms,
                    required_sequence,
                );
                debug!(
                    tenant = %self.tenant_id,
                    namespace = %self.namespace,
                    cause = ?cause,
                    path = ?outcome.path,
                    duration_ms,
                    required_sequence = required_sequence.0,
                    local_durable_before = local_progress.durable_head.0,
                    local_applied_before = local_progress.applied_head.0,
                    local_durable_after = outcome.progress.durable_head.0,
                    local_applied_after = outcome.progress.applied_head.0,
                    "libsql replica refresh completed"
                );
                Ok(outcome)
            }
            Err(error) => {
                let path = self.freshness_metrics.refresh_attempt_path();
                self.freshness_metrics.record_refresh_error(
                    cause,
                    path,
                    duration_ms,
                    required_sequence,
                    local_progress,
                    &error,
                );
                warn!(
                    tenant = %self.tenant_id,
                    namespace = %self.namespace,
                    cause = ?cause,
                    path = ?path,
                    duration_ms,
                    required_sequence = required_sequence.0,
                    error = %error,
                    "libsql replica refresh failed"
                );
                Err(error)
            }
        }
    }

    fn refresh_local_cache_from_snapshot(&self) -> Result<ReplicaRefreshOutcome> {
        let snapshot = self.block_on(fetch_remote_namespace_snapshot(
            &self.provider.primary_url,
            self.provider.auth_token.as_deref(),
            &self.namespace,
        ))?;
        let durable_head = snapshot
            .commit_log
            .last()
            .map(|record| SequenceNumber(record.sequence))
            .unwrap_or(SequenceNumber(0));
        let generation = self.next_cache_generation.fetch_add(1, Ordering::AcqRel);
        let replica_path = self
            .provider
            .replica_dir_for_tenant(&self.tenant_id)
            .join(format!("cache-{generation}.sqlite3"));
        let replica_dir = self.provider.replica_dir_for_tenant(&self.tenant_id);
        let path_for_materialize = replica_path.clone();
        let path_for_open = replica_path.clone();
        let clock = self.provider.clock.clone();
        let fault_injector = self.provider.fault_injector.clone();
        let read_parallelism = self.provider.tenant_read_parallelism;
        let next_store = {
            materialize_snapshot_to_replica_cache(
                replica_dir.as_path(),
                path_for_materialize.as_path(),
                snapshot,
            )?;
            SqliteTenantStore::open_with_simulation_and_max_read_connections(
                path_for_open,
                clock,
                fault_injector,
                read_parallelism,
            )?
        };
        let next_store = Arc::new(next_store);
        let next_handle = ReplicaCacheHandle {
            store: next_store.clone(),
            replica_path: replica_path.clone(),
        };
        let previous = {
            let mut guard = self
                .active_cache
                .write()
                .map_err(|_| Error::Internal("libsql replica cache lock poisoned".to_string()))?;
            let previous = guard.clone();
            *guard = next_handle;
            previous
        };
        self.retired_caches
            .lock()
            .map_err(|_| Error::Internal("libsql retired cache lock poisoned".to_string()))?
            .push(previous);
        self.refresh_needed.store(false, Ordering::Release);
        self.reap_retired_caches()?;
        Ok(ReplicaRefreshOutcome {
            path: LibsqlReplicaRefreshPath::FullSnapshotRebuild,
            progress: JournalProgress {
                durable_head,
                applied_head: next_store.applied_sequence()?,
            },
        })
    }

    fn catch_up_local_cache_from_remote_durable_journal(&self) -> Result<ReplicaRefreshOutcome> {
        let store = self.active_cache_store()?;
        let local_progress = store.journal_progress()?;
        let required_sequence = self.required_cache_sequence.load(Ordering::Acquire);
        if local_progress.applied_head.0 >= required_sequence {
            return Ok(ReplicaRefreshOutcome {
                path: LibsqlReplicaRefreshPath::IncrementalCatchUp,
                progress: local_progress,
            });
        }

        if local_progress.durable_head.0 < required_sequence {
            let next_sequence = SequenceNumber(local_progress.durable_head.0.saturating_add(1));
            let records = self.block_on(self.load_remote_durable_records_from(next_sequence))?;
            if !records.is_empty() {
                store.append_durable_records_batch(records.as_slice())?;
            }
        }

        let recovered = store.recover_durable_journal()?;
        if recovered.applied_head.0 >= required_sequence {
            return Ok(ReplicaRefreshOutcome {
                path: LibsqlReplicaRefreshPath::IncrementalCatchUp,
                progress: recovered,
            });
        }

        self.refresh_needed.store(true, Ordering::Release);
        self.freshness_metrics
            .note_refresh_attempt_path(LibsqlReplicaRefreshPath::IncrementalFallbackToSnapshot);
        let snapshot = self.refresh_local_cache_from_snapshot()?;
        Ok(ReplicaRefreshOutcome {
            path: LibsqlReplicaRefreshPath::IncrementalFallbackToSnapshot,
            progress: snapshot.progress,
        })
    }

    fn local_cache_satisfies_requirements(&self) -> Result<bool> {
        let required = self.required_cache_sequence.load(Ordering::Acquire);
        let full_refresh_needed = self.refresh_needed.load(Ordering::Acquire);
        let local_applied = self.active_cache_store()?.applied_sequence()?.0;
        Ok(!full_refresh_needed && local_applied >= required)
    }

    fn schedule_background_refresh(&self) {
        self.refresh_requested.store(true, Ordering::Release);
        if self
            .refresh_inflight
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }

        let store = self.clone();
        let refresh_complete = self.refresh_complete.clone();
        let refresh_inflight = self.refresh_inflight.clone();
        self.provider.runtime_handle.spawn_blocking(move || {
            let refresh_result = store.run_background_refresh_loop();
            refresh_inflight.store(false, Ordering::Release);
            refresh_complete.notify_waiters();
            let should_reschedule = refresh_result.is_ok()
                && (store.refresh_requested.load(Ordering::Acquire)
                    || store
                        .local_cache_satisfies_requirements()
                        .map(|ready| !ready)
                        .unwrap_or(false));
            if should_reschedule {
                store.schedule_background_refresh();
            }
        });
    }

    fn run_background_refresh_loop(&self) -> Result<()> {
        loop {
            self.refresh_requested.store(false, Ordering::Release);
            self.refresh_local_cache()?;
            if !self.refresh_requested.load(Ordering::Acquire)
                && self.local_cache_satisfies_requirements()?
            {
                return Ok(());
            }
        }
    }

    fn wait_for_background_refresh(&self) -> Result<()> {
        while self.refresh_inflight.load(Ordering::Acquire) {
            let notified = self.refresh_complete.notified();
            if !self.refresh_inflight.load(Ordering::Acquire) {
                break;
            }
            self.block_on(async move {
                notified.await;
                Ok(())
            })?;
        }
        Ok(())
    }

    fn reap_retired_caches(&self) -> Result<()> {
        let mut retired = self
            .retired_caches
            .lock()
            .map_err(|_| Error::Internal("libsql retired cache lock poisoned".to_string()))?;
        let mut index = 0;
        while index < retired.len() {
            if Arc::strong_count(&retired[index].store) != 1 {
                index += 1;
                continue;
            }
            let handle = retired.swap_remove(index);
            drop(handle.store);
            remove_sqlite_artifacts(handle.replica_path.as_path())?;
        }
        Ok(())
    }

    fn block_on<T, Fut>(&self, future: Fut) -> Result<T>
    where
        T: Send,
        Fut: Future<Output = Result<T>>,
    {
        let handle = self.provider.runtime_handle.clone();
        let handle_for_task = handle.clone();
        bridge_tokio_runtime_local(
            &handle,
            "libsql replica synchronous transaction bridge requires a multi-thread Tokio runtime",
            move || handle_for_task.block_on(future),
        )
    }

    async fn load_remote_schema(&self) -> Result<Schema> {
        let conn = self.remote_connection()?;
        let rows = query_remote_schema_rows(&conn).await?;
        let mut schema = Schema::default();
        for row in rows {
            let table_schema: TableSchema = deserialize_json(row.schema_json.as_str())?;
            schema
                .tables
                .insert(table_schema.table.clone(), table_schema);
        }
        Ok(schema)
    }

    async fn load_remote_latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(SequenceNumber(
            load_next_sequence_from_session(&self.remote_connection()?)
                .await?
                .saturating_sub(1),
        ))
    }

    async fn load_remote_durable_records_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let conn = self.remote_connection()?;
        let mut rows = conn
            .query(
                "SELECT record_blob FROM commit_log WHERE sequence >= ?1 ORDER BY sequence",
                libsql::params![i64_from_u64(sequence.0)?],
            )
            .await
            .map_err(map_libsql_error)?;
        let mut records = Vec::new();
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            let payload = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
            records.push(deserialize_durable_record(payload.as_slice())?);
        }
        Ok(records)
    }

    async fn load_remote_durable_journal_page(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        let latest_sequence = self.load_remote_latest_sequence().await?;
        let cursor_floor = self.load_remote_durable_journal_cursor_floor().await?;
        if after.0 < cursor_floor.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is behind the retention floor {}",
                after.0, cursor_floor.0
            )));
        }
        if after.0 > latest_sequence.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is ahead of the latest durable sequence {}",
                after.0, latest_sequence.0
            )));
        }

        let conn = self.remote_connection()?;
        let mut rows = conn
            .query(
                "SELECT record_blob FROM commit_log WHERE sequence > ?1 ORDER BY sequence LIMIT ?2",
                libsql::params![
                    i64_from_u64(after.0)?,
                    i64_from_u64(limit.saturating_add(1) as u64)?
                ],
            )
            .await
            .map_err(map_libsql_error)?;
        let mut records = Vec::with_capacity(limit);
        let mut has_more = false;
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            let payload = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
            if records.len() == limit {
                has_more = true;
                break;
            }
            records.push(deserialize_durable_record(payload.as_slice())?);
        }
        let next_cursor = records
            .last()
            .map(|record| record.sequence)
            .unwrap_or(after);
        Ok(DurableJournalPage {
            records,
            next_cursor,
            latest_sequence,
            cursor_floor,
            has_more,
        })
    }

    async fn load_remote_durable_journal_cursor_floor(&self) -> Result<SequenceNumber> {
        let conn = self.remote_connection()?;
        let mut rows = conn
            .query("SELECT MIN(sequence) FROM commit_log", ())
            .await
            .map_err(map_libsql_error)?;
        let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
            return Ok(SequenceNumber(0));
        };
        let min_sequence = row.get::<Option<i64>>(0).map_err(map_libsql_error)?;
        Ok(match min_sequence {
            Some(sequence) => SequenceNumber(sequence_from_i64(sequence)?.0.saturating_sub(1)),
            None => SequenceNumber(0),
        })
    }

    async fn load_remote_scheduled_jobs(&self, table: &str) -> Result<Vec<ScheduledJob>> {
        let conn = self.remote_connection()?;
        let sql = format!("SELECT data_json FROM {table}");
        let mut rows = conn
            .query(sql.as_str(), ())
            .await
            .map_err(map_libsql_error)?;
        let mut jobs = Vec::new();
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            jobs.push(deserialize_json::<ScheduledJob>(
                row.get::<String>(0).map_err(map_libsql_error)?.as_str(),
            )?);
        }
        Ok(jobs)
    }

    async fn load_remote_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let conn = self.remote_connection()?;
        let mut rows = conn
            .query("SELECT data_json FROM cron_jobs ORDER BY name", ())
            .await
            .map_err(map_libsql_error)?;
        let mut crons = Vec::new();
        while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
            crons.push(deserialize_json::<CronJob>(
                row.get::<String>(0).map_err(map_libsql_error)?.as_str(),
            )?);
        }
        Ok(crons)
    }

    async fn append_remote_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<()> {
        let conn = self.remote_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(map_libsql_error)?;
        let mut next = load_next_sequence_from_session(&tx).await?;
        for record in records {
            if record.sequence.0 != next {
                return Err(Error::Internal(format!(
                    "durable journal append expected sequence {}, got {}",
                    next, record.sequence.0
                )));
            }
            tx.execute(
                "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
                libsql::params![
                    i64_from_u64(record.sequence.0)?,
                    serialize_durable_record(record)?
                ],
            )
            .await
            .map_err(map_libsql_error)?;
            next = next.saturating_add(1);
        }
        put_remote_metadata_u64(&tx, NEXT_SEQUENCE_KEY, next).await?;
        tx.commit().await.map_err(map_libsql_error)?;
        Ok(())
    }

    async fn apply_remote_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<SequenceNumber> {
        let conn = self.remote_connection()?;
        let tx = conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .await
            .map_err(map_libsql_error)?;
        let mut applied_head = load_remote_metadata_u64(&tx, APPLIED_SEQUENCE_KEY)
            .await?
            .map(SequenceNumber)
            .unwrap_or(SequenceNumber(0));
        for record in records {
            if record.sequence.0 <= applied_head.0 {
                continue;
            }
            if record.sequence.0 != applied_head.0.saturating_add(1) {
                return Err(Error::Internal(format!(
                    "durable journal apply expected sequence {}, got {}",
                    applied_head.0.saturating_add(1),
                    record.sequence.0
                )));
            }
            apply_durable_record_in_remote_conn(&tx, record).await?;
            applied_head = record.sequence;
        }
        if applied_head.0 >= records[0].sequence.0 {
            put_remote_metadata_u64(&tx, APPLIED_SEQUENCE_KEY, applied_head.0).await?;
        }
        tx.commit().await.map_err(map_libsql_error)?;
        Ok(applied_head)
    }
}

fn map_write_result<T>(result: Result<TenantWriteCommit<T>>) -> Result<TenantWriteOutcome<T>> {
    match result {
        Ok(committed) => Ok(TenantWriteOutcome::Committed(committed)),
        Err(Error::Cancelled) => Ok(TenantWriteOutcome::CancelledBeforeCommit),
        Err(error) => Err(error),
    }
}

fn expect_write_commit(commit: Option<CommitEntry>, expectation: &str) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 || limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "durable journal stream limit must be between 1 and {}",
            MAX_DURABLE_JOURNAL_STREAM_LIMIT
        )));
    }
    Ok(())
}

fn apply_schedule_ops_in_libsql_transaction(
    transaction: &mut LibsqlReplicaWriteTransaction,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for op in schedule_ops {
        match op {
            ResolvedScheduleOp::Insert { job } => transaction.insert_scheduled_job(job)?,
            ResolvedScheduleOp::Cancel { job_id } => {
                transaction.cancel_scheduled_job(job_id)?;
            }
        }
    }
    Ok(())
}

async fn table_has_entries_remote(conn: &Connection, table: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {table} LIMIT 1");
    let mut rows = conn
        .query(sql.as_str(), ())
        .await
        .map_err(map_libsql_error)?;
    Ok(rows.next().await.map_err(map_libsql_error)?.is_some())
}

async fn load_remote_document_from_session(
    conn: &Connection,
    table: TableName,
    id: DocumentId,
) -> Result<Option<Document>> {
    let mut rows = conn
        .query(
            "SELECT creation_time, data_json FROM documents WHERE table_name = ?1 AND id = ?2",
            libsql::params![table.as_str(), id.to_string()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    let creation_time = row.get::<i64>(0).map_err(map_libsql_error)?;
    let data_json = row.get::<String>(1).map_err(map_libsql_error)?;
    Ok(Some(row_to_document(
        &table,
        &id,
        creation_time,
        data_json.as_str(),
    )?))
}

async fn load_next_sequence_from_session(conn: &Connection) -> Result<u64> {
    if let Some(stored) = load_remote_metadata_u64(conn, NEXT_SEQUENCE_KEY).await? {
        return Ok(stored);
    }
    let mut rows = conn
        .query("SELECT MAX(sequence) FROM commit_log", ())
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(1);
    };
    let latest = row.get::<Option<i64>>(0).map_err(map_libsql_error)?;
    Ok(latest
        .map(sequence_from_i64)
        .transpose()?
        .unwrap_or(SequenceNumber(0))
        .0
        .saturating_add(1))
}

async fn load_remote_metadata_u64(conn: &Connection, key: &str) -> Result<Option<u64>> {
    let mut rows = conn
        .query(
            "SELECT value_blob FROM metadata WHERE key = ?1",
            libsql::params![key.to_string()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    let bytes = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
    Ok(Some(decode_u64(bytes.as_slice())?))
}

async fn put_remote_metadata_u64(conn: &Connection, key: &str, value: u64) -> Result<()> {
    conn.execute(
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
        libsql::params![key.to_string(), encode_u64(value).to_vec()],
    )
    .await
    .map_err(map_libsql_error)?;
    Ok(())
}

async fn apply_durable_record_in_remote_conn(
    conn: &Connection,
    record: &DurableMutationRecord,
) -> Result<()> {
    if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
        let _ = begin_scheduled_execution_remote(conn, Some(execution_id)).await?;
    }

    for write in &record.writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                let existing =
                    load_remote_document_from_session(conn, write.table.clone(), write.doc_id)
                        .await?;
                match existing {
                    Some(existing) if existing == *current => continue,
                    Some(_) => {
                        return Err(Error::Conflict(format!(
                            "durable journal insert replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    None => {
                        conn.execute(
                            "INSERT INTO documents (table_name, id, data_json, creation_time)
                             VALUES (?1, ?2, ?3, ?4)",
                            libsql::params![
                                write.table.as_str(),
                                write.doc_id.to_string(),
                                serialize_document_fields(current)?,
                                i64_from_u64(current.creation_time.0)?
                            ],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    }
                }
            }
            (Some(previous), Some(current)) => {
                let existing =
                    load_remote_document_from_session(conn, write.table.clone(), write.doc_id)
                        .await?
                        .ok_or(Error::Conflict(format!(
                            "durable journal update replay missing document {}",
                            write.doc_id
                        )))?;
                if existing == *current {
                    continue;
                }
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "durable journal update replay found conflicting state for document {}",
                        write.doc_id
                    )));
                }
                conn.execute(
                    "UPDATE documents
                     SET data_json = ?3, creation_time = ?4
                     WHERE table_name = ?1 AND id = ?2",
                    libsql::params![
                        write.table.as_str(),
                        write.doc_id.to_string(),
                        serialize_document_fields(current)?,
                        i64_from_u64(current.creation_time.0)?
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            }
            (Some(previous), None) => {
                match load_remote_document_from_session(conn, write.table.clone(), write.doc_id)
                    .await?
                {
                    Some(existing) if existing != *previous => {
                        return Err(Error::Conflict(format!(
                            "durable journal delete replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    Some(_) => {
                        conn.execute(
                            "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                            libsql::params![write.table.as_str(), write.doc_id.to_string()],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    }
                    None => continue,
                }
            }
            (None, None) => {
                return Err(Error::Internal(
                    "durable journal write must include a previous or current document".to_string(),
                ));
            }
        }
    }

    Ok(())
}

async fn begin_scheduled_execution_remote(
    conn: &Connection,
    execution_id: Option<&str>,
) -> Result<bool> {
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
            libsql::params![execution_id],
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(inserted == 1)
}

fn serialize_json<T>(value: &T) -> Result<String>
where
    T: Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

fn serialize_document_fields(document: &Document) -> Result<String> {
    serialize_json(&document.fields)
}

fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

fn decode_u64(bytes: &[u8]) -> Result<u64> {
    <[u8; 8]>::try_from(bytes)
        .map(u64::from_be_bytes)
        .map_err(|_| Error::Serialization("invalid u64 encoding".to_string()))
}

fn row_to_document(
    table: &TableName,
    id: &DocumentId,
    creation_time: i64,
    data_json: &str,
) -> Result<Document> {
    Ok(Document {
        id: *id,
        table: table.clone(),
        creation_time: Timestamp(u64::try_from(creation_time).map_err(|_| {
            Error::storage(
                StorageErrorKind::Corruption,
                format!("negative creation_time in libsql row: {creation_time}"),
            )
        })?),
        fields: deserialize_json(data_json)?,
    })
}

fn sequence_from_i64(value: i64) -> Result<SequenceNumber> {
    Ok(SequenceNumber(u64::try_from(value).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("negative libsql sequence value: {value}"),
        )
    })?))
}

fn i64_from_u64(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::InvalidInput(format!("value {value} exceeds SQLite INTEGER")))
}

fn map_libsql_error(error: libsql::Error) -> Error {
    let message = error.to_string();
    match error {
        libsql::Error::ConnectionFailed(_)
        | libsql::Error::Hrana(_)
        | libsql::Error::WriteDelegation(_)
        | libsql::Error::Replication(_)
        | libsql::Error::Sync(_)
        | libsql::Error::InvalidTlsConfiguration(_) => {
            Error::storage(StorageErrorKind::Unavailable, message)
        }
        libsql::Error::WalConflict => Error::storage(StorageErrorKind::Busy, message),
        libsql::Error::SqliteFailure(code, _) | libsql::Error::RemoteSqliteFailure(_, code, _) => {
            map_sqlite_result_code(code, message)
        }
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}

fn map_local_sqlite_error(error: rusqlite::Error) -> Error {
    let message = error.to_string();
    match error {
        rusqlite::Error::SqliteFailure(code, _) => {
            map_sqlite_result_code(code.extended_code, message)
        }
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}

fn map_permit_error(_error: tokio::sync::AcquireError) -> Error {
    Error::Internal("libsql replica executor unexpectedly closed".to_string())
}

fn map_join_error(error: tokio::task::JoinError) -> Error {
    Error::Internal(format!("libsql replica read task failed: {error}"))
}

fn storage_io_error(error: impl std::fmt::Display) -> Error {
    Error::storage(StorageErrorKind::Io, error.to_string())
}

fn map_sqlite_result_code(code: i32, message: String) -> Error {
    match code & 0xff {
        5 | 6 => Error::storage(StorageErrorKind::Busy, message),
        3 | 8 | 23 => Error::PermissionDenied(message),
        7 | 13 => Error::ResourceExhausted(message),
        10 => Error::storage(StorageErrorKind::Io, message),
        11 | 26 => Error::storage(StorageErrorKind::Corruption, message),
        14 => Error::storage(StorageErrorKind::Unavailable, message),
        9 | 15 | 17 => Error::storage(StorageErrorKind::Transient, message),
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}
