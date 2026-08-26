use super::document_versions::{
    prune_document_versions_before_in_session, record_document_versions_for_events_in_session,
};
use super::index_versions::{
    prune_index_versions_before_in_session, record_index_versions_for_events_in_session,
};
use super::*;
use crate::sql::schema_events::{
    durable_record_changes_schema_cache, sql_record_schema_set_events,
};
use crate::sql::store_core::{
    SqlDurableJournalStore, SqlStoreCore, SqlWriteTransactionCore,
    sql_store_append_durable_records_batch, sql_store_apply_durable_records_batch,
    sql_store_core_facade, sql_store_fenced_append_and_apply_durable_records_batch_cancellable,
    sql_store_replay_durable_records_batch,
};
use crate::sql::write_core::SqlDurableJournalTransaction;
use crate::sql::write_pipeline::SqlWritePipelineMetrics;
use crate::{CommitterLeaseResult, RetentionReadFloors};

sql_store_core_facade!(MySqlTenantStore);

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
}

/// Wire MySQL into the shared store-level wrapper layer. Everything below
/// forwards to the inherent write bridge above or to an inherent journal read;
/// the wrappers built on them live once in [`crate::sql::store_core`].
impl SqlStoreCore for MySqlTenantStore {
    type Transaction = MySqlWriteTransaction;

    fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T> + Send + 'static,
    {
        MySqlTenantStore::execute_write(self, task)
    }

    fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut MySqlWriteTransaction) -> Result<T> + Send + 'static,
    {
        MySqlTenantStore::execute_write_cancellable(self, check_cancel, task)
    }

    fn retention_floor(&self) -> &RetentionFloor {
        self.retention_floor.as_ref()
    }

    fn check_retention_read_page(&self) -> Result<()> {
        self.check_fault(crate::FaultPoint::RetentionReadAfterPage)
    }

    fn journal_progress(&self) -> Result<JournalProgress> {
        MySqlTenantStore::journal_progress(self)
    }

    fn load_retention_metadata_snapshot(&self) -> Result<(Option<Vec<u8>>, RetentionReadFloors)> {
        let loaded = self.execute_write(|transaction| transaction.load_retention_metadata())?;
        debug_assert!(loaded.commit.is_none());
        Ok(loaded.value)
    }

    fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        MySqlTenantStore::read_durable_journal_from(self, sequence)
    }

    fn recover_durable_journal(&self) -> Result<JournalProgress> {
        MySqlTenantStore::recover_durable_journal(self)
    }

    fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        MySqlTenantStore::export_materialized_journal_snapshot(self)
    }

    fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        sql_store_append_durable_records_batch(self, records)
    }

    fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        sql_store_apply_durable_records_batch(self, records)
    }

    fn replay_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        sql_store_replay_durable_records_batch(self, records)
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

impl SqlDurableJournalStore for MySqlTenantStore {
    fn pipeline_metrics(&self) -> &SqlWritePipelineMetrics {
        self.pipeline_metrics.as_ref()
    }
}

/// Transaction-side seam for the shared wrappers. Each method forwards to the
/// inherent method of the same name, which wins method-call resolution.
impl SqlWriteTransactionCore for MySqlWriteTransaction {
    fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        MySqlWriteTransaction::begin_scheduled_execution(self, execution_id)
    }

    fn set_prepared_record(&mut self, record: TenantEventRecord) {
        MySqlWriteTransaction::set_prepared_record(self, record)
    }

    fn set_trigger_write_origin(&mut self, trigger_write_origin: Option<TriggerWriteOrigin>) {
        MySqlWriteTransaction::set_trigger_write_origin(self, trigger_write_origin)
    }

    fn set_commit_timestamp(&mut self, commit_timestamp: Option<Timestamp>) {
        MySqlWriteTransaction::set_commit_timestamp(self, commit_timestamp)
    }

    fn advance_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        durable_sequence: SequenceNumber,
    ) -> Result<u64> {
        MySqlWriteTransaction::advance_fenced_committer_lease(
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
        MySqlWriteTransaction::validate_fenced_committer_lease(
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
        MySqlWriteTransaction::materialize_trigger_invocations(self, records, cursor)
    }

    fn save_trigger_invocation(
        &mut self,
        record: &nimbus_core::TriggerInvocationRecord,
    ) -> Result<()> {
        MySqlWriteTransaction::save_trigger_invocation(self, record)
    }

    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        MySqlWriteTransaction::replace_table_schema(self, table_schema)
    }

    fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        MySqlWriteTransaction::delete_table_schema(self, table)
    }

    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        MySqlWriteTransaction::insert_scheduled_job(self, job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        MySqlWriteTransaction::claim_due_jobs(self, now, max_jobs)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        MySqlWriteTransaction::complete_scheduled_job(self, job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        MySqlWriteTransaction::cancel_scheduled_job(self, job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        MySqlWriteTransaction::record_scheduled_job_result(self, result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        MySqlWriteTransaction::save_cron_job(self, cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        MySqlWriteTransaction::delete_cron_job(self, name)
    }

    fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        MySqlWriteTransaction::recover_running_jobs(self, now)
    }

    fn apply_resolved_write(&mut self, write: &ResolvedWrite) -> Result<()> {
        MySqlWriteTransaction::apply_resolved_write(self, write)
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
        MySqlWriteTransaction::update_document_validated(self, table, id, patch, validate)
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
        MySqlWriteTransaction::delete_document_validated(self, table, id, validate)
    }

    fn prune_retained_versions(
        &mut self,
        document_prune_before: SequenceNumber,
        index_prune_before: SequenceNumber,
    ) -> Result<(u64, u64)> {
        MySqlWriteTransaction::prune_retained_versions(
            self,
            document_prune_before,
            index_prune_before,
        )
    }

    fn load_retention_metadata(&mut self) -> Result<(Option<Vec<u8>>, RetentionReadFloors)> {
        MySqlWriteTransaction::load_retention_metadata(self)
    }

    fn applied_sequence_for_retention(&mut self) -> Result<SequenceNumber> {
        self.applied_sequence()
    }

    fn prune_durable_journal_through(&mut self, sequence: SequenceNumber) -> Result<u64> {
        MySqlWriteTransaction::prune_durable_journal_through(self, sequence)
    }

    fn store_retention_metadata(
        &mut self,
        checkpoint_blob: &[u8],
        read_floors: RetentionReadFloors,
    ) -> Result<()> {
        MySqlWriteTransaction::store_retention_metadata(self, checkpoint_blob, read_floors)
    }
}

impl MySqlWriteTransaction {
    pub(crate) fn validate_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        durable_sequence: SequenceNumber,
    ) -> Result<u64> {
        // Scheduler writes validate without advancing the durable sequence. A
        // no-op `UPDATE durable_sequence = durable_sequence` cannot be used as
        // a matched-row test: MySQL reports changed rows by default, so a
        // valid lease produces zero affected rows. Lock the matching lease row
        // instead; the lock remains held through the scheduler mutation and
        // transaction commit, preserving the same atomic fencing boundary as
        // the sequence-advancing CAS below.
        let query = format!(
            "SELECT 1 FROM {} \
             WHERE singleton = TRUE AND owner_id = ? AND epoch = ? \
                   AND expires_at > CURRENT_TIMESTAMP(6) AND durable_sequence = ? \
             FOR UPDATE",
            qualified_table(&self.database_name, "committer_lease")
        );
        let owner_id = owner_id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_first::<u8, _, _>(query, (owner_id, epoch, durable_sequence.0))
                .await
                .map(|matched| u64::from(matched.is_some()))
                .map_err(map_mysql_error)
        })
    }

    pub(super) fn advance_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        durable_sequence: SequenceNumber,
    ) -> Result<u64> {
        let query = format!(
            "UPDATE {} SET durable_sequence = ? \
             WHERE singleton = TRUE AND owner_id = ? AND epoch = ? \
                   AND expires_at > CURRENT_TIMESTAMP(6) AND durable_sequence = ?",
            qualified_table(&self.database_name, "committer_lease")
        );
        let owner_id = owner_id.to_string();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(
                query,
                (durable_sequence.0, owner_id, epoch, expected_previous.0),
            )
            .await
            .map_err(map_mysql_error)?;
            Ok(conn.affected_rows())
        })
    }

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

    fn load_retention_metadata(&mut self) -> Result<(Option<Vec<u8>>, RetentionReadFloors)> {
        self.check_cancel()?;
        let query = format!(
            "SELECT key_name, value_blob FROM {} WHERE key_name IN (?, ?, ?, ?)",
            qualified_table(&self.database_name, "metadata")
        );
        let checkpoint_key = crate::retention::RETENTION_CHECKPOINT_METADATA_KEY;
        let document_floor_key = crate::retention::RETENTION_DOCUMENT_VERSION_FLOOR_METADATA_KEY;
        let index_floor_key = crate::retention::RETENTION_INDEX_VERSION_FLOOR_METADATA_KEY;
        let physical_floor_key = crate::retention::RETENTION_PHYSICAL_FLOOR_METADATA_KEY;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        let rows: Vec<Row> = Self::block_on(&runtime_handle, async move {
            conn.exec(
                query,
                (
                    checkpoint_key,
                    document_floor_key,
                    index_floor_key,
                    physical_floor_key,
                ),
            )
            .await
            .map_err(map_mysql_error)
        })?;
        let mut checkpoint_blob = None;
        let mut read_floors = RetentionReadFloors::default();
        for row in rows {
            let (key, value): (String, Option<Vec<u8>>) = mysql_async::from_row(row);
            let Some(value) = value else {
                return Err(Error::storage(
                    nimbus_core::StorageErrorKind::Corruption,
                    format!("retention metadata key {key} has no blob value"),
                ));
            };
            if key == checkpoint_key {
                checkpoint_blob = Some(value);
            } else if key == document_floor_key {
                read_floors.document_versions =
                    crate::retention::decode_retention_floor(value.as_slice())?;
            } else if key == index_floor_key {
                read_floors.index_versions =
                    crate::retention::decode_retention_floor(value.as_slice())?;
            } else if key == physical_floor_key {
                read_floors.journal = crate::retention::decode_retention_floor(value.as_slice())?;
            }
        }
        Ok((checkpoint_blob, read_floors))
    }

    fn prune_durable_journal_through(&mut self, sequence: SequenceNumber) -> Result<u64> {
        self.check_cancel()?;
        let query = format!(
            "DELETE FROM {} WHERE sequence <= ?",
            qualified_table(&self.database_name, "commit_log")
        );
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(query, (sequence.0,))
                .await
                .map_err(map_mysql_error)?;
            Ok(conn.affected_rows())
        })
    }

    fn store_retention_metadata(
        &mut self,
        checkpoint_blob: &[u8],
        read_floors: RetentionReadFloors,
    ) -> Result<()> {
        self.check_cancel()?;
        let query = format!(
            "INSERT INTO {} (key_name, value_blob) VALUES \
             (?, ?), (?, ?), (?, ?), (?, ?) \
             ON DUPLICATE KEY UPDATE value_blob = VALUES(value_blob)",
            qualified_table(&self.database_name, "metadata")
        );
        let checkpoint_key = crate::retention::RETENTION_CHECKPOINT_METADATA_KEY;
        let document_floor_key = crate::retention::RETENTION_DOCUMENT_VERSION_FLOOR_METADATA_KEY;
        let index_floor_key = crate::retention::RETENTION_INDEX_VERSION_FLOOR_METADATA_KEY;
        let physical_floor_key = crate::retention::RETENTION_PHYSICAL_FLOOR_METADATA_KEY;
        let checkpoint_blob = checkpoint_blob.to_vec();
        let document_floor_blob = read_floors.document_versions.0.to_be_bytes().to_vec();
        let index_floor_blob = read_floors.index_versions.0.to_be_bytes().to_vec();
        let physical_floor_blob = read_floors.journal.0.to_be_bytes().to_vec();
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(
                query,
                (
                    checkpoint_key,
                    checkpoint_blob,
                    document_floor_key,
                    document_floor_blob,
                    index_floor_key,
                    index_floor_blob,
                    physical_floor_key,
                    physical_floor_blob,
                ),
            )
            .await
            .map_err(map_mysql_error)
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
            tenant_id: store.tenant_id.clone(),
            database_name,
            schema_cache: store.schema_cache.clone(),
            pipeline_metrics: store.pipeline_metrics.clone(),
            conn: Some(conn),
            commit_writes: Vec::new(),
            tenant_events: Vec::new(),
            prepared_record: None,
            durable_records_for_fault: Vec::new(),
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
        crate::sql::scheduler_core::sql_claim_due_jobs(self, now, max_jobs)
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

    pub(super) fn latest_sequence(&mut self) -> Result<SequenceNumber> {
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
        let id_source = self.provider.id_source.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            resolve_or_create_table_id_from_session(
                conn,
                &database_name,
                &table,
                id_source.as_ref(),
            )
            .await
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

impl crate::sql::scheduler_core::SqlSchedulerTransaction for MySqlWriteTransaction {
    fn select_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        let runtime_handle = self.provider.runtime_handle.clone();
        let database_name = self.database_name.clone();
        let max_jobs = u64::try_from(max_jobs).unwrap_or(u64::MAX);
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
        })
    }

    fn move_job_to_running(&mut self, job: &ScheduledJob) -> Result<()> {
        let delete_query = format!(
            "DELETE FROM {} WHERE id = ?",
            qualified_table(&self.database_name, "scheduled_jobs")
        );
        let insert_query = format!(
            "INSERT INTO {} (id, data_json) VALUES (?, ?)",
            qualified_table(&self.database_name, "running_scheduled_jobs")
        );
        let job_id = job.id.to_string();
        let data_json = serialize_json(job)?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(delete_query, (job_id.clone(),))
                .await
                .map_err(map_mysql_error)?;
            conn.exec_drop(insert_query, (job_id, data_json))
                .await
                .map_err(map_mysql_error)?;
            Ok(())
        })
    }

    fn load_running_jobs(&mut self) -> Result<Vec<ScheduledJob>> {
        self.load_running_jobs()
    }

    fn move_job_to_pending(&mut self, job: &ScheduledJob) -> Result<()> {
        let delete_query = format!(
            "DELETE FROM {} WHERE id = ?",
            qualified_table(&self.database_name, "running_scheduled_jobs")
        );
        let insert_query = format!(
            "INSERT INTO {} (id, run_at, data_json) VALUES (?, ?, ?)",
            qualified_table(&self.database_name, "scheduled_jobs")
        );
        let job_id = job.id.to_string();
        let run_at = job.run_at.0;
        let data_json = serialize_json(job)?;
        let runtime_handle = self.provider.runtime_handle.clone();
        let conn = self.session()?;
        Self::block_on(&runtime_handle, async move {
            conn.exec_drop(insert_query, (job_id.clone(), run_at, data_json))
                .await
                .map_err(map_mysql_error)?;
            conn.exec_drop(delete_query, (job_id,))
                .await
                .map_err(map_mysql_error)?;
            Ok(())
        })
    }
}

impl crate::sql::write_core::SqlWriteBackend for MySqlWriteTransaction {
    fn check_cancel(&self) -> Result<()> {
        MySqlWriteTransaction::check_cancel(self)
    }

    fn note_durable_records_for_fault(&mut self, records: &[TenantEventRecord]) {
        self.durable_records_for_fault = records.to_vec();
    }

    fn durable_records_for_fault(&self) -> &[TenantEventRecord] {
        &self.durable_records_for_fault
    }

    fn check_fault_for_records(
        &self,
        point: FaultPoint,
        records: &[TenantEventRecord],
    ) -> Result<()> {
        self.provider
            .fault_injector
            .check_for_tenant(point, &self.tenant_id, records)
    }

    fn commit_transaction(&mut self) -> Result<()> {
        MySqlWriteTransaction::batch_execute(self, "COMMIT")
    }

    fn rollback_transaction(&mut self) {
        let _ = MySqlWriteTransaction::batch_execute(self, "ROLLBACK");
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
        MySqlWriteTransaction::apply_durable_record(self, record)
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

impl SqlDurableJournalTransaction for MySqlWriteTransaction {
    fn applied_sequence(&mut self) -> Result<SequenceNumber> {
        MySqlWriteTransaction::applied_sequence(self)
    }

    fn load_durable_record(
        &mut self,
        sequence: SequenceNumber,
    ) -> Result<Option<TenantEventRecord>> {
        MySqlWriteTransaction::load_durable_record(self, sequence)
    }

    fn write_applied_sequence(&mut self, sequence: SequenceNumber) -> Result<()> {
        MySqlWriteTransaction::write_applied_sequence(self, sequence)
    }

    fn append_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()> {
        MySqlWriteTransaction::append_durable_records_batch(self, records)
    }

    fn apply_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()> {
        MySqlWriteTransaction::apply_durable_records_batch(self, records)
    }

    /// MySQL holds a single mutable connection operation at a time, so the
    /// journal insert and the apply are separate statements and pipeline
    /// progress is reported when the batch is admitted.
    fn append_and_apply_fenced_durable_batch(
        &mut self,
        records: &[TenantEventRecord],
        on_pipeline_progress: &mut dyn FnMut(),
    ) -> Result<()> {
        MySqlWriteTransaction::append_durable_records_batch_with_admission(
            self,
            records,
            on_pipeline_progress,
        )?;
        MySqlWriteTransaction::apply_durable_records_batch(self, records)
    }
}

fn is_retryable_mysql_begin_error(error: &Error) -> bool {
    matches!(
        error.storage_kind(),
        Some(StorageErrorKind::Busy | StorageErrorKind::Transient)
    )
}
