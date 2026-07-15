//! Dialect-shared write-transaction orchestration.
//!
//! The MySQL and PostgreSQL write transactions run the same commit/apply logic;
//! only the SQL dialect and a few backend-specific concerns (PostgreSQL
//! `LISTEN/NOTIFY`, timestamp encoding) differ. [`SqlWriteBackend`] captures the
//! dialect axes each backend must supply, and the free functions below implement
//! the six orchestration methods that are functionally identical across both
//! backends exactly once against that seam.
//!
//! Storage atomicity is preserved: these functions only reorganize where the
//! shared logic lives. They perform no transaction control beyond what the
//! original per-backend methods did — the `BEGIN`/`COMMIT`/lock boundaries stay
//! entirely inside each backend's own `write.rs`.

use nimbus_core::{
    CommitEntry, Document, DocumentId, DocumentLocator, Error, ResourcePathBinding, Result,
    SequenceNumber, TableId, TableName, TenantEventKind, TenantEventRecord, TriggerWriteOrigin,
    WriteOp, WriteOpType,
};

use crate::simulation::FaultPoint;
use crate::store::ResolvedWrite;

/// Dialect seam implemented by each SQL write transaction. Methods fall into
/// three groups: primitive buffer/state accessors, dialect statement execution
/// (`*_document_row`, `insert_document`, resource-path bindings), and lifecycle
/// hooks (`enqueue_notification`, `batch_execute`).
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
    fn batch_execute(&mut self, sql: &str) -> Result<()>;

    // Commit-write and tenant-event buffers.
    fn trigger_write_origin(&self) -> Option<TriggerWriteOrigin>;
    fn push_commit_write(&mut self, write: WriteOp);
    fn last_commit_write_mut(&mut self) -> Option<&mut WriteOp>;
    fn take_commit_writes(&mut self) -> Vec<WriteOp>;
    fn push_tenant_event(&mut self, event: TenantEventKind);
    fn prepend_tenant_event(&mut self, event: TenantEventKind);
    fn tenant_events_is_empty(&self) -> bool;
    fn take_tenant_events(&mut self) -> Vec<TenantEventKind>;

    // Durable-journal application.
    fn applied_sequence(&mut self) -> Result<SequenceNumber>;
    fn apply_durable_record(&mut self, record: &TenantEventRecord) -> Result<()>;
    fn write_applied_sequence(&mut self, sequence: SequenceNumber) -> Result<()>;

    // Commit-entry append, notification, schema-cache invalidation.
    fn append_commit_entry(
        &mut self,
        writes: Vec<WriteOp>,
        events: Vec<TenantEventKind>,
    ) -> Result<CommitEntry>;
    /// PostgreSQL enqueues a `LISTEN/NOTIFY` payload here; MySQL has no
    /// notification channel and implements this as a no-op.
    fn enqueue_notification(&mut self) -> Result<()>;
    fn schema_cache_changed(&self) -> bool;
    fn invalidate_schema_cache(&self);

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

/// Attach the transaction's trigger-write origin (when the caller did not set
/// one) and buffer the write for the commit entry.
pub(crate) fn sql_record_commit_write<B: SqlWriteBackend>(backend: &mut B, mut write: WriteOp) {
    if write.trigger_write_origin.is_none() {
        write.trigger_write_origin = backend.trigger_write_origin();
    }
    backend.push_commit_write(write);
}

/// Buffer a tenant event for the commit entry.
pub(crate) fn sql_record_tenant_event<B: SqlWriteBackend>(backend: &mut B, event: TenantEventKind) {
    backend.push_tenant_event(event);
}

/// Roll the transaction back, discarding buffered writes. Errors are ignored:
/// rollback runs on the failure path and a dead connection already implies the
/// transaction did not commit.
pub(crate) fn sql_rollback<B: SqlWriteBackend>(backend: &mut B) {
    let _ = backend.batch_execute("ROLLBACK");
}

/// Idempotently apply a contiguous batch of durable journal records, advancing
/// the applied-sequence watermark. Records at or below the current watermark are
/// skipped; a gap is a hard error.
pub(crate) fn sql_apply_durable_records_batch<B: SqlWriteBackend>(
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
/// the notification, then `COMMIT` and invalidate the schema cache if touched.
/// Consumes the transaction so no further statements can run after commit.
pub(crate) fn sql_commit<B: SqlWriteBackend>(mut backend: B) -> Result<Option<CommitEntry>> {
    backend.check_cancel()?;
    let writes = backend.take_commit_writes();
    if !writes.is_empty() {
        backend.prepend_tenant_event(TenantEventKind::DocumentWrite {
            writes: writes.clone(),
        });
    }
    let commit = if backend.tenant_events_is_empty() {
        None
    } else {
        let events = backend.take_tenant_events();
        Some(backend.append_commit_entry(writes, events)?)
    };
    backend.enqueue_notification()?;
    backend.check_fault(FaultPoint::StorageCommitBeforeVisibility)?;
    backend.batch_execute("COMMIT")?;
    if backend.schema_cache_changed() {
        backend.invalidate_schema_cache();
    }
    backend.check_fault(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
    Ok(commit)
}
