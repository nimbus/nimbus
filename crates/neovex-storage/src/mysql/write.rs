use super::*;

impl MySqlTenantStore {
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

    pub fn append_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
        let records = records.to_vec();
        self.execute_write(move |transaction| transaction.append_durable_records_batch(&records))?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
        let records = records.to_vec();
        self.execute_write(move |transaction| transaction.apply_durable_records_batch(&records))?;
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
        let committed = self.execute_write(move |transaction| {
            transaction.set_trigger_write_origin(trigger_write_origin.cloned());
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
}

impl MySqlWriteTransaction {
    pub(super) fn begin<Check>(store: MySqlTenantStore, check_cancel: Check) -> Result<Self>
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
            schema_cache: store.schema_cache.clone(),
            conn: Some(conn),
            commit_writes: Vec::new(),
            trigger_write_origin: None,
            schema_cache_changed: false,
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
        self.schema_cache_changed = true;
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        if let Some(previous) = self.load_table_schema(table)? {
            self.drop_table_indexes(&previous)?;
        }
        self.delete_table_schema_entry(table)?;
        self.schema_cache_changed = true;
        Ok(())
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
            "INSERT INTO {} (table_name, id, data_json, typed_fields_json, creation_time, update_time) VALUES (?, ?, ?, ?, ?, ?)",
            qualified_table(&self.database_name, "documents")
        );
        let table_name = document.table.as_str().to_string();
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
                    table_name,
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
        document.update_time = self.provider.clock.now();
        validate(&existing_document, &document)?;
        let query = format!(
            "UPDATE {} SET data_json = ?, typed_fields_json = ?, creation_time = ?, update_time = ? WHERE table_name = ? AND id = ?",
            qualified_table(&self.database_name, "documents")
        );
        let data_json = serialize_document_fields(&document)?;
        let typed_fields_json = serialize_document_typed_fields(&document)?;
        let creation_time = document.creation_time.0;
        let update_time = document.update_time.0;
        let table_name = table.as_str().to_string();
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
                    table_name,
                    document_id,
                ),
            )
            .await
            .map_err(map_mysql_error)
        })?;
        let resource_path_binding = self.resource_path_binding(
            &neovex_core::DocumentLocator::new(table.clone(), id.clone()),
        )?;
        self.record_commit_write(WriteOp {
            table: table.clone(),
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
                let query = format!(
                    "UPDATE {} SET data_json = ?, typed_fields_json = ?, creation_time = ?, update_time = ? WHERE table_name = ? AND id = ?",
                    qualified_table(&self.database_name, "documents")
                );
                let data_json = serialize_document_fields(current)?;
                let typed_fields_json = serialize_document_typed_fields(current)?;
                let creation_time = current.creation_time.0;
                let update_time = current.update_time.0;
                let table_name = current.table.as_str().to_string();
                let document_id = current.id.to_string();
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
                            table_name,
                            document_id,
                        ),
                    )
                    .await
                    .map_err(map_mysql_error)
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
        let id = id.clone();
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

    fn set_trigger_write_origin(&mut self, trigger_write_origin: Option<TriggerWriteOrigin>) {
        self.trigger_write_origin = trigger_write_origin;
    }

    fn record_commit_write(&mut self, mut write: WriteOp) {
        if write.trigger_write_origin.is_none() {
            write.trigger_write_origin = self.trigger_write_origin.clone();
        }
        self.commit_writes.push(write);
    }
}
