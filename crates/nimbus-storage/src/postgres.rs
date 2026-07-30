use std::fmt::Write as _;
use std::future::Future;
use std::str::FromStr;
use std::sync::{Arc, RwLock};

use deadpool_postgres::{
    BuildError, Client, GenericClient, Manager, ManagerConfig, Pool, PoolError, RecyclingMethod,
    Runtime,
};
use nimbus_core::{
    CommitEntry, CronJob, Document, DocumentId, Error, FieldType, Filter, HistoricalIndexTuple,
    HistoricalReadShape, IdSource, IndexDefinition, ResourcePathBinding, Result, ScheduledJob,
    ScheduledJobResult, Schema, SchemaChangeEvent, SequenceNumber, StorageErrorKind,
    SystemIdSource, SystemWallClock, TableId, TableLifecycleEvent, TableName, TableSchema,
    TableState, TenantEventKind, TenantEventRecord, TenantId, Timestamp, TriggerDeliveryCursor,
    TriggerWriteOrigin, WallClock, WriteOp, WriteOpType,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::Semaphore;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_postgres::types::ToSql;
use tokio_postgres::{AsyncMessage, Config as PostgresConfig, IsolationLevel, NoTls};

use crate::RetentionFloor;
use crate::async_storage::{
    TenantReadStorage, TenantWriteOutcome, TenantWriteStorage, map_executor_join_error,
};
use crate::commit_log::{deserialize_tenant_event_record, serialize_tenant_event_record};
use crate::runtime_bridge::bridge_tokio_runtime;
use crate::simulation::{FaultInjector, FaultPoint, NoopFaultInjector};
use crate::store::{
    DurableJournalBootstrap, DurableJournalPage, JournalProgress, MaterializedJournalSnapshot,
    ResolvedWrite, TenantWriteCommit,
};

mod backend;
mod committer_lease;
mod config;
mod document_versions;
mod index_versions;
mod notifications;
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
mod write_pipeline;

use self::backend::*;
pub use self::config::PostgresProviderConfig;
use self::config::{
    build_pool, postgres_notification_channel_name, postgres_pool_application_name,
    qualified_table, quote_identifier, quote_literal, tenant_init_sql, tenant_schema_name,
    validate_identifier_input,
};
use self::notifications::{
    PendingPostgresNotification, PostgresProviderNotificationPayload, parse_postgres_notification,
};
pub use self::notifications::{PostgresNotificationListener, PostgresProviderNotification};
use self::query_helpers::*;
use self::table_catalog::*;

const POSTGRES_IDENTIFIER_LIMIT: usize = 63;
const TARGET_TENANT_HASH_HEX_LEN: usize = 40;
const MIN_TENANT_HASH_HEX_LEN: usize = 16;
const MIN_POSTGRES_READ_PARALLELISM: usize = 2;
const POSTGRES_TENANT_WRITE_PARALLELISM: usize = 1;
const APPLIED_SEQUENCE_KEY: &str = "applied_sequence";
const TRIGGER_DELIVERY_CURSOR_KEY: &str = "trigger_delivery_cursor";
const POSTGRES_NOTIFICATION_CHANNEL_PREFIX: &str = "nimbus_pg_";
const POSTGRES_POOL_APPLICATION_NAME_PREFIX: &str = "nimbus_pool_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresTenantRegistration {
    pub tenant_id: TenantId,
    pub schema_name: String,
    pub incarnation: u64,
}

#[derive(Clone)]
pub struct PostgresProvider {
    pool: Pool,
    connection_string: String,
    metadata_schema: String,
    tenant_schema_prefix: String,
    pool_application_name: String,
    notification_channel: String,
    runtime_handle: TokioRuntimeHandle,
    clock: Arc<dyn WallClock>,
    id_source: Arc<dyn IdSource>,
    fault_injector: Arc<dyn FaultInjector>,
    tenant_read_parallelism: usize,
}

pub struct OpenedPostgresTenant {
    pub store: Arc<PostgresTenantStore>,
    pub read_storage: Arc<PostgresTenantStorage>,
    pub incarnation: u64,
}

#[derive(Clone)]
pub struct PostgresTenantStore {
    provider: PostgresProvider,
    tenant_id: TenantId,
    schema_name: String,
    schema_cache: Arc<RwLock<Option<Schema>>>,
    pipeline_metrics: Arc<crate::sql::write_pipeline::SqlWritePipelineMetrics>,
    pub(crate) retention_floor: Arc<RetentionFloor>,
}

/// PostgreSQL's materialized read snapshot. Once the rows are loaded there is
/// nothing dialect-specific left, so the type and every accessor on it are
/// shared with MySQL; only the transaction that fills it stays per-backend.
pub type PostgresReadSnapshot = crate::sql::read_snapshot::SqlReadSnapshot;

#[derive(Clone)]
pub struct PostgresTenantStorage {
    store: Arc<PostgresTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
    write_executor: PostgresBlockingWriteExecutor,
}

pub struct PostgresWriteTransaction {
    provider: PostgresProvider,
    tenant_id: TenantId,
    schema_name: String,
    schema_cache: Arc<RwLock<Option<Schema>>>,
    pipeline_metrics: Arc<crate::sql::write_pipeline::SqlWritePipelineMetrics>,
    client: Option<Client>,
    commit_writes: Vec<WriteOp>,
    tenant_events: Vec<TenantEventKind>,
    prepared_record: Option<TenantEventRecord>,
    trigger_write_origin: Option<TriggerWriteOrigin>,
    commit_timestamp: Option<Timestamp>,
    notification: PendingPostgresNotification,
    schema_cache_changed: bool,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
}

/// Provider-owned blocking write executor. PostgreSQL keeps its own instead of
/// the generic `async_storage::write` executor because transaction and session
/// lifecycles are coupled to the Postgres async client; the bounded-permit
/// mechanics themselves are shared with MySQL.
type PostgresBlockingWriteExecutor =
    crate::sql::store_core::SqlBlockingWriteExecutor<PostgresTenantStore>;

impl PostgresTenantStore {
    fn new(provider: PostgresProvider, registration: PostgresTenantRegistration) -> Self {
        Self {
            provider,
            tenant_id: registration.tenant_id,
            schema_name: registration.schema_name,
            schema_cache: Arc::new(RwLock::new(None)),
            pipeline_metrics: Arc::new(crate::sql::write_pipeline::SqlWritePipelineMetrics::new(
                "postgres",
                crate::sql::write_pipeline::POSTGRES_MAX_IN_FLIGHT_OPERATIONS,
            )),
            retention_floor: RetentionFloor::new(),
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn schema_name(&self) -> &str {
        &self.schema_name
    }

    pub fn invalidate_schema_cache(&self) {
        invalidate_schema_cache_handle(&self.schema_cache);
    }

    pub fn now(&self) -> Timestamp {
        self.provider.clock.now()
    }

    pub fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.provider.fault_injector.check(point)
    }

    pub fn write_pipeline_diagnostic(&self) -> crate::ProviderWritePipelineDiagnostic {
        self.pipeline_metrics.snapshot()
    }

    fn block_on<T, Fut>(&self, future: Fut) -> Result<T>
    where
        T: Send + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        let handle = self.provider.runtime_handle.clone();
        let handle_for_task = handle.clone();
        bridge_tokio_runtime(
            &handle,
            "Postgres runtime bridge thread panicked",
            move || handle_for_task.block_on(future),
        )
    }
}
