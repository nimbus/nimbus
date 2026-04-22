use std::cmp::Ordering;
use std::fmt::Write as _;
use std::future::Future;
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};
use std::sync::{Arc, RwLock};

use deadpool_postgres::{
    BuildError, Client, GenericClient, Manager, ManagerConfig, Pool, PoolError, RecyclingMethod,
    Runtime,
};
use neovex_core::{
    CommitEntry, CronJob, Document, DocumentId, DurableMutationRecord, Error, FieldType, Filter,
    FilterOp, IndexDefinition, Result, ScheduledJob, ScheduledJobResult, Schema, SequenceNumber,
    StorageErrorKind, TableName, TableSchema, TenantId, Timestamp, WriteOp, WriteOpType,
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

use crate::async_storage::{TenantReadStorage, TenantWriteOutcome, TenantWriteStorage};
use crate::commit_log::{deserialize_durable_record, serialize_commit, serialize_durable_record};
use crate::runtime_bridge::bridge_tokio_runtime;
use crate::simulation::{Clock, FaultInjector, FaultPoint, NoopFaultInjector, SystemClock};
use crate::store::{
    DurableJournalBootstrap, DurableJournalPage, JournalProgress, MAX_DURABLE_JOURNAL_STREAM_LIMIT,
    MaterializedJournalSnapshot, ResolvedScheduleOp, ResolvedWrite, TenantWriteCommit,
};

mod backend;
mod config;
mod notifications;
mod provider;
mod read;
mod storage;
mod write;

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

const POSTGRES_IDENTIFIER_LIMIT: usize = 63;
const TARGET_TENANT_HASH_HEX_LEN: usize = 40;
const MIN_TENANT_HASH_HEX_LEN: usize = 16;
const MATERIALIZED_JOURNAL_SNAPSHOT_VERSION: u16 = 1;
const MIN_POSTGRES_READ_PARALLELISM: usize = 2;
const POSTGRES_TENANT_WRITE_PARALLELISM: usize = 1;
const APPLIED_SEQUENCE_KEY: &str = "applied_sequence";
const POSTGRES_NOTIFICATION_CHANNEL_PREFIX: &str = "neovex_pg_";
const POSTGRES_POOL_APPLICATION_NAME_PREFIX: &str = "neovex_pool_";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresTenantRegistration {
    pub tenant_id: TenantId,
    pub schema_name: String,
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
    clock: Arc<dyn Clock>,
    fault_injector: Arc<dyn FaultInjector>,
    tenant_read_parallelism: usize,
}

pub struct OpenedPostgresTenant {
    pub store: Arc<PostgresTenantStore>,
    pub read_storage: Arc<PostgresTenantStorage>,
}

#[derive(Clone)]
pub struct PostgresTenantStore {
    provider: PostgresProvider,
    tenant_id: TenantId,
    schema_name: String,
    schema_cache: Arc<RwLock<Option<Schema>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PostgresReadSnapshot {
    schema: Schema,
    progress: JournalProgress,
    documents: Vec<Document>,
    scheduled_execution_ids: Vec<String>,
}

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
    client: Option<Client>,
    commit_writes: Vec<WriteOp>,
    notification: PendingPostgresNotification,
    schema_cache_changed: bool,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
}

#[derive(Clone)]
struct PostgresBlockingWriteExecutor {
    store: Arc<PostgresTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
}

impl PostgresTenantStore {
    fn new(provider: PostgresProvider, registration: PostgresTenantRegistration) -> Self {
        Self {
            provider,
            tenant_id: registration.tenant_id,
            schema_name: registration.schema_name,
            schema_cache: Arc::new(RwLock::new(None)),
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
