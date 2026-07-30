//! Dialect-shared write-transaction orchestration.
//!
//! The MySQL, PostgreSQL and libsql-replica write transactions run the same
//! commit/apply logic; only the SQL dialect and a few backend-specific concerns
//! (PostgreSQL `LISTEN/NOTIFY`, timestamp encoding, the libsql replica's
//! cache-refresh bookkeeping) differ. [`SqlWriteBackend`] captures the dialect
//! axes each backend must supply, and the free functions below implement the
//! orchestration that is functionally identical across all of them exactly once
//! against that seam.
//!
//! Storage atomicity is preserved: these functions only reorganize where the
//! shared logic lives. They perform no transaction control beyond what the
//! original per-backend methods did — `BEGIN` and the lock boundaries stay
//! entirely inside each backend's own `write.rs`, and the commit boundary is
//! reached only through [`SqlWriteBackend::commit_transaction`].

#[cfg(any(feature = "mysql", feature = "postgres"))]
use nimbus_core::SequenceNumber;
use nimbus_core::{
    CommitEntry, Document, DocumentId, DocumentLocator, Error, ResourcePathBinding, Result,
    TableId, TableName, TenantEventKind, TenantEventRecord, TriggerWriteOrigin, WriteOp,
    WriteOpType,
};

use crate::simulation::FaultPoint;
use crate::store::ResolvedWrite;

/// Dialect seam implemented by each SQL write transaction. Methods fall into
/// three groups: primitive buffer/state accessors, dialect statement execution
/// (`*_document_row`, `insert_document`, resource-path bindings), and lifecycle
/// hooks (`enqueue_notification`, `commit_transaction`, `after_visibility`).
///
/// Several methods share a name with an inherent method on the implementing
/// transaction type (for example `load_document`). Inside a trait `impl` those
/// bodies resolve to the inherent method (inherent methods take method-call
/// resolution priority over trait methods), so the forwarding impls are not
/// recursive; from the generic functions here the trait method is the only one
/// visible.
pub(crate) trait SqlWriteBackend {
    fn check_cancel(&self) -> Result<()>;
    fn check_fault(&self, point: FaultPoint) -> Result<()>;
    /// Reach the visibility boundary. PostgreSQL and MySQL issue `COMMIT` on
    /// the session; the libsql replica consumes its owned `Transaction`.
    fn commit_transaction(&mut self) -> Result<()>;
    /// Abandon the transaction. Runs only on failure paths, so it cannot report
    /// an error: a dead session already implies the transaction did not commit.
    fn rollback_transaction(&mut self);

    // Commit-write and tenant-event buffers.
    fn trigger_write_origin(&self) -> Option<TriggerWriteOrigin>;
    fn push_commit_write(&mut self, write: WriteOp);
    fn last_commit_write_mut(&mut self) -> Option<&mut WriteOp>;
    fn take_commit_writes(&mut self) -> Vec<WriteOp>;
    /// Append a tenant event. Reached only through [`sql_record_tenant_event`],
    /// whose callers (the schema-event helpers and the PostgreSQL/MySQL write
    /// transactions) are PostgreSQL/MySQL-only, so the method is gated with
    /// them. libsql buffers its events through
    /// [`SqlWriteBackend::prepend_tenant_event`] on the shared commit path.
    #[cfg(any(feature = "mysql", feature = "postgres"))]
    fn push_tenant_event(&mut self, event: TenantEventKind);
    fn prepend_tenant_event(&mut self, event: TenantEventKind);
    fn tenant_events_is_empty(&self) -> bool;
    fn take_tenant_events(&mut self) -> Vec<TenantEventKind>;
    fn take_prepared_record(&mut self) -> Option<TenantEventRecord>;

    /// Materialize one durable journal record into the tenant's tables.
    fn apply_durable_record(&mut self, record: &TenantEventRecord) -> Result<()>;

    // Commit-entry append, notification, schema-cache invalidation.
    fn append_commit_entry(
        &mut self,
        writes: Vec<WriteOp>,
        events: Vec<TenantEventKind>,
    ) -> Result<CommitEntry>;
    fn append_prepared_record(&mut self, record: &TenantEventRecord) -> Result<CommitEntry>;
    /// PostgreSQL enqueues a `LISTEN/NOTIFY` payload here. Backends with no
    /// notification channel (MySQL, the libsql replica) keep the default.
    fn enqueue_notification(&mut self) -> Result<()> {
        Ok(())
    }
    /// Whether this transaction changed schema that a process-local cache
    /// mirrors. The libsql replica has no such cache and keeps the default;
    /// its cache barrier is recorded in [`SqlWriteBackend::after_visibility`].
    fn schema_cache_changed(&self) -> bool {
        false
    }
    fn invalidate_schema_cache(&self) {}
    /// Local bookkeeping once the commit is durable, run after the
    /// after-visibility fault point so an injected crash there skips it. The
    /// libsql replica records its cache-refresh barrier here.
    ///
    /// This runs after the visibility boundary, so it must never read back from
    /// storage: a libsql Hrana session can serve a post-commit read from an
    /// older snapshot (see the `progress_after_successful_durable_apply` note in
    /// `nimbus-engine`'s `tenant/committer_lease.rs`). Local state only.
    fn after_visibility(&mut self, _commit: Option<&CommitEntry>) {}

    // Document/table statement execution for `apply_resolved_write`.
    fn load_document(&mut self, table: &TableName, id: &DocumentId) -> Result<Option<Document>>;
    fn load_table_id(&mut self, table: &TableName) -> Result<Option<TableId>>;
    fn insert_document(&mut self, document: &Document) -> Result<()>;
    /// Overwrite the stored row for `current` (dialect UPDATE statement).
    fn update_document_row(&mut self, table_id: &TableId, current: &Document) -> Result<()>;
    /// Delete the stored row for `id` (dialect DELETE statement).
    fn delete_document_row(&mut self, table_id: &TableId, id: &DocumentId) -> Result<()>;
    fn upsert_resource_path_binding(&mut self, binding: &ResourcePathBinding) -> Result<()>;
    fn remove_resource_path_binding(
        &mut self,
        locator: &DocumentLocator,
    ) -> Result<Option<ResourcePathBinding>>;
}

/// Durable-journal seam for backends that replay the journal *through a write
/// transaction*: PostgreSQL and MySQL append, apply and advance the watermark
/// on the same session that later commits.
///
/// The libsql replica does not implement this. Its journal batches are issued
/// as dedicated remote round-trips against the primary — one `Immediate`
/// transaction per batch, opened and committed by the store — so it supplies
/// the store-level wrappers in [`crate::sql::store_core::SqlStoreCore`]
/// directly instead.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) trait SqlDurableJournalTransaction: SqlWriteBackend {
    fn applied_sequence(&mut self) -> Result<SequenceNumber>;
    fn load_durable_record(
        &mut self,
        sequence: SequenceNumber,
    ) -> Result<Option<TenantEventRecord>>;
    fn write_applied_sequence(&mut self, sequence: SequenceNumber) -> Result<()>;

    fn append_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()>;
    fn apply_durable_records_batch(&mut self, records: &[TenantEventRecord]) -> Result<()>;
    /// Append and apply one fenced durable batch.
    ///
    /// `on_pipeline_progress` is invoked at the provider's own pipeline
    /// accounting boundary, which differs by dialect and is deliberately not
    /// unified: PostgreSQL pipelines the append and the apply as one ordered
    /// pair and reports progress once both have completed, while MySQL issues
    /// them as separate statements and reports progress at batch admission.
    /// The fenced wrapper uses it only to decide whether an outer-boundary
    /// cancellation has already been accounted for by the provider.
    fn append_and_apply_fenced_durable_batch(
        &mut self,
        records: &[TenantEventRecord],
        on_pipeline_progress: &mut dyn FnMut(),
    ) -> Result<()>;
}

/// Attach the transaction's trigger-write origin (when the caller did not set
/// one) and buffer the write for the commit entry.
pub(crate) fn sql_record_commit_write<B: SqlWriteBackend>(backend: &mut B, mut write: WriteOp) {
    if write.trigger_write_origin.is_none() {
        write.trigger_write_origin = backend.trigger_write_origin();
    }
    backend.push_commit_write(write);
}

/// Buffer a tenant event for the commit entry.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) fn sql_record_tenant_event<B: SqlWriteBackend>(backend: &mut B, event: TenantEventKind) {
    backend.push_tenant_event(event);
}

/// Roll the transaction back, discarding buffered writes.
pub(crate) fn sql_rollback<B: SqlWriteBackend>(backend: &mut B) {
    backend.rollback_transaction();
}

/// Idempotently apply a contiguous batch of durable journal records, advancing
/// the applied-sequence watermark. Records at or below the current watermark are
/// skipped; a gap is a hard error.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) fn sql_apply_durable_records_batch<B: SqlDurableJournalTransaction>(
    backend: &mut B,
    records: &[TenantEventRecord],
) -> Result<()> {
    backend.check_cancel()?;
    if records.is_empty() {
        return Ok(());
    }

    let mut applied_head = backend.applied_sequence()?.0;
    for record in records {
        backend.check_cancel()?;
        if record.sequence.0 <= applied_head {
            let durable = backend.load_durable_record(record.sequence)?;
            crate::commit_log::ensure_applied_record_matches(record, durable.as_ref())?;
            continue;
        }
        if record.sequence.0 != applied_head.saturating_add(1) {
            return Err(Error::Internal(format!(
                "durable journal apply expected sequence {}, got {}",
                applied_head.saturating_add(1),
                record.sequence.0
            )));
        }
        backend.apply_durable_record(record)?;
        applied_head = record.sequence.0;
    }

    if applied_head >= records[0].sequence.0 {
        backend.write_applied_sequence(SequenceNumber(applied_head))?;
    }
    Ok(())
}

/// Apply a single resolved write (insert/update/delete) with optimistic
/// conflict detection against the row's pre-image, recording the resulting
/// commit write and any resource-path binding change. The row-level SQL is
/// delegated to the backend's dialect statements.
pub(crate) fn sql_apply_resolved_write<B: SqlWriteBackend>(
    backend: &mut B,
    write: &ResolvedWrite,
) -> Result<()> {
    match write {
        ResolvedWrite::Insert {
            document,
            resource_path_binding,
            ..
        } => {
            backend.check_cancel()?;
            if backend
                .load_document(&document.table, &document.id)?
                .is_some()
            {
                return Err(Error::conflict(format!(
                    "document {} changed before transaction commit",
                    document.id
                )));
            }
            backend.insert_document(document)?;
            if let Some(resource_path_binding) = resource_path_binding.as_ref() {
                if let Some(write) = backend.last_commit_write_mut() {
                    write.resource_path_binding = Some(resource_path_binding.clone());
                }
                backend.upsert_resource_path_binding(resource_path_binding)?;
            }
            Ok(())
        }
        ResolvedWrite::Update {
            previous,
            current,
            resource_path_binding,
            ..
        } => {
            backend.check_cancel()?;
            let existing =
                backend
                    .load_document(&current.table, &current.id)?
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
            let table_id = backend
                .load_table_id(&current.table)?
                .ok_or(Error::conflict(format!(
                    "document {} changed before transaction commit",
                    current.id
                )))?;
            backend.update_document_row(&table_id, current)?;
            sql_record_commit_write(
                backend,
                WriteOp {
                    table: current.table.clone(),
                    table_id,
                    op_type: WriteOpType::Update,
                    doc_id: current.id.clone(),
                    resource_path_binding: resource_path_binding.clone(),
                    trigger_write_origin: None,
                    previous: Some(previous.clone()),
                    current: Some(current.clone()),
                },
            );
            if let Some(resource_path_binding) = resource_path_binding.as_ref() {
                backend.upsert_resource_path_binding(resource_path_binding)?;
            }
            Ok(())
        }
        ResolvedWrite::Delete { previous, .. } => {
            backend.check_cancel()?;
            let existing = backend
                .load_document(&previous.table, &previous.id)?
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
            let table_id = backend
                .load_table_id(&previous.table)?
                .ok_or(Error::conflict(format!(
                    "document {} changed before transaction commit",
                    previous.id
                )))?;
            backend.delete_document_row(&table_id, &previous.id)?;
            let resource_path_binding = backend.remove_resource_path_binding(
                &DocumentLocator::new(previous.table.clone(), previous.id.clone()),
            )?;
            sql_record_commit_write(
                backend,
                WriteOp {
                    table: previous.table.clone(),
                    table_id,
                    op_type: WriteOpType::Delete,
                    doc_id: previous.id.clone(),
                    resource_path_binding,
                    trigger_write_origin: None,
                    previous: Some(previous.clone()),
                    current: None,
                },
            );
            Ok(())
        }
    }
}

/// Finalize the transaction: fold buffered document writes into a leading
/// `DocumentWrite` event, append the commit entry when anything changed, flush
/// the notification, then commit, invalidate the schema cache if touched, and
/// run the backend's post-visibility bookkeeping. Every error before the
/// visibility boundary explicitly rolls the transaction back. Consumes the
/// transaction so no further statements can run after commit.
pub(crate) fn sql_commit<B: SqlWriteBackend>(mut backend: B) -> Result<Option<CommitEntry>> {
    let before_visibility = (|| -> Result<Option<CommitEntry>> {
        backend.check_cancel()?;
        let writes = backend.take_commit_writes();
        if !writes.is_empty() {
            backend.prepend_tenant_event(TenantEventKind::DocumentWrite {
                writes: writes.clone(),
            });
        }
        let prepared_record = backend.take_prepared_record();
        let commit = if let Some(record) = prepared_record {
            let events = backend.take_tenant_events();
            crate::store::validate_prepared_record_shape(&record, &writes, &events)?;
            Some(backend.append_prepared_record(&record)?)
        } else if backend.tenant_events_is_empty() {
            None
        } else {
            let events = backend.take_tenant_events();
            Some(backend.append_commit_entry(writes, events)?)
        };
        backend.enqueue_notification()?;
        backend.check_fault(FaultPoint::StorageCommitBeforeVisibility)?;
        Ok(commit)
    })();
    let commit = match before_visibility {
        Ok(commit) => commit,
        Err(error) => {
            sql_rollback(&mut backend);
            return Err(error);
        }
    };
    if let Err(error) = backend.commit_transaction() {
        // A provider error at the commit boundary may be ambiguous to the
        // caller. Rollback cannot undo a transaction that landed, but it does
        // close any still-open transaction before the session is reused.
        sql_rollback(&mut backend);
        return Err(error);
    }
    if backend.schema_cache_changed() {
        backend.invalidate_schema_cache();
    }
    // The fault check precedes the hook: this point stands in for a crash
    // between visibility and return, which cannot have run any local
    // post-commit bookkeeping.
    //
    // It is deliberately unconditional. A `None` commit means the transaction
    // appended no commit entry, not that it changed nothing durable:
    // schedule-only execution units, trigger outcomes, and fenced durable
    // journal batches all reach here with `None` while having written rows the
    // caller must assume landed. Gating this check on `commit.is_some()` was
    // tried and reverted; see the Step 3 section of
    // docs/private/plans/proof/storage-unification/suc3/facade.md.
    backend.check_fault(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
    backend.after_visibility(commit.as_ref());
    Ok(commit)
}
