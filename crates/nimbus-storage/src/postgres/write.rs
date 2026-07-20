use super::document_versions::{
    prune_document_versions_before_in_session, record_document_versions_for_events_in_session,
};
use super::index_versions::{
    prune_index_versions_before_in_session, record_index_versions_for_events_in_session,
};
use super::write_schema_events::{
    durable_record_changes_schema_cache, record_postgres_schema_set_events,
};
use super::*;
use crate::{CommitterLeaseError, CommitterLeaseResult};

const FENCED_COMMITTER_LEASE_MARKER: &str = "fenced committer lease during durable apply";

impl PostgresTenantStore {
    pub fn apply_prepared_write_batch(
        &self,
        record: &TenantEventRecord,
        schedule_ops: &[ResolvedScheduleOp],
        scheduled_execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        if record.writes.is_empty() {
            return Err(Error::Internal(
                "prepared write batch must contain at least one document write".to_string(),
            ));
        }
        let record = record.clone();
        let schedule_ops = schedule_ops.to_vec();
        let scheduled_execution_id = scheduled_execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(scheduled_execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.apply_durable_record(&record)?;
            apply_schedule_ops_in_transaction(transaction, &schedule_ops)?;
            transaction.set_prepared_record(record);
            Ok(true)
        })?;
        Ok(committed.value.then_some(committed.commit).flatten())
    }

    pub fn fenced_apply_prepared_write_batch(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        record: &TenantEventRecord,
        schedule_ops: &[ResolvedScheduleOp],
        scheduled_execution_id: Option<&str>,
    ) -> CommitterLeaseResult<Option<CommitEntry>> {
        if record.writes.is_empty() {
            return Err(Error::Internal(
                "prepared write batch must contain at least one document write".to_string(),
            )
            .into());
        }
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let record = record.clone();
        let schedule_ops = schedule_ops.to_vec();
        let scheduled_execution_id = scheduled_execution_id.map(str::to_string);
        let result = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(scheduled_execution_id.as_deref())? {
                return Ok(false);
            }
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                record.sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.apply_durable_record(&record)?;
            apply_schedule_ops_in_transaction(transaction, &schedule_ops)?;
            transaction.set_prepared_record(record);
            Ok(true)
        });
        match result {
            Ok(committed) => Ok(committed.value.then_some(committed.commit).flatten()),
            Err(Error::PreconditionFailed(message)) if message == FENCED_COMMITTER_LEASE_MARKER => {
                Err(CommitterLeaseError::Fenced {
                    owner_id: fenced_owner_id,
                    epoch,
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn retention_gc_watermarks(
        &self,
        config: crate::RetentionGcConfig,
    ) -> Result<crate::RetentionGcWatermarks> {
        Ok(self
            .retention_floor
            .gc_watermarks(self.journal_progress()?.applied_head, config))
    }

    pub fn compact_retained_versions(
        &self,
        config: crate::RetentionGcConfig,
    ) -> Result<crate::RetentionGcSummary> {
        let watermarks = self.retention_gc_watermarks(config)?;
        let document_prune_before = watermarks.document_versions.safe_prune_before;
        let index_prune_before = watermarks.index_versions.safe_prune_before;
        let committed = self.execute_write(move |transaction| {
            transaction.prune_retained_versions(document_prune_before, index_prune_before)
        })?;
        debug_assert!(committed.commit.is_none());
        Ok(crate::RetentionGcSummary {
            watermarks,
            document_versions_pruned: committed.value.0,
            index_versions_pruned: committed.value.1,
        })
    }

    pub fn export_point_in_time_restore_archive(
        &self,
        target: PointInTimeRestoreTarget,
        retention_config: crate::RetentionGcConfig,
    ) -> Result<PointInTimeRestoreArchive> {
        let records = self.read_durable_journal_from(SequenceNumber(1))?;
        let progress = self.journal_progress()?;
        let watermarks = self.retention_gc_watermarks(retention_config)?;
        crate::store::build_point_in_time_restore_archive(
            target,
            records,
            progress.durable_head,
            watermarks.document_versions.safe_prune_before,
        )
    }

    pub fn import_point_in_time_restore_archive(
        &self,
        archive: &PointInTimeRestoreArchive,
    ) -> Result<JournalProgress> {
        crate::store::validate_point_in_time_archive_for_journal_replay_import(archive)?;
        let current = self.export_materialized_journal_snapshot()?;
        crate::store::validate_materialized_journal_replay_base_is_empty(&current)?;
        self.append_durable_records_batch(&archive.journal_tail)?;
        let progress = self.recover_durable_journal()?;
        let restored_fingerprint = self
            .export_materialized_journal_snapshot()?
            .canonical_fingerprint()?;
        if restored_fingerprint != archive.target_fingerprint {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Corruption,
                format!(
                    "point-in-time restore fingerprint mismatch: restored {} expected {}",
                    restored_fingerprint, archive.target_fingerprint
                ),
            ));
        }
        Ok(progress)
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

    pub fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        let table_schema = table_schema.clone();
        self.execute_write(move |transaction| transaction.replace_table_schema(&table_schema))?;
        Ok(())
    }

    pub fn fenced_replace_table_schema(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        table_schema: &TableSchema,
    ) -> CommitterLeaseResult<()> {
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let table_schema = table_schema.clone();
        let durable_sequence = SequenceNumber(expected_previous.0.saturating_add(1));
        let result = self.execute_write(move |transaction| {
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.replace_table_schema(&table_schema)
        });
        map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
    }

    pub fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        let table = table.clone();
        self.execute_write(move |transaction| transaction.delete_table_schema(&table))?;
        Ok(())
    }

    pub fn fenced_delete_table_schema(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        table: &TableName,
    ) -> CommitterLeaseResult<()> {
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let table = table.clone();
        let durable_sequence = SequenceNumber(expected_previous.0.saturating_add(1));
        let result = self.execute_write(move |transaction| {
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            transaction.delete_table_schema(&table)
        });
        map_fenced_write_result(result.map(|_| ()), fenced_owner_id, epoch)
    }

    pub fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        let records = records.to_vec();
        self.execute_write(move |transaction| transaction.append_durable_records_batch(&records))?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        let records = records.to_vec();
        self.execute_write(move |transaction| transaction.apply_durable_records_batch(&records))?;
        Ok(())
    }

    pub fn fenced_append_and_apply_durable_records_batch(
        &self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        records: &[TenantEventRecord],
    ) -> CommitterLeaseResult<()> {
        if records.is_empty() {
            return Err(Error::InvalidInput(
                "fenced durable apply requires at least one record".to_string(),
            )
            .into());
        }
        let owner_id = owner_id.to_string();
        let fenced_owner_id = owner_id.clone();
        let records = records.to_vec();
        let result = self.execute_write(move |transaction| {
            let durable_sequence = records
                .last()
                .expect("non-empty fenced durable apply batch")
                .sequence;
            if transaction.advance_fenced_committer_lease(
                &owner_id,
                epoch,
                expected_previous,
                durable_sequence,
            )? != 1
            {
                return Err(Error::PreconditionFailed(
                    FENCED_COMMITTER_LEASE_MARKER.to_string(),
                ));
            }
            crate::commit_log::ensure_applied_prefix_precedes(
                transaction.applied_sequence()?,
                records[0].sequence,
            )?;
            transaction.append_durable_records_batch(&records)?;
            transaction.apply_durable_records_batch(&records)
        });
        match result {
            Ok(_) => Ok(()),
            Err(Error::PreconditionFailed(message)) if message == FENCED_COMMITTER_LEASE_MARKER => {
                Err(CommitterLeaseError::Fenced {
                    owner_id: fenced_owner_id,
                    epoch,
                })
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn insert_scheduled_job(&self, job: &ScheduledJob) -> Result<()> {
        let job = job.clone();
        self.execute_write(move |transaction| transaction.insert_scheduled_job(&job))?;
        Ok(())
    }

    pub fn claim_due_jobs(&self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(move |transaction| transaction.claim_due_jobs(now, max_jobs))?
            .value)
    }

    pub fn complete_scheduled_job(&self, job_id: &DocumentId) -> Result<()> {
        let job_id = job_id.clone();
        self.execute_write(move |transaction| transaction.complete_scheduled_job(&job_id))?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&self, job_id: &DocumentId) -> Result<bool> {
        let job_id = job_id.clone();
        Ok(self
            .execute_write(move |transaction| transaction.cancel_scheduled_job(&job_id))?
            .value)
    }

    pub fn record_scheduled_job_result(&self, result: &ScheduledJobResult) -> Result<()> {
        let result = result.clone();
        self.execute_write(move |transaction| transaction.record_scheduled_job_result(&result))?;
        Ok(())
    }

    pub fn save_cron_job(&self, cron: &CronJob) -> Result<()> {
        let cron = cron.clone();
        self.execute_write(move |transaction| transaction.save_cron_job(&cron))?;
        Ok(())
    }

    pub fn delete_cron_job(&self, name: &str) -> Result<()> {
        let name = name.to_string();
        self.execute_write(move |transaction| transaction.delete_cron_job(name.as_str()))?;
        Ok(())
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
        self.apply_execution_unit_batch_with_origin(writes, schedule_ops, None, None)
    }

    pub fn apply_execution_unit_batch_with_origin(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
        trigger_write_origin: Option<&TriggerWriteOrigin>,
        commit_timestamp: Option<Timestamp>,
    ) -> Result<Option<CommitEntry>> {
        if writes.is_empty() && schedule_ops.is_empty() {
            return Err(Error::Internal(
                "execution-unit batch must contain at least one change".to_string(),
            ));
        }

        let writes = writes.to_vec();
        let schedule_ops = schedule_ops.to_vec();
        let trigger_write_origin = trigger_write_origin.cloned();
        let committed = self.execute_write(move |transaction| {
            transaction.set_trigger_write_origin(trigger_write_origin.clone());
            transaction.set_commit_timestamp(commit_timestamp);
            for write in &writes {
                transaction.apply_resolved_write(write)?;
            }
            apply_schedule_ops_in_transaction(transaction, &schedule_ops)?;
            Ok(())
        })?;
        Ok(committed.commit)
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
        let document = document.clone();
        let execution_id = execution_id.map(str::to_string);
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
        document: &Document,
        _indexes: &[IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        self.insert_once(document, execution_id)
    }

    pub fn insert_with_indexes_once_at(
        &self,
        document: &Document,
        assignment: crate::DirectWriteAssignment<'_>,
    ) -> Result<Option<CommitEntry>> {
        let document = document.clone();
        let execution_id = assignment.execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            transaction.set_commit_timestamp(Some(assignment.commit_timestamp));
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

    pub fn update_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
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
        patch: &serde_json::Map<String, Value>,
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = id.clone();
        let patch = patch.clone();
        let execution_id = execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.update_document_validated(&table, &id, &patch, validate)?;
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
        patch: &serde_json::Map<String, Value>,
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
        patch: &serde_json::Map<String, Value>,
        _indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        self.update_validated_once(table, id, patch, execution_id, validate)
    }

    pub fn update_with_indexes_validated_once_at<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        assignment: crate::DirectWriteAssignment<'_>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = id.clone();
        let patch = patch.clone();
        let execution_id = assignment.execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            transaction.set_commit_timestamp(Some(assignment.commit_timestamp));
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(false);
            }
            transaction.update_document_validated(&table, &id, &patch, validate)?;
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
        let table = table.clone();
        let id = id.clone();
        let execution_id = execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(None);
            }
            let removed_document = transaction.delete_document_validated(&table, &id, validate)?;
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

    pub fn delete_with_indexes_validated_once_at<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        assignment: crate::DirectWriteAssignment<'_>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = id.clone();
        let execution_id = assignment.execution_id.map(str::to_string);
        let committed = self.execute_write(move |transaction| {
            transaction.set_commit_timestamp(Some(assignment.commit_timestamp));
            if !transaction.begin_scheduled_execution(execution_id.as_deref())? {
                return Ok(None);
            }
            let removed_document = transaction.delete_document_validated(&table, &id, validate)?;
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
}

fn map_fenced_write_result<T>(
    result: Result<T>,
    owner_id: String,
    epoch: u64,
) -> CommitterLeaseResult<T> {
    match result {
        Ok(value) => Ok(value),
        Err(Error::PreconditionFailed(message)) if message == FENCED_COMMITTER_LEASE_MARKER => {
            Err(CommitterLeaseError::Fenced { owner_id, epoch })
        }
        Err(error) => Err(error.into()),
    }
}

impl PostgresWriteTransaction {
    fn advance_fenced_committer_lease(
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
        record_postgres_schema_set_events(self, table_id, previous, &table_schema);
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
        self.check_cancel()?;
        if max_jobs == 0 {
            return Ok(Vec::new());
        }
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
        let due = self.block_on(async move {
            let rows = client
                .query(query.as_str(), &[&run_at, &max_jobs])
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
            // A recovered running job was already DUE when it was claimed
            // (claim only takes run_at <= now), so keep its original due
            // time instead of re-stamping the recovery instant: stamping
            // `now` artificially delays the job and — under wall-clock
            // regression (e.g. NTP slew) between recovery and the next
            // tick — can push it past that tick's `now`, silently
            // deferring recovery (flaked scheduler_recovery_campaign on
            // CI). min() keeps any older due time intact and never moves
            // a job into the future.
            job.run_at = job.run_at.min(now);
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

    pub fn append_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()> {
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
            let payload = serialize_tenant_event_record(record)?;
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
        let client = self.session()?;
        self.block_on(async move {
            resolve_or_create_table_id_in_session(client, &schema_name, &table).await
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
        if result.is_ok() && changes_schema_cache {
            self.notification.schema_changed = true;
            self.schema_cache_changed = true;
        }
        result
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

impl crate::sql::write_core::SqlWriteBackend for PostgresWriteTransaction {
    fn check_cancel(&self) -> Result<()> {
        PostgresWriteTransaction::check_cancel(self)
    }

    fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.provider.fault_injector.check(point)
    }

    fn batch_execute(&mut self, sql: &str) -> Result<()> {
        PostgresWriteTransaction::batch_execute(self, sql)
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

    fn applied_sequence(&mut self) -> Result<SequenceNumber> {
        PostgresWriteTransaction::applied_sequence(self)
    }

    fn load_durable_record(
        &mut self,
        sequence: SequenceNumber,
    ) -> Result<Option<TenantEventRecord>> {
        PostgresWriteTransaction::load_durable_record(self, sequence)
    }

    fn apply_durable_record(&mut self, record: &TenantEventRecord) -> Result<()> {
        PostgresWriteTransaction::apply_durable_record(self, record)
    }

    fn write_applied_sequence(&mut self, sequence: SequenceNumber) -> Result<()> {
        PostgresWriteTransaction::write_applied_sequence(self, sequence)
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
