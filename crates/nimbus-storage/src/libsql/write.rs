use super::document_versions::{
    prune_document_versions_before_remote, record_document_versions_for_events_remote,
};
use super::index_versions::{
    prune_index_versions_before_remote, record_index_versions_for_events_remote,
};
use super::*;
use nimbus_core::ResourcePathBinding;

use crate::FaultPoint;
use crate::sql::store_core::{SqlStoreCore, SqlWriteTransactionCore, sql_store_core_facade};
use crate::sql::write_core::{SqlWriteBackend, sql_apply_resolved_write, sql_commit, sql_rollback};
use crate::{CommitterLeaseError, CommitterLeaseResult};

sql_store_core_facade!(LibsqlReplicaTenantStore);

impl LibsqlReplicaTenantStore {
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

/// Wire the libsql replica into the shared store-level wrapper layer. The write
/// bridge and the journal reads stay here; every wrapper built on them lives
/// once in [`crate::sql::store_core`].
///
/// The durable-journal batch methods are the replica's own: unlike PostgreSQL
/// and MySQL it does not replay through the write transaction opened above.
/// Each batch is a dedicated remote round-trip against the primary, after which
/// the local cache barrier is advanced.
impl SqlStoreCore for LibsqlReplicaTenantStore {
    type Transaction = LibsqlReplicaWriteTransaction;

    fn execute_write<T, F>(&self, task: F) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        F: FnOnce(&mut LibsqlReplicaWriteTransaction) -> Result<T> + Send + 'static,
    {
        LibsqlReplicaTenantStore::execute_write(self, task)
    }

    // Gated with the trait method it implements; see `SqlStoreCore`.
    #[cfg(any(feature = "mysql", feature = "postgres"))]
    fn execute_write_cancellable<T, Check, F>(
        &self,
        check_cancel: Check,
        task: F,
    ) -> Result<TenantWriteCommit<T>>
    where
        T: Send + 'static,
        Check: Fn() -> Result<()> + Send + 'static,
        F: FnOnce(&mut LibsqlReplicaWriteTransaction) -> Result<T> + Send + 'static,
    {
        LibsqlReplicaTenantStore::execute_write_cancellable(self, check_cancel, task)
    }

    fn retention_floor(&self) -> &RetentionFloor {
        self.retention_floor.as_ref()
    }

    fn journal_progress(&self) -> Result<JournalProgress> {
        LibsqlReplicaTenantStore::journal_progress(self)
    }

    fn load_retention_metadata_snapshot(&self) -> Result<(Option<Vec<u8>>, SequenceNumber)> {
        let loaded = self.execute_write(|transaction| transaction.load_retention_metadata())?;
        debug_assert!(loaded.commit.is_none());
        Ok(loaded.value)
    }

    fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        LibsqlReplicaTenantStore::read_durable_journal_from(self, sequence)
    }

    fn recover_durable_journal(&self) -> Result<JournalProgress> {
        LibsqlReplicaTenantStore::recover_durable_journal(self)
    }

    fn export_materialized_journal_snapshot(&self) -> Result<MaterializedJournalSnapshot> {
        LibsqlReplicaTenantStore::export_materialized_journal_snapshot(self)
    }

    fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }
        let records = records.to_vec();
        self.block_on(self.append_remote_durable_records_batch(records.as_slice()))?;
        Ok(())
    }

    fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        self.apply_remote_batch(records, DurableApplyKind::ClientBatch)
    }

    fn replay_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        self.apply_remote_batch(records, DurableApplyKind::JournalReplay)
    }

    /// The remote batch is a single round-trip, so cancellation is observed
    /// once before it is issued rather than between records.
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
        check_cancel().map_err(CommitterLeaseError::from)?;
        let fenced_owner_id = owner_id.to_string();
        let result = self.block_on(self.fenced_append_and_apply_remote_durable_records_batch(
            owner_id,
            epoch,
            expected_previous,
            records,
        ));
        let applied_head = match result {
            Ok(applied_head) => applied_head,
            Err(Error::PreconditionFailed(message)) if message == FENCED_COMMITTER_LEASE_MARKER => {
                return Err(CommitterLeaseError::Fenced {
                    owner_id: fenced_owner_id,
                    epoch,
                });
            }
            Err(error) => return Err(error.into()),
        };
        self.note_required_cache_sequence_with_cause(
            applied_head,
            LibsqlReplicaRefreshCause::DurableJournalReplay,
        );
        Ok(())
    }
}

/// Transaction-side seam for the shared wrappers. Each method forwards to the
/// inherent method of the same name, which wins method-call resolution.
impl SqlWriteTransactionCore for LibsqlReplicaWriteTransaction {
    fn begin_scheduled_execution(&mut self, execution_id: Option<&str>) -> Result<bool> {
        LibsqlReplicaWriteTransaction::begin_scheduled_execution(self, execution_id)
    }

    fn set_prepared_record(&mut self, record: TenantEventRecord) {
        LibsqlReplicaWriteTransaction::set_prepared_record(self, record)
    }

    fn set_trigger_write_origin(&mut self, trigger_write_origin: Option<TriggerWriteOrigin>) {
        LibsqlReplicaWriteTransaction::set_trigger_write_origin(self, trigger_write_origin)
    }

    fn set_commit_timestamp(&mut self, commit_timestamp: Option<Timestamp>) {
        LibsqlReplicaWriteTransaction::set_commit_timestamp(self, commit_timestamp)
    }

    fn advance_fenced_committer_lease(
        &mut self,
        owner_id: &str,
        epoch: u64,
        expected_previous: SequenceNumber,
        durable_sequence: SequenceNumber,
    ) -> Result<u64> {
        LibsqlReplicaWriteTransaction::advance_fenced_committer_lease(
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
        LibsqlReplicaWriteTransaction::validate_fenced_committer_lease(
            self,
            owner_id,
            epoch,
            durable_sequence,
        )
    }

    fn materialize_trigger_invocations(
        &mut self,
        records: &[nimbus_core::TriggerInvocationRecord],
        cursor: nimbus_core::TriggerDeliveryCursor,
    ) -> Result<()> {
        LibsqlReplicaWriteTransaction::materialize_trigger_invocations(self, records, cursor)
    }

    fn save_trigger_invocation(
        &mut self,
        record: &nimbus_core::TriggerInvocationRecord,
    ) -> Result<()> {
        LibsqlReplicaWriteTransaction::save_trigger_invocation(self, record)
    }

    fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        LibsqlReplicaWriteTransaction::replace_table_schema(self, table_schema)
    }

    fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        LibsqlReplicaWriteTransaction::delete_table_schema(self, table)
    }

    fn insert_scheduled_job(&mut self, job: &ScheduledJob) -> Result<()> {
        LibsqlReplicaWriteTransaction::insert_scheduled_job(self, job)
    }

    fn claim_due_jobs(&mut self, now: Timestamp, max_jobs: usize) -> Result<Vec<ScheduledJob>> {
        LibsqlReplicaWriteTransaction::claim_due_jobs(self, now, max_jobs)
    }

    fn complete_scheduled_job(&mut self, job_id: &DocumentId) -> Result<()> {
        LibsqlReplicaWriteTransaction::complete_scheduled_job(self, job_id)
    }

    fn cancel_scheduled_job(&mut self, job_id: &DocumentId) -> Result<bool> {
        LibsqlReplicaWriteTransaction::cancel_scheduled_job(self, job_id)
    }

    fn record_scheduled_job_result(&mut self, result: &ScheduledJobResult) -> Result<()> {
        LibsqlReplicaWriteTransaction::record_scheduled_job_result(self, result)
    }

    fn save_cron_job(&mut self, cron: &CronJob) -> Result<()> {
        LibsqlReplicaWriteTransaction::save_cron_job(self, cron)
    }

    fn delete_cron_job(&mut self, name: &str) -> Result<()> {
        LibsqlReplicaWriteTransaction::delete_cron_job(self, name)
    }

    fn recover_running_jobs(&mut self, now: Timestamp) -> Result<()> {
        LibsqlReplicaWriteTransaction::recover_running_jobs(self, now)
    }

    fn apply_resolved_write(&mut self, write: &ResolvedWrite) -> Result<()> {
        sql_apply_resolved_write(self, write)
    }

    fn update_document_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, serde_json::Value>,
        validate: F,
    ) -> Result<()>
    where
        F: FnOnce(&Document, &Document) -> Result<()> + Send + 'static,
    {
        LibsqlReplicaWriteTransaction::update_document_validated(self, table, id, patch, validate)
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
        LibsqlReplicaWriteTransaction::delete_document_validated(self, table, id, validate)
    }

    fn prune_retained_versions(
        &mut self,
        document_prune_before: SequenceNumber,
        index_prune_before: SequenceNumber,
    ) -> Result<(u64, u64)> {
        LibsqlReplicaWriteTransaction::prune_retained_versions(
            self,
            document_prune_before,
            index_prune_before,
        )
    }

    fn load_retention_metadata(&mut self) -> Result<(Option<Vec<u8>>, SequenceNumber)> {
        LibsqlReplicaWriteTransaction::load_retention_metadata(self)
    }

    fn applied_sequence_for_retention(&mut self) -> Result<SequenceNumber> {
        LibsqlReplicaWriteTransaction::applied_sequence_for_retention(self)
    }

    fn prune_durable_journal_through(&mut self, sequence: SequenceNumber) -> Result<u64> {
        LibsqlReplicaWriteTransaction::prune_durable_journal_through(self, sequence)
    }

    fn store_retention_metadata(
        &mut self,
        checkpoint_blob: &[u8],
        physical_floor: SequenceNumber,
    ) -> Result<()> {
        LibsqlReplicaWriteTransaction::store_retention_metadata(
            self,
            checkpoint_blob,
            physical_floor,
        )
    }
}

impl SqlWriteBackend for LibsqlReplicaWriteTransaction {
    fn check_cancel(&self) -> Result<()> {
        LibsqlReplicaWriteTransaction::check_cancel(self)
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
        self.store.check_durable_records_fault(point, records)
    }

    fn commit_transaction(&mut self) -> Result<()> {
        let tx = self.tx.take().ok_or_else(|| {
            Error::Internal("libsql replica write transaction already closed".to_string())
        })?;
        self.store
            .block_on(async move { tx.commit().await.map_err(map_libsql_error) })
    }

    fn rollback_transaction(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = self
                .store
                .block_on(async move { tx.rollback().await.map_err(map_libsql_error) });
        }
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

    // Gated with the trait method it implements; see `SqlWriteBackend`.
    #[cfg(any(feature = "mysql", feature = "postgres"))]
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
        self.store
            .block_on(super::backend::apply_durable_record_in_remote_conn(
                self.session()?,
                record,
            ))
    }

    fn append_commit_entry(
        &mut self,
        writes: Vec<WriteOp>,
        events: Vec<TenantEventKind>,
    ) -> Result<CommitEntry> {
        LibsqlReplicaWriteTransaction::append_commit_entry(self, writes, events)
    }

    fn append_prepared_record(&mut self, record: &TenantEventRecord) -> Result<CommitEntry> {
        LibsqlReplicaWriteTransaction::append_prepared_record(self, record)
    }

    /// Record the cache-refresh barrier the next replica read must clear. This
    /// is local state only: reading back from the remote session here could
    /// observe an older Hrana snapshot than the commit just made durable.
    fn after_visibility(&mut self, commit: Option<&CommitEntry>) {
        match (commit, self.refresh_cache_after_commit) {
            (Some(commit), true) => {
                self.store.refresh_needed.store(true, Ordering::Release);
                self.store.note_required_cache_sequence_with_cause(
                    commit.sequence,
                    LibsqlReplicaRefreshCause::SchemaWrite,
                );
            }
            (Some(commit), false) => {
                self.store.note_required_cache_sequence_with_cause(
                    commit.sequence,
                    LibsqlReplicaRefreshCause::CommitBarrier,
                );
            }
            (None, true) => {
                self.store.refresh_needed.store(true, Ordering::Release);
                self.store
                    .freshness_metrics
                    .note_refresh_request(LibsqlReplicaRefreshCause::SchemaWrite);
                self.store.schedule_background_refresh();
            }
            (None, false) => {}
        }
    }

    fn load_document(&mut self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
        LibsqlReplicaWriteTransaction::load_document(self, table, id)
    }

    fn load_table_id(&mut self, table: &TableName) -> Result<Option<TableId>> {
        self.store
            .block_on(load_remote_table_id_from_session(self.session()?, table))
    }

    fn insert_document(&mut self, document: &Document) -> Result<()> {
        LibsqlReplicaWriteTransaction::insert_document(self, document)
    }

    fn update_document_row(&mut self, table_id: &TableId, current: &Document) -> Result<()> {
        let data_json = serialize_document_fields(current)?;
        let typed_fields_json = serialize_document_typed_fields(current)?;
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
        })
    }

    fn delete_document_row(&mut self, table_id: &TableId, id: &DocumentId) -> Result<()> {
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM documents WHERE table_id = ?1 AND id = ?2",
                    libsql::params![table_id.as_str(), id.to_string()],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })
    }

    fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()> {
        LibsqlReplicaWriteTransaction::upsert_resource_path_binding(self, binding)
    }

    fn remove_resource_path_binding(
        &mut self,
        locator: &nimbus_core::DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>> {
        LibsqlReplicaWriteTransaction::remove_resource_path_binding(self, locator)
    }
}

impl LibsqlReplicaWriteTransaction {
    fn begin<Check>(store: LibsqlReplicaTenantStore, check_cancel: Check) -> Result<Self>
    where
        Check: Fn() -> Result<()> + Send + 'static,
    {
        let conn = store.remote_write_connection()?;
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
            durable_records_for_fault: Vec::new(),
            trigger_write_origin: None,
            commit_timestamp: None,
            check_cancel: Box::new(check_cancel),
            refresh_cache_after_commit: false,
        })
    }

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
        self.check_cancel()?;
        let epoch = i64::try_from(epoch)
            .map_err(|_| Error::InvalidInput("lease epoch exceeds INTEGER".to_string()))?;
        let expected_previous = i64_from_u64(expected_previous.0)?;
        let durable_sequence = i64_from_u64(durable_sequence.0)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "UPDATE committer_lease
                     SET durable_sequence = ?4
                     WHERE singleton = 1 AND owner_id = ?1 AND epoch = ?2
                           AND expires_at >
                               CAST(unixepoch('subsec') * 1000 AS INTEGER)
                           AND durable_sequence = ?3",
                    libsql::params![owner_id, epoch, expected_previous, durable_sequence],
                )
                .await
                .map_err(map_libsql_error)
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

    fn load_retention_metadata(&mut self) -> Result<(Option<Vec<u8>>, SequenceNumber)> {
        self.check_cancel()?;
        let checkpoint_key = crate::retention::RETENTION_CHECKPOINT_METADATA_KEY;
        let physical_floor_key = crate::retention::RETENTION_PHYSICAL_FLOOR_METADATA_KEY;
        self.store.block_on(async {
            let checkpoint_blob =
                load_remote_metadata_blob(self.session()?, checkpoint_key).await?;
            let physical_floor = load_remote_metadata_blob(self.session()?, physical_floor_key)
                .await?
                .map(|value| crate::retention::decode_retention_floor(value.as_slice()))
                .transpose()?
                .unwrap_or(SequenceNumber(0));
            Ok((checkpoint_blob, physical_floor))
        })
    }

    fn applied_sequence_for_retention(&mut self) -> Result<SequenceNumber> {
        self.check_cancel()?;
        self.store.block_on(async {
            Ok(
                load_remote_metadata_u64(self.session()?, APPLIED_SEQUENCE_KEY)
                    .await?
                    .map(SequenceNumber)
                    .unwrap_or(SequenceNumber(0)),
            )
        })
    }

    fn prune_durable_journal_through(&mut self, sequence: SequenceNumber) -> Result<u64> {
        self.check_cancel()?;
        let sequence = i64_from_u64(sequence.0)?;
        self.store.block_on(async {
            self.session()?
                .execute(
                    "DELETE FROM commit_log WHERE sequence <= ?1",
                    libsql::params![sequence],
                )
                .await
                .map_err(map_libsql_error)
        })
    }

    fn store_retention_metadata(
        &mut self,
        checkpoint_blob: &[u8],
        physical_floor: SequenceNumber,
    ) -> Result<()> {
        self.check_cancel()?;
        let checkpoint_key = crate::retention::RETENTION_CHECKPOINT_METADATA_KEY;
        let physical_floor_key = crate::retention::RETENTION_PHYSICAL_FLOOR_METADATA_KEY;
        let checkpoint_blob = checkpoint_blob.to_vec();
        let physical_floor_blob = physical_floor.0.to_be_bytes().to_vec();
        self.store.block_on(async {
            self.session()?
                .execute(
                    "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2), (?3, ?4) \
                     ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
                    libsql::params![
                        checkpoint_key,
                        checkpoint_blob,
                        physical_floor_key,
                        physical_floor_blob
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        })?;
        self.refresh_cache_after_commit = true;
        Ok(())
    }

    pub fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        let mut table_schema = table_schema.clone();
        let mut recorded_event: Option<(TableId, Option<TableSchema>, TableSchema)> = None;
        let id_source = self.store.provider.id_source.clone();
        self.store.block_on(async {
            let previous =
                load_remote_table_schema_from_session(self.session()?, &table_schema.table).await?;
            table_schema.reconcile_index_metadata(previous.as_ref());
            let schema_json = serialize_json(&table_schema)?;
            let table_id = resolve_or_create_remote_table_id(
                self.session()?,
                &table_schema.table,
                id_source.as_ref(),
            )
            .await?;
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
        let id_source = self.store.provider.id_source.clone();
        let table_id = self.store.block_on(async {
            resolve_or_create_remote_table_id(self.session()?, &document.table, id_source.as_ref())
                .await
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

    pub fn commit(self) -> Result<Option<CommitEntry>> {
        sql_commit(self)
    }

    pub fn rollback(&mut self) {
        sql_rollback(self)
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
            applied_head
                .map(SequenceNumber)
                .unwrap_or(SequenceNumber(0)),
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
        self.check_journal_append_faults()?;
        Ok(entry)
    }

    /// The replica's journal append and its flush to the primary are the same
    /// statement batch, so both journal fault points are observed here — the
    /// only place a write transaction produces a commit entry. The append point
    /// is records-scoped when this commit carries a prepared record; the flush
    /// point stays tenant-scoped, matching every other dialect.
    fn check_journal_append_faults(&self) -> Result<()> {
        self.check_fault(FaultPoint::JournalAppendBeforeDurableFlush)?;
        self.store
            .check_fault(FaultPoint::JournalFlushBeforeVisibility)
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
            applied_head
                .map(SequenceNumber)
                .unwrap_or(SequenceNumber(0)),
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
        self.check_journal_append_faults()?;
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
    let rows = conn
        .query(
            "SELECT schema_json FROM schemas WHERE table_name = ?1",
            libsql::params![table.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = take_single_remote_row(rows).await? else {
        return Ok(None);
    };
    deserialize_json(row.get::<String>(0).map_err(map_libsql_error)?.as_str()).map(Some)
}
