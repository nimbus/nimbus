use std::sync::{Arc, Mutex};

use neovex_core::{
    CommitEntry, CronJob, Document, DocumentId, DurableMutationRecord, Result, ScheduledJob,
    ScheduledJobResult, Schema, SequenceNumber, TableName, TableSchema, Timestamp,
};
use neovex_storage::{
    DurableJournalBootstrap, DurableJournalPage, FaultPoint, JournalProgress,
    LibsqlReplicaFreshnessStats, LibsqlReplicaTenantStore, MySqlTenantStore, PostgresTenantStore,
    ResolvedScheduleOp, ResolvedWrite, SqliteTenantStore, TenantStore as RedbTenantStore,
};

use super::TenantPersistenceSnapshot;

#[derive(Clone)]
pub(crate) enum TenantPersistence {
    Redb(Arc<RedbTenantStore>),
    Sqlite(Arc<SqliteTenantStore>),
    LibsqlReplica(Arc<LibsqlReplicaTenantStore>),
    Postgres(Arc<PostgresTenantStore>),
    MySql(Arc<MySqlTenantStore>),
}

macro_rules! delegate_store_method {
    ($(#[$meta:meta])* fn $name:ident(&self $(, $arg:ident : $ty:ty )* ) -> $ret:ty) => {
        $(#[$meta])*
        pub(crate) fn $name(&self, $($arg: $ty),*) -> $ret {
            match self {
                Self::Redb(store) => store.$name($($arg),*),
                Self::Sqlite(store) => store.$name($($arg),*),
                Self::LibsqlReplica(store) => store.$name($($arg),*),
                Self::Postgres(store) => store.$name($($arg),*),
                Self::MySql(store) => store.$name($($arg),*),
            }
        }
    };
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
            Self::LibsqlReplica(store) => store.replace_table_schema(table_schema),
            Self::Postgres(store) => store.replace_table_schema(table_schema),
            Self::MySql(store) => store.replace_table_schema(table_schema),
        }
    }

    pub(crate) fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        match self {
            Self::Redb(store) => store.delete_table_schema(table),
            Self::Sqlite(store) => store.delete_table_schema(table),
            Self::LibsqlReplica(store) => store.delete_table_schema(table),
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
            Self::LibsqlReplica(store) => store.append_durable_records_batch(records),
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
            Self::LibsqlReplica(store) => store.apply_durable_records_batch(records),
            Self::Postgres(store) => store.apply_durable_records_batch(records),
            Self::MySql(store) => store.apply_durable_records_batch(records),
        }
    }

    pub(crate) fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        match self {
            Self::Redb(store) => store.insert_scheduled_job(job),
            Self::Sqlite(store) => store.insert_scheduled_job(job),
            Self::LibsqlReplica(store) => store.insert_scheduled_job(job),
            Self::Postgres(store) => store.insert_scheduled_job(job),
            Self::MySql(store) => store.insert_scheduled_job(job),
        }
    }

    pub(crate) fn claim_due_jobs(&self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        match self {
            Self::Redb(store) => store.claim_due_jobs(now),
            Self::Sqlite(store) => store.claim_due_jobs(now),
            Self::LibsqlReplica(store) => store.claim_due_jobs(now),
            Self::Postgres(store) => store.claim_due_jobs(now),
            Self::MySql(store) => store.claim_due_jobs(now),
        }
    }

    pub(crate) fn complete_scheduled_job(&self, job_id: &DocumentId) -> Result<()> {
        match self {
            Self::Redb(store) => store.complete_scheduled_job(job_id),
            Self::Sqlite(store) => store.complete_scheduled_job(job_id),
            Self::LibsqlReplica(store) => store.complete_scheduled_job(job_id),
            Self::Postgres(store) => store.complete_scheduled_job(job_id),
            Self::MySql(store) => store.complete_scheduled_job(job_id),
        }
    }

    pub(crate) fn cancel_scheduled_job(&self, job_id: &DocumentId) -> Result<bool> {
        match self {
            Self::Redb(store) => store.cancel_scheduled_job(job_id),
            Self::Sqlite(store) => store.cancel_scheduled_job(job_id),
            Self::LibsqlReplica(store) => store.cancel_scheduled_job(job_id),
            Self::Postgres(store) => store.cancel_scheduled_job(job_id),
            Self::MySql(store) => store.cancel_scheduled_job(job_id),
        }
    }

    #[cfg(test)]
    pub(crate) fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        match self {
            Self::Redb(store) => store.record_scheduled_job_result(result),
            Self::Sqlite(store) => store.record_scheduled_job_result(result),
            Self::LibsqlReplica(store) => store.record_scheduled_job_result(result),
            Self::Postgres(store) => store.record_scheduled_job_result(result),
            Self::MySql(store) => store.record_scheduled_job_result(result),
        }
    }

    pub(crate) fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        match self {
            Self::Redb(store) => store.save_cron_job(cron),
            Self::Sqlite(store) => store.save_cron_job(cron),
            Self::LibsqlReplica(store) => store.save_cron_job(cron),
            Self::Postgres(store) => store.save_cron_job(cron),
            Self::MySql(store) => store.save_cron_job(cron),
        }
    }

    pub(crate) fn delete_cron_job(&self, name: &str) -> Result<()> {
        match self {
            Self::Redb(store) => store.delete_cron_job(name),
            Self::Sqlite(store) => store.delete_cron_job(name),
            Self::LibsqlReplica(store) => store.delete_cron_job(name),
            Self::Postgres(store) => store.delete_cron_job(name),
            Self::MySql(store) => store.delete_cron_job(name),
        }
    }

    pub(crate) fn recover_running_jobs(&self, now: Timestamp) -> Result<()> {
        match self {
            Self::Redb(store) => store.recover_running_jobs(now),
            Self::Sqlite(store) => store.recover_running_jobs(now),
            Self::LibsqlReplica(store) => store.recover_running_jobs(now),
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
            Self::LibsqlReplica(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::Postgres(store) => store.apply_execution_unit_batch(writes, schedule_ops),
            Self::MySql(store) => store.apply_execution_unit_batch(writes, schedule_ops),
        }
    }

    pub(crate) fn check_fault(&self, point: FaultPoint) -> Result<()> {
        match self {
            Self::Redb(store) => store.check_fault(point),
            Self::Sqlite(store) => store.check_fault(point),
            Self::LibsqlReplica(store) => store.check_fault(point),
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
            Self::LibsqlReplica(store) => store.read_snapshot().map(|snapshot| {
                TenantPersistenceSnapshot::LibsqlReplica(Arc::new(Mutex::new(snapshot)))
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
            Self::LibsqlReplica(store) => store.get(table, id),
            Self::Postgres(store) => store.get(table, id),
            Self::MySql(store) => store.get(table, id),
        }
    }

    pub(crate) fn libsql_replica_freshness_stats(&self) -> Option<LibsqlReplicaFreshnessStats> {
        match self {
            Self::LibsqlReplica(store) => store.replica_freshness_stats().ok(),
            Self::Redb(_) | Self::Sqlite(_) | Self::Postgres(_) | Self::MySql(_) => None,
        }
    }

    pub(crate) fn invalidate_schema_cache(&self) {
        match self {
            Self::Postgres(store) => store.invalidate_schema_cache(),
            Self::MySql(store) => store.invalidate_schema_cache(),
            Self::Redb(_) | Self::Sqlite(_) | Self::LibsqlReplica(_) => {}
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
            Self::LibsqlReplica(store) => {
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
            Self::LibsqlReplica(store) => store.insert(document),
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
            Self::LibsqlReplica(store) => store.insert_with_indexes(document, indexes),
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
            Self::LibsqlReplica(store) => store.insert_once(document, execution_id),
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
            Self::LibsqlReplica(store) => {
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
            Self::LibsqlReplica(store) => store.update_validated(table, id, patch, validate),
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
            Self::LibsqlReplica(store) => {
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
            Self::LibsqlReplica(store) => {
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
            Self::LibsqlReplica(store) => store.update_with_indexes_validated_once(
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
            Self::LibsqlReplica(store) => {
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
            Self::LibsqlReplica(store) => {
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
            Self::LibsqlReplica(store) => {
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
            Self::LibsqlReplica(store) => {
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
