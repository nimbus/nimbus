use super::document_versions::{
    prune_document_versions_before_in_session, record_document_versions_for_events_in_session,
};
use super::index_versions::{
    prune_index_versions_before_in_session, record_index_versions_for_events_in_session,
};
use super::*;
use crate::CommitterLeaseResult;
use crate::sql::schema_events::{
    durable_record_changes_schema_cache, sql_record_schema_set_events,
};
use crate::sql::store_core::{
    SqlDurableJournalStore, SqlStoreCore, SqlWriteTransactionCore,
    sql_store_append_durable_records_batch, sql_store_apply_durable_records_batch,
    sql_store_core_facade, sql_store_fenced_append_and_apply_durable_records_batch_cancellable,
};
use crate::sql::write_core::SqlDurableJournalTransaction;
use crate::sql::write_pipeline::SqlWritePipelineMetrics;

sql_store_core_facade!(PostgresTenantStore);

impl PostgresTenantStore {
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
}

/// Wire PostgreSQL into the shared store-level wrapper layer. Everything below
/// forwards to the inherent write bridge above or to an inherent journal read;
/// the wrappers built on them live once in [`crate::sql::store_core`].
impl SqlStoreCore for PostgresTenantStore {
    type Transaction = PostgresWriteTransaction;

    fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        PostgresTenantStore::execute_write(self, task)
    }

    fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut PostgresWriteTransaction) -> Result<T> + Send + 'static,
    {
        PostgresTenantStore::execute_write_cancellable(self, check_cancel, task)
    }

    fn retention_floor(&self) -> &RetentionFloor {
        self.retention_floor.as_ref()
    }

    fn journal_progress(&self) -> Result<JournalProgress> {
        PostgresTenantStore::journal_progress(self)
    }

    fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        PostgresTenantStore::read_durable_journal_from(self, sequence)
    }

    fn recover_durable_journal(&self) -> Result<JournalProgress> {
        PostgresTenantStore::recover_durable_journal(self)
    }

    fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        PostgresTenantStore::export_materialized_journal_snapshot(self)
    }

    fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        sql_store_append_durable_records_batch(self, records)
    }

    fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        sql_store_apply_durable_records_batch(self, records)
    }

    fn fenced_append_and_apply_durable_records_batch_cancellable<Check>(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        records: &[TenantEventRecord],
        check_cancel: Check,
    ) -> CommitterLeaseResult<()>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        sql_store_fenced_append_and_apply_durable_records_batch_cancellable(
            self,
            owner_id,
            epoch,
            expected_previous,
            records,
            check_cancel,
        )
    }
}

impl SqlDurableJournalStore for PostgresTenantStore {
    fn pipeline_metrics(&self) -> &SqlWritePipelineMetrics {
        self.pipeline_metrics.as_ref()
    }
}

/// Transaction-side seam for the shared wrappers. Each method forwards to the
/// inherent method of the same name, which wins method-call resolution.
impl SqlWriteTransactionCore for PostgresWriteTransaction {
    fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        PostgresWriteTransaction::begin_scheduled_execution(self, execution_id)
    }

    fn set_prepared_record(&mut self, record: TenantEventRecord) {
        PostgresWriteTransaction::set_prepared_record(self, record)
    }

    fn set_trigger_write_origin(&mut self, trigger_write_origin: Option<TriggerWriteOrigin>) {
        PostgresWriteTransaction::set_trigger_write_origin(self, trigger_write_origin)
    }

    fn set_commit_timestamp(&mut self, commit_timestamp: Option<Timestamp>) {
        PostgresWriteTransaction::set_commit_timestamp(self, commit_timestamp)
    }

    fn advance_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        durable_sequence: SequenceNumber,
    ) -> Result<u64> {
        PostgresWriteTransaction::advance_fenced_committer_lease(
            self,
            owner_id,
            epoch,
            expected_previous,
            durable_sequence,
        )
    }

    fn validate_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        durable_sequence: SequenceNumber,
    ) -> Result<u64> {
        PostgresWriteTransaction::validate_fenced_committer_lease(
            self,
            owner_id,
            epoch,
            durable_sequence,
        )
    }

    fn materialize_trigger_invocations(
        &mut self,
        records: &[nimbus_core::TriggerInvocationRecord],
        cursor: TriggerDeliveryCursor,
    ) -> Result<()> {
        PostgresWriteTransaction::materialize_trigger_invocations(self, records, cursor)
    }

    fn save_trigger_invocation(
        &mut self,
        record: &nimbus_core::TriggerInvocationRecord,
    ) -> Result<()> {
        PostgresWriteTransaction::save_trigger_invocation(self, record)
    }

    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        PostgresWriteTransaction::replace_table_schema(self, table_schema)
    }

    fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        PostgresWriteTransaction::delete_table_schema(self, table)
    }

    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        PostgresWriteTransaction::insert_scheduled_job(self, job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        PostgresWriteTransaction::claim_due_jobs(self, now, max_jobs)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        PostgresWriteTransaction::complete_scheduled_job(self, job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        PostgresWriteTransaction::cancel_scheduled_job(self, job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        PostgresWriteTransaction::record_scheduled_job_result(self, result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        PostgresWriteTransaction::save_cron_job(self, cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        PostgresWriteTransaction::delete_cron_job(self, name)
    }

    fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        PostgresWriteTransaction::recover_running_jobs(self, now)
    }

    fn apply_resolved_write(&mut self, write: &ResolvedWrite) -> Result<()> {
        PostgresWriteTransaction::apply_resolved_write(self, write)
    }

    fn update_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        validate: F,
    ) -> Result<()>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        PostgresWriteTransaction::update_document_validated(self, table, id, patch, validate)
    }

    fn delete_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<Document>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        PostgresWriteTransaction::delete_document_validated(self, table, id, validate)
    }

    fn prune_retained_versions(
        &mut self,
        document_prune_before: SequenceNumber,
        index_prune_before: SequenceNumber,
    ) -> Result<(u64, u64)> {
        PostgresWriteTransaction::prune_retained_versions(
            self,
            document_prune_before,
            index_prune_before,
        )
    }
}

impl PostgresWriteTransaction {
    pub(crate) fn validate_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        durable_sequence: SequenceNumber,
    ) -> Result<u64> {
        self.advance_fenced_committer_lease(owner_id, epoch, durable_sequence, durable_sequence)
    }

    pub(super) fn advance_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        durable_sequence: SequenceNumber,
    ) -> Result<u64> {
        let epoch = i64::try_from(epoch)
            .map_err(|_| Error::InvalidInput("lease epoch exceeds BIGINT".to_string()))?;
        let expected_previous = i64_from_sequence(expected_previous)?;
        let durable_sequence = i64_from_sequence(durable_sequence)?;
        let query = format!(
            "UPDATE {} SET durable_sequence = $4::BIGINT \
             WHERE singleton = TRUE AND owner_id = $1::TEXT AND epoch = $2::BIGINT \
                   AND expires_at > CURRENT_TIMESTAMP \
                   AND durable_sequence = $3::BIGINT",
            qualified_table(&self.schema_name, "committer_lease")
        );
        let owner_id = owner_id.to_string();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(
                    query.as_str(),
                    &[&owner_id, &epoch, &expected_previous, &durable_sequence],
                )
                .await
                .map_err(map_postgres_error)
        })
    }

    pub(super) fn begin<Check>(store: PostgresTenantStore, check_cancel: Check) -> Result<Self>
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
            pipeline_metrics: store.pipeline_metrics.clone(),
            client: Some(client),
            commit_writes: Vec::new(),
            tenant_events: Vec::new(),
            prepared_record: None,
            trigger_write_origin: None,
            commit_timestamp: None,
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

    pub fn prune_retained_versions(
        &mut self,
        document_prune_before: SequenceNumber,
        index_prune_before: SequenceNumber,
    ) -> Result<(u64, u64)> {
        self.check_cancel()?;
        let schema_name = self.schema_name.clone();
        let client = self.session()?;
        self.block_on(async move {
            let document_versions_pruned = prune_document_versions_before_in_session(
                client,
                &schema_name,
                document_prune_before,
            )
            .await?;
            let index_versions_pruned =
                prune_index_versions_before_in_session(client, &schema_name, index_prune_before)
                    .await?;
            Ok((document_versions_pruned, index_versions_pruned))
        })
    }

    pub fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        let table_id = self.resolve_or_create_table_id(&table_schema.table)?;
        let previous = self.load_table_schema(&table_schema.table)?;
        let mut table_schema = table_schema.clone();
        table_schema.reconcile_index_metadata(previous.as_ref());
        if let Some(previous) = previous.as_ref() {
            self.drop_table_indexes(previous)?;
        }
        self.upsert_table_schema(&table_schema)?;
        self.create_table_indexes(&table_schema)?;
        self.notification.schema_changed = true;
        self.schema_cache_changed = true;
        sql_record_schema_set_events(self, table_id, previous, &table_schema);
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        let previous = self.load_table_schema(table)?;
        let table_id = self.load_table_id(table)?;
        if let Some(previous) = previous.as_ref() {
            self.drop_table_indexes(previous)?;
        }
        self.delete_table_schema_entry(table)?;
        self.notification.schema_changed = true;
        self.schema_cache_changed = true;
        self.record_tenant_event(TenantEventKind::SchemaChange {
            change: Box::new(SchemaChangeEvent::DeleteTable {
                table: table.clone(),
                table_id,
                previous,
            }),
        });
        Ok(())
    }

    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        let schema_name = self.schema_name.clone();
        let execution_id = execution_id.map(str::to_string);
        let event_execution_id = execution_id.clone();
        let client = self.session()?;
        let inserted = self.block_on(async move {
            begin_scheduled_execution_in_session(client, &schema_name, execution_id.as_deref())
                .await
        })?;
        if inserted && let Some(execution_id) = event_execution_id {
            self.record_tenant_event(TenantEventKind::ScheduledExecution { execution_id });
        }
        Ok(inserted)
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        let table_id = self.resolve_or_create_table_id(&document.table)?;
        let write_table_id = table_id.clone();
        let query = format!(
            "INSERT INTO {} (table_id, id, data_json, typed_fields_json, creation_time, update_time) VALUES ($1, $2, $3, $4, $5, $6)",
            qualified_table(&self.schema_name, "documents")
        );
        let id = document.id.to_string();
        let data_json = serialize_document_fields(document)?;
        let typed_fields_json = serialize_document_typed_fields(document)?;
        let creation_time = i64_from_timestamp(document.creation_time)?;
        let update_time = i64_from_timestamp(document.update_time)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(
                    query.as_str(),
                    &[
                        &table_id.as_str(),
                        &id,
                        &data_json,
                        &typed_fields_json,
                        &creation_time,
                        &update_time,
                    ],
                )
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        self.record_commit_write(WriteOp {
            table: document.table.clone(),
            table_id: write_table_id,
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
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
            .ok_or(Error::DocumentNotFound(id.clone()))?;
        let mut document = existing_document.clone();
        for (field, value) in patch {
            document.set_field(field.clone(), value.clone());
        }
        document.update_time = self
            .commit_timestamp
            .unwrap_or_else(|| self.provider.clock.now());
        validate(&existing_document, &document)?;
        let table_id = self
            .load_table_id(table)?
            .ok_or(Error::DocumentNotFound(id.clone()))?;
        let write_table_id = table_id.clone();

        let query = format!(
            "UPDATE {} SET data_json = $3, typed_fields_json = $4, creation_time = $5, update_time = $6 WHERE table_id = $1 AND id = $2",
            qualified_table(&self.schema_name, "documents")
        );
        let document_id = id.to_string();
        let data_json = serialize_document_fields(&document)?;
        let typed_fields_json = serialize_document_typed_fields(&document)?;
        let creation_time = i64_from_timestamp(document.creation_time)?;
        let update_time = i64_from_timestamp(document.update_time)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(
                    query.as_str(),
                    &[
                        &table_id.as_str(),
                        &document_id,
                        &data_json,
                        &typed_fields_json,
                        &creation_time,
                        &update_time,
                    ],
                )
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        let resource_path_binding = self.resource_path_binding(
            &nimbus_core::DocumentLocator::new(table.clone(), id.clone()),
        )?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
            table_id: write_table_id,
            op_type: WriteOpType::Update,
            doc_id: id.clone(),
            resource_path_binding,
            trigger_write_origin: None,
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
            .ok_or(Error::DocumentNotFound(id.clone()))?;
        validate(&removed_document)?;
        let table_id = self
            .load_table_id(table)?
            .ok_or(Error::DocumentNotFound(id.clone()))?;
        let write_table_id = table_id.clone();

        let query = format!(
            "DELETE FROM {} WHERE table_id = $1 AND id = $2",
            qualified_table(&self.schema_name, "documents")
        );
        let document_id = id.to_string();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&table_id.as_str(), &document_id])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })?;
        let resource_path_binding = self.remove_resource_path_binding(
            &nimbus_core::DocumentLocator::new(table.clone(), id.clone()),
        )?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
            table_id: write_table_id,
            op_type: WriteOpType::Delete,
            doc_id: id.clone(),
            resource_path_binding,
            trigger_write_origin: None,
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

    pub fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        crate::sql::scheduler_core::sql_claim_due_jobs(self, now, max_jobs)
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
        crate::sql::scheduler_core::sql_recover_running_jobs(self, now)
    }

    pub fn apply_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()> {
        crate::sql::write_core::sql_apply_durable_records_batch(self, records)
    }

    pub fn apply_resolved_write(&mut self, write: &ResolvedWrite) -> Result<()> {
        crate::sql::write_core::sql_apply_resolved_write(self, write)
    }

    pub(crate) fn set_prepared_record(&mut self, record: TenantEventRecord) {
        self.commit_writes = record.writes.clone();
        self.tenant_events = record
            .events
            .iter()
            .filter(|event| !matches!(event, TenantEventKind::DocumentWrite { .. }))
            .cloned()
            .collect();
        self.prepared_record = Some(record);
    }

    fn append_prepared_record(&mut self, record: &TenantEventRecord) -> Result<CommitEntry> {
        record.validate_integrity()?;
        let expected = self.latest_sequence()?.0.saturating_add(1);
        if record.sequence.0 != expected {
            return Err(Error::conflict(format!(
                "prepared commit expected storage sequence {expected}, got {}",
                record.sequence.0
            )));
        }
        crate::commit_log::ensure_applied_prefix_precedes(
            self.applied_sequence()?,
            record.sequence,
        )?;
        self.append_durable_records_batch(std::slice::from_ref(record))?;
        let sequence = record.sequence;
        self.write_applied_sequence(sequence)?;
        Ok(record.as_commit_entry())
    }

    pub fn commit(self) -> Result<Option<CommitEntry>> {
        crate::sql::write_core::sql_commit(self)
    }

    pub fn rollback(&mut self) {
        crate::sql::write_core::sql_rollback(self)
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

    pub(super) fn block_on<T, Fut>(&self, future: Fut) -> Result<T>
    where
        Fut: Future<Output = Result<T>>,
    {
        self.provider.runtime_handle.block_on(future)
    }

    pub(super) fn session(&self) -> Result<&Client> {
        self.client
            .as_ref()
            .ok_or_else(|| Error::Internal("Postgres write transaction already closed".to_string()))
    }

    // PG `pg_advisory_xact_lock` tenant-lock order — do not unify with MySQL's
    // `SELECT ... FOR UPDATE`; lock acquisition order differs by dialect, see CO6.
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

    pub(super) fn latest_sequence(&mut self) -> Result<SequenceNumber> {
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

    fn load_durable_record(
        &mut self,
        sequence: SequenceNumber,
    ) -> Result<Option<TenantEventRecord>> {
        let query = format!(
            "SELECT record_blob FROM {} WHERE sequence = $1",
            qualified_table(&self.schema_name, "commit_log")
        );
        let sequence = i64_from_sequence(sequence)?;
        let client = self.session()?;
        self.block_on(async move {
            client
                .query_opt(query.as_str(), &[&sequence])
                .await
                .map_err(map_postgres_error)?
                .map(|row| {
                    let payload: Vec<u8> = row.get(0);
                    deserialize_tenant_event_record(payload.as_slice())
                })
                .transpose()
        })
    }

    fn append_commit_entry(
        &mut self,
        writes: Vec<WriteOp>,
        events: Vec<TenantEventKind>,
    ) -> Result<CommitEntry> {
        let sequence = SequenceNumber(self.latest_sequence()?.0.saturating_add(1));
        crate::commit_log::ensure_applied_prefix_precedes(self.applied_sequence()?, sequence)?;
        let timestamp = self
            .commit_timestamp
            .unwrap_or_else(|| self.provider.clock.now());
        let record = TenantEventRecord::from_events(sequence, timestamp, events)?;
        let entry = CommitEntry {
            sequence,
            timestamp,
            writes,
        };
        let query = format!(
            "INSERT INTO {} (sequence, record_blob) VALUES ($1, $2)",
            qualified_table(&self.schema_name, "commit_log")
        );
        let schema_name = self.schema_name.clone();
        let sequence_i64 = i64_from_sequence(entry.sequence)?;
        let payload = serialize_tenant_event_record(&record)?;
        let record_sequence = record.sequence;
        let record_timestamp = record.timestamp;
        let record_events = record.events.clone();
        let client = self.session()?;
        self.block_on(async move {
            record_document_versions_for_events_in_session(
                client,
                &schema_name,
                record_sequence,
                record_timestamp,
                &record_events,
            )
            .await?;
            record_index_versions_for_events_in_session(
                client,
                &schema_name,
                record_sequence,
                &record_events,
            )
            .await?;
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
        let id = id.clone();
        let client = self.session()?;
        self.block_on(
            async move { load_document_from_session(client, &schema_name, &table, &id).await },
        )
    }

    pub(super) fn load_table_id(&mut self, table: &TableName) -> Result<Option<TableId>> {
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let client = self.session()?;
        self.block_on(async move { load_table_id_from_session(client, &schema_name, &table).await })
    }

    fn resolve_or_create_table_id(&mut self, table: &TableName) -> Result<TableId> {
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let id_source = self.provider.id_source.clone();
        let client = self.session()?;
        self.block_on(async move {
            resolve_or_create_table_id_in_session(client, &schema_name, &table, id_source.as_ref())
                .await
        })
    }

    pub(super) fn load_table_schema(&mut self, table: &TableName) -> Result<Option<TableSchema>> {
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

    pub(super) fn delete_table_schema_entry(&mut self, table: &TableName) -> Result<()> {
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

    pub(super) fn drop_table_indexes(&mut self, table_schema: &TableSchema) -> Result<()> {
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

    fn apply_durable_record(&mut self, record: &TenantEventRecord) -> Result<()> {
        let schema_name = self.schema_name.clone();
        let record = record.clone();
        let changes_schema_cache = durable_record_changes_schema_cache(&record);
        let client = self.session()?;
        let result = self.block_on(async move {
            apply_durable_record_in_session(client, &schema_name, &record).await
        });
        if result.is_ok() {
            self.record_durable_schema_change_effects(changes_schema_cache);
        }
        result
    }

    pub(super) fn record_durable_schema_change_effects(&mut self, changed: bool) {
        if changed {
            self.notification.schema_changed = true;
            self.schema_cache_changed = true;
        }
    }

    fn set_trigger_write_origin(&mut self, trigger_write_origin: Option<TriggerWriteOrigin>) {
        self.trigger_write_origin = trigger_write_origin;
    }

    fn set_commit_timestamp(&mut self, commit_timestamp: Option<Timestamp>) {
        self.commit_timestamp = commit_timestamp;
    }

    fn record_commit_write(&mut self, write: WriteOp) {
        crate::sql::write_core::sql_record_commit_write(self, write)
    }

    pub(super) fn record_tenant_event(&mut self, event: TenantEventKind) {
        crate::sql::write_core::sql_record_tenant_event(self, event)
    }

    // PG-only `pg_notify` fan-out. MySQL has no notification channel, so this
    // stays per-backend; the shared write core invokes it through the
    // `enqueue_notification` seam (a no-op on MySQL), see CO6.
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

impl crate::sql::scheduler_core::SqlSchedulerTransaction for PostgresWriteTransaction {
    fn select_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        // PG claim relies on the per-tenant advisory transaction lock to serialize
        // claimers, so it omits the `FOR UPDATE` row lock MySQL uses. Dialect lock
        // mode is load-bearing — not unified with the write core, see CO6.
        let query = format!(
            "SELECT data_json FROM {} WHERE run_at <= $1 ORDER BY run_at, id LIMIT $2",
            qualified_table(&self.schema_name, "scheduled_jobs")
        );
        let run_at = claim_due_jobs_upper_bound(now);
        let max_jobs = i64::try_from(max_jobs).unwrap_or(i64::MAX);
        let client = self.session()?;
        self.block_on(async move {
            let rows = client
                .query(query.as_str(), &[&run_at, &max_jobs])
                .await
                .map_err(map_postgres_error)?;
            rows.into_iter()
                .map(|row| deserialize_json::<ScheduledJob>(row.get::<_, String>(0).as_str()))
                .collect::<Result<Vec<_>>>()
        })
    }

    fn move_job_to_running(&mut self, job: &ScheduledJob) -> Result<()> {
        let delete_query = format!(
            "DELETE FROM {} WHERE id = $1",
            qualified_table(&self.schema_name, "scheduled_jobs")
        );
        let insert_query = format!(
            "INSERT INTO {} (id, data_json) VALUES ($1, $2)",
            qualified_table(&self.schema_name, "running_scheduled_jobs")
        );
        let job_id = job.id.to_string();
        let data_json = serialize_json(job)?;
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
        })
    }

    fn load_running_jobs(&mut self) -> Result<Vec<ScheduledJob>> {
        self.load_running_jobs()
    }

    fn move_job_to_pending(&mut self, job: &ScheduledJob) -> Result<()> {
        let delete_query = format!(
            "DELETE FROM {} WHERE id = $1",
            qualified_table(&self.schema_name, "running_scheduled_jobs")
        );
        let insert_query = format!(
            "INSERT INTO {} (id, run_at, data_json) VALUES ($1, $2, $3)",
            qualified_table(&self.schema_name, "scheduled_jobs")
        );
        let job_id = job.id.to_string();
        let run_at = i64_from_timestamp(job.run_at)?;
        let data_json = serialize_json(job)?;
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
        })
    }

    fn mark_scheduler_changed(&mut self) {
        self.notification.scheduler_changed = true;
    }
}

impl crate::sql::write_core::SqlWriteBackend for PostgresWriteTransaction {
    fn check_cancel(&self) -> Result<()> {
        PostgresWriteTransaction::check_cancel(self)
    }

    fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.provider
            .fault_injector
            .check_for_tenant(point, &self.tenant_id)
    }

    fn commit_transaction(&mut self) -> Result<()> {
        PostgresWriteTransaction::batch_execute(self, "COMMIT")
    }

    fn rollback_transaction(&mut self) {
        let _ = PostgresWriteTransaction::batch_execute(self, "ROLLBACK");
    }

    fn trigger_write_origin(&self) -> Option<TriggerWriteOrigin> {
        self.trigger_write_origin.clone()
    }

    fn push_commit_write(&mut self, write: WriteOp) {
        self.commit_writes.push(write);
    }

    fn last_commit_write_mut(&mut self) -> Option<&mut WriteOp> {
        self.commit_writes.last_mut()
    }

    fn take_commit_writes(&mut self) -> Vec<WriteOp> {
        std::mem::take(&mut self.commit_writes)
    }

    fn push_tenant_event(&mut self, event: TenantEventKind) {
        self.tenant_events.push(event);
    }

    fn prepend_tenant_event(&mut self, event: TenantEventKind) {
        self.tenant_events.insert(0, event);
    }

    fn tenant_events_is_empty(&self) -> bool {
        self.tenant_events.is_empty()
    }

    fn take_tenant_events(&mut self) -> Vec<TenantEventKind> {
        std::mem::take(&mut self.tenant_events)
    }

    fn take_prepared_record(&mut self) -> Option<TenantEventRecord> {
        self.prepared_record.take()
    }

    fn apply_durable_record(&mut self, record: &TenantEventRecord) -> Result<()> {
        PostgresWriteTransaction::apply_durable_record(self, record)
    }

    fn append_commit_entry(
        &mut self,
        writes: Vec<WriteOp>,
        events: Vec<TenantEventKind>,
    ) -> Result<CommitEntry> {
        PostgresWriteTransaction::append_commit_entry(self, writes, events)
    }

    fn append_prepared_record(&mut self, record: &TenantEventRecord) -> Result<CommitEntry> {
        PostgresWriteTransaction::append_prepared_record(self, record)
    }

    fn enqueue_notification(&mut self) -> Result<()> {
        PostgresWriteTransaction::enqueue_notification(self)
    }

    fn schema_cache_changed(&self) -> bool {
        self.schema_cache_changed
    }

    fn invalidate_schema_cache(&self) {
        invalidate_schema_cache_handle(&self.schema_cache);
    }

    fn load_document(&mut self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        PostgresWriteTransaction::load_document(self, table, id)
    }

    fn load_table_id(&mut self, table: &TableName) -> Result<Option<TableId>> {
        PostgresWriteTransaction::load_table_id(self, table)
    }

    fn insert_document(&mut self, document: &Document) -> Result<()> {
        PostgresWriteTransaction::insert_document(self, document)
    }

    fn update_document_row(&mut self, table_id: &TableId, current: &Document) -> Result<()> {
        let query = format!(
            "UPDATE {} SET data_json = $3, typed_fields_json = $4, creation_time = $5, update_time = $6 WHERE table_id = $1 AND id = $2",
            qualified_table(&self.schema_name, "documents")
        );
        let document_id = current.id.to_string();
        let data_json = serialize_document_fields(current)?;
        let typed_fields_json = serialize_document_typed_fields(current)?;
        let creation_time = i64_from_timestamp(current.creation_time)?;
        let update_time = i64_from_timestamp(current.update_time)?;
        let table_id = table_id.as_str().to_string();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(
                    query.as_str(),
                    &[
                        &table_id,
                        &document_id,
                        &data_json,
                        &typed_fields_json,
                        &creation_time,
                        &update_time,
                    ],
                )
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }

    fn delete_document_row(&mut self, table_id: &TableId, id: &DocumentId) -> Result<()> {
        let query = format!(
            "DELETE FROM {} WHERE table_id = $1 AND id = $2",
            qualified_table(&self.schema_name, "documents")
        );
        let document_id = id.to_string();
        let table_id = table_id.as_str().to_string();
        let client = self.session()?;
        self.block_on(async move {
            client
                .execute(query.as_str(), &[&table_id, &document_id])
                .await
                .map_err(map_postgres_error)?;
            Ok(())
        })
    }

    fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()> {
        PostgresWriteTransaction::upsert_resource_path_binding(self, binding)
    }

    fn remove_resource_path_binding(
        &mut self,
        locator: &nimbus_core::DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        PostgresWriteTransaction::remove_resource_path_binding(self, locator)
    }
}

impl SqlDurableJournalTransaction for PostgresWriteTransaction {
    fn applied_sequence(&mut self) -> Result<SequenceNumber> {
        PostgresWriteTransaction::applied_sequence(self)
    }

    fn load_durable_record(
        &mut self,
        sequence: SequenceNumber,
    ) -> Result<Option<TenantEventRecord>> {
        PostgresWriteTransaction::load_durable_record(self, sequence)
    }

    fn write_applied_sequence(&mut self, sequence: SequenceNumber) -> Result<()> {
        PostgresWriteTransaction::write_applied_sequence(self, sequence)
    }

    fn append_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()> {
        PostgresWriteTransaction::append_durable_records_batch(self, records)
    }

    fn apply_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()> {
        PostgresWriteTransaction::apply_durable_records_batch(self, records)
    }

    /// PostgreSQL pipelines the journal insert and the apply as one ordered pair
    /// on a shared connection, so pipeline progress is reported once both have
    /// completed.
    fn append_and_apply_fenced_durable_batch(
        &mut self,
        records: &[TenantEventRecord],
        on_pipeline_progress: &mut dyn FnMut(),
    ) -> Result<()> {
        PostgresWriteTransaction::append_and_apply_durable_records_batch(self, records)?;
        on_pipeline_progress();
        Ok(())
    }
}
