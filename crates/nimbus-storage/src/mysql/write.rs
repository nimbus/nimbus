use super::document_versions::{
    prune_document_versions_before_in_session, record_document_versions_for_events_in_session,
};
use super::index_versions::{
    prune_index_versions_before_in_session, record_index_versions_for_events_in_session,
};
use super::write_schema_events::{
    durable_record_changes_schema_cache, record_mysql_schema_set_events,
};
use super::*;

impl MySqlTenantStore {
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
        let committed = self.execute_write(|transaction| {
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
        let records = records.to_vec();
        self.execute_write(move |transaction| transaction.append_durable_records_batch(&records))?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        let records = records.to_vec();
        self.execute_write(move |transaction| transaction.apply_durable_records_batch(&records))?;
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
        self.execute_write(move |transaction| transaction.delete_cron_job(&name))?;
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
        let committed = self.execute_write(move |transaction| {
            transaction.set_trigger_write_origin(trigger_write_origin.cloned());
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

impl MySqlWriteTransaction {
    pub(super) fn begin<Check>(store: MySqlTenantStore, check_cancel: Check) -> Result<Self>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        // MySQL-only begin-retry loop: MySQL surfaces tenant-lock contention as a
        // retryable begin error, unlike PG's `pg_advisory_xact_lock`. Not unified
        // with the shared write core — dialect-load-bearing, see CO6.
        let check_cancel = Arc::new(Mutex::new(check_cancel));
        let mut attempt = 0;
        loop {
            // Retry only the begin/tenant-lock path; the write closure has not run yet.
            let check_cancel_for_attempt = check_cancel.clone();
            let transaction = Self::begin_once(
                store.clone(),
                Box::new(move || {
                    check_cancel_for_attempt
                        .lock()
                        .map_err(|_| {
                            Error::Internal("mysql write cancellation lock poisoned".to_string())
                        })
                        .and_then(|check_cancel| check_cancel())
                }),
            );
            match transaction {
                Ok(transaction) => return Ok(transaction),
                Err(error)
                    if is_retryable_mysql_begin_error(&error)
                        && attempt + 1 < MYSQL_WRITE_BEGIN_RETRY_ATTEMPTS =>
                {
                    attempt += 1;
                    std::thread::sleep(std::time::Duration::from_millis(
                        5 * u64::try_from(attempt).unwrap_or(1),
                    ));
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub fn prune_retained_versions(
        &mut self,
        document_prune_before: SequenceNumber,
        index_prune_before: SequenceNumber,
    ) -> Result<(u64, u64)> {
        self.check_cancel()?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            let document_versions_pruned = prune_document_versions_before_in_session(
                conn,
                &database_name,
                document_prune_before,
            )
            .await?;
            let index_versions_pruned =
                prune_index_versions_before_in_session(conn, &database_name, index_prune_before)
                    .await?;
            Ok((document_versions_pruned, index_versions_pruned))
        })
    }

    fn begin_once(
        store: MySqlTenantStore,
        check_cancel: Box<dyn Fn() -> Result<()> + Send>,
    ) -> Result<Self> {
        let provider = store.provider.clone();
        let database_name = store.database_name.clone();
        let conn = store.block_on({
            let provider = provider.clone();
            async move { provider.conn().await }
        })?;

        let mut transaction = Self {
            provider,
            database_name,
            schema_cache: store.schema_cache.clone(),
            conn: Some(conn),
            commit_writes: Vec::new(),
            tenant_events: Vec::new(),
            prepared_record: None,
            trigger_write_origin: None,
            commit_timestamp: None,
            schema_cache_changed: false,
            check_cancel,
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
        let table_id = self.resolve_or_create_table_id(&table_schema.table)?;
        let previous = self.load_table_schema(&table_schema.table)?;
        let mut table_schema = table_schema.clone();
        table_schema.reconcile_index_metadata(previous.as_ref());
        if let Some(previous) = previous.as_ref() {
            self.drop_table_indexes(previous)?;
        }
        self.upsert_table_schema(&table_schema)?;
        self.create_table_indexes(&table_schema)?;
        self.schema_cache_changed = true;
        record_mysql_schema_set_events(self, table_id, previous, &table_schema);
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
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let conn = self.session()?;
        let inserted = Self::block_on(&runtime_handle, async move {
            begin_scheduled_execution_in_session(conn, &database_name, execution_id).await
        })?;
        if inserted && let Some(execution_id) = execution_id {
            self.record_tenant_event(TenantEventKind::ScheduledExecution {
                execution_id: execution_id.to_string(),
            });
        }
        Ok(inserted)
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        let table_id = self.resolve_or_create_table_id(&document.table)?;
        let write_table_id = table_id.clone();
        let query = format!(
            "INSERT INTO {} (table_id, id, data_json, typed_fields_json, creation_time, update_time) VALUES (?, ?, ?, ?, ?, ?)",
            qualified_table(&self.database_name, "documents")
        );
        let document_id = document.id.to_string();
        let data_json = serialize_document_fields(document)?;
        let typed_fields_json = serialize_document_typed_fields(document)?;
        let creation_time = document.creation_time.0;
        let update_time = document.update_time.0;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(
                query,
                (
                    table_id.to_string(),
                    document_id,
                    data_json,
                    typed_fields_json,
                    creation_time,
                    update_time,
                ),
            )
            .await
            .map_err(map_mysql_error)
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
            .unwrap_or_else(|| self.provider.clock.now());
        validate(&existing_document, &document)?;
        let table_id = self
            .load_table_id(table)?
            .ok_or(Error::DocumentNotFound(id.clone()))?;
        let write_table_id = table_id.clone();
        let query = format!(
            "UPDATE {} SET data_json = ?, typed_fields_json = ?, creation_time = ?, update_time = ? WHERE table_id = ? AND id = ?",
            qualified_table(&self.database_name, "documents")
        );
        let data_json = serialize_document_fields(&document)?;
        let typed_fields_json = serialize_document_typed_fields(&document)?;
        let creation_time = document.creation_time.0;
        let update_time = document.update_time.0;
        let document_id = id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(
                query,
                (
                    data_json,
                    typed_fields_json,
                    creation_time,
                    update_time,
                    table_id.to_string(),
                    document_id,
                ),
            )
            .await
            .map_err(map_mysql_error)
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
            "DELETE FROM {} WHERE table_id = ? AND id = ?",
            qualified_table(&self.database_name, "documents")
        );
        let document_id = id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (table_id.to_string(), document_id))
                .await
                .map_err(map_mysql_error)
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

    pub fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        if max_jobs == 0 {
            return Ok(Vec::new());
        }
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let max_jobs = u64::try_from(max_jobs).unwrap_or(u64::MAX);
        let due: Vec<ScheduledJob> = {
            let conn = self.session()?;
            Self::block_on(&runtime_handle, async move {
                // MySQL claim uses `FOR UPDATE` row locks to serialize claimers;
                // PG relies on its advisory transaction lock instead. Dialect
                // lock mode is load-bearing — not unified with the write core, see CO6.
                let query = format!(
                    "SELECT data_json FROM {} WHERE run_at <= ? ORDER BY run_at, id LIMIT ? FOR UPDATE",
                    qualified_table(&database_name, "scheduled_jobs")
                );
                let rows: Vec<Row> = conn
                    .exec(query, (claim_due_jobs_upper_bound(now), max_jobs))
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

    pub fn append_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()> {
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
            let payload = serialize_tenant_event_record(record)?;
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

    fn batch_execute(&mut self, sql: &str) -> Result<()> {
        let query = sql.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.query_drop(query).await.map_err(map_mysql_error)
        })
    }

    pub(super) fn block_on<F, T>(runtime_handle: &TokioRuntimeHandle, future: F) -> Result<T>
    where
        F: Future<Output = Result<T>> + Send,
        T: Send,
    {
        bridge_tokio_runtime(
            runtime_handle,
            "MySQL write bridge thread panicked",
            move || runtime_handle.block_on(future),
        )
    }

    pub(super) fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    pub(super) fn session(&mut self) -> Result<&mut Conn> {
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

    // MySQL `SELECT ... FOR UPDATE` tenant-lock order — do not unify with PG's
    // `pg_advisory_xact_lock`; the lock acquisition order differs by dialect, see CO6.
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

    fn load_durable_record(
        &mut self,
        sequence: SequenceNumber,
    ) -> Result<Option<TenantEventRecord>> {
        let query = format!(
            "SELECT record_blob FROM {} WHERE sequence = ?",
            qualified_table(&self.database_name, "commit_log")
        );
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_first::<Vec<u8>, _, _>(query, (sequence.0,))
                .await
                .map_err(map_mysql_error)?
                .map(|payload| deserialize_tenant_event_record(payload.as_slice()))
                .transpose()
        })
    }

    fn append_commit_entry(
        &mut self,
        writes: Vec<WriteOp>,
        events: Vec<TenantEventKind>,
    ) -> Result<CommitEntry> {
        let sequence = SequenceNumber(self.latest_sequence()?.0.saturating_add(1));
        let timestamp = self
            .commit_timestamp
            .unwrap_or_else(|| self.provider.clock.now());
        let record = TenantEventRecord::from_events(sequence, timestamp, events)?;
        let entry = CommitEntry {
            sequence,
            timestamp,
            writes,
        };
        let payload = serialize_tenant_event_record(&record)?;
        let query = format!(
            "INSERT INTO {} (sequence, record_blob) VALUES (?, ?)",
            qualified_table(&self.database_name, "commit_log")
        );
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let record_sequence = record.sequence;
        let record_timestamp = record.timestamp;
        let record_events = record.events.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            record_document_versions_for_events_in_session(
                conn,
                &database_name,
                record_sequence,
                record_timestamp,
                &record_events,
            )
            .await?;
            record_index_versions_for_events_in_session(
                conn,
                &database_name,
                record_sequence,
                &record_events,
            )
            .await?;
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
        let id = id.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            load_document_from_session(conn, &database_name, &table, &id).await
        })
    }

    pub(super) fn load_table_id(&mut self, table: &TableName) -> Result<Option<TableId>> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let table = table.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            load_table_id_from_session(conn, &database_name, &table).await
        })
    }

    fn resolve_or_create_table_id(&mut self, table: &TableName) -> Result<TableId> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let table = table.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            resolve_or_create_table_id_from_session(conn, &database_name, &table).await
        })
    }

    pub(super) fn load_table_schema(&mut self, table: &TableName) -> Result<Option<TableSchema>> {
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

    pub(super) fn delete_table_schema_entry(&mut self, table: &TableName) -> Result<()> {
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

    pub(super) fn drop_table_indexes(&mut self, table_schema: &TableSchema) -> Result<()> {
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

    fn apply_durable_record(&mut self, record: &TenantEventRecord) -> Result<()> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let record = record.clone();
        let changes_schema_cache = durable_record_changes_schema_cache(&record);
        let conn = self.session()?;
        let result = Self::block_on(&runtime_handle, async move {
            apply_durable_record_in_session(conn, &database_name, &record).await
        });
        if result.is_ok() && changes_schema_cache {
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
}

impl crate::sql::write_core::SqlWriteBackend for MySqlWriteTransaction {
    fn check_cancel(&self) -> Result<()> {
        MySqlWriteTransaction::check_cancel(self)
    }

    fn check_fault(&self, point: FaultPoint) -> Result<()> {
        self.provider.fault_injector.check(point)
    }

    fn batch_execute(&mut self, sql: &str) -> Result<()> {
        MySqlWriteTransaction::batch_execute(self, sql)
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
        MySqlWriteTransaction::applied_sequence(self)
    }

    fn load_durable_record(
        &mut self,
        sequence: SequenceNumber,
    ) -> Result<Option<TenantEventRecord>> {
        MySqlWriteTransaction::load_durable_record(self, sequence)
    }

    fn apply_durable_record(&mut self, record: &TenantEventRecord) -> Result<()> {
        MySqlWriteTransaction::apply_durable_record(self, record)
    }

    fn write_applied_sequence(&mut self, sequence: SequenceNumber) -> Result<()> {
        MySqlWriteTransaction::write_applied_sequence(self, sequence)
    }

    fn append_commit_entry(
        &mut self,
        writes: Vec<WriteOp>,
        events: Vec<TenantEventKind>,
    ) -> Result<CommitEntry> {
        MySqlWriteTransaction::append_commit_entry(self, writes, events)
    }

    fn append_prepared_record(&mut self, record: &TenantEventRecord) -> Result<CommitEntry> {
        MySqlWriteTransaction::append_prepared_record(self, record)
    }

    fn enqueue_notification(&mut self) -> Result<()> {
        // MySQL has no LISTEN/NOTIFY channel; there is nothing to flush.
        Ok(())
    }

    fn schema_cache_changed(&self) -> bool {
        self.schema_cache_changed
    }

    fn invalidate_schema_cache(&self) {
        invalidate_schema_cache_handle(&self.schema_cache);
    }

    fn load_document(&mut self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        MySqlWriteTransaction::load_document(self, table, id)
    }

    fn load_table_id(&mut self, table: &TableName) -> Result<Option<TableId>> {
        MySqlWriteTransaction::load_table_id(self, table)
    }

    fn insert_document(&mut self, document: &Document) -> Result<()> {
        MySqlWriteTransaction::insert_document(self, document)
    }

    fn update_document_row(&mut self, table_id: &TableId, current: &Document) -> Result<()> {
        let query = format!(
            "UPDATE {} SET data_json = ?, typed_fields_json = ?, creation_time = ?, update_time = ? WHERE table_id = ? AND id = ?",
            qualified_table(&self.database_name, "documents")
        );
        let data_json = serialize_document_fields(current)?;
        let typed_fields_json = serialize_document_typed_fields(current)?;
        let creation_time = current.creation_time.0;
        let update_time = current.update_time.0;
        let document_id = current.id.to_string();
        let table_id = table_id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(
                query,
                (
                    data_json,
                    typed_fields_json,
                    creation_time,
                    update_time,
                    table_id,
                    document_id,
                ),
            )
            .await
            .map_err(map_mysql_error)
        })
    }

    fn delete_document_row(&mut self, table_id: &TableId, id: &DocumentId) -> Result<()> {
        let query = format!(
            "DELETE FROM {} WHERE table_id = ? AND id = ?",
            qualified_table(&self.database_name, "documents")
        );
        let document_id = id.to_string();
        let table_id = table_id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (table_id, document_id))
                .await
                .map_err(map_mysql_error)
        })
    }

    fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()> {
        MySqlWriteTransaction::upsert_resource_path_binding(self, binding)
    }

    fn remove_resource_path_binding(
        &mut self,
        locator: &nimbus_core::DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        MySqlWriteTransaction::remove_resource_path_binding(self, locator)
    }
}

fn is_retryable_mysql_begin_error(error: &Error) -> bool {
    matches!(
        error.storage_kind(),
        Some(StorageErrorKind::Busy | StorageErrorKind::Transient)
    )
}
