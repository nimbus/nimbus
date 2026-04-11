use std::fmt::Write as _;
use std::future::Future;
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use mysql_async::prelude::Queryable;
use mysql_async::{
    Conn, Opts, OptsBuilder, Params, Pool, PoolConstraints, Row, Value as MySqlValue,
};
use neovex_core::{
    CommitEntry, CronJob, Document, DocumentId, DurableMutationRecord, Error, FieldType, Filter,
    FilterOp, IndexDefinition, Result, ScheduledJob, ScheduledJobResult, Schema, SequenceNumber,
    StorageErrorKind, TableName, TableSchema, TenantId, Timestamp, WriteOp, WriteOpType,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::runtime::Handle as TokioRuntimeHandle;
use tokio::sync::Semaphore;

use crate::async_storage::{TenantReadStorage, TenantWriteOutcome, TenantWriteStorage};
use crate::commit_log::{deserialize_durable_record, serialize_commit, serialize_durable_record};
use crate::simulation::{Clock, FaultInjector, FaultPoint, NoopFaultInjector, SystemClock};
use crate::store::{
    DurableJournalBootstrap, DurableJournalPage, JournalProgress, MAX_DURABLE_JOURNAL_STREAM_LIMIT,
    MaterializedJournalSnapshot, TenantWriteCommit,
};
use crate::{ResolvedScheduleOp, ResolvedWrite};

const MYSQL_IDENTIFIER_LIMIT: usize = 64;
const TARGET_TENANT_HASH_HEX_LEN: usize = 40;
const MIN_TENANT_HASH_HEX_LEN: usize = 16;
const MIN_MYSQL_READ_PARALLELISM: usize = 2;
const MYSQL_TENANT_WRITE_PARALLELISM: usize = 1;
const MYSQL_MAX_INDEX_KEY_BYTES: usize = 3072;
const MYSQL_INDEX_KEY_BYTES_PER_CHAR: usize = 4;
const APPLIED_SEQUENCE_KEY: &str = "applied_sequence";
const MATERIALIZED_JOURNAL_SNAPSHOT_VERSION: u16 = 1;
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
            metadata_database: "neovex_provider".to_string(),
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct MySqlReadSnapshot {
    schema: Schema,
    progress: JournalProgress,
    documents: Vec<Document>,
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
    conn: Option<Conn>,
    commit_writes: Vec<WriteOp>,
    check_cancel: Box<dyn Fn() -> Result<()> + Send>,
}

#[derive(Clone)]
struct MySqlBlockingWriteExecutor {
    store: Arc<MySqlTenantStore>,
    permits: Arc<Semaphore>,
    runtime_handle: TokioRuntimeHandle,
}

impl MySqlProvider {
    pub async fn connect(config: MySqlProviderConfig) -> Result<Self> {
        Self::connect_with_simulation(
            config,
            TokioRuntimeHandle::current(),
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    pub async fn connect_with_runtime(
        config: MySqlProviderConfig,
        runtime_handle: TokioRuntimeHandle,
    ) -> Result<Self> {
        Self::connect_with_simulation(
            config,
            runtime_handle,
            Arc::new(SystemClock),
            Arc::new(NoopFaultInjector),
        )
        .await
    }

    pub async fn connect_with_simulation(
        config: MySqlProviderConfig,
        runtime_handle: TokioRuntimeHandle,
        clock: Arc<dyn Clock>,
        fault_injector: Arc<dyn FaultInjector>,
    ) -> Result<Self> {
        validate_identifier_input(&config.metadata_database, "metadata database")?;
        validate_identifier_input(&config.tenant_database_prefix, "tenant database prefix")?;

        let pool = build_pool(&config)?;
        let provider = Self {
            pool,
            metadata_database: config.metadata_database,
            tenant_database_prefix: config.tenant_database_prefix,
            runtime_handle,
            clock,
            fault_injector,
            tenant_read_parallelism: default_mysql_read_parallelism(),
        };
        provider.ensure_metadata_database().await?;
        Ok(provider)
    }

    pub fn metadata_database(&self) -> &str {
        &self.metadata_database
    }

    pub fn tenant_database_name(&self, tenant_id: &TenantId) -> Result<String> {
        tenant_database_name(&self.tenant_database_prefix, tenant_id)
    }

    pub fn read_storage_for_store(&self, store: Arc<MySqlTenantStore>) -> Arc<MySqlTenantStorage> {
        Arc::new(MySqlTenantStorage::with_max_concurrent_reads(
            store,
            self.runtime_handle.clone(),
            self.tenant_read_parallelism,
        ))
    }

    pub async fn create_opened_tenant(&self, tenant_id: &TenantId) -> Result<OpenedMySqlTenant> {
        let registration = self.create_tenant(tenant_id).await?;
        Ok(self.open_registration(registration))
    }

    pub async fn open_existing_opened_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedMySqlTenant>> {
        self.open_existing_tenant(tenant_id)
            .await?
            .map(|registration| Ok(self.open_registration(registration)))
            .transpose()
    }

    pub async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        let mut conn = self.conn().await?;
        let query = format!(
            "SELECT tenant_id FROM {} ORDER BY tenant_id",
            qualified_table(&self.metadata_database, "tenants")
        );
        let rows: Vec<Row> = conn.query(query).await.map_err(map_mysql_error)?;
        rows.into_iter()
            .map(|row| {
                let (tenant_id,): (String,) = mysql_async::from_row(row);
                TenantId::new(tenant_id)
            })
            .collect()
    }

    pub async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        let mut conn = self.conn().await?;
        let query = format!(
            "SELECT database_name FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        let row = conn
            .exec_first::<Row, _, _>(query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?;
        Ok(row.is_some())
    }

    pub async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<MySqlTenantRegistration>> {
        let mut conn = self.conn().await?;
        let query = format!(
            "SELECT database_name FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        let row = conn
            .exec_first::<Row, _, _>(query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let (database_name,): (String,) = mysql_async::from_row(row);
        if !database_exists(&mut conn, &database_name).await? {
            return Err(Error::Internal(format!(
                "tenant registry points at missing MySQL database '{database_name}'"
            )));
        }
        Ok(Some(MySqlTenantRegistration {
            tenant_id: tenant_id.clone(),
            database_name,
        }))
    }

    pub async fn create_tenant(&self, tenant_id: &TenantId) -> Result<MySqlTenantRegistration> {
        let mut conn = self.conn().await?;
        let fetch_query = format!(
            "SELECT database_name FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        if conn
            .exec_first::<Row, _, _>(fetch_query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?
            .is_some()
        {
            return Err(Error::AlreadyExists(format!(
                "tenant already exists: {tenant_id}"
            )));
        }

        let database_name = self.tenant_database_name(tenant_id)?;
        let create_database_sql = format!("CREATE DATABASE {}", quote_identifier(&database_name));
        if let Err(error) = conn.query_drop(create_database_sql).await {
            if mysql_server_error_code(&error) == Some(1007) {
                return Err(Error::AlreadyExists(format!(
                    "tenant already exists: {tenant_id}"
                )));
            }
            return Err(map_mysql_error(error));
        }
        if let Err(error) = initialize_tenant_database(&mut conn, &database_name).await {
            let cleanup_sql = format!(
                "DROP DATABASE IF EXISTS {}",
                quote_identifier(&database_name)
            );
            let _ = conn.query_drop(cleanup_sql).await;
            return Err(error);
        }

        let insert_query = format!(
            "INSERT INTO {} (tenant_id, database_name) VALUES (?, ?)",
            qualified_table(&self.metadata_database, "tenants")
        );
        if let Err(error) = conn
            .exec_drop(insert_query, (tenant_id.as_str(), database_name.as_str()))
            .await
        {
            let cleanup_sql = format!(
                "DROP DATABASE IF EXISTS {}",
                quote_identifier(&database_name)
            );
            let _ = conn.query_drop(cleanup_sql).await;
            if mysql_server_error_code(&error) == Some(1062) {
                return Err(Error::AlreadyExists(format!(
                    "tenant already exists: {tenant_id}"
                )));
            }
            return Err(map_mysql_error(error));
        }

        Ok(MySqlTenantRegistration {
            tenant_id: tenant_id.clone(),
            database_name,
        })
    }

    pub async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        let mut conn = self.conn().await?;
        let fetch_query = format!(
            "SELECT database_name FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        let Some(row) = conn
            .exec_first::<Row, _, _>(fetch_query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?
        else {
            return Err(Error::TenantNotFound(tenant_id.clone()));
        };
        let (database_name,): (String,) = mysql_async::from_row(row);

        let drop_database_sql = format!("DROP DATABASE {}", quote_identifier(&database_name));
        conn.query_drop(drop_database_sql)
            .await
            .map_err(map_mysql_error)?;

        let delete_query = format!(
            "DELETE FROM {} WHERE tenant_id = ?",
            qualified_table(&self.metadata_database, "tenants")
        );
        conn.exec_drop(delete_query, (tenant_id.as_str(),))
            .await
            .map_err(map_mysql_error)?;
        Ok(())
    }

    #[doc(hidden)]
    pub async fn drop_provider_databases_for_test(&self) -> Result<()> {
        let mut conn = self.conn().await?;
        if !database_exists(&mut conn, &self.metadata_database).await? {
            return Ok(());
        }

        let query = format!(
            "SELECT database_name FROM {}",
            qualified_table(&self.metadata_database, "tenants")
        );
        let rows: Vec<Row> = conn.query(query).await.map_err(map_mysql_error)?;
        for row in rows {
            let (database_name,): (String,) = mysql_async::from_row(row);
            let drop_tenant_sql = format!(
                "DROP DATABASE IF EXISTS {}",
                quote_identifier(&database_name)
            );
            conn.query_drop(drop_tenant_sql)
                .await
                .map_err(map_mysql_error)?;
        }

        let drop_metadata_sql = format!(
            "DROP DATABASE IF EXISTS {}",
            quote_identifier(&self.metadata_database)
        );
        conn.query_drop(drop_metadata_sql)
            .await
            .map_err(map_mysql_error)
    }

    fn open_registration(&self, registration: MySqlTenantRegistration) -> OpenedMySqlTenant {
        let store = Arc::new(MySqlTenantStore::new(self.clone(), registration));
        let read_storage = self.read_storage_for_store(store.clone());
        OpenedMySqlTenant {
            store,
            read_storage,
        }
    }

    async fn ensure_metadata_database(&self) -> Result<()> {
        let mut conn = self.conn().await?;
        let create_database_sql = format!(
            "CREATE DATABASE IF NOT EXISTS {}",
            quote_identifier(&self.metadata_database)
        );
        conn.query_drop(create_database_sql)
            .await
            .map_err(map_mysql_error)?;
        let bootstrap = format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                tenant_id VARCHAR(191) PRIMARY KEY,\
                database_name VARCHAR(191) NOT NULL UNIQUE,\
                created_at TIMESTAMP(6) NOT NULL DEFAULT CURRENT_TIMESTAMP(6)\
            ) ENGINE=InnoDB",
            qualified_table(&self.metadata_database, "tenants")
        );
        conn.query_drop(bootstrap).await.map_err(map_mysql_error)
    }

    async fn conn(&self) -> Result<Conn> {
        self.pool.get_conn().await.map_err(map_mysql_error)
    }
}

impl MySqlTenantStore {
    fn new(provider: MySqlProvider, registration: MySqlTenantRegistration) -> Self {
        Self {
            provider,
            tenant_id: registration.tenant_id,
            database_name: registration.database_name,
        }
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn database_name(&self) -> &str {
        &self.database_name
    }

    pub fn metadata_database(&self) -> &str {
        self.provider.metadata_database()
    }

    pub fn begin_write_transaction(&self) -> Result<MySqlWriteTransaction> {
        self.begin_write_transaction_cancellable(|| Ok(()))
    }

    pub fn begin_write_transaction_cancellable<Check>(
        &self,
        check_cancel: Check,
    ) -> Result<MySqlWriteTransaction>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        MySqlWriteTransaction::begin(self.clone(), check_cancel)
    }

    pub fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T>,
    {
        self.execute_write_cancellable(|| Ok(()), task)
    }

    pub fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T>,
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

    pub fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        self.execute_write(move |transaction| transaction.replace_table_schema(table_schema))?;
        Ok(())
    }

    pub fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        self.execute_write(move |transaction| transaction.delete_table_schema(table))?;
        Ok(())
    }

    pub fn load_schema(&self) -> Result<Schema> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_schema_from_session(&mut conn, &database_name).await
        })
    }

    pub fn latest_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.journal_progress()?.durable_head)
    }

    pub fn applied_sequence(&self) -> Result<SequenceNumber> {
        Ok(self.journal_progress()?.applied_head)
    }

    pub fn journal_progress(&self) -> Result<JournalProgress> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_journal_progress_from_session(&mut conn, &database_name).await
        })
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

    pub fn read_snapshot(&self) -> Result<MySqlReadSnapshot> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            conn.query_drop("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                .await
                .map_err(map_mysql_error)?;
            let mut transaction = conn
                .start_transaction(mysql_async::TxOpts::default())
                .await
                .map_err(map_mysql_error)?;
            let schema = load_schema_from_session(&mut transaction, &database_name).await?;
            let progress =
                load_journal_progress_from_session(&mut transaction, &database_name).await?;
            let documents =
                load_documents_from_session(&mut transaction, &database_name, None).await?;
            let scheduled_execution_ids =
                load_scheduled_execution_ids_from_session(&mut transaction, &database_name).await?;
            transaction.commit().await.map_err(map_mysql_error)?;
            Ok(MySqlReadSnapshot {
                schema,
                progress,
                documents,
                scheduled_execution_ids,
            })
        })
    }

    pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let table = table.clone();
        let id = *id;
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_document_from_session(&mut conn, &database_name, &table, &id).await
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
        let database_name = self.database_name.clone();
        let table = table.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_documents_from_session(&mut conn, &database_name, Some(&table)).await
        })
    }

    fn load_table_schema(&self, table: &TableName) -> Result<TableSchema> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let table = table.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_table_schema_from_session(&mut conn, &database_name, &table)
                .await?
                .ok_or(Error::SchemaNotFound(table))
        })
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
        let database_name = self.database_name.clone();
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
            let mut conn = provider.conn().await?;
            load_index_candidate_documents_from_session(
                &mut conn,
                &database_name,
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

    pub fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from(sequence)?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<DurableMutationRecord>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_durable_records_from_session(&mut conn, &database_name, sequence).await
        })
    }

    pub fn append_durable_records_batch(&self, _records: &[DurableMutationRecord]) -> Result<()> {
        self.execute_write(move |transaction| transaction.append_durable_records_batch(_records))?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, _records: &[DurableMutationRecord]) -> Result<()> {
        self.execute_write(move |transaction| transaction.apply_durable_records_batch(_records))?;
        Ok(())
    }

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        validate_durable_journal_stream_limit(limit)?;
        let latest_sequence = self.latest_sequence()?;
        if after.0 > latest_sequence.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is ahead of the latest durable sequence {}",
                after.0, latest_sequence.0
            )));
        }

        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            let query = format!(
                "SELECT record_blob FROM {} WHERE sequence > ? ORDER BY sequence LIMIT ?",
                qualified_table(&database_name, "commit_log")
            );
            let rows: Vec<Row> = conn
                .exec(
                    query,
                    (
                        after.0,
                        u64::try_from(limit.saturating_add(1)).unwrap_or(u64::MAX),
                    ),
                )
                .await
                .map_err(map_mysql_error)?;
            let mut records = Vec::with_capacity(limit);
            let mut has_more = false;
            for row in rows {
                let (record_blob,): (Vec<u8>,) = mysql_async::from_row(row);
                if records.len() == limit {
                    has_more = true;
                    break;
                }
                records.push(deserialize_durable_record(record_blob.as_slice())?);
            }
            let next_cursor = records
                .last()
                .map(|record| record.sequence)
                .unwrap_or(after);
            Ok(DurableJournalPage {
                records,
                next_cursor,
                latest_sequence,
                cursor_floor: SequenceNumber(0),
                has_more,
            })
        })
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        self.read_snapshot()?.export_durable_journal_bootstrap()
    }

    pub fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let execution_id = execution_id.to_string();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            let query = format!(
                "SELECT execution_id FROM {} WHERE execution_id = ?",
                qualified_table(&database_name, "scheduled_job_executions")
            );
            let row = conn
                .exec_first::<Row, _, _>(query, (execution_id,))
                .await
                .map_err(map_mysql_error)?;
            Ok(row.is_some())
        })
    }

    pub fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        self.execute_write(move |transaction| transaction.insert_scheduled_job(job))?;
        Ok(())
    }

    pub fn claim_due_jobs(&self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(move |transaction| transaction.claim_due_jobs(now))?
            .value)
    }

    pub fn complete_scheduled_job(&self, job_id: &DocumentId) -> Result<()> {
        self.execute_write(move |transaction| transaction.complete_scheduled_job(job_id))?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&self, job_id: &DocumentId) -> Result<bool> {
        Ok(self
            .execute_write(move |transaction| transaction.cancel_scheduled_job(job_id))?
            .value)
    }

    pub fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        self.execute_write(move |transaction| transaction.record_scheduled_job_result(result))?;
        Ok(())
    }

    pub fn get_scheduled_job_result(
        &self,
        job_id: &DocumentId,
    ) -> Result<Option<ScheduledJobResult>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let job_id = job_id.to_string();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_scheduled_job_result_from_session(&mut conn, &database_name, &job_id).await
        })
    }

    pub fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_scheduled_jobs_from_session(&mut conn, &database_name, "scheduled_jobs").await
        })
    }

    pub fn load_cron_jobs(&self) -> Result<Vec<CronJob>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            load_cron_jobs_from_session(&mut conn, &database_name).await
        })
    }

    pub fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        self.execute_write(move |transaction| transaction.save_cron_job(cron))?;
        Ok(())
    }

    pub fn delete_cron_job(&self, name: &str) -> Result<()> {
        let name = name.to_string();
        self.execute_write(move |transaction| transaction.delete_cron_job(&name))?;
        Ok(())
    }

    pub fn has_scheduled_work(&self) -> Result<bool> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            if table_has_entries(&mut conn, &database_name, "scheduled_jobs").await?
                || table_has_entries(&mut conn, &database_name, "running_scheduled_jobs").await?
            {
                return Ok(true);
            }
            let query = format!(
                "SELECT 1 FROM {} WHERE enabled = TRUE LIMIT 1",
                qualified_table(&database_name, "cron_jobs")
            );
            Ok(conn
                .query_first::<Row, _>(query)
                .await
                .map_err(map_mysql_error)?
                .is_some())
        })
    }

    pub fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            let scheduled_jobs_query = format!(
                "SELECT MIN(run_at) FROM {}",
                qualified_table(&database_name, "scheduled_jobs")
            );
            let cron_jobs_query = format!(
                "SELECT MIN(next_run) FROM {} WHERE enabled = TRUE",
                qualified_table(&database_name, "cron_jobs")
            );
            let scheduled = conn
                .query_first::<Option<u64>, _>(scheduled_jobs_query)
                .await
                .map_err(map_mysql_error)?
                .flatten();
            let cron = conn
                .query_first::<Option<u64>, _>(cron_jobs_query)
                .await
                .map_err(map_mysql_error)?
                .flatten();
            Ok(match (scheduled, cron) {
                (Some(left), Some(right)) => Some(Timestamp(left.min(right))),
                (Some(value), None) | (None, Some(value)) => Some(Timestamp(value)),
                (None, None) => None,
            })
        })
    }

    pub fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        self.execute_write(move |transaction| transaction.recover_running_jobs(now))?;
        Ok(())
    }

    pub fn apply_execution_unit_batch(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        if writes.is_empty() && schedule_ops.is_empty() {
            return Err(Error::Internal(
                "execution-unit batch must contain at least one change".to_string(),
            ));
        }

        let committed = self.execute_write(move |transaction| {
            for write in writes {
                transaction.apply_resolved_write(write)?;
            }
            apply_schedule_ops_in_transaction(transaction, schedule_ops)?;
            Ok(())
        })?;
        Ok(committed.commit)
    }

    pub fn now(&self) -> Timestamp {
        self.provider.clock.now()
    }

    pub fn insert(&self, document: &Document) -> Result<CommitEntry> {
        self.insert_once(document, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated insert should commit".to_string()))
    }

    pub fn insert_with_indexes(
        &self,
        document: &Document,
        _indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        self.insert(document)
    }

    pub fn insert_once(
        &self,
        document: &Document,
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(false);
            }
            transaction.insert_document(document)?;
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
        document: &Document,
        _indexes: &[IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        self.insert_once(document, execution_id)
    }

    pub fn update_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(table, id, patch, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated update should commit".to_string()))
    }

    pub fn update_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(false);
            }
            transaction.update_document_validated(table, id, patch, validate)?;
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
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        _indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated(table, id, patch, validate)
    }

    pub fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        _indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(table, id, patch, execution_id, validate)
    }

    pub fn delete_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(table, id, None, validate)?
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    pub fn delete_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(None);
            }
            let removed_document = transaction.delete_document_validated(table, id, validate)?;
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
        table: &TableName,
        id: &DocumentId,
        _indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_returning_document(table, id, validate)
    }

    pub fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(table, id, execution_id, validate)
    }

    pub fn block_on<F, T>(&self, future: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send + 'static,
        T: Send + 'static,
    {
        if TokioRuntimeHandle::try_current().is_ok() {
            let runtime_handle = self.provider.runtime_handle.clone();
            return std::thread::spawn(move || runtime_handle.block_on(future))
                .join()
                .map_err(|_| Error::Internal("MySQL bridge thread panicked".to_string()))?;
        }
        self.provider.runtime_handle.block_on(future)
    }
}

impl MySqlTenantStorage {
    pub fn new(store: Arc<MySqlTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self::with_max_concurrent_reads(store, runtime_handle, default_mysql_read_parallelism())
    }

    pub fn with_max_concurrent_reads(
        store: Arc<MySqlTenantStore>,
        runtime_handle: TokioRuntimeHandle,
        max_concurrent_reads: usize,
    ) -> Self {
        Self {
            write_executor: MySqlBlockingWriteExecutor::new(store.clone(), runtime_handle.clone()),
            store,
            permits: Arc::new(Semaphore::new(max_concurrent_reads.max(1))),
            runtime_handle,
        }
    }

    pub fn store(&self) -> Arc<MySqlTenantStore> {
        self.store.clone()
    }
}

impl TenantReadStorage for MySqlTenantStorage {
    type Store = MySqlTenantStore;

    async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(Arc<MySqlTenantStore>) -> Result<T> + Send + 'static,
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
        F: FnOnce(Arc<MySqlTenantStore>, &mut dyn FnMut() -> Result<()>) -> Result<T>
            + Send
            + 'static,
    {
        tokio::pin!(cancel_wait);

        let permit = tokio::select! {
            _ = &mut cancel_wait => return Err(Error::Cancelled),
            permit = self.permits.clone().acquire_owned() => permit.map_err(map_permit_error)?,
        };

        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_for_task = cancelled.clone();
        let store = self.store.clone();
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

impl TenantWriteStorage for MySqlTenantStorage {
    type WriteTransaction = MySqlWriteTransaction;

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor.execute_write(task).await
    }

    async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        _check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T> + Send + 'static,
    {
        self.write_executor
            .execute_write_cancellable(cancel_wait, _check_cancel, task)
            .await
    }
}

impl MySqlBlockingWriteExecutor {
    fn new(store: Arc<MySqlTenantStore>, runtime_handle: TokioRuntimeHandle) -> Self {
        Self {
            store,
            permits: Arc::new(Semaphore::new(MYSQL_TENANT_WRITE_PARALLELISM)),
            runtime_handle,
        }
    }

    async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T> + Send + 'static,
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
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T> + Send + 'static,
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

impl MySqlWriteTransaction {
    fn begin<Check>(store: MySqlTenantStore, check_cancel: Check) -> Result<Self>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let provider = store.provider.clone();
        let database_name = store.database_name.clone();
        let conn = store.block_on({
            let provider = provider.clone();
            async move { provider.conn().await }
        })?;

        let mut transaction = Self {
            provider,
            database_name,
            conn: Some(conn),
            commit_writes: Vec::new(),
            check_cancel: Box::new(check_cancel),
        };
        if let Err(error) = (|| -> Result<()> {
            transaction.check_cancel()?;
            transaction.batch_execute("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")?;
            transaction.batch_execute("START TRANSACTION")?;
            transaction.ensure_metadata_rows()?;
            transaction.acquire_tenant_lock()?;
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
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        if let Some(previous) = self.load_table_schema(table)? {
            self.drop_table_indexes(&previous)?;
        }
        self.delete_table_schema_entry(table)
    }

    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            begin_scheduled_execution_in_session(conn, &database_name, execution_id).await
        })
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (table_name, id, data_json, creation_time) VALUES (?, ?, ?, ?)",
            qualified_table(&self.database_name, "documents")
        );
        let table_name = document.table.as_str().to_string();
        let document_id = document.id.to_string();
        let data_json = serialize_document_fields(document)?;
        let creation_time = document.creation_time.0;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (table_name, document_id, data_json, creation_time))
                .await
                .map_err(map_mysql_error)
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
        patch: &serde_json::Map<String, serde_json::Value>,
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
            "UPDATE {} SET data_json = ?, creation_time = ? WHERE table_name = ? AND id = ?",
            qualified_table(&self.database_name, "documents")
        );
        let data_json = serialize_document_fields(&document)?;
        let creation_time = document.creation_time.0;
        let table_name = table.as_str().to_string();
        let document_id = id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (data_json, creation_time, table_name, document_id))
                .await
                .map_err(map_mysql_error)
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
            "DELETE FROM {} WHERE table_name = ? AND id = ?",
            qualified_table(&self.database_name, "documents")
        );
        let table_name = table.as_str().to_string();
        let document_id = id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (table_name, document_id))
                .await
                .map_err(map_mysql_error)
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
            "INSERT INTO {} (id, run_at, data_json) VALUES (?, ?, ?)",
            qualified_table(&self.database_name, "scheduled_jobs")
        );
        let job_id = job.id.to_string();
        let run_at = job.run_at.0;
        let data_json = serialize_json(job)?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (job_id, run_at, data_json))
                .await
                .map_err(map_mysql_error)
        })
    }

    pub fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let due: Vec<ScheduledJob> = {
            let conn = self.session()?;
            Self::block_on(&runtime_handle, async move {
                let query = format!(
                    "SELECT data_json FROM {} WHERE run_at <= ? ORDER BY run_at, id FOR UPDATE",
                    qualified_table(&database_name, "scheduled_jobs")
                );
                let rows: Vec<Row> = conn
                    .exec(query, (claim_due_jobs_upper_bound(now),))
                    .await
                    .map_err(map_mysql_error)?;
                rows.into_iter()
                    .map(|row| {
                        deserialize_json::<ScheduledJob>(
                            mysql_async::from_row::<(String,)>(row).0.as_str(),
                        )
                    })
                    .collect::<Result<Vec<_>>>()
            })?
        };
        let delete_query = format!(
            "DELETE FROM {} WHERE id = ?",
            qualified_table(&self.database_name, "scheduled_jobs")
        );
        let insert_query = format!(
            "INSERT INTO {} (id, data_json) VALUES (?, ?)",
            qualified_table(&self.database_name, "running_scheduled_jobs")
        );
        for job in &due {
            self.check_cancel()?;
            let job_id = job.id.to_string();
            let data_json = serialize_json(job)?;
            let delete_query = delete_query.clone();
            let insert_query = insert_query.clone();
            let runtime_handle = self.provider.runtime_handle.clone();
            let conn = self.session()?;
            Self::block_on(&runtime_handle, async move {
                conn.exec_drop(delete_query.clone(), (job_id.clone(),))
                    .await
                    .map_err(map_mysql_error)?;
                conn.exec_drop(insert_query.clone(), (job_id, data_json))
                    .await
                    .map_err(map_mysql_error)?;
                Ok(())
            })?;
        }
        Ok(due)
    }

    pub fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "DELETE FROM {} WHERE id = ?",
            qualified_table(&self.database_name, "running_scheduled_jobs")
        );
        let job_id = job_id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (job_id,))
                .await
                .map_err(map_mysql_error)
        })
    }

    pub fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.check_cancel()?;
        let query = format!(
            "DELETE FROM {} WHERE id = ?",
            qualified_table(&self.database_name, "scheduled_jobs")
        );
        let job_id = job_id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (job_id,))
                .await
                .map_err(map_mysql_error)?;
            Ok(conn.affected_rows() == 1)
        })
    }

    pub fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (job_id, data_json) VALUES (?, ?)
             ON DUPLICATE KEY UPDATE data_json = VALUES(data_json)",
            qualified_table(&self.database_name, "scheduled_job_results")
        );
        let job_id = result.id.to_string();
        let data_json = serialize_json(result)?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (job_id, data_json))
                .await
                .map_err(map_mysql_error)
        })
    }

    pub fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (name, next_run, enabled, data_json) VALUES (?, ?, ?, ?)
             ON DUPLICATE KEY UPDATE next_run = VALUES(next_run), enabled = VALUES(enabled), data_json = VALUES(data_json)",
            qualified_table(&self.database_name, "cron_jobs")
        );
        let name = cron.name.clone();
        let next_run = cron.next_run.0;
        let enabled = cron.enabled;
        let data_json = serialize_json(cron)?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (name, next_run, enabled, data_json))
                .await
                .map_err(map_mysql_error)
        })
    }

    pub fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "DELETE FROM {} WHERE name = ?",
            qualified_table(&self.database_name, "cron_jobs")
        );
        let name = name.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (name,))
                .await
                .map_err(map_mysql_error)
        })
    }

    pub fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        self.check_cancel()?;
        let running_jobs = self.load_running_jobs()?;
        let delete_query = format!(
            "DELETE FROM {} WHERE id = ?",
            qualified_table(&self.database_name, "running_scheduled_jobs")
        );
        let insert_query = format!(
            "INSERT INTO {} (id, run_at, data_json) VALUES (?, ?, ?)",
            qualified_table(&self.database_name, "scheduled_jobs")
        );
        for mut job in running_jobs {
            self.check_cancel()?;
            job.run_at = now;
            let job_id = job.id.to_string();
            let run_at = job.run_at.0;
            let data_json = serialize_json(&job)?;
            let insert_query = insert_query.clone();
            let delete_query = delete_query.clone();
            let runtime_handle = self.provider.runtime_handle.clone();
            let conn = self.session()?;
            Self::block_on(&runtime_handle, async move {
                conn.exec_drop(insert_query.clone(), (job_id.clone(), run_at, data_json))
                    .await
                    .map_err(map_mysql_error)?;
                conn.exec_drop(delete_query.clone(), (job_id,))
                    .await
                    .map_err(map_mysql_error)?;
                Ok(())
            })?;
        }
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
            "INSERT INTO {} (sequence, record_blob) VALUES (?, ?)",
            qualified_table(&self.database_name, "commit_log")
        );
        for record in records {
            self.check_cancel()?;
            if record.sequence.0 != next {
                return Err(Error::Internal(format!(
                    "durable journal append expected sequence {}, got {}",
                    next, record.sequence.0
                )));
            }
            let payload = serialize_durable_record(record)?;
            let sequence = record.sequence.0;
            let query = query.clone();
            let runtime_handle = self.provider.runtime_handle.clone();
            let conn = self.session()?;
            Self::block_on(&runtime_handle, async move {
                conn.exec_drop(query.clone(), (sequence, payload))
                    .await
                    .map_err(map_mysql_error)
            })?;
            next = next.saturating_add(1);
        }
        self.provider
            .fault_injector
            .check(FaultPoint::JournalAppendBeforeDurableFlush)?;
        self.provider
            .fault_injector
            .check(FaultPoint::JournalFlushBeforeVisibility)?;
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
                    "UPDATE {} SET data_json = ?, creation_time = ? WHERE table_name = ? AND id = ?",
                    qualified_table(&self.database_name, "documents")
                );
                let data_json = serialize_document_fields(current)?;
                let creation_time = current.creation_time.0;
                let table_name = current.table.as_str().to_string();
                let document_id = current.id.to_string();
                let runtime_handle = self.provider.runtime_handle.clone();
                let conn = self.session()?;
                Self::block_on(&runtime_handle, async move {
                    conn.exec_drop(query, (data_json, creation_time, table_name, document_id))
                        .await
                        .map_err(map_mysql_error)
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
                    "DELETE FROM {} WHERE table_name = ? AND id = ?",
                    qualified_table(&self.database_name, "documents")
                );
                let table_name = previous.table.as_str().to_string();
                let document_id = previous.id.to_string();
                let runtime_handle = self.provider.runtime_handle.clone();
                let conn = self.session()?;
                Self::block_on(&runtime_handle, async move {
                    conn.exec_drop(query, (table_name, document_id))
                        .await
                        .map_err(map_mysql_error)
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
        self.provider
            .fault_injector
            .check(FaultPoint::StorageCommitBeforeVisibility)?;
        self.batch_execute("COMMIT")?;
        self.provider
            .fault_injector
            .check(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
        Ok(commit)
    }

    pub fn rollback(&mut self) {
        let _ = self.batch_execute("ROLLBACK");
    }

    fn batch_execute(&mut self, sql: &str) -> Result<()> {
        let query = sql.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.query_drop(query).await.map_err(map_mysql_error)
        })
    }

    fn block_on<F, T>(runtime_handle: &TokioRuntimeHandle, future: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send,
        T: Send,
    {
        if TokioRuntimeHandle::try_current().is_ok() {
            return std::thread::scope(|scope| {
                scope
                    .spawn(move || runtime_handle.block_on(future))
                    .join()
                    .map_err(|_| Error::Internal("MySQL write bridge thread panicked".to_string()))
            })?;
        }
        runtime_handle.block_on(future)
    }

    fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    fn session(&mut self) -> Result<&mut Conn> {
        self.conn
            .as_mut()
            .ok_or_else(|| Error::Internal("MySQL write transaction already closed".to_string()))
    }

    fn ensure_metadata_rows(&mut self) -> Result<()> {
        let query = format!(
            "INSERT IGNORE INTO {} (key_name, value_u64) VALUES (?, ?)",
            qualified_table(&self.database_name, "metadata")
        );
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (APPLIED_SEQUENCE_KEY, 0_u64))
                .await
                .map_err(map_mysql_error)
        })
    }

    fn acquire_tenant_lock(&mut self) -> Result<()> {
        let query = format!(
            "SELECT value_u64 FROM {} WHERE key_name = ? FOR UPDATE",
            qualified_table(&self.database_name, "metadata")
        );
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            let row = conn
                .exec_first::<Row, _, _>(query, (APPLIED_SEQUENCE_KEY,))
                .await
                .map_err(map_mysql_error)?;
            if row.is_none() {
                return Err(Error::Internal(
                    "MySQL write transaction missing applied_sequence metadata row".to_string(),
                ));
            }
            Ok(())
        })
    }

    fn latest_sequence(&mut self) -> Result<SequenceNumber> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            load_latest_sequence_from_session(conn, &database_name).await
        })
    }

    fn applied_sequence(&mut self) -> Result<SequenceNumber> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            Ok(
                load_metadata_u64_from_session(conn, &database_name, APPLIED_SEQUENCE_KEY)
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
        let payload = serialize_commit(&entry)?;
        let query = format!(
            "INSERT INTO {} (sequence, record_blob) VALUES (?, ?)",
            qualified_table(&self.database_name, "commit_log")
        );
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (entry.sequence.0, payload))
                .await
                .map_err(map_mysql_error)
        })?;
        self.write_applied_sequence(entry.sequence)?;
        Ok(entry)
    }

    fn write_applied_sequence(&mut self, sequence: SequenceNumber) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (key_name, value_u64) VALUES (?, ?)
             ON DUPLICATE KEY UPDATE value_u64 = VALUES(value_u64)",
            qualified_table(&self.database_name, "metadata")
        );
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (APPLIED_SEQUENCE_KEY, sequence.0))
                .await
                .map_err(map_mysql_error)
        })
    }

    fn load_document(&mut self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let table = table.clone();
        let id = *id;
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            load_document_from_session(conn, &database_name, &table, &id).await
        })
    }

    fn load_table_schema(&mut self, table: &TableName) -> Result<Option<TableSchema>> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let table = table.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            load_table_schema_from_session(conn, &database_name, &table).await
        })
    }

    fn upsert_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (table_name, schema_json) VALUES (?, ?)
             ON DUPLICATE KEY UPDATE schema_json = VALUES(schema_json)",
            qualified_table(&self.database_name, "schemas")
        );
        let table_name = table_schema.table.as_str().to_string();
        let schema_json = serialize_json(table_schema)?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (table_name, schema_json))
                .await
                .map_err(map_mysql_error)
        })
    }

    fn delete_table_schema_entry(&mut self, table: &TableName) -> Result<()> {
        let query = format!(
            "DELETE FROM {} WHERE table_name = ?",
            qualified_table(&self.database_name, "schemas")
        );
        let table_name = table.as_str().to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (table_name,))
                .await
                .map_err(map_mysql_error)
        })
    }

    fn create_table_indexes(&mut self, table_schema: &TableSchema) -> Result<()> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let table_schema = table_schema.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            create_mysql_indexes_for_table_schema(conn, &database_name, &table_schema).await
        })
    }

    fn drop_table_indexes(&mut self, table_schema: &TableSchema) -> Result<()> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let table_schema = table_schema.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            drop_mysql_indexes_for_table_schema(conn, &database_name, &table_schema).await
        })
    }

    fn load_running_jobs(&mut self) -> Result<Vec<ScheduledJob>> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            load_scheduled_jobs_from_session(conn, &database_name, "running_scheduled_jobs").await
        })
    }

    fn apply_durable_record(&mut self, record: &DurableMutationRecord) -> Result<()> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let record = record.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            apply_durable_record_in_session(conn, &database_name, &record).await
        })
    }

    fn record_commit_write(&mut self, write: WriteOp) {
        self.commit_writes.push(write);
    }
}

impl MySqlReadSnapshot {
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

    pub fn stream_durable_journal(
        &self,
        after: SequenceNumber,
        limit: usize,
    ) -> Result<DurableJournalPage> {
        validate_durable_journal_stream_limit(limit)?;
        let latest_sequence = self.latest_sequence()?;
        if after.0 > latest_sequence.0 {
            return Err(Error::InvalidInput(format!(
                "journal cursor {} is ahead of the latest durable sequence {}",
                after.0, latest_sequence.0
            )));
        }
        Ok(DurableJournalPage {
            records: Vec::new(),
            next_cursor: after,
            latest_sequence,
            cursor_floor: SequenceNumber(0),
            has_more: false,
        })
    }

    pub fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap> {
        let snapshot = self.export_materialized_journal_snapshot()?;
        Ok(DurableJournalBootstrap {
            resume_after: snapshot.applied_sequence,
            bootstrap_cut: snapshot.durable_head,
            snapshot,
            cursor_floor: SequenceNumber(0),
        })
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

fn validate_identifier_input(value: &str, label: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(format!("{label} cannot be empty")));
    }
    if value.len() >= MYSQL_IDENTIFIER_LIMIT {
        return Err(Error::InvalidInput(format!(
            "{label} must be shorter than {MYSQL_IDENTIFIER_LIMIT} bytes for MySQL"
        )));
    }
    Ok(())
}

fn qualified_table(database_name: &str, table_name: &str) -> String {
    format!(
        "{}.{}",
        quote_identifier(database_name),
        quote_identifier(table_name)
    )
}

fn quote_identifier(identifier: &str) -> String {
    let mut quoted = String::with_capacity(identifier.len() + 2);
    quoted.push('`');
    for character in identifier.chars() {
        if character == '`' {
            quoted.push('`');
        }
        quoted.push(character);
    }
    quoted.push('`');
    quoted
}

fn mysql_index_key_prefix_chars(key_part_count: usize) -> usize {
    let part_count = key_part_count.max(1);
    let max_chars = MYSQL_MAX_INDEX_KEY_BYTES / MYSQL_INDEX_KEY_BYTES_PER_CHAR;
    (max_chars / part_count).clamp(1, MYSQL_INDEX_KEY_VALUE_LEN)
}

fn mysql_index_key_part(identifier: &str, prefix_chars: usize) -> String {
    format!("{}({prefix_chars})", quote_identifier(identifier))
}

async fn initialize_tenant_database(conn: &mut Conn, database_name: &str) -> Result<()> {
    for statement in tenant_init_statements(database_name) {
        conn.query_drop(statement).await.map_err(map_mysql_error)?;
    }
    Ok(())
}

fn tenant_init_statements(database_name: &str) -> Vec<String> {
    vec![
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                table_name VARCHAR(191) NOT NULL,\
                id VARCHAR(191) NOT NULL,\
                data_json LONGTEXT NOT NULL,\
                creation_time BIGINT UNSIGNED NOT NULL,\
                PRIMARY KEY (table_name, id)\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "documents")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                table_name VARCHAR(191) PRIMARY KEY,\
                schema_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "schemas")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                execution_id VARCHAR(191) PRIMARY KEY\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "scheduled_job_executions")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                id VARCHAR(191) PRIMARY KEY,\
                run_at BIGINT UNSIGNED NOT NULL,\
                data_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "scheduled_jobs")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                id VARCHAR(191) PRIMARY KEY,\
                data_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "running_scheduled_jobs")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                job_id VARCHAR(191) PRIMARY KEY,\
                data_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "scheduled_job_results")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                name VARCHAR(191) PRIMARY KEY,\
                next_run BIGINT UNSIGNED NOT NULL,\
                enabled BOOLEAN NOT NULL,\
                data_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "cron_jobs")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                sequence BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,\
                record_blob LONGBLOB NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "commit_log")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                key_name VARCHAR(191) PRIMARY KEY,\
                value_u64 BIGINT UNSIGNED NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "metadata")
        ),
        format!(
            "INSERT IGNORE INTO {} (key_name, value_u64) VALUES ('{}', 0)",
            qualified_table(database_name, "metadata"),
            APPLIED_SEQUENCE_KEY
        ),
    ]
}

async fn database_exists(conn: &mut Conn, database_name: &str) -> Result<bool> {
    let row = conn
        .exec_first::<Row, _, _>(
            "SELECT SCHEMA_NAME FROM INFORMATION_SCHEMA.SCHEMATA WHERE SCHEMA_NAME = ?",
            (database_name,),
        )
        .await
        .map_err(map_mysql_error)?;
    Ok(row.is_some())
}

fn map_mysql_error(error: mysql_async::Error) -> Error {
    let message = error.to_string();
    match error {
        mysql_async::Error::Server(server) => match server.code {
            1040 | 1041 | 1206 | 1226 => Error::ResourceExhausted(message),
            1044 | 1045 | 1142 | 1143 | 1227 => Error::PermissionDenied(message),
            1062 => Error::AlreadyExists(message),
            1205 => Error::storage(StorageErrorKind::Busy, message),
            1213 => Error::storage(StorageErrorKind::Transient, message),
            2006 | 2013 => Error::storage(StorageErrorKind::Unavailable, message),
            _ => Error::storage(StorageErrorKind::Other, message),
        },
        mysql_async::Error::Io(_) => Error::storage(StorageErrorKind::Io, message),
        mysql_async::Error::Url(_) => Error::InvalidInput(message),
        mysql_async::Error::Driver(driver) => match driver {
            mysql_async::DriverError::ConnectionClosed
            | mysql_async::DriverError::PoolDisconnected => {
                Error::storage(StorageErrorKind::Unavailable, message)
            }
            mysql_async::DriverError::PacketOutOfOrder
            | mysql_async::DriverError::UnexpectedPacket { .. } => {
                Error::storage(StorageErrorKind::Corruption, message)
            }
            _ => Error::storage(StorageErrorKind::Other, message),
        },
        mysql_async::Error::Other(_) => Error::storage(StorageErrorKind::Other, message),
    }
}

fn mysql_server_error_code(error: &mysql_async::Error) -> Option<u16> {
    match error {
        mysql_async::Error::Server(error) => Some(error.code),
        _ => None,
    }
}

fn map_join_error(error: tokio::task::JoinError) -> Error {
    if error.is_cancelled() {
        Error::Cancelled
    } else {
        Error::Internal(error.to_string())
    }
}

fn map_permit_error(error: tokio::sync::AcquireError) -> Error {
    Error::Internal(error.to_string())
}

async fn load_schema_from_session<C>(session: &mut C, database_name: &str) -> Result<Schema>
where
    C: Queryable,
{
    let query = format!(
        "SELECT schema_json FROM {} ORDER BY table_name",
        qualified_table(database_name, "schemas")
    );
    let rows: Vec<Row> = session.query(query).await.map_err(map_mysql_error)?;
    let mut schema = Schema::default();
    for row in rows {
        let (schema_json,): (String,) = mysql_async::from_row(row);
        let table_schema: TableSchema = deserialize_json(schema_json.as_str())?;
        schema
            .tables
            .insert(table_schema.table.clone(), table_schema);
    }
    Ok(schema)
}

async fn load_journal_progress_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<JournalProgress>
where
    C: Queryable,
{
    let durable_head = load_latest_sequence_from_session(session, database_name).await?;
    let applied_head = load_metadata_u64_from_session(session, database_name, APPLIED_SEQUENCE_KEY)
        .await?
        .map(SequenceNumber)
        .unwrap_or(SequenceNumber(0));
    Ok(JournalProgress {
        durable_head,
        applied_head,
    })
}

async fn load_latest_sequence_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<SequenceNumber>
where
    C: Queryable,
{
    let query = format!(
        "SELECT COALESCE(MAX(sequence), 0) FROM {}",
        qualified_table(database_name, "commit_log")
    );
    let value = session
        .query_first::<Option<u64>, _>(query)
        .await
        .map_err(map_mysql_error)?
        .flatten()
        .unwrap_or(0);
    Ok(SequenceNumber(value))
}

async fn load_metadata_u64_from_session<C>(
    session: &mut C,
    database_name: &str,
    key: &str,
) -> Result<Option<u64>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT value_u64 FROM {} WHERE key_name = ?",
        qualified_table(database_name, "metadata")
    );
    session
        .exec_first::<Row, _, _>(query, (key,))
        .await
        .map_err(map_mysql_error)
        .map(|row| row.map(|row| mysql_async::from_row::<(u64,)>(row).0))
}

async fn load_documents_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: Option<&TableName>,
) -> Result<Vec<Document>>
where
    C: Queryable,
{
    let (query, params_table) = if let Some(table) = table {
        (
            format!(
                "SELECT table_name, id, creation_time, data_json \
                 FROM {} WHERE table_name = ? ORDER BY id",
                qualified_table(database_name, "documents")
            ),
            Some(table.as_str().to_string()),
        )
    } else {
        (
            format!(
                "SELECT table_name, id, creation_time, data_json \
                 FROM {} ORDER BY table_name, id",
                qualified_table(database_name, "documents")
            ),
            None,
        )
    };
    let rows: Vec<Row> = if let Some(table_name) = params_table {
        session
            .exec(query, (table_name,))
            .await
            .map_err(map_mysql_error)?
    } else {
        session.query(query).await.map_err(map_mysql_error)?
    };
    rows.into_iter()
        .map(|row| {
            let (table_name, id, creation_time, data_json): (String, String, u64, String) =
                mysql_async::from_row(row);
            let mut document =
                Document::new(TableName::new(table_name)?, deserialize_json(&data_json)?);
            document.id = DocumentId::from_str(&id)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            document.creation_time = Timestamp(creation_time);
            Ok(document)
        })
        .collect()
}

async fn load_scheduled_execution_ids_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<Vec<String>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT execution_id FROM {} ORDER BY execution_id",
        qualified_table(database_name, "scheduled_job_executions")
    );
    let rows: Vec<Row> = session.query(query).await.map_err(map_mysql_error)?;
    Ok(rows
        .into_iter()
        .map(|row| mysql_async::from_row::<(String,)>(row).0)
        .collect())
}

async fn load_durable_records_from_session<C>(
    session: &mut C,
    database_name: &str,
    sequence: SequenceNumber,
) -> Result<Vec<DurableMutationRecord>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT record_blob FROM {} WHERE sequence >= ? ORDER BY sequence",
        qualified_table(database_name, "commit_log")
    );
    let rows: Vec<Row> = session
        .exec(query, (sequence.0,))
        .await
        .map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| {
            let (record_blob,): (Vec<u8>,) = mysql_async::from_row(row);
            deserialize_durable_record(record_blob.as_slice())
        })
        .collect()
}

async fn load_document_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    id: &DocumentId,
) -> Result<Option<Document>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT creation_time, data_json FROM {} WHERE table_name = ? AND id = ?",
        qualified_table(database_name, "documents")
    );
    session
        .exec_first::<Row, _, _>(query, (table.as_str(), id.to_string()))
        .await
        .map_err(map_mysql_error)?
        .map(|row| {
            let (creation_time, data_json): (u64, String) = mysql_async::from_row(row);
            row_to_document(table, id, creation_time, data_json)
        })
        .transpose()
}

async fn load_table_schema_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
) -> Result<Option<TableSchema>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT schema_json FROM {} WHERE table_name = ?",
        qualified_table(database_name, "schemas")
    );
    session
        .exec_first::<Row, _, _>(query, (table.as_str(),))
        .await
        .map_err(map_mysql_error)?
        .map(|row| {
            deserialize_json::<TableSchema>(mysql_async::from_row::<(String,)>(row).0.as_str())
        })
        .transpose()
}

#[allow(clippy::too_many_arguments)]
async fn load_index_candidate_documents_from_session<C>(
    session: &mut C,
    database_name: &str,
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
    C: Queryable,
{
    let index_fields = index_fields_for_table_schema(table_schema, index_name)?;
    let range_field = index_fields.get(exact_prefix.len());

    let mut clauses = vec!["table_name = ?".to_string()];
    let mut params = vec![MySqlValue::Bytes(table.as_str().as_bytes().to_vec())];

    for (field, value) in index_fields.iter().zip(exact_prefix.iter()) {
        clauses.push(format!(
            "{} = ?",
            quote_identifier(&mysql_generated_column_name(table, field))
        ));
        params.push(mysql_index_text_value(value)?);
    }

    if let Some(range_field) = range_field {
        let field_type = field_type_for_table_schema(table_schema, range_field)?;
        match field_type {
            FieldType::String => {
                append_mysql_range_clause(
                    &mut clauses,
                    &mut params,
                    quote_identifier(&mysql_generated_column_name(table, range_field)),
                    start.map(mysql_index_text_value).transpose()?,
                    end.map(mysql_index_text_value).transpose()?,
                    start_inclusive,
                    end_inclusive,
                );
            }
            FieldType::Number => {
                append_mysql_range_clause(
                    &mut clauses,
                    &mut params,
                    mysql_numeric_column_expr(table, range_field),
                    start.map(mysql_numeric_value).transpose()?,
                    end.map(mysql_numeric_value).transpose()?,
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
        qualified_table(database_name, "documents"),
        clauses.join(" AND ")
    );
    let rows: Vec<Row> = session
        .exec(sql, Params::Positional(params))
        .await
        .map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| {
            let (table_name, id, creation_time, data_json): (String, String, u64, String) =
                mysql_async::from_row(row);
            let table = TableName::new(table_name)?;
            let id = DocumentId::from_str(&id)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            row_to_document(&table, &id, creation_time, data_json)
        })
        .collect()
}

async fn load_scheduled_jobs_from_session<C>(
    session: &mut C,
    database_name: &str,
    table_name: &str,
) -> Result<Vec<ScheduledJob>>
where
    C: Queryable,
{
    let order_by = if table_name == "scheduled_jobs" {
        "run_at, id"
    } else {
        "id"
    };
    let query = format!(
        "SELECT data_json FROM {} ORDER BY {}",
        qualified_table(database_name, table_name),
        order_by
    );
    let rows: Vec<Row> = session.query(query).await.map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| {
            deserialize_json::<ScheduledJob>(mysql_async::from_row::<(String,)>(row).0.as_str())
        })
        .collect()
}

async fn load_scheduled_job_result_from_session<C>(
    session: &mut C,
    database_name: &str,
    job_id: &str,
) -> Result<Option<ScheduledJobResult>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT data_json FROM {} WHERE job_id = ?",
        qualified_table(database_name, "scheduled_job_results")
    );
    session
        .exec_first::<Row, _, _>(query, (job_id,))
        .await
        .map_err(map_mysql_error)?
        .map(|row| {
            deserialize_json::<ScheduledJobResult>(
                mysql_async::from_row::<(String,)>(row).0.as_str(),
            )
        })
        .transpose()
}

async fn load_cron_jobs_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<Vec<CronJob>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT data_json FROM {} ORDER BY name",
        qualified_table(database_name, "cron_jobs")
    );
    let rows: Vec<Row> = session.query(query).await.map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| deserialize_json::<CronJob>(mysql_async::from_row::<(String,)>(row).0.as_str()))
        .collect()
}

async fn begin_scheduled_execution_in_session<C>(
    session: &mut C,
    database_name: &str,
    execution_id: Option<&str>,
) -> Result<bool>
where
    C: Queryable,
{
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };
    let exists_query = format!(
        "SELECT execution_id FROM {} WHERE execution_id = ?",
        qualified_table(database_name, "scheduled_job_executions")
    );
    if session
        .exec_first::<Row, _, _>(exists_query, (execution_id,))
        .await
        .map_err(map_mysql_error)?
        .is_some()
    {
        return Ok(false);
    }
    let query = format!(
        "INSERT INTO {} (execution_id) VALUES (?)",
        qualified_table(database_name, "scheduled_job_executions")
    );
    session
        .exec_drop(query, (execution_id,))
        .await
        .map_err(map_mysql_error)?;
    Ok(true)
}

async fn apply_durable_record_in_session<C>(
    session: &mut C,
    database_name: &str,
    record: &DurableMutationRecord,
) -> Result<()>
where
    C: Queryable,
{
    if !begin_scheduled_execution_in_session(
        session,
        database_name,
        record.scheduled_execution_id.as_deref(),
    )
    .await?
    {
        return Ok(());
    }

    for write in &record.writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                let existing =
                    load_document_from_session(session, database_name, &write.table, &write.doc_id)
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
                            "INSERT INTO {} (table_name, id, data_json, creation_time) VALUES (?, ?, ?, ?)",
                            qualified_table(database_name, "documents")
                        );
                        session
                            .exec_drop(
                                query,
                                (
                                    write.table.as_str(),
                                    write.doc_id.to_string(),
                                    serialize_document_fields(current)?,
                                    current.creation_time.0,
                                ),
                            )
                            .await
                            .map_err(map_mysql_error)?;
                    }
                }
            }
            (Some(previous), Some(current)) => {
                let existing =
                    load_document_from_session(session, database_name, &write.table, &write.doc_id)
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
                    "UPDATE {} SET data_json = ?, creation_time = ? WHERE table_name = ? AND id = ?",
                    qualified_table(database_name, "documents")
                );
                session
                    .exec_drop(
                        query,
                        (
                            serialize_document_fields(current)?,
                            current.creation_time.0,
                            write.table.as_str(),
                            write.doc_id.to_string(),
                        ),
                    )
                    .await
                    .map_err(map_mysql_error)?;
            }
            (Some(previous), None) => {
                match load_document_from_session(
                    session,
                    database_name,
                    &write.table,
                    &write.doc_id,
                )
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
                            "DELETE FROM {} WHERE table_name = ? AND id = ?",
                            qualified_table(database_name, "documents")
                        );
                        session
                            .exec_drop(query, (write.table.as_str(), write.doc_id.to_string()))
                            .await
                            .map_err(map_mysql_error)?;
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

async fn table_has_entries<C>(
    session: &mut C,
    database_name: &str,
    table_name: &str,
) -> Result<bool>
where
    C: Queryable,
{
    let query = format!(
        "SELECT 1 FROM {} LIMIT 1",
        qualified_table(database_name, table_name)
    );
    Ok(session
        .query_first::<Row, _>(query)
        .await
        .map_err(map_mysql_error)?
        .is_some())
}

async fn create_mysql_indexes_for_table_schema<C>(
    session: &mut C,
    database_name: &str,
    table_schema: &TableSchema,
) -> Result<()>
where
    C: Queryable,
{
    for field in unique_index_fields(table_schema) {
        let column_name = mysql_generated_column_name(&table_schema.table, field);
        if !mysql_document_column_exists(session, database_name, &column_name).await? {
            let sql = format!(
                "ALTER TABLE {} ADD COLUMN {} VARCHAR({}) GENERATED ALWAYS AS ({}) VIRTUAL",
                qualified_table(database_name, "documents"),
                quote_identifier(&column_name),
                MYSQL_INDEX_KEY_VALUE_LEN,
                mysql_generated_column_expr(&table_schema.table, field),
            );
            session.query_drop(sql).await.map_err(map_mysql_error)?;
        }
    }
    for index in &table_schema.indexes {
        let index_name = mysql_index_name(&table_schema.table, &index.name);
        if mysql_document_index_exists(session, database_name, &index_name).await? {
            continue;
        }
        let key_part_prefix = mysql_index_key_prefix_chars(index.fields.len() + 2);
        let mut columns = Vec::with_capacity(index.fields.len() + 2);
        columns.push(mysql_index_key_part("table_name", key_part_prefix));
        columns.extend(index.fields.iter().map(|field| {
            mysql_index_key_part(
                &mysql_generated_column_name(&table_schema.table, field),
                key_part_prefix,
            )
        }));
        columns.push(mysql_index_key_part("id", key_part_prefix));
        let sql = format!(
            "CREATE INDEX {} ON {} ({})",
            quote_identifier(&index_name),
            qualified_table(database_name, "documents"),
            columns.join(", ")
        );
        session.query_drop(sql).await.map_err(map_mysql_error)?;
    }
    Ok(())
}

async fn drop_mysql_indexes_for_table_schema<C>(
    session: &mut C,
    database_name: &str,
    table_schema: &TableSchema,
) -> Result<()>
where
    C: Queryable,
{
    for index in &table_schema.indexes {
        let index_name = mysql_index_name(&table_schema.table, &index.name);
        if mysql_document_index_exists(session, database_name, &index_name).await? {
            let sql = format!(
                "DROP INDEX {} ON {}",
                quote_identifier(&index_name),
                qualified_table(database_name, "documents")
            );
            session.query_drop(sql).await.map_err(map_mysql_error)?;
        }
    }
    for field in unique_index_fields(table_schema) {
        let column_name = mysql_generated_column_name(&table_schema.table, field);
        if mysql_document_column_exists(session, database_name, &column_name).await? {
            let sql = format!(
                "ALTER TABLE {} DROP COLUMN {}",
                qualified_table(database_name, "documents"),
                quote_identifier(&column_name),
            );
            session.query_drop(sql).await.map_err(map_mysql_error)?;
        }
    }
    Ok(())
}

async fn mysql_document_column_exists<C>(
    session: &mut C,
    database_name: &str,
    column_name: &str,
) -> Result<bool>
where
    C: Queryable,
{
    let row = session
        .exec_first::<Row, _, _>(
            "SELECT COLUMN_NAME \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'documents' AND COLUMN_NAME = ?",
            (database_name, column_name),
        )
        .await
        .map_err(map_mysql_error)?;
    Ok(row.is_some())
}

async fn mysql_document_index_exists<C>(
    session: &mut C,
    database_name: &str,
    index_name: &str,
) -> Result<bool>
where
    C: Queryable,
{
    let row = session
        .exec_first::<Row, _, _>(
            "SELECT INDEX_NAME \
             FROM INFORMATION_SCHEMA.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'documents' AND INDEX_NAME = ?",
            (database_name, index_name),
        )
        .await
        .map_err(map_mysql_error)?;
    Ok(row.is_some())
}

fn expect_write_commit(commit: Option<CommitEntry>, expectation: &str) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

fn apply_schedule_ops_in_transaction(
    transaction: &mut MySqlWriteTransaction,
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

fn serialize_json<T>(value: &T) -> Result<String>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

fn serialize_document_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.fields).map_err(|error| Error::Serialization(error.to_string()))
}

fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

fn matches_filters(document: &Document, filters: &[Filter]) -> Result<bool> {
    for filter in filters {
        let Some(field_value) = document.get_field(&filter.field) else {
            return Ok(false);
        };
        let matched = match filter.op {
            FilterOp::Eq => field_value == &filter.value,
            FilterOp::Neq => field_value != &filter.value,
            FilterOp::Gt => {
                compare_values(field_value, &filter.value)? == std::cmp::Ordering::Greater
            }
            FilterOp::Gte => matches!(
                compare_values(field_value, &filter.value)?,
                std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
            ),
            FilterOp::Lt => compare_values(field_value, &filter.value)? == std::cmp::Ordering::Less,
            FilterOp::Lte => matches!(
                compare_values(field_value, &filter.value)?,
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            ),
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

fn compare_values(left: &Value, right: &Value) -> Result<std::cmp::Ordering> {
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
    let index = table_schema
        .indexes
        .iter()
        .find(|index| index.name == index_name)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "index '{}' not found for table '{}'",
                index_name,
                table_schema.table.as_str()
            ))
        })?;
    Ok(index.fields.clone())
}

fn field_type_for_table_schema(table_schema: &TableSchema, field_name: &str) -> Result<FieldType> {
    table_schema
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .map(|field| field.field_type)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "field '{}' not found in schema for table '{}'",
                field_name,
                table_schema.table.as_str()
            ))
        })
}

fn mysql_index_text_value(value: &Value) -> Result<MySqlValue> {
    match value {
        Value::String(value) => Ok(MySqlValue::Bytes(value.as_bytes().to_vec())),
        Value::Number(number) => Ok(MySqlValue::Bytes(number.to_string().into_bytes())),
        _ => Err(Error::InvalidInput(
            "index equality and prefix scans only support string and number values".to_string(),
        )),
    }
}

fn mysql_numeric_value(value: &Value) -> Result<MySqlValue> {
    let number = value.as_f64().ok_or_else(|| {
        Error::InvalidInput("numeric range bounds require number values".to_string())
    })?;
    Ok(MySqlValue::Double(number))
}

fn mysql_numeric_column_expr(table: &TableName, field: &str) -> String {
    format!(
        "CAST({} AS DOUBLE)",
        quote_identifier(&mysql_generated_column_name(table, field))
    )
}

fn append_mysql_range_clause(
    clauses: &mut Vec<String>,
    params: &mut Vec<MySqlValue>,
    expr: String,
    start: Option<MySqlValue>,
    end: Option<MySqlValue>,
    start_inclusive: bool,
    end_inclusive: bool,
) {
    if let Some(start) = start {
        let operator = if start_inclusive { ">=" } else { ">" };
        clauses.push(format!("{expr} {operator} ?"));
        params.push(start);
    }
    if let Some(end) = end {
        let operator = if end_inclusive { "<=" } else { "<" };
        clauses.push(format!("{expr} {operator} ?"));
        params.push(end);
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
        .all(|(field, expected)| document.get_field(field) == Some(expected))
}

#[allow(clippy::too_many_arguments)]
fn document_matches_range_bounds(
    document: &Document,
    field: &str,
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<bool> {
    if let Some(start) = start {
        let Some(value) = document.get_field(field) else {
            return Ok(false);
        };
        let ordering = compare_values(value, start)?;
        if start_inclusive {
            if ordering == std::cmp::Ordering::Less {
                return Ok(false);
            }
        } else if !matches!(ordering, std::cmp::Ordering::Greater) {
            return Ok(false);
        }
    }
    if let Some(end) = end {
        let Some(value) = document.get_field(field) else {
            return Ok(false);
        };
        let ordering = compare_values(value, end)?;
        if end_inclusive {
            if ordering == std::cmp::Ordering::Greater {
                return Ok(false);
            }
        } else if !matches!(ordering, std::cmp::Ordering::Less) {
            return Ok(false);
        }
    }
    Ok(true)
}

fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(Error::InvalidInput(
            "durable journal stream limit must be greater than zero".to_string(),
        ));
    }
    if limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "durable journal stream limit {limit} exceeds maximum {MAX_DURABLE_JOURNAL_STREAM_LIMIT}"
        )));
    }
    Ok(())
}

fn row_to_document(
    table: &TableName,
    id: &DocumentId,
    creation_time: u64,
    data_json: String,
) -> Result<Document> {
    Ok(Document {
        id: *id,
        table: table.clone(),
        creation_time: Timestamp(creation_time),
        fields: serde_json::from_str(&data_json)
            .map_err(|error| Error::Serialization(error.to_string()))?,
    })
}

fn claim_due_jobs_upper_bound(timestamp: Timestamp) -> u64 {
    timestamp.0
}

fn mysql_index_name(table: &TableName, index_name: &str) -> String {
    let digest = Sha256::digest(format!("{}:{index_name}", table.as_str()).as_bytes());
    let mut suffix = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("idx_{suffix}")
}

fn mysql_generated_column_name(table: &TableName, field: &str) -> String {
    let digest = Sha256::digest(format!("{}:{field}", table.as_str()).as_bytes());
    let mut suffix = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("gcol_{suffix}")
}

fn unique_index_fields(table_schema: &TableSchema) -> Vec<&str> {
    let mut fields = Vec::new();
    for index in &table_schema.indexes {
        for field in &index.fields {
            if !fields.contains(&field.as_str()) {
                fields.push(field.as_str());
            }
        }
    }
    fields
}

fn mysql_generated_column_expr(table: &TableName, field: &str) -> String {
    format!(
        "CASE WHEN table_name = {} THEN JSON_UNQUOTE(JSON_EXTRACT(data_json, '$.\"{}\"')) ELSE NULL END",
        mysql_string_literal(table.as_str()),
        field.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

fn mysql_string_literal(value: &str) -> String {
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
