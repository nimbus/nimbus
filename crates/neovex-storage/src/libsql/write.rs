use super::*;

impl LibsqlReplicaTenantStore {
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

    pub fn append_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records.to_vec();
        self.block_on(self.append_remote_durable_records_batch(records.as_slice()))?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
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

    pub fn claim_due_jobs(&self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        Ok(self
            .execute_write(move |transaction| transaction.claim_due_jobs(now))?
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
        _indexes: &[neovex_core::IndexDefinition],
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
        _indexes: &[neovex_core::IndexDefinition],
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
        _indexes: &[neovex_core::IndexDefinition],
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
        _indexes: &[neovex_core::IndexDefinition],
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
        _indexes: &[neovex_core::IndexDefinition],
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
        _indexes: &[neovex_core::IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()> + Send + 'static,
    {
        self.delete_validated_once(table, id, execution_id, validate)
    }

    pub fn apply_execution_unit_batch(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
    ) -> Result<Option<CommitEntry>> {
        self.apply_execution_unit_batch_with_origin(writes, schedule_ops, None)
    }

    pub fn apply_execution_unit_batch_with_origin(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
        trigger_write_origin: Option<&TriggerWriteOrigin>,
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
            trigger_write_origin: None,
            check_cancel: Box::new(check_cancel),
            refresh_cache_after_commit: false,
        })
    }

    pub fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        let schema_json = serialize_json(table_schema)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)
                     ON CONFLICT(table_name) DO UPDATE SET schema_json = excluded.schema_json",
                    libsql::params![table_schema.table.as_str(), schema_json],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.refresh_cache_after_commit = true;
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM schemas WHERE table_name = ?1",
                    libsql::params![table.as_str()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.refresh_cache_after_commit = true;
        Ok(())
    }

    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        let Some(execution_id) = execution_id else {
            return Ok(true);
        };
        self.store.block_on(async {
            let changed = self
                .session()?
                .execute(
                    "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
                    libsql::params![execution_id],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(changed == 1)
        })
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        let data_json = serialize_document_fields(document)?;
        let typed_fields_json = serialize_document_typed_fields(document)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO documents (table_name, id, data_json, typed_fields_json, creation_time, update_time)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    libsql::params![
                        document.table.as_str(),
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
        document.update_time = self.store.provider.clock.now();
        validate(&existing_document, &document)?;
        let data_json = serialize_document_fields(&document)?;
        let typed_fields_json = serialize_document_typed_fields(&document)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "UPDATE documents
                     SET data_json = ?3, typed_fields_json = ?4, creation_time = ?5, update_time = ?6
                     WHERE table_name = ?1 AND id = ?2",
                    libsql::params![
                        table.as_str(),
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
            op_type: WriteOpType::Update,
            doc_id: id.clone(),
            resource_path_binding: self.store.resource_path_binding(
                &neovex_core::DocumentLocator::new(table.clone(), id.clone()),
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
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                    libsql::params![table.as_str(), id.to_string()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        let resource_path_binding = self.remove_resource_path_binding(
            &neovex_core::DocumentLocator::new(table.clone(), id.clone()),
        )?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
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
                    "INSERT INTO scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                    libsql::params![job.id.to_string(), data_json],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    pub fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        let due = self
            .store
            .block_on(self.store.load_remote_scheduled_jobs("scheduled_jobs"))?;
        let due = due
            .into_iter()
            .filter(|job| job.run_at.0 <= now.0)
            .collect::<Vec<_>>();
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
            job.run_at = now;
            let data_json = serialize_json(&job)?;
            self.store.block_on(async {
                self.session()?
                    .execute(
                        "INSERT INTO scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                        libsql::params![job.id.to_string(), data_json],
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
                    return Err(Error::Conflict(format!(
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
                let data_json = serialize_document_fields(current)?;
                let typed_fields_json = serialize_document_typed_fields(current)?;
                self.store.block_on(async {
                    self.session()?
                        .execute(
                            "UPDATE documents
                             SET data_json = ?3, typed_fields_json = ?4, creation_time = ?5, update_time = ?6
                             WHERE table_name = ?1 AND id = ?2",
                            libsql::params![
                                current.table.as_str(),
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
                self.store.block_on(async {
                    self.session()?
                        .execute(
                            "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                            libsql::params![previous.table.as_str(), previous.id.to_string()],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    Ok(())
                })?;
                let resource_path_binding = self.remove_resource_path_binding(
                    &neovex_core::DocumentLocator::new(previous.table.clone(), previous.id.clone()),
                )?;
                self.record_commit_write(WriteOp {
                    table: previous.table.clone(),
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

    pub fn commit(mut self) -> Result<Option<CommitEntry>> {
        self.check_cancel()?;
        let writes = std::mem::take(&mut self.commit_writes);
        let commit = if writes.is_empty() {
            None
        } else {
            Some(self.append_commit_entry(writes)?)
        };
        let tx = self.tx.take().ok_or_else(|| {
            Error::Internal("libsql replica write transaction already closed".to_string())
        })?;
        self.store.block_on(async move {
            tx.commit().await.map_err(map_libsql_error)?;
            Ok(())
        })?;
        if let Some(commit) = &commit {
            self.store.note_required_cache_sequence_with_cause(
                commit.sequence,
                LibsqlReplicaRefreshCause::CommitBarrier,
            );
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

    fn record_commit_write(&mut self, mut write: WriteOp) {
        if write.trigger_write_origin.is_none() {
            write.trigger_write_origin = self.trigger_write_origin.clone();
        }
        self.commit_writes.push(write);
    }

    fn load_document(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        self.store.block_on(load_remote_document_from_session(
            self.session()?,
            table.clone(),
            id.clone(),
        ))
    }

    fn append_commit_entry(&self, writes: Vec<WriteOp>) -> Result<CommitEntry> {
        let sequence = SequenceNumber(
            self.store
                .block_on(load_next_sequence_from_session(self.session()?))?,
        );
        let entry = CommitEntry {
            sequence,
            timestamp: self.store.provider.clock.now(),
            writes,
        };
        let payload = serialize_commit(&entry)?;
        self.store.block_on(async {
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
}
