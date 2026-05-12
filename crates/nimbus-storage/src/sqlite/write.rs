use super::*;

impl SqliteTenantStore {
    pub fn insert_document_for_testing(&self, document: &Document) -> Result<()> {
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO documents (table_name, id, data_json, creation_time, update_time)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                document.table.as_str(),
                document.id.to_string(),
                serde_json::to_string(&document.fields)
                    .map_err(|error| Error::Serialization(error.to_string()))?,
                document.creation_time.0,
                document.update_time.0,
            ],
        )
        .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn insert(&self, document: &Document) -> Result<CommitEntry> {
        self.insert_once(document, None)?
            .ok_or_else(|| Error::Internal("non-deduplicated insert should commit".to_string()))
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

    pub fn update(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
    ) -> Result<CommitEntry> {
        self.update_validated(table, id, patch, |_, _| Ok(()))
    }

    pub fn update_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
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
        F: FnOnce(&Document, &Document) -> Result<()>,
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

    pub fn delete(&self, table: &TableName, id: &DocumentId) -> Result<CommitEntry> {
        self.delete_validated_once(table, id, None, |_| Ok(()))?
            .map(|commit| commit.0)
            .ok_or_else(|| Error::Internal("non-deduplicated delete should commit".to_string()))
    }

    pub fn delete_once(
        &self,
        table: &TableName,
        id: &DocumentId,
        execution_id: Option<&str>,
    ) -> Result<Option<(CommitEntry, Document)>> {
        self.delete_validated_once(table, id, execution_id, |_| Ok(()))
    }

    pub fn delete_returning_document(
        &self,
        table: &TableName,
        id: &DocumentId,
    ) -> Result<(CommitEntry, Document)> {
        self.delete_validated_returning_document(table, id, |_| Ok(()))
    }

    pub fn delete_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()>,
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
        F: FnOnce(&Document) -> Result<()>,
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

    pub fn insert_with_indexes(
        &self,
        document: &Document,
        _indexes: &[nimbus_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        self.insert(document)
    }

    pub fn insert_with_indexes_once(
        &self,
        document: &Document,
        _indexes: &[nimbus_core::IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        self.insert_once(document, execution_id)
    }

    pub fn update_with_indexes(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        _indexes: &[nimbus_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        self.update(table, id, patch)
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
        F: FnOnce(&Document, &Document) -> Result<()>,
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
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        self.update_validated_once(table, id, patch, execution_id, validate)
    }

    pub fn delete_with_indexes(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[nimbus_core::IndexDefinition],
    ) -> Result<CommitEntry> {
        self.delete(table, id)
    }

    pub fn delete_with_indexes_once(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[nimbus_core::IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<(CommitEntry, Document)>> {
        self.delete_once(table, id, execution_id)
    }

    pub fn delete_with_indexes_returning_document(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[nimbus_core::IndexDefinition],
    ) -> Result<(CommitEntry, Document)> {
        self.delete_returning_document(table, id)
    }

    pub fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        _indexes: &[nimbus_core::IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()>,
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
        F: FnOnce(&Document) -> Result<()>,
    {
        self.delete_validated_once(table, id, execution_id, validate)
    }

    pub fn apply_resolved_write_batch(&self, writes: &[ResolvedWrite]) -> Result<CommitEntry> {
        self.apply_execution_unit_batch(writes, &[])?
            .ok_or_else(|| {
                Error::Internal("resolved write batch must contain at least one write".to_string())
            })
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

        let committed = self.execute_write(move |transaction| {
            transaction.set_trigger_write_origin(trigger_write_origin.cloned());
            for write in writes {
                transaction.apply_resolved_write(write)?;
            }
            apply_schedule_ops_in_transaction(transaction, schedule_ops)?;
            Ok(())
        })?;
        Ok(committed.commit)
    }
}

impl SqliteWriteTransaction {
    pub fn save_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)
                 ON CONFLICT(table_name) DO UPDATE SET schema_json = excluded.schema_json",
                params![table_schema.table.as_str(), serialize_json(table_schema)?],
            )
            .map_err(map_sqlite_error)?;
        self.schema_cache_dirty = true;
        Ok(())
    }

    pub fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        if let Some(previous) =
            load_table_schema_from_conn(self.connection_mut()?, &table_schema.table)?
        {
            drop_sqlite_indexes_for_table_schema(self.connection_mut()?, &previous)?;
        }
        self.save_table_schema(table_schema)?;
        create_sqlite_indexes_for_table_schema(self.connection_mut()?, table_schema)?;
        Ok(())
    }

    pub fn delete_table_schema_entry(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "DELETE FROM schemas WHERE table_name = ?1",
                params![table.as_str()],
            )
            .map_err(map_sqlite_error)?;
        self.schema_cache_dirty = true;
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        if let Some(previous) = load_table_schema_from_conn(self.connection_mut()?, table)? {
            drop_sqlite_indexes_for_table_schema(self.connection_mut()?, &previous)?;
        }
        self.delete_table_schema_entry(table)
    }

    pub fn put_metadata(&mut self, key: &str, value: &[u8]) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
                params![key, value],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        self.check_cancel()?;
        let Some(execution_id) = execution_id else {
            return Ok(true);
        };

        let inserted = self
            .connection_mut()?
            .execute(
                "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
                params![execution_id],
            )
            .map_err(map_sqlite_error)?;
        Ok(inserted == 1)
    }

    pub fn insert_document(&mut self, document: &Document) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO documents (table_name, id, data_json, typed_fields_json, creation_time, update_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    document.table.as_str(),
                    document.id.to_string(),
                    serialize_document_fields(document)?,
                    serialize_document_typed_fields(document)?,
                    document.creation_time.0,
                    document.update_time.0,
                ],
            )
            .map_err(map_sqlite_error)?;
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
        document.update_time = self.clock.now();
        validate(&existing_document, &document)?;
        self.connection_mut()?
            .execute(
                "UPDATE documents
                 SET data_json = ?3, typed_fields_json = ?4, creation_time = ?5, update_time = ?6
                 WHERE table_name = ?1 AND id = ?2",
                params![
                    table.as_str(),
                    id.to_string(),
                    serialize_document_fields(&document)?,
                    serialize_document_typed_fields(&document)?,
                    document.creation_time.0,
                    document.update_time.0,
                ],
            )
            .map_err(map_sqlite_error)?;
        let resource_path_binding = self.resource_path_binding(
            &nimbus_core::DocumentLocator::new(table.clone(), id.clone()),
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
        self.connection_mut()?
            .execute(
                "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                params![table.as_str(), id.to_string()],
            )
            .map_err(map_sqlite_error)?;
        let resource_path_binding = self.remove_resource_path_binding(
            &nimbus_core::DocumentLocator::new(table.clone(), id.clone()),
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
        self.connection_mut()?
            .execute(
                "INSERT INTO scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                params![job.id.to_string(), serialize_json(job)?],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn claim_due_jobs(&mut self, now: Timestamp) -> Result<Vec<ScheduledJob>> {
        self.check_cancel()?;
        let due = load_scheduled_jobs_from_conn(self.connection_mut()?, "scheduled_jobs")?
            .into_iter()
            .filter(|job| job.run_at.0 <= now.0)
            .collect::<Vec<_>>();
        for job in &due {
            self.check_cancel()?;
            self.connection_mut()?
                .execute(
                    "DELETE FROM scheduled_jobs WHERE id = ?1",
                    params![job.id.to_string()],
                )
                .map_err(map_sqlite_error)?;
            self.connection_mut()?
                .execute(
                    "INSERT INTO running_scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                    params![job.id.to_string(), serialize_json(job)?],
                )
                .map_err(map_sqlite_error)?;
        }
        Ok(due)
    }

    pub fn complete_scheduled_job(&mut self, job_id: &JobId) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "DELETE FROM running_scheduled_jobs WHERE id = ?1",
                params![job_id.to_string()],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn cancel_scheduled_job(&mut self, job_id: &JobId) -> Result<bool> {
        self.check_cancel()?;
        let affected = self
            .connection_mut()?
            .execute(
                "DELETE FROM scheduled_jobs WHERE id = ?1",
                params![job_id.to_string()],
            )
            .map_err(map_sqlite_error)?;
        Ok(affected == 1)
    }

    pub fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO scheduled_job_results (job_id, data_json) VALUES (?1, ?2)
                 ON CONFLICT(job_id) DO UPDATE SET data_json = excluded.data_json",
                params![result.id.to_string(), serialize_json(result)?],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute(
                "INSERT INTO cron_jobs (name, data_json) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET data_json = excluded.data_json",
                params![cron.name, serialize_json(cron)?],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        self.check_cancel()?;
        self.connection_mut()?
            .execute("DELETE FROM cron_jobs WHERE name = ?1", params![name])
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    pub fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        self.check_cancel()?;
        let running_jobs =
            load_scheduled_jobs_from_conn(self.connection_mut()?, "running_scheduled_jobs")?;
        for mut job in running_jobs {
            self.check_cancel()?;
            job.run_at = now;
            self.connection_mut()?
                .execute(
                    "INSERT INTO scheduled_jobs (id, data_json) VALUES (?1, ?2)",
                    params![job.id.to_string(), serialize_json(&job)?],
                )
                .map_err(map_sqlite_error)?;
            self.connection_mut()?
                .execute(
                    "DELETE FROM running_scheduled_jobs WHERE id = ?1",
                    params![job.id.to_string()],
                )
                .map_err(map_sqlite_error)?;
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
                self.connection_mut()?
                    .execute(
                        "UPDATE documents
                         SET data_json = ?3, typed_fields_json = ?4, creation_time = ?5, update_time = ?6
                         WHERE table_name = ?1 AND id = ?2",
                        params![
                            current.table.as_str(),
                            current.id.to_string(),
                            serialize_document_fields(current)?,
                            serialize_document_typed_fields(current)?,
                            current.creation_time.0,
                            current.update_time.0,
                        ],
                    )
                    .map_err(map_sqlite_error)?;
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
                self.connection_mut()?
                    .execute(
                        "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                        params![previous.table.as_str(), previous.id.to_string()],
                    )
                    .map_err(map_sqlite_error)?;
                let resource_path_binding = self.remove_resource_path_binding(
                    &nimbus_core::DocumentLocator::new(previous.table.clone(), previous.id.clone()),
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

    pub(crate) fn check_cancel(&self) -> Result<()> {
        (self.check_cancel.as_ref())()
    }

    pub fn commit(mut self) -> Result<Option<CommitEntry>> {
        self.check_cancel()?;
        let Some(conn) = self.conn.take() else {
            return Err(Error::Internal(
                "sqlite write transaction already closed".to_string(),
            ));
        };
        let commit = if self.commit_writes.is_empty() {
            None
        } else {
            Some(append_commit_entry(
                &conn,
                self.clock.now(),
                std::mem::take(&mut self.commit_writes),
            )?)
        };
        self.fault_injector
            .check(FaultPoint::StorageCommitBeforeVisibility)?;
        conn.execute_batch("COMMIT").map_err(map_sqlite_error)?;
        if self.schema_cache_dirty {
            let schema = load_schema_from_conn(&conn)?;
            *self
                .schema_cache
                .write()
                .map_err(|_| Error::Internal("sqlite schema cache lock poisoned".to_string()))? =
                schema;
        }
        self.fault_injector
            .check(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
        Ok(commit)
    }

    pub fn rollback(mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = conn.execute_batch("ROLLBACK");
        }
    }

    pub(super) fn connection_mut(&mut self) -> Result<&mut Connection> {
        self.conn
            .as_mut()
            .ok_or_else(|| Error::Internal("sqlite write transaction already closed".to_string()))
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

    fn load_document(&mut self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        load_document_from_conn(self.connection_mut()?, table, id)
    }
}
