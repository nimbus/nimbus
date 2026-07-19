use super::document_versions::{
    prune_document_versions_before_remote, record_document_versions_for_events_remote,
};
use super::index_versions::{
    prune_index_versions_before_remote, record_index_versions_for_events_remote,
};
use super::*;

impl LibsqlReplicaTenantStore {
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
            transaction
                .store
                .block_on(super::backend::apply_durable_record_in_remote_conn(
                    transaction.session()?,
                    &record,
                ))?;
            apply_schedule_ops_in_libsql_transaction(transaction, &schedule_ops)?;
            transaction.set_prepared_record(record);
            Ok(true)
        })?;
        Ok(committed.value.then_some(committed.commit).flatten())
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

    pub fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        let table_schema = table_schema.clone();
        self.execute_write(move |transaction| transaction.replace_table_schema(&table_schema))?;
        Ok(())
    }

    pub fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        let table = table.clone();
        self.execute_write(move |transaction| transaction.delete_table_schema(&table))?;
        Ok(())
    }

    pub fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records.to_vec();
        self.block_on(self.append_remote_durable_records_batch(records.as_slice()))?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records.to_vec();
        let applied_head =
            self.block_on(self.apply_remote_durable_records_batch(records.as_slice()))?;
        self.note_required_cache_sequence_with_cause(
            applied_head,
            LibsqlReplicaRefreshCause::DurableJournalReplay,
        );
        Ok(())
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

    pub fn insert(&self, document: &Document) -> Result<CommitEntry> {
        self.insert_once(document, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated insert should commit".to_string()))
    }

    pub fn insert_with_indexes(
        &self,
        document: &Document,
        _indexes: &[nimbus_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        self.insert(document)
    }

    pub fn insert_once(
        &self,
        document: &Document,
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        let document = document.clone();
        let execution_id = execution_id.map(ToOwned::to_owned);
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
        _indexes: &[nimbus_core::IndexDefinition],
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
        let execution_id = assignment.execution_id.map(ToOwned::to_owned);
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
        let table = table.clone();
        let id = id.clone();
        let patch = patch.clone();
        let execution_id = execution_id.map(ToOwned::to_owned);
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
        patch: &serde_json::Map<String, serde_json::Value>,
        _indexes: &[nimbus_core::IndexDefinition],
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
        _indexes: &[nimbus_core::IndexDefinition],
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
        patch: &serde_json::Map<String, serde_json::Value>,
        assignment: crate::DirectWriteAssignment<'_>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        let table = table.clone();
        let id = id.clone();
        let patch = patch.clone();
        let execution_id = assignment.execution_id.map(ToOwned::to_owned);
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
        let execution_id = execution_id.map(ToOwned::to_owned);
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
        _indexes: &[nimbus_core::IndexDefinition],
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
        _indexes: &[nimbus_core::IndexDefinition],
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
        let execution_id = assignment.execution_id.map(ToOwned::to_owned);
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
            apply_schedule_ops_in_libsql_transaction(transaction, &schedule_ops)?;
            Ok(())
        })?;
        Ok(committed.commit)
    }

    pub fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut LibsqlReplicaWriteTransaction) -> Result<T> + Send + 'static,
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
        F: FnOnce(&mut LibsqlReplicaWriteTransaction) -> Result<T> + Send + 'static,
    {
        let store = self.clone();
        let runtime_handle = self.provider.runtime_handle.clone();
        bridge_tokio_runtime(
            &runtime_handle,
            "libsql replica write bridge thread panicked",
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
        F: FnOnce(&mut LibsqlReplicaWriteTransaction) -> Result<T> + Send + 'static,
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
    ) -> Result<LibsqlReplicaWriteTransaction>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        LibsqlReplicaWriteTransaction::begin(self.clone(), check_cancel)
    }
}

impl LibsqlReplicaWriteTransaction {
    fn begin<Check>(store: LibsqlReplicaTenantStore, check_cancel: Check) -> Result<Self>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let conn = store.remote_connection()?;
        let tx = store.block_on(async move {
            conn.transaction_with_behavior(TransactionBehavior::Immediate)
                .await
                .map_err(map_libsql_error)
        })?;
        Ok(Self {
            store,
            tx: Some(tx),
            commit_writes: Vec::new(),
            tenant_events: Vec::new(),
            prepared_record: None,
            trigger_write_origin: None,
            commit_timestamp: None,
            check_cancel: Box::new(check_cancel),
            refresh_cache_after_commit: false,
        })
    }

    pub fn prune_retained_versions(
        &mut self,
        document_prune_before: SequenceNumber,
        index_prune_before: SequenceNumber,
    ) -> Result<(u64, u64)> {
        self.check_cancel()?;
        self.store.block_on(async {
            let document_versions_pruned =
                prune_document_versions_before_remote(self.session()?, document_prune_before)
                    .await?;
            let index_versions_pruned =
                prune_index_versions_before_remote(self.session()?, index_prune_before).await?;
            Ok((document_versions_pruned, index_versions_pruned))
        })
    }

    pub fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        let mut table_schema = table_schema.clone();
        let mut recorded_event: Option<(TableId, Option<TableSchema>, TableSchema)> = None;
        self.store.block_on(async {
            let previous =
                load_remote_table_schema_from_session(self.session()?, &table_schema.table).await?;
            table_schema.reconcile_index_metadata(previous.as_ref());
            let schema_json = serialize_json(&table_schema)?;
            let table_id =
                resolve_or_create_remote_table_id(self.session()?, &table_schema.table).await?;
            self.session()?
                .execute(
                    "INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)
                     ON CONFLICT(table_name) DO UPDATE SET schema_json = excluded.schema_json",
                    libsql::params![table_schema.table.as_str(), schema_json],
                )
                .await
                .map_err(map_libsql_error)?;
            recorded_event = Some((table_id, previous, table_schema.clone()));
            Ok(())
        })?;
        if let Some((table_id, previous, table_schema)) = recorded_event {
            record_libsql_schema_set_events(self, table_id, previous, &table_schema);
        }
        self.refresh_cache_after_commit = true;
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        let mut previous = None;
        let mut table_id = None;
        self.store.block_on(async {
            previous = load_remote_table_schema_from_session(self.session()?, table).await?;
            table_id = load_remote_table_id_from_session(self.session()?, table).await?;
            self.session()?
                .execute(
                    "DELETE FROM schemas WHERE table_name = ?1",
                    libsql::params![table.as_str()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.record_tenant_event(TenantEventKind::SchemaChange {
            change: Box::new(SchemaChangeEvent::DeleteTable {
                table: table.clone(),
                table_id,
                previous,
            }),
        });
        self.refresh_cache_after_commit = true;
        Ok(())
    }

    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        let Some(execution_id) = execution_id else {
            return Ok(true);
        };
        let inserted = self.store.block_on(async {
            let changed = self
                .session()?
                .execute(
                    "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
                    libsql::params![execution_id],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(changed == 1)
        })?;
        if inserted {
            self.record_tenant_event(TenantEventKind::ScheduledExecution {
                execution_id: execution_id.to_string(),
            });
        }
        Ok(inserted)
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        let data_json = serialize_document_fields(document)?;
        let typed_fields_json = serialize_document_typed_fields(document)?;
        let table_id = self.store.block_on(async {
            resolve_or_create_remote_table_id(self.session()?, &document.table).await
        })?;
        let write_table_id = table_id.clone();
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO documents (table_id, id, data_json, typed_fields_json, creation_time, update_time)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    libsql::params![
                        table_id.as_str(),
                        document.id.to_string(),
                        data_json,
                        typed_fields_json,
                        i64_from_u64(document.creation_time.0)?,
                        i64_from_u64(document.update_time.0)?
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
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
        patch: &serde_json::Map<String, serde_json::Value>,
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
            .unwrap_or_else(|| self.store.provider.clock.now());
        validate(&existing_document, &document)?;
        let data_json = serialize_document_fields(&document)?;
        let typed_fields_json = serialize_document_typed_fields(&document)?;
        let table_id = self.store.block_on(async {
            load_remote_table_id_from_session(self.session()?, table)
                .await?
                .ok_or(Error::DocumentNotFound(id.clone()))
        })?;
        let write_table_id = table_id.clone();
        self.store.block_on(async {
            self.session()?
                .execute(
                    "UPDATE documents
                     SET data_json = ?3, typed_fields_json = ?4, creation_time = ?5, update_time = ?6
                     WHERE table_id = ?1 AND id = ?2",
                    libsql::params![
                        table_id.as_str(),
                        id.to_string(),
                        data_json,
                        typed_fields_json,
                        i64_from_u64(document.creation_time.0)?,
                        i64_from_u64(document.update_time.0)?
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
            table_id: write_table_id,
            op_type: WriteOpType::Update,
            doc_id: id.clone(),
            resource_path_binding: self.store.resource_path_binding(
                &nimbus_core::DocumentLocator::new(table.clone(), id.clone()),
            )?,
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
        let table_id = self.store.block_on(async {
            load_remote_table_id_from_session(self.session()?, table)
                .await?
                .ok_or(Error::DocumentNotFound(id.clone()))
        })?;
        let write_table_id = table_id.clone();
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM documents WHERE table_id = ?1 AND id = ?2",
                    libsql::params![table_id.as_str(), id.to_string()],
                )
                .await
                .map_err(map_libsql_error)?;
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
        let data_json = serialize_json(job)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO scheduled_jobs (id, run_at, data_json) VALUES (?1, ?2, ?3)",
                    libsql::params![
                        job.id.to_string(),
                        scheduled_run_at_key(job.run_at),
                        data_json
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        let due = if max_jobs == 0 {
            Vec::new()
        } else {
            let run_at_upper = scheduled_run_at_key(now);
            let max_jobs = i64::try_from(max_jobs).unwrap_or(i64::MAX);
            self.store.block_on(async {
                let mut rows = self
                    .session()?
                    .query(
                        "SELECT data_json FROM scheduled_jobs
                         WHERE run_at <= ?1
                         ORDER BY run_at, id
                         LIMIT ?2",
                        libsql::params![run_at_upper, max_jobs],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                let mut due = Vec::new();
                while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
                    due.push(deserialize_json::<ScheduledJob>(
                        row.get::<String>(0).map_err(map_libsql_error)?.as_str(),
                    )?);
                }
                Ok(due)
            })?
        };
        for job in &due {
            self.check_cancel()?;
            let data_json = serialize_json(job)?;
            self.store.block_on(async {
                self.session()?
                    .execute(
                        "DELETE FROM scheduled_jobs WHERE id = ?1",
                        libsql::params![job.id.to_string()],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                self.session()?
                    .execute(
                        "INSERT INTO running_scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                        libsql::params![job.id.to_string(), data_json],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                Ok(())
            })?;
        }
        Ok(due)
    }

    pub fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        self.check_cancel()?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM running_scheduled_jobs WHERE id = ?1",
                    libsql::params![job_id.to_string()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        self.check_cancel()?;
        self.store.block_on(async {
            let affected = self
                .session()?
                .execute(
                    "DELETE FROM scheduled_jobs WHERE id = ?1",
                    libsql::params![job_id.to_string()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(affected == 1)
        })
    }

    pub fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.check_cancel()?;
        let data_json = serialize_json(result)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO scheduled_job_results (job_id, data_json) VALUES (?1, ?2)
                     ON CONFLICT(job_id) DO UPDATE SET data_json = excluded.data_json",
                    libsql::params![result.id.to_string(), data_json],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.check_cancel()?;
        let data_json = serialize_json(cron)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO cron_jobs (name, data_json) VALUES (?1, ?2)
                     ON CONFLICT(name) DO UPDATE SET data_json = excluded.data_json",
                    libsql::params![cron.name.clone(), data_json],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.check_cancel()?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM cron_jobs WHERE name = ?1",
                    libsql::params![name],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        self.check_cancel()?;
        let running_jobs = self.store.block_on(
            self.store
                .load_remote_scheduled_jobs("running_scheduled_jobs"),
        )?;
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
            let data_json = serialize_json(&job)?;
            self.store.block_on(async {
                self.session()?
                    .execute(
                        "INSERT INTO scheduled_jobs (id, run_at, data_json) VALUES (?1, ?2, ?3)",
                        libsql::params![
                            job.id.to_string(),
                            scheduled_run_at_key(job.run_at),
                            data_json
                        ],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                self.session()?
                    .execute(
                        "DELETE FROM running_scheduled_jobs WHERE id = ?1",
                        libsql::params![job.id.to_string()],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                Ok(())
            })?;
        }
        Ok(())
    }

    pub fn apply_resolved_write(&mut self, write: &ResolvedWrite) -> Result<()> {
        match write {
            ResolvedWrite::Insert {
                document,
                resource_path_binding,
                ..
            } => {
                self.check_cancel()?;
                if self.load_document(&document.table, &document.id)?.is_some() {
                    return Err(Error::conflict(format!(
                        "document {} changed before transaction commit",
                        document.id
                    )));
                }
                self.insert_document(document)?;
                if let Some(resource_path_binding) = resource_path_binding.as_ref() {
                    if let Some(write) = self.commit_writes.last_mut() {
                        write.resource_path_binding = Some(resource_path_binding.clone());
                    }
                    self.upsert_resource_path_binding(resource_path_binding)?;
                }
                Ok(())
            }
            ResolvedWrite::Update {
                previous,
                current,
                resource_path_binding,
                ..
            } => {
                self.check_cancel()?;
                let existing =
                    self.load_document(&current.table, &current.id)?
                        .ok_or(Error::conflict(format!(
                            "document {} changed before transaction commit",
                            current.id
                        )))?;
                if existing != *previous {
                    return Err(Error::conflict(format!(
                        "document {} changed before transaction commit",
                        current.id
                    )));
                }
                let data_json = serialize_document_fields(current)?;
                let typed_fields_json = serialize_document_typed_fields(current)?;
                let table_id = self.store.block_on(async {
                    load_remote_table_id_from_session(self.session()?, &current.table)
                        .await?
                        .ok_or(Error::conflict(format!(
                            "document {} changed before transaction commit",
                            current.id
                        )))
                })?;
                let write_table_id = table_id.clone();
                self.store.block_on(async {
                    self.session()?
                        .execute(
                            "UPDATE documents
                             SET data_json = ?3, typed_fields_json = ?4, creation_time = ?5, update_time = ?6
                             WHERE table_id = ?1 AND id = ?2",
                            libsql::params![
                                table_id.as_str(),
                                current.id.to_string(),
                                data_json,
                                typed_fields_json,
                                i64_from_u64(current.creation_time.0)?,
                                i64_from_u64(current.update_time.0)?
                            ],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    Ok(())
                })?;
                self.record_commit_write(WriteOp {
                    table: current.table.clone(),
                    table_id: write_table_id,
                    op_type: WriteOpType::Update,
                    doc_id: current.id.clone(),
                    resource_path_binding: resource_path_binding.clone(),
                    trigger_write_origin: None,
                    previous: Some(previous.clone()),
                    current: Some(current.clone()),
                });
                if let Some(resource_path_binding) = resource_path_binding.as_ref() {
                    self.upsert_resource_path_binding(resource_path_binding)?;
                }
                Ok(())
            }
            ResolvedWrite::Delete { previous, .. } => {
                self.check_cancel()?;
                let existing =
                    self.load_document(&previous.table, &previous.id)?
                        .ok_or(Error::conflict(format!(
                            "document {} changed before transaction commit",
                            previous.id
                        )))?;
                if existing != *previous {
                    return Err(Error::conflict(format!(
                        "document {} changed before transaction commit",
                        previous.id
                    )));
                }
                let table_id = self.store.block_on(async {
                    load_remote_table_id_from_session(self.session()?, &previous.table)
                        .await?
                        .ok_or(Error::conflict(format!(
                            "document {} changed before transaction commit",
                            previous.id
                        )))
                })?;
                let write_table_id = table_id.clone();
                self.store.block_on(async {
                    self.session()?
                        .execute(
                            "DELETE FROM documents WHERE table_id = ?1 AND id = ?2",
                            libsql::params![table_id.as_str(), previous.id.to_string()],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    Ok(())
                })?;
                let resource_path_binding = self.remove_resource_path_binding(
                    &nimbus_core::DocumentLocator::new(previous.table.clone(), previous.id.clone()),
                )?;
                self.record_commit_write(WriteOp {
                    table: previous.table.clone(),
                    table_id: write_table_id,
                    op_type: WriteOpType::Delete,
                    doc_id: previous.id.clone(),
                    resource_path_binding,
                    trigger_write_origin: None,
                    previous: Some(previous.clone()),
                    current: None,
                });
                Ok(())
            }
        }
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

    pub fn commit(mut self) -> Result<Option<CommitEntry>> {
        self.check_cancel()?;
        let writes = std::mem::take(&mut self.commit_writes);
        if !writes.is_empty() {
            self.tenant_events.insert(
                0,
                TenantEventKind::DocumentWrite {
                    writes: writes.clone(),
                },
            );
        }
        let commit = if let Some(record) = self.prepared_record.take() {
            crate::store::validate_prepared_record_shape(&record, &writes, &self.tenant_events)?;
            Some(self.append_prepared_record(&record)?)
        } else if self.tenant_events.is_empty() {
            None
        } else {
            let events = std::mem::take(&mut self.tenant_events);
            Some(self.append_commit_entry(writes, events)?)
        };
        let tx = self.tx.take().ok_or_else(|| {
            Error::Internal("libsql replica write transaction already closed".to_string())
        })?;
        self.store.block_on(async move {
            tx.commit().await.map_err(map_libsql_error)?;
            Ok(())
        })?;
        if let Some(commit) = &commit {
            if self.refresh_cache_after_commit {
                self.store.refresh_needed.store(true, Ordering::Release);
                self.store.note_required_cache_sequence_with_cause(
                    commit.sequence,
                    LibsqlReplicaRefreshCause::SchemaWrite,
                );
            } else {
                self.store.note_required_cache_sequence_with_cause(
                    commit.sequence,
                    LibsqlReplicaRefreshCause::CommitBarrier,
                );
            }
        } else if self.refresh_cache_after_commit {
            self.store.refresh_needed.store(true, Ordering::Release);
            self.store
                .freshness_metrics
                .note_refresh_request(LibsqlReplicaRefreshCause::SchemaWrite);
            self.store.schedule_background_refresh();
        }
        Ok(commit)
    }

    pub fn rollback(mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = self.store.block_on(async move {
                tx.rollback().await.map_err(map_libsql_error)?;
                Ok(())
            });
        }
    }

    pub(super) fn session(&self) -> Result<&Transaction> {
        self.tx.as_ref().ok_or_else(|| {
            Error::Internal("libsql replica write transaction already closed".to_string())
        })
    }

    pub(super) fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    fn set_trigger_write_origin(&mut self, trigger_write_origin: Option<TriggerWriteOrigin>) {
        self.trigger_write_origin = trigger_write_origin;
    }

    fn set_commit_timestamp(&mut self, commit_timestamp: Option<Timestamp>) {
        self.commit_timestamp = commit_timestamp;
    }

    fn record_commit_write(&mut self, mut write: WriteOp) {
        if write.trigger_write_origin.is_none() {
            write.trigger_write_origin = self.trigger_write_origin.clone();
        }
        self.commit_writes.push(write);
    }

    pub(super) fn record_tenant_event(&mut self, event: TenantEventKind) {
        self.tenant_events.push(event);
    }

    fn load_document(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.store.block_on(load_remote_document_from_session(
            self.session()?,
            table.clone(),
            id.clone(),
        ))
    }

    fn append_commit_entry(
        &self,
        writes: Vec<WriteOp>,
        events: Vec<TenantEventKind>,
    ) -> Result<CommitEntry> {
        let sequence = SequenceNumber(
            self.store
                .block_on(load_next_sequence_from_session(self.session()?))?,
        );
        let applied_head = self.store.block_on(load_remote_metadata_u64(
            self.session()?,
            APPLIED_SEQUENCE_KEY,
        ))?;
        crate::commit_log::ensure_applied_prefix_precedes(
            applied_head.map(SequenceNumber).unwrap_or(SequenceNumber(0)),
            sequence,
        )?;
        let timestamp = self
            .commit_timestamp
            .unwrap_or_else(|| self.store.provider.clock.now());
        let record = TenantEventRecord::from_events(sequence, timestamp, events)?;
        let entry = CommitEntry {
            sequence,
            timestamp,
            writes,
        };
        let payload = serialize_tenant_event_record(&record)?;
        let record_sequence = record.sequence;
        let record_timestamp = record.timestamp;
        let record_events = record.events.clone();
        self.store.block_on(async {
            record_document_versions_for_events_remote(
                self.session()?,
                record_sequence,
                record_timestamp,
                &record_events,
            )
            .await?;
            record_index_versions_for_events_remote(
                self.session()?,
                record_sequence,
                &record_events,
            )
            .await?;
            self.session()?
                .execute(
                    "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
                    libsql::params![i64_from_u64(sequence.0)?, payload],
                )
                .await
                .map_err(map_libsql_error)?;
            put_remote_metadata_u64(
                self.session()?,
                NEXT_SEQUENCE_KEY,
                sequence.0.saturating_add(1),
            )
            .await?;
            put_remote_metadata_u64(self.session()?, APPLIED_SEQUENCE_KEY, sequence.0).await?;
            Ok(())
        })?;
        Ok(entry)
    }

    fn append_prepared_record(&self, record: &TenantEventRecord) -> Result<CommitEntry> {
        record.validate_integrity()?;
        let expected = self
            .store
            .block_on(load_next_sequence_from_session(self.session()?))?;
        if record.sequence.0 != expected {
            return Err(Error::conflict(format!(
                "prepared commit expected storage sequence {expected}, got {}",
                record.sequence.0
            )));
        }
        let applied_head = self.store.block_on(load_remote_metadata_u64(
            self.session()?,
            APPLIED_SEQUENCE_KEY,
        ))?;
        crate::commit_log::ensure_applied_prefix_precedes(
            applied_head.map(SequenceNumber).unwrap_or(SequenceNumber(0)),
            record.sequence,
        )?;
        let payload = serialize_tenant_event_record(record)?;
        let record_sequence = record.sequence;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)",
                    libsql::params![i64_from_u64(record_sequence.0)?, payload],
                )
                .await
                .map_err(map_libsql_error)?;
            put_remote_metadata_u64(
                self.session()?,
                NEXT_SEQUENCE_KEY,
                record_sequence.0.saturating_add(1),
            )
            .await?;
            put_remote_metadata_u64(self.session()?, APPLIED_SEQUENCE_KEY, record_sequence.0)
                .await?;
            Ok(())
        })?;
        Ok(record.as_commit_entry())
    }
}

fn record_libsql_schema_set_events(
    transaction: &mut LibsqlReplicaWriteTransaction,
    table_id: TableId,
    previous: Option<TableSchema>,
    table_schema: &TableSchema,
) {
    transaction.record_tenant_event(TenantEventKind::SchemaChange {
        change: Box::new(SchemaChangeEvent::SetTable {
            table: table_schema.table.clone(),
            table_id: table_id.clone(),
            previous,
            current: table_schema.clone(),
        }),
    });
    for index in &table_schema.indexes {
        transaction.record_tenant_event(TenantEventKind::IndexLifecycle {
            index: IndexLifecycleEvent {
                table: table_schema.table.clone(),
                table_id: table_id.clone(),
                index_id: index.id.clone(),
                state: index.state,
                definition: index.clone(),
            },
        });
    }
}

async fn load_remote_table_schema_from_session(
    conn: &Connection,
    table: &TableName,
) -> Result<Option<TableSchema>> {
    let mut rows = conn
        .query(
            "SELECT schema_json FROM schemas WHERE table_name = ?1",
            libsql::params![table.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    deserialize_json(row.get::<String>(0).map_err(map_libsql_error)?.as_str()).map(Some)
}
