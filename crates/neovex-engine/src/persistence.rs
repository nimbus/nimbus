use std::future::Future;
use std::sync::{Arc, Mutex};

use neovex_core::{
    CommitEntry, CronJob, Document, DocumentId, DurableMutationRecord, Result, ScheduledJob,
    ScheduledJobResult, Schema, SequenceNumber, TableName, TableSchema, TenantId, Timestamp,
};
use neovex_storage::{
    DurableJournalBootstrap, DurableJournalPage, EmbeddedPersistenceProvider,
    EmbeddedRedbControlPlaneProvider, EmbeddedRedbProvider, EmbeddedSqliteProvider,
    JournalProgress, LibsqlReplicaProvider, LibsqlReplicaTenantStorage, LibsqlReplicaTenantStore,
    LibsqlReplicaWriteTransaction, MonthlyActiveUsersSnapshot, MySqlProvider, MySqlReadSnapshot,
    MySqlTenantStorage, MySqlTenantStore, MySqlWriteTransaction, OpenedEmbeddedRedbTenant,
    OpenedEmbeddedSqliteTenant, OpenedLibsqlReplicaTenant, OpenedMySqlTenant, OpenedPostgresTenant,
    PostgresProvider, PostgresReadSnapshot, PostgresTenantStorage, PostgresTenantStore,
    PostgresWriteTransaction, QueryReadStore, RedbTenantStorage, ResolvedScheduleOp, ResolvedWrite,
    SqliteReadSnapshot, SqliteTenantStorage, SqliteTenantStore,
    TenantReadSnapshot as RedbReadSnapshot, TenantReadStorage, TenantStore as RedbTenantStore,
    TenantWriteCommit, TenantWriteOutcome, TenantWriteStorage,
    TenantWriteTransaction as RedbWriteTransaction, UsageStorage,
};

#[derive(Clone)]
pub(crate) enum PersistenceProvider {
    Redb(Arc<EmbeddedRedbProvider>),
    Sqlite(Arc<EmbeddedSqliteProvider>),
    SqliteReplica(Arc<LibsqlReplicaProvider>),
    Postgres(Arc<PostgresProvider>),
    MySql(Arc<MySqlProvider>),
}

#[derive(Clone)]
pub(crate) enum ControlPlaneProvider {
    EmbeddedRedb(Arc<EmbeddedRedbControlPlaneProvider>),
}

pub(crate) struct OpenedTenantPersistence {
    pub persistence: TenantPersistence,
    pub executor: TenantPersistenceExecutor,
}

#[derive(Clone)]
pub(crate) enum TenantPersistence {
    Redb(Arc<RedbTenantStore>),
    Sqlite(Arc<SqliteTenantStore>),
    SqliteReplica(Arc<LibsqlReplicaTenantStore>),
    Postgres(Arc<PostgresTenantStore>),
    MySql(Arc<MySqlTenantStore>),
}

#[derive(Clone)]
pub(crate) enum TenantPersistenceExecutor {
    Redb(Arc<RedbTenantStorage>),
    Sqlite(Arc<SqliteTenantStorage>),
    SqliteReplica(Arc<LibsqlReplicaTenantStorage>),
    Postgres(Arc<PostgresTenantStorage>),
    MySql(Arc<MySqlTenantStorage>),
}

pub(crate) enum TenantPersistenceSnapshot {
    Redb(RedbReadSnapshot),
    Sqlite(Arc<Mutex<SqliteReadSnapshot>>),
    SqliteReplica(Arc<Mutex<SqliteReadSnapshot>>),
    Postgres(PostgresReadSnapshot),
    MySql(MySqlReadSnapshot),
}

impl ControlPlaneProvider {
    pub(crate) fn record_monthly_active_user(
        &self,
        token_identifier: &str,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        match self {
            Self::EmbeddedRedb(provider) => provider
                .usage_store()
                .record_monthly_active_user(token_identifier, observed_at_unix_ms),
        }
    }

    pub(crate) async fn record_monthly_active_user_async(
        &self,
        token_identifier: String,
        observed_at_unix_ms: u64,
    ) -> Result<bool> {
        match self {
            Self::EmbeddedRedb(provider) => {
                provider
                    .usage_storage()
                    .execute(move |usage_store| {
                        usage_store
                            .record_monthly_active_user(&token_identifier, observed_at_unix_ms)
                    })
                    .await
            }
        }
    }

    pub(crate) fn current_monthly_active_users(
        &self,
        observed_at_unix_ms: u64,
    ) -> Result<MonthlyActiveUsersSnapshot> {
        match self {
            Self::EmbeddedRedb(provider) => provider
                .usage_store()
                .monthly_active_users_for(observed_at_unix_ms),
        }
    }

    pub(crate) async fn current_monthly_active_users_async(
        &self,
        observed_at_unix_ms: u64,
    ) -> Result<MonthlyActiveUsersSnapshot> {
        match self {
            Self::EmbeddedRedb(provider) => {
                provider
                    .usage_storage()
                    .execute(move |usage_store| {
                        usage_store.monthly_active_users_for(observed_at_unix_ms)
                    })
                    .await
            }
        }
    }
}

pub(crate) trait TenantPersistenceWriteOps {
    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()>;
    fn delete_table_schema(&mut self, table: &TableName) -> Result<()>;
    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()>;
    fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>>;
    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()>;
    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool>;
    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()>;
    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()>;
    fn delete_cron_job(&mut self, name: &str) -> Result<()>;
}

impl TenantPersistenceWriteOps for RedbWriteTransaction {
    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.replace_table_schema(table_schema)
    }

    fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.delete_table_schema(table)
    }

    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

impl TenantPersistenceWriteOps for neovex_storage::SqliteWriteTransaction {
    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.replace_table_schema(table_schema)
    }

    fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.delete_table_schema(table)
    }

    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

impl TenantPersistenceWriteOps for PostgresWriteTransaction {
    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.replace_table_schema(table_schema)
    }

    fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.delete_table_schema(table)
    }

    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

impl TenantPersistenceWriteOps for LibsqlReplicaWriteTransaction {
    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.replace_table_schema(table_schema)
    }

    fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.delete_table_schema(table)
    }

    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

impl TenantPersistenceWriteOps for MySqlWriteTransaction {
    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.replace_table_schema(table_schema)
    }

    fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.delete_table_schema(table)
    }

    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        self.insert_scheduled_job(job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.claim_due_jobs(now)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.complete_scheduled_job(job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.cancel_scheduled_job(job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.record_scheduled_job_result(result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.save_cron_job(cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.delete_cron_job(name)
    }
}

macro_rules! delegate_store_method {
    ($(#[$meta:meta])* fn $name:ident(&self $(, $arg:ident : $ty:ty )* ) -> $ret:ty) => {
        $(#[$meta])*
        pub(crate) fn $name(&self, $($arg: $ty),*) -> $ret {
            match self {
                Self::Redb(store) => store.$name($($arg),*),
                Self::Sqlite(store) => store.$name($($arg),*),
                Self::SqliteReplica(store) => store.$name($($arg),*),
                Self::Postgres(store) => store.$name($($arg),*),
                Self::MySql(store) => store.$name($($arg),*),
            }
        }
    };
}

impl PersistenceProvider {
    pub(crate) async fn list_tenants(&self) -> Result<Vec<TenantId>> {
        match self {
            Self::Redb(engine) => engine.list_tenants().await,
            Self::Sqlite(engine) => engine.list_tenants().await,
            Self::SqliteReplica(engine) => engine.list_tenants().await,
            Self::Postgres(engine) => engine.list_tenants().await,
            Self::MySql(engine) => engine.list_tenants().await,
        }
    }

    pub(crate) async fn create_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<OpenedTenantPersistence> {
        match self {
            Self::Redb(engine) => map_opened_redb_tenant(engine.create_tenant(tenant_id).await),
            Self::Sqlite(engine) => map_opened_sqlite_tenant(engine.create_tenant(tenant_id).await),
            Self::SqliteReplica(engine) => {
                map_opened_sqlite_replica_tenant(engine.create_opened_tenant(tenant_id).await)
            }
            Self::Postgres(engine) => {
                map_opened_postgres_tenant(engine.create_opened_tenant(tenant_id).await)
            }
            Self::MySql(engine) => {
                map_opened_mysql_tenant(engine.create_opened_tenant(tenant_id).await)
            }
        }
    }

    pub(crate) async fn open_existing_tenant(
        &self,
        tenant_id: &TenantId,
    ) -> Result<Option<OpenedTenantPersistence>> {
        match self {
            Self::Redb(engine) => engine
                .open_existing_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_redb_tenant_sync)),
            Self::Sqlite(engine) => engine
                .open_existing_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_sqlite_tenant_sync)),
            Self::SqliteReplica(engine) => engine
                .open_existing_opened_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_sqlite_replica_tenant_sync)),
            Self::Postgres(engine) => engine
                .open_existing_opened_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_postgres_tenant_sync)),
            Self::MySql(engine) => engine
                .open_existing_opened_tenant(tenant_id)
                .await
                .map(|opened| opened.map(map_opened_mysql_tenant_sync)),
        }
    }

    pub(crate) async fn delete_tenant(&self, tenant_id: &TenantId) -> Result<()> {
        match self {
            Self::Redb(engine) => engine.delete_tenant(tenant_id).await,
            Self::Sqlite(engine) => engine.delete_tenant(tenant_id).await,
            Self::SqliteReplica(engine) => engine.delete_tenant(tenant_id).await,
            Self::Postgres(engine) => engine.delete_tenant(tenant_id).await,
            Self::MySql(engine) => engine.delete_tenant(tenant_id).await,
        }
    }

    pub(crate) async fn tenant_exists(&self, tenant_id: &TenantId) -> Result<bool> {
        match self {
            Self::Redb(engine) => engine.tenant_exists(tenant_id).await,
            Self::Sqlite(engine) => engine.tenant_exists(tenant_id).await,
            Self::SqliteReplica(engine) => engine.tenant_exists(tenant_id).await,
            Self::Postgres(engine) => engine.tenant_exists(tenant_id).await,
            Self::MySql(engine) => engine.tenant_exists(tenant_id).await,
        }
    }

    pub(crate) fn read_storage_for_store(
        &self,
        store: TenantPersistence,
    ) -> Result<TenantPersistenceExecutor> {
        match (self, store) {
            (Self::Redb(engine), TenantPersistence::Redb(store)) => Ok(
                TenantPersistenceExecutor::Redb(engine.read_storage_for_store(store)),
            ),
            (Self::Sqlite(engine), TenantPersistence::Sqlite(store)) => Ok(
                TenantPersistenceExecutor::Sqlite(engine.read_storage_for_store(store)),
            ),
            (Self::SqliteReplica(engine), TenantPersistence::SqliteReplica(store)) => Ok(
                TenantPersistenceExecutor::SqliteReplica(engine.read_storage_for_store(store)),
            ),
            (Self::Postgres(engine), TenantPersistence::Postgres(store)) => Ok(
                TenantPersistenceExecutor::Postgres(engine.read_storage_for_store(store)),
            ),
            (Self::MySql(engine), TenantPersistence::MySql(store)) => Ok(
                TenantPersistenceExecutor::MySql(engine.read_storage_for_store(store)),
            ),
            _ => Err(neovex_core::Error::Internal(
                "persistence provider and tenant persistence mismatch".to_string(),
            )),
        }
    }
}

impl TenantPersistence {
    delegate_store_method!(fn load_schema(&self) -> Result<Schema>);
    delegate_store_method!(fn latest_sequence(&self) -> Result<SequenceNumber>);
    delegate_store_method!(fn applied_sequence(&self) -> Result<SequenceNumber>);
    delegate_store_method!(fn journal_progress(&self) -> Result<JournalProgress>);
    delegate_store_method!(fn recover_durable_journal(&self) -> Result<JournalProgress>);
    delegate_store_method!(fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>>);
    delegate_store_method!(fn read_durable_journal_from(&self, sequence: SequenceNumber) -> Result<Vec<DurableMutationRecord>>);
    delegate_store_method!(fn stream_durable_journal(&self, after: SequenceNumber, limit: usize) -> Result<DurableJournalPage>);
    delegate_store_method!(fn export_durable_journal_bootstrap(&self) -> Result<DurableJournalBootstrap>);
    delegate_store_method!(fn scheduled_execution_exists(&self, execution_id: &str) -> Result<bool>);
    delegate_store_method!(fn get_scheduled_job_result(&self, job_id: &DocumentId) -> Result<Option<ScheduledJobResult>>);
    delegate_store_method!(fn list_scheduled_jobs(&self) -> Result<Vec<ScheduledJob>>);
    delegate_store_method!(fn load_cron_jobs(&self) -> Result<Vec<CronJob>>);
    delegate_store_method!(fn next_scheduled_work_at(&self) -> Result<Option<Timestamp>>);
    delegate_store_method!(fn has_scheduled_work(&self) -> Result<bool>);
    delegate_store_method!(fn now(&self) -> Timestamp);

    pub(crate) fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        match self {
            Self::Redb(store) => store.replace_table_schema(table_schema),
            Self::Sqlite(store) => store.replace_table_schema(table_schema),
            Self::SqliteReplica(store) => store.replace_table_schema(table_schema),
            Self::Postgres(store) => store.replace_table_schema(table_schema),
            Self::MySql(store) => store.replace_table_schema(table_schema),
        }
    }

    pub(crate) fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        match self {
            Self::Redb(store) => store.delete_table_schema(table),
            Self::Sqlite(store) => store.delete_table_schema(table),
            Self::SqliteReplica(store) => store.delete_table_schema(table),
            Self::Postgres(store) => store.delete_table_schema(table),
            Self::MySql(store) => store.delete_table_schema(table),
        }
    }

    pub(crate) fn append_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<()> {
        match self {
            Self::Redb(store) => store.append_durable_records_batch(records),
            Self::Sqlite(store) => store.append_durable_records_batch(records),
            Self::SqliteReplica(store) => store.append_durable_records_batch(records),
            Self::Postgres(store) => store.append_durable_records_batch(records),
            Self::MySql(store) => store.append_durable_records_batch(records),
        }
    }

    pub(crate) fn apply_durable_records_batch(
        &self,
        records: &[DurableMutationRecord],
    ) -> Result<()> {
        match self {
            Self::Redb(store) => store.apply_durable_records_batch(records),
            Self::Sqlite(store) => store.apply_durable_records_batch(records),
            Self::SqliteReplica(store) => store.apply_durable_records_batch(records),
            Self::Postgres(store) => store.apply_durable_records_batch(records),
            Self::MySql(store) => store.apply_durable_records_batch(records),
        }
    }

    pub(crate) fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        match self {
            Self::Redb(store) => store.insert_scheduled_job(job),
            Self::Sqlite(store) => store.insert_scheduled_job(job),
            Self::SqliteReplica(store) => store.insert_scheduled_job(job),
            Self::Postgres(store) => store.insert_scheduled_job(job),
            Self::MySql(store) => store.insert_scheduled_job(job),
        }
    }

    pub(crate) fn claim_due_jobs(&self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        match self {
            Self::Redb(store) => store.claim_due_jobs(now),
            Self::Sqlite(store) => store.claim_due_jobs(now),
            Self::SqliteReplica(store) => store.claim_due_jobs(now),
            Self::Postgres(store) => store.claim_due_jobs(now),
            Self::MySql(store) => store.claim_due_jobs(now),
        }
    }

    pub(crate) fn complete_scheduled_job(&self, job_id: &DocumentId) -> Result<()> {
        match self {
            Self::Redb(store) => store.complete_scheduled_job(job_id),
            Self::Sqlite(store) => store.complete_scheduled_job(job_id),
            Self::SqliteReplica(store) => store.complete_scheduled_job(job_id),
            Self::Postgres(store) => store.complete_scheduled_job(job_id),
            Self::MySql(store) => store.complete_scheduled_job(job_id),
        }
    }

    pub(crate) fn cancel_scheduled_job(&self, job_id: &DocumentId) -> Result<bool> {
        match self {
            Self::Redb(store) => store.cancel_scheduled_job(job_id),
            Self::Sqlite(store) => store.cancel_scheduled_job(job_id),
            Self::SqliteReplica(store) => store.cancel_scheduled_job(job_id),
            Self::Postgres(store) => store.cancel_scheduled_job(job_id),
            Self::MySql(store) => store.cancel_scheduled_job(job_id),
        }
    }

    #[cfg(test)]
    pub(crate) fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        match self {
            Self::Redb(store) => store.record_scheduled_job_result(result),
            Self::Sqlite(store) => store.record_scheduled_job_result(result),
            Self::SqliteReplica(store) => store.record_scheduled_job_result(result),
            Self::Postgres(store) => store.record_scheduled_job_result(result),
            Self::MySql(store) => store.record_scheduled_job_result(result),
        }
    }

    pub(crate) fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        match self {
            Self::Redb(store) => store.save_cron_job(cron),
            Self::Sqlite(store) => store.save_cron_job(cron),
            Self::SqliteReplica(store) => store.save_cron_job(cron),
            Self::Postgres(store) => store.save_cron_job(cron),
            Self::MySql(store) => store.save_cron_job(cron),
        }
    }

    pub(crate) fn delete_cron_job(&self, name: &str) -> Result<()> {
        match self {
            Self::Redb(store) => store.delete_cron_job(name),
            Self::Sqlite(store) => store.delete_cron_job(name),
            Self::SqliteReplica(store) => store.delete_cron_job(name),
            Self::Postgres(store) => store.delete_cron_job(name),
            Self::MySql(store) => store.delete_cron_job(name),
        }
    }

    pub(crate) fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        match self {
            Self::Redb(store) => store.recover_running_jobs(now),
            Self::Sqlite(store) => store.recover_running_jobs(now),
            Self::SqliteReplica(store) => store.recover_running_jobs(now),
            Self::Postgres(store) => store.recover_running_jobs(now),
            Self::MySql(store) => store.recover_running_jobs(now),
        }
    }

    pub(crate) fn apply_execution_unit_batch(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        match self {
            Self::Redb(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::Sqlite(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::SqliteReplica(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::Postgres(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::MySql(store) => store.apply_execution_unit_batch(writes, schedule_ops),
        }
    }

    pub(crate) fn check_fault(&self, point: neovex_storage::FaultPoint) -> Result<()> {
        match self {
            Self::Redb(store) => store.check_fault(point),
            Self::Sqlite(store) => store.check_fault(point),
            Self::SqliteReplica(store) => store.check_fault(point),
            Self::Postgres(store) => store.check_fault(point),
            Self::MySql(_store) => Ok(()),
        }
    }

    pub(crate) fn read_snapshot(&self) -> Result<TenantPersistenceSnapshot> {
        match self {
            Self::Redb(store) => store.read_snapshot().map(TenantPersistenceSnapshot::Redb),
            Self::Sqlite(store) => store
                .read_snapshot()
                .map(|snapshot| TenantPersistenceSnapshot::Sqlite(Arc::new(Mutex::new(snapshot)))),
            Self::SqliteReplica(store) => store.read_snapshot().map(|snapshot| {
                TenantPersistenceSnapshot::SqliteReplica(Arc::new(Mutex::new(snapshot)))
            }),
            Self::Postgres(store) => store
                .read_snapshot()
                .map(TenantPersistenceSnapshot::Postgres),
            Self::MySql(store) => store.read_snapshot().map(TenantPersistenceSnapshot::MySql),
        }
    }

    pub(crate) fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        match self {
            Self::Redb(store) => store.get(table, id),
            Self::Sqlite(store) => store.get(table, id),
            Self::SqliteReplica(store) => store.get(table, id),
            Self::Postgres(store) => store.get(table, id),
            Self::MySql(store) => store.get(table, id),
        }
    }

    pub(crate) fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        match self {
            Self::Redb(store) => {
                store.scan_table_matching_cancellable(table, check_cancel, include_document)
            }
            Self::Sqlite(store) => {
                store.scan_table_matching_cancellable(table, check_cancel, include_document)
            }
            Self::SqliteReplica(store) => {
                store.scan_table_matching_cancellable(table, check_cancel, include_document)
            }
            Self::Postgres(store) => {
                store.scan_table_matching_cancellable(table, check_cancel, include_document)
            }
            Self::MySql(store) => {
                store.scan_table_matching_cancellable(table, check_cancel, include_document)
            }
        }
    }

    pub(crate) fn insert(&self, document: &Document) -> Result<CommitEntry> {
        match self {
            Self::Redb(store) => store.insert(document),
            Self::Sqlite(store) => store.insert(document),
            Self::SqliteReplica(store) => store.insert(document),
            Self::Postgres(store) => store.insert(document),
            Self::MySql(store) => store.insert(document),
        }
    }

    pub(crate) fn insert_with_indexes(
        &self,
        document: &Document,
        indexes: &[neovex_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        match self {
            Self::Redb(store) => store.insert_with_indexes(document, indexes),
            Self::Sqlite(store) => store.insert_with_indexes(document, indexes),
            Self::SqliteReplica(store) => store.insert_with_indexes(document, indexes),
            Self::Postgres(store) => store.insert_with_indexes(document, indexes),
            Self::MySql(store) => store.insert_with_indexes(document, indexes),
        }
    }

    pub(crate) fn insert_once(
        &self,
        document: &Document,
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        match self {
            Self::Redb(store) => store.insert_once(document, execution_id),
            Self::Sqlite(store) => store.insert_once(document, execution_id),
            Self::SqliteReplica(store) => store.insert_once(document, execution_id),
            Self::Postgres(store) => store.insert_once(document, execution_id),
            Self::MySql(store) => store.insert_once(document, execution_id),
        }
    }

    pub(crate) fn insert_with_indexes_once(
        &self,
        document: &Document,
        indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        match self {
            Self::Redb(store) => store.insert_with_indexes_once(document, indexes, execution_id),
            Self::Sqlite(store) => store.insert_with_indexes_once(document, indexes, execution_id),
            Self::SqliteReplica(store) => {
                store.insert_with_indexes_once(document, indexes, execution_id)
            }
            Self::Postgres(store) => {
                store.insert_with_indexes_once(document, indexes, execution_id)
            }
            Self::MySql(store) => store.insert_with_indexes_once(document, indexes, execution_id),
        }
    }

    pub(crate) fn update_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => store.update_validated(table, id, patch, validate),
            Self::Sqlite(store) => store.update_validated(table, id, patch, validate),
            Self::SqliteReplica(store) => store.update_validated(table, id, patch, validate),
            Self::Postgres(store) => store.update_validated(table, id, patch, validate),
            Self::MySql(store) => store.update_validated(table, id, patch, validate),
        }
    }

    pub(crate) fn update_validated_once<F>(
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
        match self {
            Self::Redb(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
            Self::Sqlite(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
            Self::SqliteReplica(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
            Self::Postgres(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
            Self::MySql(store) => {
                store.update_validated_once(table, id, patch, execution_id, validate)
            }
        }
    }

    pub(crate) fn update_with_indexes_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        indexes: &[neovex_core::IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
            Self::Sqlite(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
            Self::SqliteReplica(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
            Self::Postgres(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
            Self::MySql(store) => {
                store.update_with_indexes_validated(table, id, patch, indexes, validate)
            }
        }
    }

    pub(crate) fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
            Self::Sqlite(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
            Self::SqliteReplica(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
            Self::Postgres(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
            Self::MySql(store) => store.update_with_indexes_validated_once(
                table,
                id,
                patch,
                indexes,
                execution_id,
                validate,
            ),
        }
    }

    pub(crate) fn delete_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => store.delete_validated_returning_document(table, id, validate),
            Self::Sqlite(store) => store.delete_validated_returning_document(table, id, validate),
            Self::SqliteReplica(store) => {
                store.delete_validated_returning_document(table, id, validate)
            }
            Self::Postgres(store) => store.delete_validated_returning_document(table, id, validate),
            Self::MySql(store) => store.delete_validated_returning_document(table, id, validate),
        }
    }

    pub(crate) fn delete_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => store.delete_validated_once(table, id, execution_id, validate),
            Self::Sqlite(store) => store.delete_validated_once(table, id, execution_id, validate),
            Self::SqliteReplica(store) => {
                store.delete_validated_once(table, id, execution_id, validate)
            }
            Self::Postgres(store) => store.delete_validated_once(table, id, execution_id, validate),
            Self::MySql(store) => store.delete_validated_once(table, id, execution_id, validate),
        }
    }

    pub(crate) fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[neovex_core::IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
            Self::Sqlite(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
            Self::SqliteReplica(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
            Self::Postgres(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
            Self::MySql(store) => {
                store.delete_with_indexes_validated_returning_document(table, id, indexes, validate)
            }
        }
    }

    pub(crate) fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        match self {
            Self::Redb(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
            Self::Sqlite(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
            Self::SqliteReplica(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
            Self::Postgres(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
            Self::MySql(store) => {
                store.delete_with_indexes_validated_once(table, id, indexes, execution_id, validate)
            }
        }
    }
}

impl TenantPersistenceExecutor {
    pub(crate) async fn execute<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(TenantPersistence) -> Result<T> + Send + 'static,
    {
        match self {
            Self::Redb(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::Redb(store)))
                    .await
            }
            Self::Sqlite(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::Sqlite(store)))
                    .await
            }
            Self::SqliteReplica(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::SqliteReplica(store)))
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::Postgres(store)))
                    .await
            }
            Self::MySql(storage) => {
                storage
                    .execute(move |store| task(TenantPersistence::MySql(store)))
                    .await
            }
        }
    }

    pub(crate) async fn execute_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<T>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(TenantPersistence, &mut dyn FnMut() -> Result<()>) -> Result<T> + Send + 'static,
    {
        match self {
            Self::Redb(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::Redb(store), check_cancel)
                    })
                    .await
            }
            Self::Sqlite(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::Sqlite(store), check_cancel)
                    })
                    .await
            }
            Self::SqliteReplica(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::SqliteReplica(store), check_cancel)
                    })
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::Postgres(store), check_cancel)
                    })
                    .await
            }
            Self::MySql(storage) => {
                storage
                    .execute_cancellable(cancel_wait, check_cancel, move |store, check_cancel| {
                        task(TenantPersistence::MySql(store), check_cancel)
                    })
                    .await
            }
        }
    }

    pub(crate) async fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut dyn TenantPersistenceWriteOps) -> Result<T> + Send + 'static,
    {
        match self {
            Self::Redb(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
            Self::Sqlite(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
            Self::SqliteReplica(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
            Self::MySql(storage) => {
                storage
                    .execute_write(move |transaction| task(transaction))
                    .await
            }
        }
    }

    pub(crate) async fn execute_write_cancellable<T, Fut, Check, F>(
        &self,
        cancel_wait: Fut,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteOutcome<T>>
    where
        T: Send + 'static,
        Fut: Future<Output = ()> + Send,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut dyn TenantPersistenceWriteOps) -> Result<T> + Send + 'static,
    {
        match self {
            Self::Redb(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
            Self::Sqlite(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
            Self::SqliteReplica(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
            Self::Postgres(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
            Self::MySql(storage) => {
                storage
                    .execute_write_cancellable(cancel_wait, check_cancel, move |transaction| {
                        task(transaction)
                    })
                    .await
            }
        }
    }
}

impl TenantPersistenceSnapshot {
    pub(crate) fn applied_sequence(&self) -> Result<SequenceNumber> {
        match self {
            Self::Redb(snapshot) => snapshot.applied_sequence(),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .applied_sequence(),
            Self::SqliteReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .applied_sequence(),
            Self::Postgres(snapshot) => snapshot.applied_sequence(),
            Self::MySql(snapshot) => snapshot.applied_sequence(),
        }
    }

    pub(crate) fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        match self {
            Self::Redb(snapshot) => snapshot.get(table, id),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .get(table, id),
            Self::SqliteReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .get(table, id),
            Self::Postgres(snapshot) => snapshot.get(table, id),
            Self::MySql(snapshot) => snapshot.get(table, id),
        }
    }

    pub(crate) fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        match self {
            Self::Redb(snapshot) => {
                snapshot.scan_table_matching_cancellable(table, check_cancel, include_document)
            }
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_table_matching_with_filters_cancellable(
                    table,
                    &[],
                    check_cancel,
                    include_document,
                ),
            Self::SqliteReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_table_matching_with_filters_cancellable(
                    table,
                    &[],
                    check_cancel,
                    include_document,
                ),
            Self::Postgres(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                &[],
                check_cancel,
                include_document,
            ),
            Self::MySql(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                &[],
                check_cancel,
                include_document,
            ),
        }
    }
}

impl QueryReadStore for TenantPersistence {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        TenantPersistence::get(self, table, id)
    }

    fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[neovex_core::Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        match self {
            Self::Redb(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::Sqlite(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::SqliteReplica(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::Postgres(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::MySql(store) => store.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
        }
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::Sqlite(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::SqliteReplica(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::Postgres(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::MySql(store) => {
                store.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
        }
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
            Self::Sqlite(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
            Self::SqliteReplica(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
            Self::Postgres(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
            Self::MySql(store) => {
                store.index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel)
            }
        }
    }

    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Sqlite(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::SqliteReplica(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Postgres(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::MySql(store) => store.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
        }
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Sqlite(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::SqliteReplica(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Postgres(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::MySql(store) => store.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
        }
    }
}

impl QueryReadStore for TenantPersistenceSnapshot {
    fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        TenantPersistenceSnapshot::get(self, table, id)
    }

    fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[neovex_core::Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        match self {
            Self::Redb(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_table_matching_with_filters_cancellable(
                    table,
                    filters,
                    check_cancel,
                    include_document,
                ),
            Self::SqliteReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .scan_table_matching_with_filters_cancellable(
                    table,
                    filters,
                    check_cancel,
                    include_document,
                ),
            Self::Postgres(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
            Self::MySql(snapshot) => snapshot.scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            ),
        }
    }

    fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &serde_json::Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(snapshot) => {
                snapshot.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_eq_cancellable(table, index_name, value, check_cancel),
            Self::SqliteReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_eq_cancellable(table, index_name, value, check_cancel),
            Self::Postgres(snapshot) => {
                snapshot.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
            Self::MySql(snapshot) => {
                snapshot.index_scan_eq_cancellable(table, index_name, value, check_cancel)
            }
        }
    }

    fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[serde_json::Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(snapshot) => snapshot.index_scan_prefix_cancellable(
                table,
                index_name,
                prefix_values,
                check_cancel,
            ),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel),
            Self::SqliteReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_prefix_cancellable(table, index_name, prefix_values, check_cancel),
            Self::Postgres(snapshot) => snapshot.index_scan_prefix_cancellable(
                table,
                index_name,
                prefix_values,
                check_cancel,
            ),
            Self::MySql(snapshot) => snapshot.index_scan_prefix_cancellable(
                table,
                index_name,
                prefix_values,
                check_cancel,
            ),
        }
    }

    fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(snapshot) => snapshot.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_range_cancellable(
                    table,
                    index_name,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                    check_cancel,
                ),
            Self::SqliteReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_range_cancellable(
                    table,
                    index_name,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                    check_cancel,
                ),
            Self::Postgres(snapshot) => snapshot.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::MySql(snapshot) => snapshot.index_scan_range_cancellable(
                table,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
        }
    }

    fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[serde_json::Value],
        start: Option<&serde_json::Value>,
        end: Option<&serde_json::Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        match self {
            Self::Redb(snapshot) => snapshot.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::Sqlite(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_composite_range_cancellable(
                    table,
                    index_name,
                    exact_prefix,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                    check_cancel,
                ),
            Self::SqliteReplica(snapshot) => snapshot
                .lock()
                .expect("sqlite read snapshot lock should not be poisoned")
                .index_scan_composite_range_cancellable(
                    table,
                    index_name,
                    exact_prefix,
                    start,
                    end,
                    start_inclusive,
                    end_inclusive,
                    check_cancel,
                ),
            Self::Postgres(snapshot) => snapshot.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
            Self::MySql(snapshot) => snapshot.index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                check_cancel,
            ),
        }
    }
}

fn map_opened_redb_tenant(
    result: Result<OpenedEmbeddedRedbTenant>,
) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_redb_tenant_sync)
}

fn map_opened_redb_tenant_sync(opened: OpenedEmbeddedRedbTenant) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::Redb(opened.store),
        executor: TenantPersistenceExecutor::Redb(opened.read_storage),
    }
}

fn map_opened_sqlite_tenant(
    result: Result<OpenedEmbeddedSqliteTenant>,
) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_sqlite_tenant_sync)
}

fn map_opened_sqlite_tenant_sync(opened: OpenedEmbeddedSqliteTenant) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::Sqlite(opened.store),
        executor: TenantPersistenceExecutor::Sqlite(opened.read_storage),
    }
}

fn map_opened_sqlite_replica_tenant(
    result: Result<OpenedLibsqlReplicaTenant>,
) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_sqlite_replica_tenant_sync)
}

fn map_opened_sqlite_replica_tenant_sync(
    opened: OpenedLibsqlReplicaTenant,
) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::SqliteReplica(opened.store),
        executor: TenantPersistenceExecutor::SqliteReplica(opened.read_storage),
    }
}

fn map_opened_postgres_tenant(
    result: Result<OpenedPostgresTenant>,
) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_postgres_tenant_sync)
}

fn map_opened_postgres_tenant_sync(opened: OpenedPostgresTenant) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::Postgres(opened.store),
        executor: TenantPersistenceExecutor::Postgres(opened.read_storage),
    }
}

fn map_opened_mysql_tenant(result: Result<OpenedMySqlTenant>) -> Result<OpenedTenantPersistence> {
    result.map(map_opened_mysql_tenant_sync)
}

fn map_opened_mysql_tenant_sync(opened: OpenedMySqlTenant) -> OpenedTenantPersistence {
    OpenedTenantPersistence {
        persistence: TenantPersistence::MySql(opened.store),
        executor: TenantPersistenceExecutor::MySql(opened.read_storage),
    }
}
