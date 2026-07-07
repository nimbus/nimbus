use std::fmt::Write as _;
use std::future::Future;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, RwLock};

use mysql_async::prelude::Queryable;
use mysql_async::{
    Conn, Opts, OptsBuilder, Params, Pool, PoolConstraints, Row, Value as MySqlValue,
};
use nimbus_core::{
    CommitEntry, CronJob, Document, DocumentId, Error, FieldType, Filter, HistoricalIndexCursor,
    HistoricalIndexTuple, HistoricalReadShape, IndexDefinition, IndexLifecycleEvent,
    ResourcePathBinding, Result, ScheduledJob, ScheduledJobResult, Schema, SchemaChangeEvent,
    SequenceNumber, StorageErrorKind, TableId, TableLifecycleEvent, TableName, TableSchema,
    TableState, TenantEventKind, TenantEventRecord, TenantId, Timestamp, TriggerDeliveryCursor,
    TriggerWriteOrigin, WriteOp, WriteOpType,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::Semaphore;

use crate::RetentionFloor;
use crate::async_storage::{
    TenantReadStorage, TenantWriteOutcome, TenantWriteStorage, map_executor_join_error,
    map_executor_permit_error,
};
use crate::commit_log::{deserialize_tenant_event_record, serialize_tenant_event_record};
use crate::runtime_bridge::bridge_tokio_runtime;
use crate::simulation::{Clock, FaultInjector, FaultPoint, NoopFaultInjector, SystemClock};
use crate::store::{
    DurableJournalBootstrap, DurableJournalPage, JournalProgress, MaterializedJournalSnapshot,
    PointInTimeRestoreArchive, PointInTimeRestoreTarget, TenantWriteCommit,
};
use crate::{ResolvedScheduleOp, ResolvedWrite};

mod backend;
mod document_versions;
mod index_versions;
mod provider;
mod query_helpers;
mod read;
mod resource_paths;
mod storage;
mod table_catalog;
mod table_lifecycle;
mod trigger_delivery;
mod trigger_invocations;
mod write;
mod write_schema_events;

use self::backend::*;
use self::query_helpers::*;
use self::table_catalog::*;

const MYSQL_IDENTIFIER_LIMIT: usize = 64;
const TARGET_TENANT_HASH_HEX_LEN: usize = 40;
const MIN_TENANT_HASH_HEX_LEN: usize = 16;
const MIN_MYSQL_READ_PARALLELISM: usize = 2;
const MYSQL_TENANT_WRITE_PARALLELISM: usize = 1;
const MYSQL_WRITE_BEGIN_RETRY_ATTEMPTS: usize = 4;
const MYSQL_MAX_INDEX_KEY_BYTES: usize = 3072;
const MYSQL_INDEX_KEY_BYTES_PER_CHAR: usize = 4;
const APPLIED_SEQUENCE_KEY: &str = "applied_sequence";
const TRIGGER_DELIVERY_CURSOR_KEY: &str = "trigger_delivery_cursor";
const MATERIALIZED_JOURNAL_SNAPSHOT_VERSION: u16 =
    crate::store::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION;
const MYSQL_INDEX_KEY_VALUE_LEN: usize = 191;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MySqlProviderConfig {
    pub connection_string: String,
    pub metadata_database: String,
    pub tenant_database_prefix: String,
    pub min_connections: Option<usize>,
    pub max_connections: Option<usize>,
}

impl MySqlProviderConfig {
    pub fn new(connection_string: impl Into<String>) -> Self {
        Self {
            connection_string: connection_string.into(),
            metadata_database: "nimbus_provider".to_string(),
            tenant_database_prefix: "tenant_".to_string(),
            min_connections: None,
            max_connections: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MySqlTenantRegistration {
    pub tenant_id: TenantId,
    pub database_name: String,
}

#[derive(Clone)]
pub struct MySqlProvider {
    pool: Pool,
    metadata_database: String,
    tenant_database_prefix: String,
    runtime_handle: TokioRuntimeHandle,
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    tenant_read_parallelism: usize,
}

pub struct OpenedMySqlTenant {
    pub store: Arc<MySqlTenantStore>,
    pub read_storage: Arc<MySqlTenantStorage>,
}

#[derive(Clone)]
pub struct MySqlTenantStore {
    provider: MySqlProvider,
    tenant_id: TenantId,
    database_name: String,
    schema_cache: Arc<RwLock<Option<Schema>>>,
    pub(crate) retention_floor: Arc<RetentionFloor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MySqlReadSnapshot {
    schema: Schema,
    progress: JournalProgress,
    journal_cursor_floor: SequenceNumber,
    table_identities: Vec<crate::TableIdentitySnapshotEntry>,
    documents: Vec<Document>,
    resource_path_bindings: Vec<ResourcePathBinding>,
    scheduled_execution_ids: Vec<String>,
}

#[derive(Clone)]
pub struct MySqlTenantStorage {
    store: Arc<MySqlTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
    write_executor: MySqlBlockingWriteExecutor,
}

pub struct MySqlWriteTransaction {
    provider: MySqlProvider,
    database_name: String,
    schema_cache: Arc<RwLock<Option<Schema>>>,
    conn: Option<Conn>,
    commit_writes: Vec<WriteOp>,
    tenant_events: Vec<TenantEventKind>,
    trigger_write_origin: Option<TriggerWriteOrigin>,
    commit_timestamp: Option<Timestamp>,
    schema_cache_changed: bool,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
}

#[derive(Clone)]
struct MySqlBlockingWriteExecutor {
    store: Arc<MySqlTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
}

impl MySqlTenantStore {
    fn new(provider: MySqlProvider, registration: MySqlTenantRegistration) -> Self {
        Self {
            provider,
            tenant_id: registration.tenant_id,
            database_name: registration.database_name,
            schema_cache: Arc::new(RwLock::new(None)),
            retention_floor: RetentionFloor::new(),
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn database_name(&self) -> &str {
        &self.database_name
    }

    pub fn invalidate_schema_cache(&self) {
        invalidate_schema_cache_handle(&self.schema_cache);
    }

    pub fn metadata_database(&self) -> &str {
        self.provider.metadata_database()
    }

    pub fn now(&self) -> Timestamp {
        self.provider.clock.now()
    }

    pub fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.provider.fault_injector.check(point)
    }

    pub fn block_on<F, T>(&self, future: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send,
        T: Send,
    {
        let runtime_handle = self.provider.runtime_handle.clone();
        let handle_for_task = runtime_handle.clone();
        bridge_tokio_runtime(&runtime_handle, "MySQL bridge thread panicked", move || {
            handle_for_task.block_on(future)
        })
    }
}

fn build_pool(config: &MySqlProviderConfig) -> Result<Pool> {
    let opts = Opts::from_url(&config.connection_string)
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let default_constraints = opts.pool_opts().constraints();
    let min_connections = config
        .min_connections
        .unwrap_or_else(|| default_constraints.min());
    let max_connections = config
        .max_connections
        .unwrap_or_else(|| default_constraints.max());
    let constraints = PoolConstraints::new(min_connections, max_connections).ok_or_else(|| {
        Error::InvalidInput("mysql pool min_connections cannot exceed max_connections".to_string())
    })?;
    let pool_opts = opts.pool_opts().clone().with_constraints(constraints);
    let mut builder = OptsBuilder::from_opts(opts);
    builder = builder.db_name(None::<String>).pool_opts(pool_opts);
    Ok(Pool::new(builder))
}

fn default_mysql_read_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get().max(MIN_MYSQL_READ_PARALLELISM))
        .unwrap_or(MIN_MYSQL_READ_PARALLELISM)
}

fn tenant_database_name(prefix: &str, tenant_id: &TenantId) -> Result<String> {
    let available_hex_len = MYSQL_IDENTIFIER_LIMIT
        .checked_sub(prefix.len())
        .ok_or_else(|| {
            Error::InvalidInput("tenant database prefix is too long for MySQL".to_string())
        })?;
    let bounded_hex_len = available_hex_len.min(TARGET_TENANT_HASH_HEX_LEN);
    let hash_hex_len = bounded_hex_len - (bounded_hex_len % 2);
    if hash_hex_len < MIN_TENANT_HASH_HEX_LEN {
        return Err(Error::InvalidInput(
            "tenant database prefix leaves too little room for a safe tenant hash".to_string(),
        ));
    }

    let digest = Sha256::digest(tenant_id.as_str().as_bytes());
    let mut hash = String::with_capacity(hash_hex_len);
    for byte in digest.iter().take(hash_hex_len / 2) {
        let _ = write!(&mut hash, "{byte:02x}");
    }
    Ok(format!("{prefix}{hash}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FailingFaultInjector;

    impl FaultInjector for FailingFaultInjector {
        fn check(&self, point: FaultPoint) -> Result<()> {
            Err(Error::Internal(format!("fault point {}", point.as_str())))
        }
    }

    #[test]
    fn mysql_tenant_store_check_fault_delegates_to_provider_injector() {
        let runtime = tokio::runtime::Runtime::new().expect("test runtime should build");
        let provider = MySqlProvider {
            pool: build_pool(&MySqlProviderConfig::new(
                "mysql://root:password@127.0.0.1:1/nimbus",
            ))
            .expect("mysql pool should build without opening a connection"),
            metadata_database: "nimbus_provider".to_string(),
            tenant_database_prefix: "tenant_".to_string(),
            runtime_handle: runtime.handle().clone(),
            clock: Arc::new(SystemClock),
            fault_injector: Arc::new(FailingFaultInjector),
            tenant_read_parallelism: MIN_MYSQL_READ_PARALLELISM,
        };
        let store = MySqlTenantStore::new(
            provider,
            MySqlTenantRegistration {
                tenant_id: TenantId::new("demo").expect("tenant id should be valid"),
                database_name: "tenant_demo".to_string(),
            },
        );

        let error = store
            .check_fault(FaultPoint::StorageCommitBeforeVisibility)
            .expect_err("mysql store should delegate to the configured fault injector");
        assert!(
            error
                .to_string()
                .contains(FaultPoint::StorageCommitBeforeVisibility.as_str()),
            "delegated fault error should identify the fault point: {error}"
        );
    }
}
