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
pub struct PostgresProviderConfig {
    pub connection_string: String,
    pub metadata_schema: String,
    pub tenant_schema_prefix: String,
    pub min_connections: Option<usize>,
    pub max_connections: Option<usize>,
}

impl PostgresProviderConfig {
    pub fn new(connection_string: impl Into<String>) -> Self {
        Self {
            connection_string: connection_string.into(),
            metadata_schema: "neovex_provider".to_string(),
            tenant_schema_prefix: "tenant_".to_string(),
            min_connections: None,
            max_connections: None,
        }
    }

    pub fn derived_pool_application_name(&self) -> Result<String> {
        postgres_pool_application_name(self)
    }

    pub fn derived_notification_channel_name(&self) -> Result<String> {
        postgres_notification_channel_name(self)
    }
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostgresProviderNotification {
    pub tenant_id: TenantId,
    pub journal_changed: bool,
    pub scheduler_changed: bool,
    pub schema_changed: bool,
}

pub struct PostgresNotificationListener {
    _client: tokio_postgres::Client,
    receiver: mpsc::UnboundedReceiver<Result<PostgresProviderNotification>>,
    pump_task: JoinHandle<()>,
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

#[derive(Debug, Clone, Copy, Default)]
struct PendingPostgresNotification {
    journal_changed: bool,
    scheduler_changed: bool,
    schema_changed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PostgresProviderNotificationPayload {
    tenant_id: String,
    journal_changed: bool,
    scheduler_changed: bool,
    schema_changed: bool,
}

impl PendingPostgresNotification {
    fn has_any(self) -> bool {
        self.journal_changed || self.scheduler_changed || self.schema_changed
    }
}

impl PostgresProvider {
    pub async fn connect(config: PostgresProviderConfig) -> Result<Self> {
        Self::connect_with_simulation(
            config,
            TokioRuntimeHandle::current(),
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    pub async fn connect_with_simulation(
        config: PostgresProviderConfig,
        runtime_handle: TokioRuntimeHandle,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        validate_identifier_input(&config.metadata_schema, "metadata schema")?;
        validate_identifier_input(&config.tenant_schema_prefix, "tenant schema prefix")?;

        let pool_application_name = postgres_pool_application_name(&config)?;
        let pool = build_pool(&config, &pool_application_name)?;
        let notification_channel = postgres_notification_channel_name(&config)?;
        let provider = Self {
            pool,
            connection_string: config.connection_string.clone(),
            metadata_schema: config.metadata_schema,
            tenant_schema_prefix: config.tenant_schema_prefix,
            pool_application_name,
            notification_channel,
            runtime_handle,
            clock,
            fault_injector,
            tenant_read_parallelism: default_postgres_read_parallelism(),
        };
        provider.ensure_metadata_schema().await?;
        Ok(provider)
    }

    pub fn metadata_schema(&self) -> &str {
        &self.metadata_schema
    }

    pub fn tenant_schema_name(&self, tenant_id: &TenantId) -> Result<String> {
        tenant_schema_name(&self.tenant_schema_prefix, tenant_id)
    }

    pub fn notification_channel(&self) -> &str {
        &self.notification_channel
    }

    pub fn pool_application_name(&self) -> &str {
        &self.pool_application_name
    }

    pub fn notification_listener_application_name(&self) -> &str {
        &self.notification_channel
    }

    pub async fn connect_notification_listener(&self) -> Result<PostgresNotificationListener> {
        let (client, connection) = tokio_postgres::connect(&self.connection_string, NoTls)
            .await
            .map_err(map_postgres_error)?;
        let channel = self.notification_channel.clone();
        let application_name = quote_literal(self.notification_listener_application_name());
        let quoted_channel = quote_identifier(&channel);
        let (sender, receiver) = mpsc::unbounded_channel();
        let pump_task = self.runtime_handle.spawn(async move {
            let mut connection = connection;
            loop {
                match std::future::poll_fn(|cx| connection.poll_message(cx)).await {
                    Some(Ok(AsyncMessage::Notification(notification))) => {
                        if notification.channel() != channel {
                            continue;
                        }
                        let _ = sender.send(parse_postgres_notification(notification));
                    }
                    Some(Ok(AsyncMessage::Notice(_))) => {}
                    Some(Ok(_)) => {}
                    Some(Err(error)) => {
                        let _ = sender.send(Err(map_postgres_error(error)));
                        break;
                    }
                    None => break,
                }
            }
        });
        if let Err(error) = client
            .batch_execute(
                format!("SET application_name = {application_name}; LISTEN {quoted_channel}")
                    .as_str(),
            )
            .await
        {
            pump_task.abort();
            return Err(map_postgres_error(error));
        }
        Ok(PostgresNotificationListener {
            _client: client,
            receiver,
            pump_task,
        })
    }

    pub fn read_storage_for_store(
        &self,
        store: Arc<PostgresTenantStore>,
    ) -> Arc<PostgresTenantStorage> {
        Arc::new(PostgresTenantStorage::with_max_concurrent_reads(
            store,
            self.runtime_handle.clone(),
            self.tenant_read_parallelism,
        ))
    }

    pub async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<OpenedPostgresTenant> {
        let registration = self.create_tenant(tenant_id).await?;
        self.open_registration(registration)
    }

    pub async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedPostgresTenant>> {
        self.open_existing_tenant(tenant_id)
            .await?
            .map(|registration| self.open_registration(registration))
            .transpose()
    }

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let client = self.client().await?;
        let query = format!(
            "SELECT tenant_id FROM {} ORDER BY tenant_id",
            qualified_table(&self.metadata_schema, "tenants")
        );
        let rows = client
            .query(query.as_str(), &[])
            .await
            .map_err(map_postgres_error)?;
        rows.into_iter()
            .map(|row| TenantId::new(row.get::<_, String>(0)))
            .collect()
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        let client = self.client().await?;
        let query = format!(
            "SELECT 1 FROM {} WHERE tenant_id = $1",
            qualified_table(&self.metadata_schema, "tenants")
        );
        client
            .query_opt(query.as_str(), &[&tenant_id.as_str()])
            .await
            .map(|row| row.is_some())
            .map_err(map_postgres_error)
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<PostgresTenantRegistration>> {
        let client = self.client().await?;
        let query = format!(
            "SELECT schema_name FROM {} WHERE tenant_id = $1",
            qualified_table(&self.metadata_schema, "tenants")
        );
        let row = client
            .query_opt(query.as_str(), &[&tenant_id.as_str()])
            .await
            .map_err(map_postgres_error)?;
        Ok(row.map(|row| PostgresTenantRegistration {
            tenant_id: tenant_id.clone(),
            schema_name: row.get(0),
        }))
    }

    pub async fn create_tenant(&self, tenant_id: &TenantId) -> Result<PostgresTenantRegistration> {
        let mut client = self.client().await?;
        let transaction = client.transaction().await.map_err(map_postgres_error)?;
        let fetch_query = format!(
            "SELECT schema_name FROM {} WHERE tenant_id = $1",
            qualified_table(&self.metadata_schema, "tenants")
        );
        if transaction
            .query_opt(fetch_query.as_str(), &[&tenant_id.as_str()])
            .await
            .map_err(map_postgres_error)?
            .is_some()
        {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }

        let schema_name = self.tenant_schema_name(tenant_id)?;
        let create_schema_sql = format!("CREATE SCHEMA {}", quote_identifier(&schema_name));
        transaction
            .batch_execute(create_schema_sql.as_str())
            .await
            .map_err(map_postgres_error)?;
        transaction
            .batch_execute(tenant_init_sql(&schema_name).as_str())
            .await
            .map_err(map_postgres_error)?;
        let insert_query = format!(
            "INSERT INTO {} (tenant_id, schema_name) VALUES ($1, $2)",
            qualified_table(&self.metadata_schema, "tenants")
        );
        transaction
            .execute(insert_query.as_str(), &[&tenant_id.as_str(), &schema_name])
            .await
            .map_err(map_postgres_error)?;
        transaction.commit().await.map_err(map_postgres_error)?;

        Ok(PostgresTenantRegistration {
            tenant_id: tenant_id.clone(),
            schema_name,
        })
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let mut client = self.client().await?;
        let transaction = client.transaction().await.map_err(map_postgres_error)?;
        let delete_query = format!(
            "DELETE FROM {} WHERE tenant_id = $1 RETURNING schema_name",
            qualified_table(&self.metadata_schema, "tenants")
        );
        let Some(row) = transaction
            .query_opt(delete_query.as_str(), &[&tenant_id.as_str()])
            .await
            .map_err(map_postgres_error)?
        else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };

        let schema_name: String = row.get(0);
        let drop_schema_sql = format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            quote_identifier(&schema_name)
        );
        transaction
            .batch_execute(drop_schema_sql.as_str())
            .await
            .map_err(map_postgres_error)?;
        transaction.commit().await.map_err(map_postgres_error)?;
        Ok(())
    }

    #[doc(hidden)]
    pub async fn drop_metadata_schema_for_test(&self) -> Result<()> {
        let client = self.client().await?;
        let drop_sql = format!(
            "DROP SCHEMA IF EXISTS {} CASCADE",
            quote_identifier(&self.metadata_schema)
        );
        client
            .batch_execute(drop_sql.as_str())
            .await
            .map_err(map_postgres_error)
    }

    fn open_registration(
        &self,
        registration: PostgresTenantRegistration,
    ) -> Result<OpenedPostgresTenant> {
        let store = Arc::new(PostgresTenantStore::new(self.clone(), registration));
        let read_storage = self.read_storage_for_store(store.clone());
        Ok(OpenedPostgresTenant {
            store,
            read_storage,
        })
    }

    async fn ensure_metadata_schema(&self) -> Result<()> {
        let client = self.client().await?;
        let metadata_schema = quote_identifier(&self.metadata_schema);
        let bootstrap = format!(
            "CREATE SCHEMA IF NOT EXISTS {metadata_schema}; \
             CREATE TABLE IF NOT EXISTS {metadata_schema}.tenants (\
                 tenant_id TEXT PRIMARY KEY,\
                 schema_name TEXT NOT NULL UNIQUE,\
                 created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()\
             )"
        );
        client
            .batch_execute(bootstrap.as_str())
            .await
            .map_err(map_postgres_error)
    }

    async fn client(&self) -> Result<Client> {
        self.pool.get().await.map_err(map_pool_error)
    }
}

impl PostgresNotificationListener {
    pub async fn recv(&mut self) -> Option<Result<PostgresProviderNotification>> {
        self.receiver.recv().await
    }
}

impl Drop for PostgresNotificationListener {
    fn drop(&mut self) {
        self.pump_task.abort();
    }
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

    pub fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.execute_write_cancellable(|| Ok(()), task)
    }

    pub fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        let store = self.clone();
        let runtime_handle = self.provider.runtime_handle.clone();
        bridge_tokio_runtime(
            &runtime_handle,
            "Postgres write bridge thread panicked",
            move || store.execute_write_cancellable_inline(check_cancel, task),
        )
    }

    fn execute_write_cancellable_inline<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        let mut transaction = self.begin_write_transaction_cancellable(check_cancel)?;
        let value = match task(&mut transaction) {
            Ok(value) => value,
            Err(error) => {
                transaction.rollback();
                return Err(error);
            }
        };
        let commit = transaction.commit()?;
        Ok(TenantWriteCommit { value, commit })
    }

    fn begin_write_transaction_cancellable<Check>(
        &self,
        check_cancel: Check,
    ) -> Result<PostgresWriteTransaction>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        PostgresWriteTransaction::begin(self.clone(), check_cancel)
    }

    pub fn load_schema(&self) -> Result<Schema> {
        if let Some(schema) = cached_schema(&self.schema_cache) {
            return Ok(schema);
        }
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let schema = self.block_on(async move {
            let client = provider.client().await?;
            load_schema_from_session(&client, &schema_name).await
        })?;
        publish_schema_cache(&self.schema_cache, &schema);
        Ok(schema)
    }

    pub async fn load_schema_async(&self) -> Result<Schema> {
        if let Some(schema) = cached_schema(&self.schema_cache) {
            return Ok(schema);
        }
        let client = self.provider.client().await?;
        let schema = load_schema_from_session(&client, &self.schema_name).await?;
        publish_schema_cache(&self.schema_cache, &schema);
        Ok(schema)
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.journal_progress()?.durable_head)
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.journal_progress()?.applied_head)
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_journal_progress_from_session(&client, &schema_name).await
        })
    }

    pub async fn journal_progress_async(&self) -> Result<JournalProgress> {
        let client = self.provider.client().await?;
        load_journal_progress_from_session(&client, &self.schema_name).await
    }

    pub fn recover_durable_journal(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress()?;
        if progress.applied_head.0 >= progress.durable_head.0 {
            return Ok(progress);
        }
        let from = SequenceNumber(progress.applied_head.0.saturating_add(1));
        let pending = self.read_durable_journal_from(from)?;
        self.apply_durable_records_batch(&pending)?;
        self.journal_progress()
    }

    pub async fn recover_durable_journal_async(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress_async().await?;
        if progress.applied_head.0 >= progress.durable_head.0 {
            return Ok(progress);
        }
        let from = SequenceNumber(progress.applied_head.0.saturating_add(1));
        let pending = self.read_durable_journal_from_async(from).await?;
        let store = self.clone();
        self.provider
            .runtime_handle
            .spawn_blocking(move || store.apply_durable_records_batch(&pending))
            .await
            .map_err(map_join_error)??;
        self.journal_progress_async().await
    }

    pub fn read_snapshot(&self) -> Result<PostgresReadSnapshot> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let mut client = provider.client().await?;
            let transaction = client
                .build_transaction()
                .read_only(true)
                .isolation_level(IsolationLevel::RepeatableRead)
                .start()
                .await
                .map_err(map_postgres_error)?;
            let schema = load_schema_from_session(&transaction, &schema_name).await?;
            let progress = load_journal_progress_from_session(&transaction, &schema_name).await?;
            let documents = load_documents_from_session(&transaction, &schema_name, None).await?;
            let scheduled_execution_ids =
                load_scheduled_execution_ids_from_session(&transaction, &schema_name).await?;
            transaction.commit().await.map_err(map_postgres_error)?;
            Ok(PostgresReadSnapshot {
                schema,
                progress,
                documents,
                scheduled_execution_ids,
            })
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let id = *id;
        self.block_on(async move {
            let client = provider.client().await?;
            load_document_from_session(&client, &schema_name, &table, &id).await
        })
    }

    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.scan_table_matching_with_filters_cancellable(
            table,
            &[],
            check_cancel,
            include_document,
        )
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        let documents = self.load_table_documents(table)?;
        filter_documents_with_predicate(documents, filters, check_cancel, include_document)
    }

    pub fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from(sequence)?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub async fn read_commit_log_from_async(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from_async(sequence)
            .await?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_durable_records_from_session(&client, &schema_name, sequence).await
        })
    }

    pub async fn read_durable_journal_from_async(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let client = self.provider.client().await?;
        load_durable_records_from_session(&client, &self.schema_name, sequence).await
    }

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        validate_durable_journal_stream_limit(limit)?;

        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            stream_durable_journal_from_session(&client, &schema_name, after, limit).await
        })
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        let snapshot = self
            .read_snapshot()?
            .export_materialized_journal_snapshot()?;
        let cursor_floor = self.durable_journal_cursor_floor()?;
        Ok(DurableJournalBootstrap {
            resume_after: snapshot.applied_sequence,
            bootstrap_cut: snapshot.durable_head,
            snapshot,
            cursor_floor,
        })
    }

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let execution_id = execution_id.to_string();
        self.block_on(async move {
            let client = provider.client().await?;
            let query = format!(
                "SELECT 1 FROM {} WHERE execution_id = $1",
                qualified_table(&schema_name, "scheduled_job_executions")
            );
            client
                .query_opt(query.as_str(), &[&execution_id])
                .await
                .map(|row| row.is_some())
                .map_err(map_postgres_error)
        })
    }

    pub fn replace_table_schema(&self, _table_schema: &TableSchema) -> Result<()> {
        let table_schema = _table_schema.clone();
        self.execute_write(move |transaction| transaction.replace_table_schema(&table_schema))?;
        Ok(())
    }

    pub fn delete_table_schema(&self, _table: &TableName) -> Result<()> {
        let table = _table.clone();
        self.execute_write(move |transaction| transaction.delete_table_schema(&table))?;
        Ok(())
    }

    pub fn append_durable_records_batch(&self, _records: &[DurableMutationRecord]) -> Result<()> {
        let records = _records.to_vec();
        self.execute_write(move |transaction| transaction.append_durable_records_batch(&records))?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, _records: &[DurableMutationRecord]) -> Result<()> {
        let records = _records.to_vec();
        self.execute_write(move |transaction| transaction.apply_durable_records_batch(&records))?;
        Ok(())
    }

    pub fn insert_scheduled_job(&self, _job: &ScheduledJob) -> Result<()> {
        let job = _job.clone();
        self.execute_write(move |transaction| transaction.insert_scheduled_job(&job))?;
        Ok(())
    }

    pub fn claim_due_jobs(&self, _now: Timestamp) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(move |transaction| transaction.claim_due_jobs(_now))?
            .value)
    }

    pub fn complete_scheduled_job(&self, _job_id: &DocumentId) -> Result<()> {
        let job_id = *_job_id;
        self.execute_write(move |transaction| transaction.complete_scheduled_job(&job_id))?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&self, _job_id: &DocumentId) -> Result<bool> {
        let job_id = *_job_id;
        Ok(self
            .execute_write(move |transaction| transaction.cancel_scheduled_job(&job_id))?
            .value)
    }

    pub fn record_scheduled_job_result(&self, _result: &ScheduledJobResult) -> Result<()> {
        let result = _result.clone();
        self.execute_write(move |transaction| transaction.record_scheduled_job_result(&result))?;
        Ok(())
    }

    pub fn get_scheduled_job_result(
        &self,
        _job_id: &DocumentId,
    ) -> Result<Option<ScheduledJobResult>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let job_id = *_job_id;
        self.block_on(async move {
            let client = provider.client().await?;
            load_scheduled_job_result_from_session(&client, &schema_name, &job_id).await
        })
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_scheduled_jobs_from_session(&client, &schema_name, "scheduled_jobs").await
        })
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_cron_jobs_from_session(&client, &schema_name).await
        })
    }

    pub fn save_cron_job(&self, _cron: &CronJob) -> Result<()> {
        let cron = _cron.clone();
        self.execute_write(move |transaction| transaction.save_cron_job(&cron))?;
        Ok(())
    }

    pub fn delete_cron_job(&self, _name: &str) -> Result<()> {
        let name = _name.to_string();
        self.execute_write(move |transaction| transaction.delete_cron_job(name.as_str()))?;
        Ok(())
    }

    pub fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        let next_job_at = self.list_scheduled_jobs()?.first().map(|job| job.run_at);
        let next_cron_at = self
            .load_cron_jobs()?
            .into_iter()
            .filter(|cron| cron.enabled)
            .map(|cron| cron.next_run)
            .min();
        Ok(match (next_job_at, next_cron_at) {
            (Some(left), Some(right)) => Some(left.min(right)),
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        })
    }

    pub fn has_scheduled_work(&self) -> Result<bool> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move { has_scheduled_work_from_provider(provider, schema_name).await })
    }

    pub async fn has_scheduled_work_async(&self) -> Result<bool> {
        has_scheduled_work_from_provider(self.provider.clone(), self.schema_name.clone()).await
    }

    pub fn recover_running_jobs(&self, _now: Timestamp) -> Result<()> {
        self.execute_write(move |transaction| transaction.recover_running_jobs(_now))?;
        Ok(())
    }

    pub fn apply_execution_unit_batch(
        &self,
        _writes: &[ResolvedWrite],
        _schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        if _writes.is_empty() && _schedule_ops.is_empty() {
            return Err(Error::Internal(
                "execution-unit batch must contain at least one change".to_string(),
            ));
        }

        let writes = _writes.to_vec();
        let schedule_ops = _schedule_ops.to_vec();
        let committed = self.execute_write(move |transaction| {
            for write in &writes {
                transaction.apply_resolved_write(write)?;
            }
            apply_schedule_ops_in_transaction(transaction, &schedule_ops)?;
            Ok(())
        })?;
        Ok(committed.commit)
    }

    pub fn now(&self) -> Timestamp {
        self.provider.clock.now()
    }

    pub fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.provider.fault_injector.check(point)
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(
            table,
            index_name,
            std::slice::from_ref(value),
            check_cancel,
        )
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.load_index_documents_cancellable(
            table,
            index_name,
            prefix_values,
            None,
            None,
            true,
            true,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.load_index_documents_cancellable(
            table,
            index_name,
            &[],
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.load_index_documents_cancellable(
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn load_table_documents(&self, table: &TableName) -> Result<Vec<Document>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_documents_from_session(&client, &schema_name, Some(&table)).await
        })
    }

    fn load_table_schema(&self, table: &TableName) -> Result<TableSchema> {
        self.load_schema()?
            .get_table(table)
            .cloned()
            .ok_or(Error::SchemaNotFound(table.clone()))
    }

    #[allow(clippy::too_many_arguments)]
    fn load_index_documents_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let table_schema = self.load_table_schema(table)?;
        let index_fields = index_fields_for_table_schema(&table_schema, index_name)?;
        if exact_prefix.len() > index_fields.len() {
            return Err(Error::InvalidInput(format!(
                "index prefix length {} exceeds index '{}' field count {}",
                exact_prefix.len(),
                index_name,
                index_fields.len()
            )));
        }
        if (start.is_some() || end.is_some()) && exact_prefix.len() >= index_fields.len() {
            return Err(Error::InvalidInput(format!(
                "composite range prefix length {} leaves no range field for index '{}'",
                exact_prefix.len(),
                index_name
            )));
        }

        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table_for_query = table.clone();
        let table_for_filter = table.clone();
        let table_schema_for_query = table_schema.clone();
        let exact_prefix = exact_prefix.to_vec();
        let exact_prefix_for_query = exact_prefix.clone();
        let start = start.cloned();
        let start_for_query = start.clone();
        let end = end.cloned();
        let end_for_query = end.clone();
        let index_name = index_name.to_string();
        let documents = self.block_on(async move {
            let client = provider.client().await?;
            load_index_candidate_documents_from_session(
                &client,
                &schema_name,
                &table_for_query,
                &table_schema_for_query,
                index_name.as_str(),
                &exact_prefix_for_query,
                start_for_query.as_ref(),
                end_for_query.as_ref(),
                start_inclusive,
                end_inclusive,
            )
            .await
        })?;

        filter_index_documents_with_cancel(
            documents,
            &table_for_filter,
            &index_fields,
            &exact_prefix,
            start.as_ref(),
            end.as_ref(),
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    pub fn insert(&self, _document: &Document) -> Result<CommitEntry> {
        self.insert_once(_document, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated insert should commit".to_string()))
    }

    pub fn insert_with_indexes(
        &self,
        _document: &Document,
        _indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        self.insert(_document)
    }

    pub fn insert_once(
        &self,
        _document: &Document,
        _execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        let document = _document.clone();
        let execution_id = _execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.insert_document(&document)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(expect_write_commit(
                committed.commit,
                "deduplicated insert should record a commit entry",
            )?)
        } else {
            None
        })
    }

    pub fn insert_with_indexes_once(
        &self,
        _document: &Document,
        _indexes: &[IndexDefinition],
        _execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        self.insert_once(_document, _execution_id)
    }

    pub fn update_validated<F>(
        &self,
        _table: &TableName,
        _id: &DocumentId,
        _patch: &serde_json::Map<String, Value>,
        _validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(_table, _id, _patch, None, _validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated update should commit".to_string()))
    }

    pub fn update_validated_once<F>(
        &self,
        _table: &TableName,
        _id: &DocumentId,
        _patch: &serde_json::Map<String, Value>,
        _execution_id: Option<&str>,
        _validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        let table = _table.clone();
        let id = *_id;
        let patch = _patch.clone();
        let execution_id = _execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.update_document_validated(&table, &id, &patch, _validate)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(expect_write_commit(
                committed.commit,
                "deduplicated update should record a commit entry",
            )?)
        } else {
            None
        })
    }

    pub fn update_with_indexes_validated<F>(
        &self,
        _table: &TableName,
        _id: &DocumentId,
        _patch: &serde_json::Map<String, Value>,
        _indexes: &[IndexDefinition],
        _validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated(_table, _id, _patch, _validate)
    }

    pub fn update_with_indexes_validated_once<F>(
        &self,
        _table: &TableName,
        _id: &DocumentId,
        _patch: &serde_json::Map<String, Value>,
        _indexes: &[IndexDefinition],
        _execution_id: Option<&str>,
        _validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(_table, _id, _patch, _execution_id, _validate)
    }

    pub fn delete_validated_returning_document<F>(
        &self,
        _table: &TableName,
        _id: &DocumentId,
        _validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(_table, _id, None, _validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    pub fn delete_validated_once<F>(
        &self,
        _table: &TableName,
        _id: &DocumentId,
        _execution_id: Option<&str>,
        _validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        let table = _table.clone();
        let id = *_id;
        let execution_id = _execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(None);
            }
            let removed_document = transaction.delete_document_validated(&table, &id, _validate)?;
            Ok(Some(removed_document))
        })?;
        Ok(if let Some(removed_document) = committed.value {
            Some((
                expect_write_commit(
                    committed.commit,
                    "deduplicated delete should record a commit entry",
                )?,
                removed_document,
            ))
        } else {
            None
        })
    }

    pub fn delete_with_indexes_validated_returning_document<F>(
        &self,
        _table: &TableName,
        _id: &DocumentId,
        _indexes: &[IndexDefinition],
        _validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_returning_document(_table, _id, _validate)
    }

    pub fn delete_with_indexes_validated_once<F>(
        &self,
        _table: &TableName,
        _id: &DocumentId,
        _indexes: &[IndexDefinition],
        _execution_id: Option<&str>,
        _validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(_table, _id, _execution_id, _validate)
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

    fn durable_journal_cursor_floor(&self) -> Result<SequenceNumber> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            load_durable_journal_cursor_floor_from_session(&client, &schema_name).await
        })
    }
}

async fn has_scheduled_work_from_provider(
    provider: PostgresProvider,
    schema_name: String,
) -> Result<bool> {
    let client = provider.client().await?;
    Ok(
        table_has_rows_in_session(&client, &schema_name, "scheduled_jobs").await?
            || table_has_rows_in_session(&client, &schema_name, "running_scheduled_jobs").await?
            || table_has_rows_in_session(&client, &schema_name, "cron_jobs").await?,
    )
}

impl PostgresReadSnapshot {
    pub fn load_schema(&self) -> Result<Schema> {
        Ok(self.schema.clone())
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.progress.durable_head)
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.progress.applied_head)
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        Ok(self.progress)
    }

    pub fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        Ok(MaterializedJournalSnapshot {
            version: MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
            applied_sequence: self.progress.applied_head,
            durable_head: self.progress.durable_head,
            schema: self.schema.clone(),
            documents: self.documents.clone(),
            scheduled_execution_ids: self.scheduled_execution_ids.clone(),
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        Ok(self
            .documents
            .iter()
            .find(|document| &document.table == table && &document.id == id)
            .cloned())
    }

    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.scan_table_matching_with_filters_cancellable(
            table,
            &[],
            check_cancel,
            include_document,
        )
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        mut include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        let mut documents = Vec::new();
        for document in self
            .documents
            .iter()
            .filter(|document| &document.table == table)
        {
            check_cancel()?;
            if matches_filters(document, filters)? && include_document(document)? {
                documents.push(document.clone());
            }
        }
        Ok(documents)
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(
            table,
            index_name,
            std::slice::from_ref(value),
            check_cancel,
        )
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let index_fields = self.index_fields(table, index_name)?;
        if prefix_values.len() > index_fields.len() {
            return Err(Error::InvalidInput(format!(
                "index prefix length {} exceeds index '{}' field count {}",
                prefix_values.len(),
                index_name,
                index_fields.len()
            )));
        }
        self.filter_index_documents(
            table,
            &index_fields,
            prefix_values,
            None,
            None,
            true,
            true,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let index_fields = self.index_fields(table, index_name)?;
        self.filter_index_documents(
            table,
            &index_fields,
            &[],
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let index_fields = self.index_fields(table, index_name)?;
        if exact_prefix.len() >= index_fields.len() {
            return Err(Error::InvalidInput(format!(
                "composite range prefix length {} leaves no range field for index '{}'",
                exact_prefix.len(),
                index_name
            )));
        }
        self.filter_index_documents(
            table,
            &index_fields,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    fn index_fields(&self, table: &TableName, index_name: &str) -> Result<Vec<String>> {
        let table_schema = self
            .schema
            .get_table(table)
            .ok_or_else(|| Error::SchemaNotFound(table.clone()))?;
        let index = table_schema
            .indexes
            .iter()
            .find(|index| index.name == index_name)
            .ok_or_else(|| {
                Error::InvalidInput(format!(
                    "index '{}' not found for table '{}'",
                    index_name,
                    table.as_str()
                ))
            })?;
        Ok(index.fields.clone())
    }

    #[allow(clippy::too_many_arguments)]
    fn filter_index_documents(
        &self,
        table: &TableName,
        index_fields: &[String],
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let range_field = index_fields.get(exact_prefix.len());
        let mut documents = Vec::new();
        for document in self
            .documents
            .iter()
            .filter(|document| &document.table == table)
        {
            check_cancel()?;
            if !document_matches_exact_prefix(document, index_fields, exact_prefix) {
                continue;
            }
            if let Some(range_field) = range_field
                && !document_matches_range_bounds(
                    document,
                    range_field,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                )?
            {
                continue;
            }
            documents.push(document.clone());
        }
        Ok(documents)
    }
}

impl PostgresTenantStorage {
    pub fn new(store: Arc<PostgresTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self::with_max_concurrent_reads(store, runtime_handle, default_postgres_read_parallelism())
    }

    pub fn with_max_concurrent_reads(
        store: Arc<PostgresTenantStore>,
        runtime_handle: TokioRuntimeHandle,
        max_concurrent_reads: usize,
    ) -> Self {
        Self {
            store: store.clone(),
            permits: Arc::new(Semaphore::new(max_concurrent_reads.max(1))),
            runtime_handle: runtime_handle.clone(),
            write_executor: PostgresBlockingWriteExecutor::new(store, runtime_handle),
        }
    }

    pub fn store(&self) -> Arc<PostgresTenantStore> {
        self.store.clone()
    }
}

impl TenantReadStorage for PostgresTenantStorage {
    type Store = PostgresTenantStore;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<PostgresTenantStore>) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(map_permit_error)?;
        let store = self.store.clone();
        self.runtime_handle
            .spawn_blocking(move || {
                let _permit = permit;
                task(store)
            })
            .await
            .map_err(map_join_error)?
    }

    async fn execute_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(Arc<PostgresTenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T>
            + Send
            + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Err(Error::Cancelled),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let store = self.store.clone();
        let cancelled_for_task = cancelled.clone();
        let mut handle = self.runtime_handle.spawn_blocking(move || {
            let _permit = permit;
            let mut combined_cancel = || {
                if cancelled_for_task.load(AtomicOrdering::SeqCst) {
                    return Err(Error::Cancelled);
                }
                check_cancel()
            };
            task(store, &mut combined_cancel)
        });

        tokio::select! {
            _ = &mut cancel_wait => {
                cancelled.store(true, AtomicOrdering::SeqCst);
                handle.abort();
                Err(Error::Cancelled)
            }
            result = &mut handle => result.map_err(map_join_error)?,
        }
    }
}

impl TenantWriteStorage for PostgresTenantStorage {
    type WriteTransaction = PostgresWriteTransaction;

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor.execute_write(task).await
    }

    async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor
            .execute_write_cancellable(cancel_wait, check_cancel, task)
            .await
    }
}

impl PostgresBlockingWriteExecutor {
    fn new(store: Arc<PostgresTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self {
            store,
            permits: Arc::new(Semaphore::new(POSTGRES_TENANT_WRITE_PARALLELISM)),
            runtime_handle,
        }
    }

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(map_permit_error)?;
        let store = self.store.clone();
        self.runtime_handle
            .spawn_blocking(move || {
                let _permit = permit;
                store.execute_write(task)
            })
            .await
            .map_err(map_join_error)?
    }

    async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Ok(TenantWriteOutcome::CancelledBeforeCommit),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let store = self.store.clone();
        let cancelled_for_task = cancelled.clone();
        let mut handle = self.runtime_handle.spawn_blocking(move || {
            let _permit = permit;
            store.execute_write_cancellable(
                move || {
                    if cancelled_for_task.load(AtomicOrdering::SeqCst) {
                        return Err(Error::Cancelled);
                    }
                    check_cancel()
                },
                task,
            )
        });

        tokio::select! {
            result = &mut handle => map_write_result(result.map_err(map_join_error)?),
            _ = &mut cancel_wait => {
                cancelled.store(true, AtomicOrdering::SeqCst);
                map_write_result(handle.await.map_err(map_join_error)?)
            }
        }
    }
}

fn map_write_result<T>(result: Result<TenantWriteCommit<T>>) -> Result<TenantWriteOutcome<T>> {
    match result {
        Ok(committed) => Ok(TenantWriteOutcome::Committed(committed)),
        Err(Error::Cancelled) => Ok(TenantWriteOutcome::CancelledBeforeCommit),
        Err(error) => Err(error),
    }
}

impl PostgresWriteTransaction {
    fn begin<Check>(store: PostgresTenantStore, check_cancel: Check) -> Result<Self>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let provider = store.provider.clone();
        let tenant_id = store.tenant_id.clone();
        let schema_name = store.schema_name.clone();
        let client = store.block_on({
            let provider = provider.clone();
            async move { provider.client().await }
        })?;

        let mut transaction = Self {
            provider,
            tenant_id,
            schema_name,
            schema_cache: store.schema_cache.clone(),
            client: Some(client),
            commit_writes: Vec::new(),
            notification: PendingPostgresNotification::default(),
            schema_cache_changed: false,
            check_cancel: Box::new(check_cancel),
        };
        if let Err(error) = (|| -> Result<()> {
            transaction.check_cancel()?;
            transaction.batch_execute("BEGIN")?;
            transaction.acquire_tenant_lock()?;
            transaction.ensure_metadata_rows()?;
            Ok(())
        })() {
            transaction.rollback();
            return Err(error);
        }
        Ok(transaction)
    }

    pub fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        if let Some(previous) = self.load_table_schema(&table_schema.table)? {
            self.drop_table_indexes(&previous)?;
        }
        self.upsert_table_schema(table_schema)?;
        self.create_table_indexes(table_schema)?;
        self.notification.schema_changed = true;
        self.schema_cache_changed = true;
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        if let Some(previous) = self.load_table_schema(table)? {
            self.drop_table_indexes(&previous)?;
        }
        self.delete_table_schema_entry(table)?;
        self.notification.schema_changed = true;
        self.schema_cache_changed = true;
        Ok(())
    }

    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        let schema_name = self.schema_name.clone();
        let execution_id = execution_id.map(str::to_string);
        let client = self.session()?;
        self.block_on(async move {
            begin_scheduled_execution_in_session(client, &schema_name, execution_id.as_deref())
                .await
        })
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (table_name, id, data_json, creation_time) VALUES ($1, $2, $3, $4)",
            qualified_table(&self.schema_name, "documents")
        );
        let table = document.table.as_str().to_string();
        let id = document.id.to_string();
        let data_json = serialize_document_fields(document)?;
        let creation_time = i64_from_timestamp(document.creation_time)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&table, &id, &data_json, &creation_time])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.record_commit_write(WriteOp {
            table: document.table.clone(),
            op_type: WriteOpType::Insert,
            doc_id: document.id,
            previous: None,
            current: Some(document.clone()),
        });
        Ok(())
    }

    pub fn update_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        validate: F,
    ) -> Result<()>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        self.check_cancel()?;
        let existing_document = self
            .load_document(table, id)?
            .ok_or(Error::DocumentNotFound(*id))?;
        let mut document = existing_document.clone();
        for (field, value) in patch {
            document.fields.insert(field.clone(), value.clone());
        }
        validate(&existing_document, &document)?;

        let query = format!(
            "UPDATE {} SET data_json = $3, creation_time = $4 WHERE table_name = $1 AND id = $2",
            qualified_table(&self.schema_name, "documents")
        );
        let table_name = table.as_str().to_string();
        let document_id = id.to_string();
        let data_json = serialize_document_fields(&document)?;
        let creation_time = i64_from_timestamp(document.creation_time)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(
                    query.as_str(),
                    &[&table_name, &document_id, &data_json, &creation_time],
                )
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Update,
            doc_id: *id,
            previous: Some(existing_document),
            current: Some(document),
        });
        Ok(())
    }

    pub fn delete_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<Document>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.check_cancel()?;
        let removed_document = self
            .load_document(table, id)?
            .ok_or(Error::DocumentNotFound(*id))?;
        validate(&removed_document)?;

        let query = format!(
            "DELETE FROM {} WHERE table_name = $1 AND id = $2",
            qualified_table(&self.schema_name, "documents")
        );
        let table_name = table.as_str().to_string();
        let document_id = id.to_string();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&table_name, &document_id])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Delete,
            doc_id: *id,
            previous: Some(removed_document.clone()),
            current: None,
        });
        Ok(removed_document)
    }

    pub fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (id, run_at, data_json) VALUES ($1, $2, $3)",
            qualified_table(&self.schema_name, "scheduled_jobs")
        );
        let id = job.id.to_string();
        let run_at = i64_from_timestamp(job.run_at)?;
        let data_json = serialize_json(job)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&id, &run_at, &data_json])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.notification.scheduler_changed = true;
        Ok(())
    }

    pub fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        let query = format!(
            "SELECT data_json FROM {} WHERE run_at <= $1 ORDER BY run_at, id",
            qualified_table(&self.schema_name, "scheduled_jobs")
        );
        let run_at = claim_due_jobs_upper_bound(now);
        let client = self.session()?;
        let due = self.block_on(async move {
            let rows = client
                .query(query.as_str(), &[&run_at])
                .await
                .map_err(map_postgres_error)?;
            rows.into_iter()
                .map(|row| deserialize_json::<ScheduledJob>(row.get::<_, String>(0).as_str()))
                .collect::<Result<Vec<_>>>()
        })?;

        if due.is_empty() {
            return Ok(Vec::new());
        }

        let delete_query = format!(
            "DELETE FROM {} WHERE id = $1",
            qualified_table(&self.schema_name, "scheduled_jobs")
        );
        let insert_query = format!(
            "INSERT INTO {} (id, data_json) VALUES ($1, $2)",
            qualified_table(&self.schema_name, "running_scheduled_jobs")
        );
        for job in &due {
            self.check_cancel()?;
            let job_id = job.id.to_string();
            let data_json = serialize_json(job)?;
            let delete_query = delete_query.clone();
            let insert_query = insert_query.clone();
            let client = self.session()?;
            self.block_on(async move {
                client
                    .execute(delete_query.as_str(), &[&job_id])
                    .await
                    .map_err(map_postgres_error)?;
                client
                    .execute(insert_query.as_str(), &[&job_id, &data_json])
                    .await
                    .map_err(map_postgres_error)?;
                Ok(())
            })?;
        }
        self.notification.scheduler_changed = true;
        Ok(due)
    }

    pub fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "DELETE FROM {} WHERE id = $1",
            qualified_table(&self.schema_name, "running_scheduled_jobs")
        );
        let job_id = job_id.to_string();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&job_id])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.notification.scheduler_changed = true;
        Ok(())
    }

    pub fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.check_cancel()?;
        let query = format!(
            "DELETE FROM {} WHERE id = $1",
            qualified_table(&self.schema_name, "scheduled_jobs")
        );
        let job_id = job_id.to_string();
        let client = self.session()?;
        let removed = self.block_on(async move {
            client
                .execute(query.as_str(), &[&job_id])
                .await
                .map(|affected| affected == 1)
                .map_err(map_postgres_error)
        })?;
        if removed {
            self.notification.scheduler_changed = true;
        }
        Ok(removed)
    }

    pub fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (job_id, data_json) VALUES ($1, $2)
             ON CONFLICT(job_id) DO UPDATE SET data_json = EXCLUDED.data_json",
            qualified_table(&self.schema_name, "scheduled_job_results")
        );
        let job_id = result.id.to_string();
        let data_json = serialize_json(result)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&job_id, &data_json])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.notification.scheduler_changed = true;
        Ok(())
    }

    pub fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (name, next_run, enabled, data_json) VALUES ($1, $2, $3, $4)
             ON CONFLICT(name) DO UPDATE
             SET next_run = EXCLUDED.next_run,
                 enabled = EXCLUDED.enabled,
                 data_json = EXCLUDED.data_json",
            qualified_table(&self.schema_name, "cron_jobs")
        );
        let name = cron.name.clone();
        let next_run = i64_from_timestamp(cron.next_run)?;
        let enabled = cron.enabled;
        let data_json = serialize_json(cron)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&name, &next_run, &enabled, &data_json])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.notification.scheduler_changed = true;
        Ok(())
    }

    pub fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "DELETE FROM {} WHERE name = $1",
            qualified_table(&self.schema_name, "cron_jobs")
        );
        let name = name.to_string();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&name])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.notification.scheduler_changed = true;
        Ok(())
    }

    pub fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        self.check_cancel()?;
        let running_jobs = self.load_running_jobs()?;
        let delete_query = format!(
            "DELETE FROM {} WHERE id = $1",
            qualified_table(&self.schema_name, "running_scheduled_jobs")
        );
        let insert_query = format!(
            "INSERT INTO {} (id, run_at, data_json) VALUES ($1, $2, $3)",
            qualified_table(&self.schema_name, "scheduled_jobs")
        );
        for mut job in running_jobs {
            self.check_cancel()?;
            job.run_at = now;
            let job_id = job.id.to_string();
            let run_at = i64_from_timestamp(job.run_at)?;
            let data_json = serialize_json(&job)?;
            let insert_query = insert_query.clone();
            let delete_query = delete_query.clone();
            let client = self.session()?;
            self.block_on(async move {
                client
                    .execute(insert_query.as_str(), &[&job_id, &run_at, &data_json])
                    .await
                    .map_err(map_postgres_error)?;
                client
                    .execute(delete_query.as_str(), &[&job_id])
                    .await
                    .map_err(map_postgres_error)?;
                Ok(())
            })?;
        }
        self.notification.scheduler_changed = true;
        Ok(())
    }

    pub fn append_durable_records_batch(
        &mut self,
        records: &[DurableMutationRecord],
    ) -> Result<()> {
        self.check_cancel()?;
        if records.is_empty() {
            return Ok(());
        }

        let mut next = self.latest_sequence()?.0.saturating_add(1);
        let query = format!(
            "INSERT INTO {} (sequence, record_blob) VALUES ($1, $2)",
            qualified_table(&self.schema_name, "commit_log")
        );
        for record in records {
            self.check_cancel()?;
            if record.sequence.0 != next {
                return Err(Error::Internal(format!(
                    "durable journal append expected sequence {}, got {}",
                    next, record.sequence.0
                )));
            }
            let sequence = i64_from_sequence(record.sequence)?;
            let payload = serialize_durable_record(record)?;
            let query = query.clone();
            let client = self.session()?;
            self.block_on(async move {
                client
                    .execute(query.as_str(), &[&sequence, &payload])
                    .await
                    .map_err(map_postgres_error)?;
                Ok(())
            })?;
            next = next.saturating_add(1);
        }
        self.provider
            .fault_injector
            .check(FaultPoint::JournalAppendBeforeDurableFlush)?;
        self.provider
            .fault_injector
            .check(FaultPoint::JournalFlushBeforeVisibility)?;
        self.notification.journal_changed = true;
        Ok(())
    }

    pub fn apply_durable_records_batch(&mut self, records: &[DurableMutationRecord]) -> Result<()> {
        self.check_cancel()?;
        if records.is_empty() {
            return Ok(());
        }

        let mut applied_head = self.applied_sequence()?.0;
        for record in records {
            self.check_cancel()?;
            if record.sequence.0 <= applied_head {
                continue;
            }
            if record.sequence.0 != applied_head.saturating_add(1) {
                return Err(Error::Internal(format!(
                    "durable journal apply expected sequence {}, got {}",
                    applied_head.saturating_add(1),
                    record.sequence.0
                )));
            }
            self.apply_durable_record(record)?;
            applied_head = record.sequence.0;
        }

        if applied_head >= records[0].sequence.0 {
            self.write_applied_sequence(SequenceNumber(applied_head))?;
        }
        Ok(())
    }

    pub fn apply_resolved_write(&mut self, write: &ResolvedWrite) -> Result<()> {
        match write {
            ResolvedWrite::Insert { document, .. } => {
                self.check_cancel()?;
                if self.load_document(&document.table, &document.id)?.is_some() {
                    return Err(Error::Conflict(format!(
                        "document {} changed before transaction commit",
                        document.id
                    )));
                }
                self.insert_document(document)
            }
            ResolvedWrite::Update {
                previous, current, ..
            } => {
                self.check_cancel()?;
                let existing =
                    self.load_document(&current.table, &current.id)?
                        .ok_or(Error::Conflict(format!(
                            "document {} changed before transaction commit",
                            current.id
                        )))?;
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "document {} changed before transaction commit",
                        current.id
                    )));
                }

                let query = format!(
                    "UPDATE {} SET data_json = $3, creation_time = $4 WHERE table_name = $1 AND id = $2",
                    qualified_table(&self.schema_name, "documents")
                );
                let table_name = current.table.as_str().to_string();
                let document_id = current.id.to_string();
                let data_json = serialize_document_fields(current)?;
                let creation_time = i64_from_timestamp(current.creation_time)?;
                let client = self.session()?;
                self.block_on(async move {
                    client
                        .execute(
                            query.as_str(),
                            &[&table_name, &document_id, &data_json, &creation_time],
                        )
                        .await
                        .map_err(map_postgres_error)?;
                    Ok(())
                })?;
                self.record_commit_write(WriteOp {
                    table: current.table.clone(),
                    op_type: WriteOpType::Update,
                    doc_id: current.id,
                    previous: Some(previous.clone()),
                    current: Some(current.clone()),
                });
                Ok(())
            }
            ResolvedWrite::Delete { previous, .. } => {
                self.check_cancel()?;
                let existing =
                    self.load_document(&previous.table, &previous.id)?
                        .ok_or(Error::Conflict(format!(
                            "document {} changed before transaction commit",
                            previous.id
                        )))?;
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "document {} changed before transaction commit",
                        previous.id
                    )));
                }

                let query = format!(
                    "DELETE FROM {} WHERE table_name = $1 AND id = $2",
                    qualified_table(&self.schema_name, "documents")
                );
                let table_name = previous.table.as_str().to_string();
                let document_id = previous.id.to_string();
                let client = self.session()?;
                self.block_on(async move {
                    client
                        .execute(query.as_str(), &[&table_name, &document_id])
                        .await
                        .map_err(map_postgres_error)?;
                    Ok(())
                })?;
                self.record_commit_write(WriteOp {
                    table: previous.table.clone(),
                    op_type: WriteOpType::Delete,
                    doc_id: previous.id,
                    previous: Some(previous.clone()),
                    current: None,
                });
                Ok(())
            }
        }
    }

    pub fn commit(mut self) -> Result<Option<CommitEntry>> {
        self.check_cancel()?;
        let commit = if self.commit_writes.is_empty() {
            None
        } else {
            let writes = std::mem::take(&mut self.commit_writes);
            Some(self.append_commit_entry(writes)?)
        };
        self.enqueue_notification()?;
        self.provider
            .fault_injector
            .check(FaultPoint::StorageCommitBeforeVisibility)?;
        self.batch_execute("COMMIT")?;
        if self.schema_cache_changed {
            invalidate_schema_cache_handle(&self.schema_cache);
        }
        self.provider
            .fault_injector
            .check(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
        Ok(commit)
    }

    pub fn rollback(&mut self) {
        let _ = self.batch_execute("ROLLBACK");
    }

    pub(crate) fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    fn batch_execute(&mut self, sql: &str) -> Result<()> {
        let sql = sql.to_string();
        let client = self.session()?;
        self.block_on(async move {
            client
                .batch_execute(sql.as_str())
                .await
                .map_err(map_postgres_error)
        })
    }

    fn block_on<T, Fut>(&self, future: Fut) -> Result<T>
    where
        Fut: Future<Output = Result<T>>,
    {
        self.provider.runtime_handle.block_on(future)
    }

    fn session(&self) -> Result<&Client> {
        self.client
            .as_ref()
            .ok_or_else(|| Error::Internal("Postgres write transaction already closed".to_string()))
    }

    fn acquire_tenant_lock(&mut self) -> Result<()> {
        let lock_key = tenant_advisory_lock_key(&self.tenant_id);
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute("SELECT pg_advisory_xact_lock($1)", &[&lock_key])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }

    fn ensure_metadata_rows(&mut self) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (key, value_blob) VALUES ($1, $2) ON CONFLICT(key) DO NOTHING",
            qualified_table(&self.schema_name, "metadata")
        );
        let key = APPLIED_SEQUENCE_KEY.to_string();
        let value = encode_u64(0).to_vec();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&key, &value])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }

    fn latest_sequence(&mut self) -> Result<SequenceNumber> {
        let schema_name = self.schema_name.clone();
        let client = self.session()?;
        self.block_on(async move { load_latest_sequence_from_session(client, &schema_name).await })
    }

    fn applied_sequence(&mut self) -> Result<SequenceNumber> {
        let schema_name = self.schema_name.clone();
        let client = self.session()?;
        self.block_on(async move {
            Ok(
                load_metadata_u64_from_session(client, &schema_name, APPLIED_SEQUENCE_KEY)
                    .await?
                    .map(SequenceNumber)
                    .unwrap_or(SequenceNumber(0)),
            )
        })
    }

    fn append_commit_entry(&mut self, writes: Vec<WriteOp>) -> Result<CommitEntry> {
        let sequence = SequenceNumber(self.latest_sequence()?.0.saturating_add(1));
        let entry = CommitEntry {
            sequence,
            timestamp: self.provider.clock.now(),
            writes,
        };
        let query = format!(
            "INSERT INTO {} (sequence, record_blob) VALUES ($1, $2)",
            qualified_table(&self.schema_name, "commit_log")
        );
        let sequence_i64 = i64_from_sequence(entry.sequence)?;
        let payload = serialize_commit(&entry)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&sequence_i64, &payload])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.write_applied_sequence(entry.sequence)?;
        self.notification.journal_changed = true;
        Ok(entry)
    }

    fn write_applied_sequence(&mut self, sequence: SequenceNumber) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (key, value_blob) VALUES ($1, $2)
             ON CONFLICT(key) DO UPDATE SET value_blob = EXCLUDED.value_blob",
            qualified_table(&self.schema_name, "metadata")
        );
        let key = APPLIED_SEQUENCE_KEY.to_string();
        let value = encode_u64(sequence.0).to_vec();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&key, &value])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }

    fn load_document(&mut self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let id = *id;
        let client = self.session()?;
        self.block_on(
            async move { load_document_from_session(client, &schema_name, &table, &id).await },
        )
    }

    fn load_table_schema(&mut self, table: &TableName) -> Result<Option<TableSchema>> {
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let client = self.session()?;
        self.block_on(
            async move { load_table_schema_from_session(client, &schema_name, &table).await },
        )
    }

    fn upsert_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (table_name, schema_json) VALUES ($1, $2)
             ON CONFLICT(table_name) DO UPDATE SET schema_json = EXCLUDED.schema_json",
            qualified_table(&self.schema_name, "schemas")
        );
        let table_name = table_schema.table.as_str().to_string();
        let schema_json = serialize_json(table_schema)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&table_name, &schema_json])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }

    fn delete_table_schema_entry(&mut self, table: &TableName) -> Result<()> {
        let query = format!(
            "DELETE FROM {} WHERE table_name = $1",
            qualified_table(&self.schema_name, "schemas")
        );
        let table_name = table.as_str().to_string();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&table_name])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }

    fn create_table_indexes(&mut self, table_schema: &TableSchema) -> Result<()> {
        let schema_name = self.schema_name.clone();
        let table_schema = table_schema.clone();
        let client = self.session()?;
        self.block_on(async move {
            create_postgres_indexes_for_table_schema(client, &schema_name, &table_schema).await
        })
    }

    fn drop_table_indexes(&mut self, table_schema: &TableSchema) -> Result<()> {
        let schema_name = self.schema_name.clone();
        let table_schema = table_schema.clone();
        let client = self.session()?;
        self.block_on(async move {
            drop_postgres_indexes_for_table_schema(client, &schema_name, &table_schema).await
        })
    }

    fn load_running_jobs(&mut self) -> Result<Vec<ScheduledJob>> {
        let schema_name = self.schema_name.clone();
        let client = self.session()?;
        self.block_on(async move {
            load_scheduled_jobs_from_session(client, &schema_name, "running_scheduled_jobs").await
        })
    }

    fn apply_durable_record(&mut self, record: &DurableMutationRecord) -> Result<()> {
        let schema_name = self.schema_name.clone();
        let record = record.clone();
        let client = self.session()?;
        self.block_on(async move {
            apply_durable_record_in_session(client, &schema_name, &record).await
        })
    }

    fn record_commit_write(&mut self, write: WriteOp) {
        self.commit_writes.push(write);
    }

    fn enqueue_notification(&mut self) -> Result<()> {
        if !self.notification.has_any() {
            return Ok(());
        }
        let query = "SELECT pg_notify($1, $2)";
        let channel = self.provider.notification_channel.clone();
        let payload = serde_json::to_string(&PostgresProviderNotificationPayload {
            tenant_id: self.tenant_id.to_string(),
            journal_changed: self.notification.journal_changed,
            scheduler_changed: self.notification.scheduler_changed,
            schema_changed: self.notification.schema_changed,
        })
        .map_err(|error| Error::Serialization(error.to_string()))?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query, &[&channel, &payload])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }
}

fn build_pool(config: &PostgresProviderConfig, pool_application_name: &str) -> Result<Pool> {
    if let (Some(min_connections), Some(max_connections)) =
        (config.min_connections, config.max_connections)
        && min_connections > max_connections
    {
        return Err(Error::InvalidInput(
            "postgres pool min_connections cannot exceed max_connections".to_string(),
        ));
    }

    let mut connection_config =
        PostgresConfig::from_str(&config.connection_string).map_err(map_postgres_error)?;
    if connection_config.get_application_name().is_none() {
        connection_config.application_name(pool_application_name);
    }
    let manager = Manager::from_config(
        connection_config,
        NoTls,
        ManagerConfig {
            recycling_method: RecyclingMethod::Fast,
        },
    );
    let mut builder = Pool::builder(manager).runtime(Runtime::Tokio1);
    if let Some(max_connections) = config.max_connections {
        builder = builder.max_size(max_connections);
    }
    builder.build().map_err(map_build_error)
}

fn postgres_notification_channel_name(config: &PostgresProviderConfig) -> Result<String> {
    let digest = Sha256::digest(
        format!("{}:{}", config.metadata_schema, config.tenant_schema_prefix).as_bytes(),
    );
    let available_hex_len = POSTGRES_IDENTIFIER_LIMIT
        .checked_sub(POSTGRES_NOTIFICATION_CHANNEL_PREFIX.len())
        .ok_or_else(|| {
            Error::InvalidInput(
                "notification channel prefix is too long for PostgreSQL".to_string(),
            )
        })?;
    let hex_len = available_hex_len.min(16);
    let mut suffix = String::with_capacity(hex_len);
    for byte in digest.iter().take(hex_len / 2) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    Ok(format!("{POSTGRES_NOTIFICATION_CHANNEL_PREFIX}{suffix}"))
}

fn postgres_pool_application_name(config: &PostgresProviderConfig) -> Result<String> {
    let digest = Sha256::digest(
        format!("{}:{}", config.metadata_schema, config.tenant_schema_prefix).as_bytes(),
    );
    let available_hex_len = POSTGRES_IDENTIFIER_LIMIT
        .checked_sub(POSTGRES_POOL_APPLICATION_NAME_PREFIX.len())
        .ok_or_else(|| {
            Error::InvalidInput(
                "postgres pool application-name prefix exceeds identifier budget".to_string(),
            )
        })?;
    let hex_len = available_hex_len.min(16);
    let mut suffix = String::with_capacity(hex_len);
    for byte in digest.iter().take(hex_len / 2) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    Ok(format!("{POSTGRES_POOL_APPLICATION_NAME_PREFIX}{suffix}"))
}

fn parse_postgres_notification(
    notification: tokio_postgres::Notification,
) -> Result<PostgresProviderNotification> {
    let payload: PostgresProviderNotificationPayload = serde_json::from_str(notification.payload())
        .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok(PostgresProviderNotification {
        tenant_id: TenantId::new(payload.tenant_id)?,
        journal_changed: payload.journal_changed,
        scheduler_changed: payload.scheduler_changed,
        schema_changed: payload.schema_changed,
    })
}

fn tenant_schema_name(prefix: &str, tenant_id: &TenantId) -> Result<String> {
    let available_hex_len = POSTGRES_IDENTIFIER_LIMIT
        .checked_sub(prefix.len())
        .ok_or_else(|| {
            Error::InvalidInput("tenant schema prefix is too long for PostgreSQL".to_string())
        })?;
    let bounded_hex_len = available_hex_len.min(TARGET_TENANT_HASH_HEX_LEN);
    let hash_hex_len = bounded_hex_len - (bounded_hex_len % 2);
    if hash_hex_len < MIN_TENANT_HASH_HEX_LEN {
        return Err(Error::InvalidInput(
            "tenant schema prefix leaves too little room for a safe tenant hash".to_string(),
        ));
    }

    let digest = Sha256::digest(tenant_id.as_str().as_bytes());
    let mut hash = String::with_capacity(hash_hex_len);
    for byte in digest.iter().take(hash_hex_len / 2) {
        let _ = write!(&mut hash, "{byte:02x}");
    }
    Ok(format!("{prefix}{hash}"))
}

fn validate_identifier_input(value: &str, label: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(format!("{label} cannot be empty")));
    }
    if value.len() >= POSTGRES_IDENTIFIER_LIMIT {
        return Err(Error::InvalidInput(format!(
            "{label} must be shorter than {POSTGRES_IDENTIFIER_LIMIT} bytes for PostgreSQL"
        )));
    }
    Ok(())
}

fn quote_identifier(identifier: &str) -> String {
    let mut quoted = String::with_capacity(identifier.len() + 2);
    quoted.push('"');
    for character in identifier.chars() {
        if character == '"' {
            quoted.push('"');
        }
        quoted.push(character);
    }
    quoted.push('"');
    quoted
}

fn quote_literal(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push('\'');
        }
        quoted.push(character);
    }
    quoted.push('\'');
    quoted
}

fn cached_schema(schema_cache: &RwLock<Option<Schema>>) -> Option<Schema> {
    schema_cache.read().ok().and_then(|guard| guard.clone())
}

fn publish_schema_cache(schema_cache: &RwLock<Option<Schema>>, schema: &Schema) {
    if let Ok(mut guard) = schema_cache.write() {
        *guard = Some(schema.clone());
    }
}

fn invalidate_schema_cache_handle(schema_cache: &RwLock<Option<Schema>>) {
    if let Ok(mut guard) = schema_cache.write() {
        *guard = None;
    }
}

fn qualified_table(schema_name: &str, table_name: &str) -> String {
    format!(
        "{}.{}",
        quote_identifier(schema_name),
        quote_identifier(table_name)
    )
}

fn tenant_init_sql(schema_name: &str) -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS {} (\
            table_name TEXT NOT NULL,\
            id TEXT NOT NULL,\
            data_json TEXT NOT NULL,\
            creation_time BIGINT NOT NULL,\
            PRIMARY KEY (table_name, id)\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            table_name TEXT PRIMARY KEY,\
            schema_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            execution_id TEXT PRIMARY KEY\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            id TEXT PRIMARY KEY,\
            run_at BIGINT NOT NULL,\
            data_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            id TEXT PRIMARY KEY,\
            data_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            job_id TEXT PRIMARY KEY,\
            data_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            name TEXT PRIMARY KEY,\
            next_run BIGINT NOT NULL,\
            enabled BOOLEAN NOT NULL,\
            data_json TEXT NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            sequence BIGINT PRIMARY KEY,\
            record_blob BYTEA NOT NULL\
        );\
        CREATE TABLE IF NOT EXISTS {} (\
            key TEXT PRIMARY KEY,\
            value_blob BYTEA NOT NULL\
        );",
        qualified_table(schema_name, "documents"),
        qualified_table(schema_name, "schemas"),
        qualified_table(schema_name, "scheduled_job_executions"),
        qualified_table(schema_name, "scheduled_jobs"),
        qualified_table(schema_name, "running_scheduled_jobs"),
        qualified_table(schema_name, "scheduled_job_results"),
        qualified_table(schema_name, "cron_jobs"),
        qualified_table(schema_name, "commit_log"),
        qualified_table(schema_name, "metadata"),
    )
}

async fn load_schema_from_session<C>(session: &C, schema_name: &str) -> Result<Schema>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT schema_json FROM {} ORDER BY table_name",
        qualified_table(schema_name, "schemas")
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    let mut schema = Schema::default();
    for row in rows {
        let table_schema: TableSchema = serde_json::from_str(row.get::<_, String>(0).as_str())
            .map_err(|error| Error::Serialization(error.to_string()))?;
        schema
            .tables
            .insert(table_schema.table.clone(), table_schema);
    }
    Ok(schema)
}

async fn load_journal_progress_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<JournalProgress>
where
    C: GenericClient + Sync,
{
    let durable_head = load_latest_sequence_from_session(session, schema_name).await?;
    let applied_head = load_metadata_u64_from_session(session, schema_name, APPLIED_SEQUENCE_KEY)
        .await?
        .map(SequenceNumber)
        .unwrap_or(SequenceNumber(0));
    Ok(JournalProgress {
        durable_head,
        applied_head,
    })
}

async fn load_latest_sequence_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<SequenceNumber>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT COALESCE(MAX(sequence), 0) FROM {}",
        qualified_table(schema_name, "commit_log")
    );
    let row = session
        .query_one(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    sequence_number_from_i64(row.get::<_, i64>(0))
}

async fn load_documents_from_session<C>(
    session: &C,
    schema_name: &str,
    table: Option<&TableName>,
) -> Result<Vec<Document>>
where
    C: GenericClient + Sync,
{
    let query = if table.is_some() {
        format!(
            "SELECT table_name, id, creation_time, data_json \
             FROM {} \
             WHERE table_name = $1 \
             ORDER BY id",
            qualified_table(schema_name, "documents")
        )
    } else {
        format!(
            "SELECT table_name, id, creation_time, data_json \
             FROM {} \
             ORDER BY table_name, id",
            qualified_table(schema_name, "documents")
        )
    };

    let rows = match table {
        Some(table) => session
            .query(query.as_str(), &[&table.as_str()])
            .await
            .map_err(map_postgres_error)?,
        None => session
            .query(query.as_str(), &[])
            .await
            .map_err(map_postgres_error)?,
    };

    rows.into_iter().map(row_to_document).collect()
}

async fn load_document_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    id: &DocumentId,
) -> Result<Option<Document>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT table_name, id, creation_time, data_json \
         FROM {} \
         WHERE table_name = $1 AND id = $2",
        qualified_table(schema_name, "documents")
    );
    session
        .query_opt(query.as_str(), &[&table.as_str(), &id.to_string()])
        .await
        .map_err(map_postgres_error)?
        .map(row_to_document)
        .transpose()
}

#[allow(clippy::too_many_arguments)]
async fn load_index_candidate_documents_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    table_schema: &TableSchema,
    index_name: &str,
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<Vec<Document>>
where
    C: GenericClient + Sync,
{
    let index_fields = index_fields_for_table_schema(table_schema, index_name)?;
    let range_field = index_fields.get(exact_prefix.len());

    let mut clauses = vec!["table_name = $1".to_string()];
    let mut params: Vec<Box<dyn ToSql + Sync + Send>> = vec![Box::new(table.as_str().to_string())];

    for (field, value) in index_fields.iter().zip(exact_prefix.iter()) {
        clauses.push(format!(
            "{} = ${}",
            postgres_json_extract_expr(field),
            params.len() + 1
        ));
        params.push(Box::new(postgres_index_text_value(value)?));
    }

    if let Some(range_field) = range_field {
        let field_type = field_type_for_table_schema(table_schema, range_field)?;
        match field_type {
            FieldType::String => {
                append_postgres_range_clause(
                    &mut clauses,
                    &mut params,
                    postgres_json_extract_expr(range_field),
                    start.map(postgres_index_text_value).transpose()?,
                    end.map(postgres_index_text_value).transpose()?,
                    start_inclusive,
                    end_inclusive,
                );
            }
            FieldType::Number => {
                append_postgres_range_clause(
                    &mut clauses,
                    &mut params,
                    postgres_numeric_extract_expr(range_field),
                    start.map(postgres_numeric_value).transpose()?,
                    end.map(postgres_numeric_value).transpose()?,
                    start_inclusive,
                    end_inclusive,
                );
            }
            _ if start.is_some() || end.is_some() => {
                return Err(Error::InvalidInput(
                    "range scans only support string and number indexed fields".to_string(),
                ));
            }
            _ => {}
        }
    }

    let sql = format!(
        "SELECT table_name, id, creation_time, data_json \
         FROM {} \
         WHERE {} \
         ORDER BY id",
        qualified_table(schema_name, "documents"),
        clauses.join(" AND ")
    );
    let param_refs = params
        .iter()
        .map(|param| param.as_ref() as &(dyn ToSql + Sync))
        .collect::<Vec<_>>();
    let rows = session
        .query(sql.as_str(), &param_refs)
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter().map(row_to_document).collect()
}

async fn load_table_schema_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
) -> Result<Option<TableSchema>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT schema_json FROM {} WHERE table_name = $1",
        qualified_table(schema_name, "schemas")
    );
    session
        .query_opt(query.as_str(), &[&table.as_str()])
        .await
        .map_err(map_postgres_error)?
        .map(|row| deserialize_json::<TableSchema>(row.get::<_, String>(0).as_str()))
        .transpose()
}

async fn load_scheduled_execution_ids_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<Vec<String>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT execution_id FROM {} ORDER BY execution_id",
        qualified_table(schema_name, "scheduled_job_executions")
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    Ok(rows
        .into_iter()
        .map(|row| row.get::<_, String>(0))
        .collect())
}

async fn load_scheduled_jobs_from_session<C>(
    session: &C,
    schema_name: &str,
    table_name: &str,
) -> Result<Vec<ScheduledJob>>
where
    C: GenericClient + Sync,
{
    let order_by = if table_name == "scheduled_jobs" {
        "run_at, id"
    } else {
        "id"
    };
    let query = format!(
        "SELECT data_json FROM {} ORDER BY {order_by}",
        qualified_table(schema_name, table_name)
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter()
        .map(|row| deserialize_json::<ScheduledJob>(row.get::<_, String>(0).as_str()))
        .collect()
}

async fn load_scheduled_job_result_from_session<C>(
    session: &C,
    schema_name: &str,
    job_id: &DocumentId,
) -> Result<Option<ScheduledJobResult>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT data_json FROM {} WHERE job_id = $1",
        qualified_table(schema_name, "scheduled_job_results")
    );
    session
        .query_opt(query.as_str(), &[&job_id.to_string()])
        .await
        .map_err(map_postgres_error)?
        .map(|row| deserialize_json::<ScheduledJobResult>(row.get::<_, String>(0).as_str()))
        .transpose()
}

async fn load_cron_jobs_from_session<C>(session: &C, schema_name: &str) -> Result<Vec<CronJob>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT data_json FROM {} ORDER BY name",
        qualified_table(schema_name, "cron_jobs")
    );
    let rows = session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter()
        .map(|row| deserialize_json::<CronJob>(row.get::<_, String>(0).as_str()))
        .collect()
}

async fn table_has_rows_in_session<C>(
    session: &C,
    schema_name: &str,
    table_name: &str,
) -> Result<bool>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT 1 FROM {} LIMIT 1",
        qualified_table(schema_name, table_name)
    );
    session
        .query_opt(query.as_str(), &[])
        .await
        .map(|row| row.is_some())
        .map_err(map_postgres_error)
}

async fn load_durable_records_from_session<C>(
    session: &C,
    schema_name: &str,
    sequence: SequenceNumber,
) -> Result<Vec<DurableMutationRecord>>
where
    C: GenericClient + Sync,
{
    let from = i64_from_sequence(sequence)?;
    let query = format!(
        "SELECT record_blob FROM {} WHERE sequence >= $1 ORDER BY sequence",
        qualified_table(schema_name, "commit_log")
    );
    let rows = session
        .query(query.as_str(), &[&from])
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter()
        .map(|row| {
            let payload: Vec<u8> = row.get(0);
            deserialize_durable_record(payload.as_slice())
        })
        .collect()
}

async fn load_durable_journal_cursor_floor_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<SequenceNumber>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT MIN(sequence) FROM {}",
        qualified_table(schema_name, "commit_log")
    );
    let row = session
        .query_one(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    let min_sequence = row.get::<_, Option<i64>>(0);
    match min_sequence {
        Some(sequence) => Ok(SequenceNumber(
            sequence_number_from_i64(sequence)?.0.saturating_sub(1),
        )),
        None => Ok(SequenceNumber(0)),
    }
}

async fn stream_durable_journal_from_session<C>(
    session: &C,
    schema_name: &str,
    after: SequenceNumber,
    limit: usize,
) -> Result<DurableJournalPage>
where
    C: GenericClient + Sync,
{
    let latest_sequence = load_latest_sequence_from_session(session, schema_name).await?;
    let cursor_floor = load_durable_journal_cursor_floor_from_session(session, schema_name).await?;
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

    let after_i64 = i64_from_sequence(after)?;
    let limit_i64 = i64::try_from(limit.saturating_add(1))
        .map_err(|_| Error::InvalidInput("journal stream limit overflow".to_string()))?;
    let query = format!(
        "SELECT record_blob FROM {} WHERE sequence > $1 ORDER BY sequence LIMIT $2",
        qualified_table(schema_name, "commit_log")
    );
    let rows = session
        .query(query.as_str(), &[&after_i64, &limit_i64])
        .await
        .map_err(map_postgres_error)?;
    let mut records = Vec::with_capacity(limit);
    let mut has_more = false;
    for row in rows {
        let payload: Vec<u8> = row.get(0);
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

async fn load_metadata_u64_from_session<C>(
    session: &C,
    schema_name: &str,
    key: &str,
) -> Result<Option<u64>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT value_blob FROM {} WHERE key = $1",
        qualified_table(schema_name, "metadata")
    );
    let row = session
        .query_opt(query.as_str(), &[&key])
        .await
        .map_err(map_postgres_error)?;
    row.map(|row| {
        let bytes: Vec<u8> = row.get(0);
        decode_u64(bytes.as_slice())
    })
    .transpose()
}

fn row_to_document(row: tokio_postgres::Row) -> Result<Document> {
    let table = TableName::new(row.get::<_, String>(0))?;
    let id = DocumentId::from_str(row.get::<_, String>(1).as_str())
        .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let creation_time = timestamp_from_i64(row.get::<_, i64>(2))?;
    let fields =
        serde_json::from_str::<serde_json::Map<String, Value>>(row.get::<_, String>(3).as_str())
            .map_err(|error| Error::Serialization(error.to_string()))?;
    Ok(Document {
        id,
        table,
        creation_time,
        fields,
    })
}

async fn begin_scheduled_execution_in_session<C>(
    session: &C,
    schema_name: &str,
    execution_id: Option<&str>,
) -> Result<bool>
where
    C: GenericClient + Sync,
{
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };

    let query = format!(
        "INSERT INTO {} (execution_id) VALUES ($1) ON CONFLICT DO NOTHING",
        qualified_table(schema_name, "scheduled_job_executions")
    );
    let inserted = session
        .execute(query.as_str(), &[&execution_id])
        .await
        .map_err(map_postgres_error)?;
    Ok(inserted == 1)
}

async fn create_postgres_indexes_for_table_schema<C>(
    session: &C,
    schema_name: &str,
    table_schema: &TableSchema,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    for index in &table_schema.indexes {
        let expressions = index
            .fields
            .iter()
            .map(|field| postgres_json_extract_expr(field))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            "CREATE INDEX IF NOT EXISTS {} ON {} (table_name, {}, id)",
            quote_identifier(&postgres_index_name(&table_schema.table, &index.name)),
            qualified_table(schema_name, "documents"),
            expressions
        );
        session
            .batch_execute(sql.as_str())
            .await
            .map_err(map_postgres_error)?;
    }
    Ok(())
}

async fn drop_postgres_indexes_for_table_schema<C>(
    session: &C,
    schema_name: &str,
    table_schema: &TableSchema,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    for index in &table_schema.indexes {
        let sql = format!(
            "DROP INDEX IF EXISTS {}.{}",
            quote_identifier(schema_name),
            quote_identifier(&postgres_index_name(&table_schema.table, &index.name))
        );
        session
            .batch_execute(sql.as_str())
            .await
            .map_err(map_postgres_error)?;
    }
    Ok(())
}

async fn apply_durable_record_in_session<C>(
    session: &C,
    schema_name: &str,
    record: &DurableMutationRecord,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
        let _ =
            begin_scheduled_execution_in_session(session, schema_name, Some(execution_id)).await?;
    }

    for write in &record.writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                let existing =
                    load_document_from_session(session, schema_name, &write.table, &write.doc_id)
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
                        let query = format!(
                            "INSERT INTO {} (table_name, id, data_json, creation_time) VALUES ($1, $2, $3, $4)",
                            qualified_table(schema_name, "documents")
                        );
                        let table = write.table.as_str().to_string();
                        let id = write.doc_id.to_string();
                        let data_json = serialize_document_fields(current)?;
                        let creation_time = i64_from_timestamp(current.creation_time)?;
                        session
                            .execute(query.as_str(), &[&table, &id, &data_json, &creation_time])
                            .await
                            .map_err(map_postgres_error)?;
                    }
                }
            }
            (Some(previous), Some(current)) => {
                let existing =
                    load_document_from_session(session, schema_name, &write.table, &write.doc_id)
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
                let query = format!(
                    "UPDATE {} SET data_json = $3, creation_time = $4 WHERE table_name = $1 AND id = $2",
                    qualified_table(schema_name, "documents")
                );
                let table = write.table.as_str().to_string();
                let id = write.doc_id.to_string();
                let data_json = serialize_document_fields(current)?;
                let creation_time = i64_from_timestamp(current.creation_time)?;
                session
                    .execute(query.as_str(), &[&table, &id, &data_json, &creation_time])
                    .await
                    .map_err(map_postgres_error)?;
            }
            (Some(previous), None) => {
                match load_document_from_session(session, schema_name, &write.table, &write.doc_id)
                    .await?
                {
                    Some(existing) if existing != *previous => {
                        return Err(Error::Conflict(format!(
                            "durable journal delete replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    Some(_) => {
                        let query = format!(
                            "DELETE FROM {} WHERE table_name = $1 AND id = $2",
                            qualified_table(schema_name, "documents")
                        );
                        let table = write.table.as_str().to_string();
                        let id = write.doc_id.to_string();
                        session
                            .execute(query.as_str(), &[&table, &id])
                            .await
                            .map_err(map_postgres_error)?;
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

fn sequence_number_from_i64(value: i64) -> Result<SequenceNumber> {
    u64::try_from(value)
        .map(SequenceNumber)
        .map_err(|_| Error::Internal(format!("negative PostgreSQL sequence value: {value}")))
}

fn timestamp_from_i64(value: i64) -> Result<Timestamp> {
    u64::try_from(value)
        .map(Timestamp)
        .map_err(|_| Error::Internal(format!("negative PostgreSQL timestamp value: {value}")))
}

fn i64_from_sequence(sequence: SequenceNumber) -> Result<i64> {
    i64::try_from(sequence.0).map_err(|_| {
        Error::InvalidInput(format!("sequence {} exceeds PostgreSQL BIGINT", sequence.0))
    })
}

fn i64_from_timestamp(timestamp: Timestamp) -> Result<i64> {
    i64::try_from(timestamp.0).map_err(|_| {
        Error::InvalidInput(format!(
            "timestamp {} exceeds PostgreSQL BIGINT",
            timestamp.0
        ))
    })
}

fn claim_due_jobs_upper_bound(timestamp: Timestamp) -> i64 {
    i64::try_from(timestamp.0).unwrap_or(i64::MAX)
}

fn tenant_advisory_lock_key(tenant_id: &TenantId) -> i64 {
    let digest = Sha256::digest(tenant_id.as_str().as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    i64::from_be_bytes(bytes)
}

fn postgres_index_name(table: &TableName, index_name: &str) -> String {
    let digest = Sha256::digest(format!("{}:{index_name}", table.as_str()).as_bytes());
    let mut suffix = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("idx_{suffix}")
}

fn postgres_json_extract_expr(field: &str) -> String {
    format!(
        "jsonb_extract_path_text(data_json::jsonb, {})",
        postgres_string_literal(field)
    )
}

fn postgres_string_literal(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push('\'');
        }
        quoted.push(character);
    }
    quoted.push('\'');
    quoted
}

fn expect_write_commit(commit: Option<CommitEntry>, expectation: &str) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

fn serialize_json<T>(value: &T) -> Result<String>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

fn serialize_document_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.fields).map_err(|error| Error::Serialization(error.to_string()))
}

fn matches_filters(document: &Document, filters: &[Filter]) -> Result<bool> {
    for filter in filters {
        let Some(field_value) = document.get_field(&filter.field) else {
            return Ok(false);
        };
        let matched = match filter.op {
            FilterOp::Eq => field_value == &filter.value,
            FilterOp::Neq => field_value != &filter.value,
            FilterOp::Gt => compare_values(field_value, &filter.value)? == Ordering::Greater,
            FilterOp::Gte => {
                matches!(
                    compare_values(field_value, &filter.value)?,
                    Ordering::Greater | Ordering::Equal
                )
            }
            FilterOp::Lt => compare_values(field_value, &filter.value)? == Ordering::Less,
            FilterOp::Lte => {
                matches!(
                    compare_values(field_value, &filter.value)?,
                    Ordering::Less | Ordering::Equal
                )
            }
        };
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

fn filter_documents_with_predicate<F>(
    documents: Vec<Document>,
    filters: &[Filter],
    check_cancel: &mut dyn FnMut() -> Result<()>,
    mut include_document: F,
) -> Result<Vec<Document>>
where
    F: FnMut(&Document) -> Result<bool>,
{
    let mut filtered = Vec::new();
    for document in documents {
        check_cancel()?;
        if matches_filters(&document, filters)? && include_document(&document)? {
            filtered.push(document);
        }
    }
    Ok(filtered)
}

fn compare_values(left: &Value, right: &Value) -> Result<Ordering> {
    match (left, right) {
        (Value::String(left), Value::String(right)) => Ok(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => {
            let left = left
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            let right = right
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            left.partial_cmp(&right).ok_or_else(|| {
                Error::InvalidInput("invalid numeric ordering comparison".to_string())
            })
        }
        _ => Err(Error::InvalidInput(
            "comparisons only support string and number fields in phase 1".to_string(),
        )),
    }
}

fn document_matches_exact_prefix(
    document: &Document,
    index_fields: &[String],
    exact_prefix: &[Value],
) -> bool {
    index_fields
        .iter()
        .zip(exact_prefix.iter())
        .all(|(field, value)| document.get_field(field) == Some(value))
}

#[allow(clippy::too_many_arguments)]
fn filter_index_documents_with_cancel(
    documents: Vec<Document>,
    table: &TableName,
    index_fields: &[String],
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let range_field = index_fields.get(exact_prefix.len());
    let mut filtered = Vec::new();
    for document in documents {
        check_cancel()?;
        if &document.table != table {
            continue;
        }
        if !document_matches_exact_prefix(&document, index_fields, exact_prefix) {
            continue;
        }
        if let Some(range_field) = range_field
            && !document_matches_range_bounds(
                &document,
                range_field,
                start,
                end,
                start_inclusive,
                end_inclusive,
            )?
        {
            continue;
        }
        filtered.push(document);
    }
    Ok(filtered)
}

fn index_fields_for_table_schema(
    table_schema: &TableSchema,
    index_name: &str,
) -> Result<Vec<String>> {
    table_schema
        .indexes
        .iter()
        .find(|index| index.name == index_name)
        .map(|index| index.fields.clone())
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "index '{}' not found for table '{}'",
                index_name,
                table_schema.table.as_str()
            ))
        })
}

fn field_type_for_table_schema(table_schema: &TableSchema, field_name: &str) -> Result<FieldType> {
    table_schema
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .map(|field| field.field_type)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "indexed field '{}' not found for table '{}'",
                field_name,
                table_schema.table.as_str()
            ))
        })
}

fn postgres_index_text_value(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        _ => Err(Error::InvalidInput(
            "indexed values must be string, number, or boolean scalars".to_string(),
        )),
    }
}

fn postgres_numeric_value(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .ok_or_else(|| Error::InvalidInput("numeric indexed value expected".to_string()))
}

fn postgres_numeric_extract_expr(field: &str) -> String {
    format!(
        "CAST({} AS DOUBLE PRECISION)",
        postgres_json_extract_expr(field)
    )
}

fn append_postgres_range_clause<T>(
    clauses: &mut Vec<String>,
    params: &mut Vec<Box<dyn ToSql + Sync + Send>>,
    expr: String,
    start: Option<T>,
    end: Option<T>,
    start_inclusive: bool,
    end_inclusive: bool,
) where
    T: ToSql + Sync + Send + 'static,
{
    if let Some(start) = start {
        let operator = if start_inclusive { ">=" } else { ">" };
        clauses.push(format!("{expr} {operator} ${}", params.len() + 1));
        params.push(Box::new(start));
    }
    if let Some(end) = end {
        let operator = if end_inclusive { "<=" } else { "<" };
        clauses.push(format!("{expr} {operator} ${}", params.len() + 1));
        params.push(Box::new(end));
    }
}

fn document_matches_range_bounds(
    document: &Document,
    field: &str,
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<bool> {
    let Some(value) = document.get_field(field) else {
        return Ok(false);
    };

    if let Some(start) = start {
        let ordering = compare_values(value, start)?;
        let passes = if start_inclusive {
            matches!(ordering, Ordering::Greater | Ordering::Equal)
        } else {
            ordering == Ordering::Greater
        };
        if !passes {
            return Ok(false);
        }
    }

    if let Some(end) = end {
        let ordering = compare_values(value, end)?;
        let passes = if end_inclusive {
            matches!(ordering, Ordering::Less | Ordering::Equal)
        } else {
            ordering == Ordering::Less
        };
        if !passes {
            return Ok(false);
        }
    }

    Ok(true)
}

fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(Error::InvalidInput(
            "journal stream limit must be greater than zero".to_string(),
        ));
    }
    if limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "journal stream limit {limit} exceeds the maximum {}",
            MAX_DURABLE_JOURNAL_STREAM_LIMIT
        )));
    }
    Ok(())
}

fn decode_u64(bytes: &[u8]) -> Result<u64> {
    let bytes: [u8; 8] = bytes
        .try_into()
        .map_err(|_| Error::Serialization("invalid u64 metadata blob".to_string()))?;
    Ok(u64::from_le_bytes(bytes))
}

fn encode_u64(value: u64) -> [u8; 8] {
    value.to_le_bytes()
}

fn default_postgres_read_parallelism() -> usize {
    std::thread::available_parallelism()
        .map(|parallelism| parallelism.get().max(MIN_POSTGRES_READ_PARALLELISM))
        .unwrap_or(MIN_POSTGRES_READ_PARALLELISM)
}

fn apply_schedule_ops_in_transaction(
    transaction: &mut PostgresWriteTransaction,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for schedule_op in schedule_ops {
        match schedule_op {
            ResolvedScheduleOp::Insert { job } => transaction.insert_scheduled_job(job)?,
            ResolvedScheduleOp::Cancel { job_id } => {
                if !transaction.cancel_scheduled_job(job_id)? {
                    return Err(Error::ScheduledJobNotFound(*job_id));
                }
            }
        }
    }
    Ok(())
}

fn map_pool_error(error: PoolError) -> Error {
    Error::storage(
        StorageErrorKind::Unavailable,
        format!("postgres pool error: {error}"),
    )
}

fn map_build_error(error: BuildError) -> Error {
    Error::storage(
        StorageErrorKind::Unavailable,
        format!("postgres pool build error: {error}"),
    )
}

fn map_join_error(error: tokio::task::JoinError) -> Error {
    Error::Internal(format!("postgres executor join error: {error}"))
}

fn map_permit_error(error: tokio::sync::AcquireError) -> Error {
    Error::Internal(format!("postgres executor permit error: {error}"))
}

fn map_postgres_error(error: tokio_postgres::Error) -> Error {
    if let Some(db_error) = error.as_db_error() {
        let code = db_error.code().code();
        let mut message = format!(
            "postgres error [{:?}]: {}",
            db_error.code(),
            db_error.message()
        );
        if let Some(detail) = db_error.detail() {
            let _ = write!(&mut message, " (detail: {detail})");
        }
        if let Some(hint) = db_error.hint() {
            let _ = write!(&mut message, " (hint: {hint})");
        }
        return match code {
            "40001" | "40P01" | "55P03" => Error::storage(StorageErrorKind::Transient, message),
            "08000" | "08001" | "08003" | "08004" | "08006" | "08007" | "08P01" => {
                Error::storage(StorageErrorKind::Unavailable, message)
            }
            "42501" => Error::PermissionDenied(message),
            "53100" | "53200" | "53300" | "53400" => Error::ResourceExhausted(message),
            "57P03" => Error::storage(StorageErrorKind::Unavailable, message),
            "58P01" | "58P02" => Error::storage(StorageErrorKind::Io, message),
            "XX001" | "XX002" => Error::storage(StorageErrorKind::Corruption, message),
            _ if code.starts_with("08") => Error::storage(StorageErrorKind::Unavailable, message),
            _ if code.starts_with("53") => Error::ResourceExhausted(message),
            _ => Error::storage(StorageErrorKind::Other, message),
        };
    }

    if error.is_closed() {
        Error::storage(
            StorageErrorKind::Unavailable,
            format!("postgres error: {error}"),
        )
    } else {
        Error::storage(StorageErrorKind::Other, format!("postgres error: {error}"))
    }
}
