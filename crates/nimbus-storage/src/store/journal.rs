use nimbus_core::{
    CommitEntry, Error, Result, SchemaChangeEvent, SequenceNumber, TableLifecycleEvent,
    TenantEventKind, TenantEventRecord, Timestamp, WriteOp,
};
use redb::{ReadableTable, TableError};

use crate::document_codec::{decode_document_msgpack, encode_document_msgpack};
use crate::index::table_index_prefix;
use crate::keys::{document_key, prefix_end, table_prefix};
use crate::simulation::{FaultInjector, FaultPoint};

#[cfg(test)]
mod tests;

use super::document_versions::record_document_versions_for_writes;
use super::index_versions::record_index_versions_for_writes;
use super::schema_rewrite::{
    durable_record_index_keys_for_table_id, rewrite_document_indexes_in_write_txn,
};
use super::table_catalog::{
    activate_hidden_table_identity_in_write_txn, ensure_default_table_id_in_write_txn,
    hard_delete_deleting_table_identity_in_write_txn, mark_default_table_deleting_in_write_txn,
    resolve_table_id_in_write_txn, stage_hidden_table_identity_in_write_txn,
};
use super::{
    APPLIED_SEQUENCE_KEY, COMMIT_LOG, DOCUMENTS, EMPTY_TABLE_VALUE, INDEXES, JournalProgress,
    METADATA, NEXT_SEQUENCE_KEY, SCHEDULED_JOB_EXECUTIONS, SCHEMAS, TRIGGER_DELIVERY_CURSOR_KEY,
    TenantStore, map_redb_error,
};

impl TenantStore {
    pub fn read_commit_log_from(&self, sequence: SequenceNumber) -> Result<Vec<CommitEntry>> {
        Ok(self
            .read_durable_journal_from(sequence)?
            .into_iter()
            .map(|record| record.as_commit_entry())
            .collect())
    }

    pub fn read_durable_journal_from(
        &self,
        sequence: SequenceNumber,
    ) -> Result<Vec<TenantEventRecord>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table_handle = match read_txn.open_table(COMMIT_LOG) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut entries = Vec::new();
        for item in table_handle.range(sequence.0..).map_err(map_redb_error)? {
            let (_, value) = item.map_err(map_redb_error)?;
            entries.push(crate::commit_log::deserialize_tenant_event_record(
                value.value(),
            )?);
        }
        Ok(entries)
    }

    pub fn append_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut log = write_txn.open_table(COMMIT_LOG).map_err(map_redb_error)?;
            let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
            let mut next = match metadata.get(NEXT_SEQUENCE_KEY).map_err(map_redb_error)? {
                Some(value) => decode_u64(value.value())?,
                None => 1,
            };

            for record in records {
                if record.sequence.0 != next {
                    return Err(Error::Internal(format!(
                        "durable journal append expected sequence {}, got {}",
                        next, record.sequence.0
                    )));
                }
                let payload = crate::commit_log::serialize_tenant_event_record(record)?;
                log.insert(next, payload.as_slice())
                    .map_err(map_redb_error)?;
                next = next.saturating_add(1);
            }

            metadata
                .insert(NEXT_SEQUENCE_KEY, encode_u64(next).as_slice())
                .map_err(map_redb_error)?;
        }

        commit_journal_txn(&*self.fault_injector, write_txn)?;
        Ok(())
    }

    pub fn apply_durable_records_batch(&self, records: &[TenantEventRecord]) -> Result<()> {
        if records.is_empty() {
            return Ok(());
        }

        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let mut applied_head = self.applied_sequence()?.0;
        for record in records {
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
            apply_durable_record_in_write_txn(&write_txn, record)?;
            applied_head = record.sequence.0;
        }

        if applied_head >= records[0].sequence.0 {
            write_applied_sequence(&write_txn, SequenceNumber(applied_head))?;
        }
        self.commit_write_txn(write_txn)?;
        Ok(())
    }

    pub fn recover_durable_journal(&self) -> Result<JournalProgress> {
        let progress = self.journal_progress()?;
        if progress.applied_head.0 >= progress.durable_head.0 {
            return Ok(progress);
        }
        let from = SequenceNumber(progress.applied_head.0.saturating_add(1));
        let pending = self.read_durable_journal_from(from)?;
        self.apply_durable_records_batch(&pending)?;
        self.journal_progress()
    }
}

pub(super) fn append_commit(
    write_txn: &redb::WriteTransaction,
    timestamp: Timestamp,
    writes: Vec<WriteOp>,
    events: Vec<TenantEventKind>,
) -> Result<CommitEntry> {
    let sequence = next_sequence(write_txn)?;
    let record = append_tenant_event(write_txn, SequenceNumber(sequence), timestamp, events)?;

    Ok(CommitEntry {
        sequence: record.sequence,
        timestamp: record.timestamp,
        writes,
    })
}

pub(super) fn append_tenant_event(
    write_txn: &redb::WriteTransaction,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    events: Vec<TenantEventKind>,
) -> Result<TenantEventRecord> {
    let record = TenantEventRecord::from_events(sequence, timestamp, events)?;
    record_document_versions_for_events(
        write_txn,
        record.sequence,
        record.timestamp,
        &record.events,
    )?;
    record_index_versions_for_events(write_txn, record.sequence, &record.events)?;
    let mut log = write_txn.open_table(COMMIT_LOG).map_err(map_redb_error)?;
    let payload = crate::commit_log::serialize_tenant_event_record(&record)?;
    log.insert(sequence.0, payload.as_slice())
        .map_err(map_redb_error)?;
    write_next_sequence(write_txn, sequence.0.saturating_add(1))?;
    write_applied_sequence(write_txn, sequence)?;
    Ok(record)
}

fn apply_durable_record_in_write_txn(
    write_txn: &redb::WriteTransaction,
    record: &TenantEventRecord,
) -> Result<()> {
    if record.events.is_empty() {
        if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
            let _ = begin_scheduled_execution(write_txn, Some(execution_id))?;
        }
        record_document_versions_for_writes(
            write_txn,
            record.sequence,
            record.timestamp,
            &record.writes,
        )?;
        record_index_versions_for_writes(write_txn, record.sequence, &record.writes)?;
        return apply_document_writes_in_write_txn(write_txn, &record.writes);
    }

    record_document_versions_for_events(
        write_txn,
        record.sequence,
        record.timestamp,
        &record.events,
    )?;
    record_index_versions_for_events(write_txn, record.sequence, &record.events)?;
    for event in &record.events {
        apply_tenant_event_in_write_txn(write_txn, event)?;
    }
    Ok(())
}

fn record_document_versions_for_events(
    write_txn: &redb::WriteTransaction,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    events: &[TenantEventKind],
) -> Result<()> {
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_document_versions_for_writes(write_txn, sequence, timestamp, writes)?;
        }
    }
    Ok(())
}

fn record_index_versions_for_events(
    write_txn: &redb::WriteTransaction,
    sequence: SequenceNumber,
    events: &[TenantEventKind],
) -> Result<()> {
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_index_versions_for_writes(write_txn, sequence, writes)?;
        }
    }
    Ok(())
}

fn apply_tenant_event_in_write_txn(
    write_txn: &redb::WriteTransaction,
    event: &TenantEventKind,
) -> Result<()> {
    match event {
        TenantEventKind::DocumentWrite { writes } => {
            apply_document_writes_in_write_txn(write_txn, writes)
        }
        TenantEventKind::SchemaChange { change } => {
            apply_schema_change_in_write_txn(write_txn, change)
        }
        TenantEventKind::TableLifecycle { lifecycle } => {
            apply_table_lifecycle_in_write_txn(write_txn, lifecycle)
        }
        TenantEventKind::IndexLifecycle { .. } | TenantEventKind::Barrier { .. } => Ok(()),
        TenantEventKind::ScheduledExecution { execution_id } => {
            let _ = begin_scheduled_execution(write_txn, Some(execution_id))?;
            Ok(())
        }
        TenantEventKind::TriggerDelivery { cursor } => {
            let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
            metadata
                .insert(
                    TRIGGER_DELIVERY_CURSOR_KEY,
                    encode_u64(cursor.materialized_through.0).as_slice(),
                )
                .map_err(map_redb_error)?;
            Ok(())
        }
    }
}

fn apply_document_writes_in_write_txn(
    write_txn: &redb::WriteTransaction,
    writes: &[WriteOp],
) -> Result<()> {
    let mut documents = write_txn.open_table(DOCUMENTS).map_err(map_redb_error)?;
    for write in writes {
        apply_document_write_in_write_txn(write_txn, &mut documents, write)?;
    }
    Ok(())
}

fn apply_document_write_in_write_txn(
    write_txn: &redb::WriteTransaction,
    documents: &mut redb::Table<'_, &[u8], &[u8]>,
    write: &WriteOp,
) -> Result<()> {
    match (&write.previous, &write.current) {
        (None, Some(current)) => {
            ensure_default_table_id_in_write_txn(write_txn, &write.table, &write.table_id)?;
            let key = document_key(&write.table_id, &write.doc_id);
            let already_applied = {
                let existing = documents.get(key.as_slice()).map_err(map_redb_error)?;
                if let Some(existing) = existing {
                    let existing = decode_document_msgpack(existing.value())
                        .map_err(|error| Error::Serialization(error.to_string()))?;
                    if existing != *current {
                        return Err(Error::conflict(format!(
                            "durable journal insert replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    true
                } else {
                    false
                }
            };
            if !already_applied {
                let payload = encode_document_msgpack(current)
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                documents
                    .insert(key.as_slice(), payload.as_slice())
                    .map_err(map_redb_error)?;
            }
        }
        (Some(previous), Some(current)) => {
            ensure_default_table_id_in_write_txn(write_txn, &write.table, &write.table_id)?;
            let key = document_key(&write.table_id, &write.doc_id);
            let existing = {
                let existing = documents
                    .get(key.as_slice())
                    .map_err(map_redb_error)?
                    .ok_or(Error::conflict(format!(
                        "durable journal update replay missing document {}",
                        write.doc_id
                    )))?;
                decode_document_msgpack(existing.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?
            };
            if existing == *current {
                return Ok(());
            }
            if existing != *previous {
                return Err(Error::conflict(format!(
                    "durable journal update replay found conflicting state for document {}",
                    write.doc_id
                )));
            }
            let payload = encode_document_msgpack(current)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            documents
                .insert(key.as_slice(), payload.as_slice())
                .map_err(map_redb_error)?;
        }
        (Some(previous), None) => {
            ensure_default_table_id_in_write_txn(write_txn, &write.table, &write.table_id)?;
            let key = document_key(&write.table_id, &write.doc_id);
            match documents.remove(key.as_slice()).map_err(map_redb_error)? {
                Some(removed) => {
                    let removed = decode_document_msgpack(removed.value())
                        .map_err(|error| Error::Serialization(error.to_string()))?;
                    if removed != *previous {
                        return Err(Error::conflict(format!(
                            "durable journal delete replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                }
                None => return Ok(()),
            }
        }
        (None, None) => {
            return Err(Error::Internal(
                "durable journal write must include a previous or current document".to_string(),
            ));
        }
    }

    rewrite_document_indexes_in_write_txn(
        write_txn,
        write.previous.as_ref(),
        write.current.as_ref(),
    )?;
    Ok(())
}

fn apply_schema_change_in_write_txn(
    write_txn: &redb::WriteTransaction,
    change: &SchemaChangeEvent,
) -> Result<()> {
    match change {
        SchemaChangeEvent::SetTable {
            table,
            table_id,
            current,
            ..
        } => {
            ensure_default_table_id_in_write_txn(write_txn, table, table_id)?;
            let payload = rmp_serde::to_vec(current)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            {
                let mut schemas = write_txn.open_table(SCHEMAS).map_err(map_redb_error)?;
                schemas
                    .insert(table.as_str(), payload.as_slice())
                    .map_err(map_redb_error)?;
            }
            rebuild_indexes_for_table_schema_in_write_txn(write_txn, current, table_id)
        }
        SchemaChangeEvent::DeleteTable {
            table, table_id, ..
        } => {
            if let Some(table_id) = table_id {
                remove_indexes_for_table_id(write_txn, table_id)?;
            }
            let mut schemas = match write_txn.open_table(SCHEMAS) {
                Ok(schemas) => schemas,
                Err(TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(error) => return Err(map_redb_error(error)),
            };
            schemas.remove(table.as_str()).map_err(map_redb_error)?;
            Ok(())
        }
    }
}

fn apply_table_lifecycle_in_write_txn(
    write_txn: &redb::WriteTransaction,
    lifecycle: &TableLifecycleEvent,
) -> Result<()> {
    match lifecycle {
        TableLifecycleEvent::StageHidden { table, table_id } => {
            stage_hidden_table_identity_in_write_txn(write_txn, table, table_id)
        }
        TableLifecycleEvent::ActivateHidden {
            table, table_id, ..
        } => {
            let _ = activate_hidden_table_identity_in_write_txn(write_txn, table, table_id)?;
            Ok(())
        }
        TableLifecycleEvent::MarkDeleting { table, table_id } => {
            match mark_default_table_deleting_in_write_txn(write_txn, table)? {
                Some(actual_table_id) if actual_table_id == *table_id => Ok(()),
                Some(actual_table_id) => Err(Error::conflict(format!(
                    "tenant event table lifecycle expected table id {} but marked {} deleting",
                    table_id, actual_table_id
                ))),
                None => Ok(()),
            }
        }
        TableLifecycleEvent::HardDelete { table, table_id } => {
            let deleted = hard_delete_deleting_table_identity_in_write_txn(write_txn, table_id)?;
            if deleted.is_some() {
                remove_documents_for_table_id(write_txn, table_id)?;
                remove_indexes_for_table_id(write_txn, table_id)?;
                if resolve_table_id_in_write_txn(write_txn, table)?.is_none() {
                    remove_schema_for_table(write_txn, table)?;
                }
            }
            Ok(())
        }
    }
}

fn rebuild_indexes_for_table_schema_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table_schema: &nimbus_core::TableSchema,
    table_id: &nimbus_core::TableId,
) -> Result<()> {
    remove_indexes_for_table_id(write_txn, table_id)?;
    let mut indexes = write_txn.open_table(INDEXES).map_err(map_redb_error)?;
    let documents = match write_txn.open_table(DOCUMENTS) {
        Ok(documents) => documents,
        Err(TableError::TableDoesNotExist(_)) => return Ok(()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let prefix = table_prefix(table_id);
    let mut insert_keys = Vec::new();
    match prefix_end(&prefix) {
        Some(end) => {
            for item in documents
                .range(prefix.as_slice()..end.as_slice())
                .map_err(map_redb_error)?
            {
                let (_, value) = item.map_err(map_redb_error)?;
                let document = decode_document_msgpack(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                insert_keys.extend(durable_record_index_keys_for_table_id(
                    &document,
                    table_schema,
                    table_id,
                )?);
            }
        }
        None => {
            for item in documents
                .range(prefix.as_slice()..)
                .map_err(map_redb_error)?
            {
                let (key, value) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(&prefix) {
                    break;
                }
                let document = decode_document_msgpack(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                insert_keys.extend(durable_record_index_keys_for_table_id(
                    &document,
                    table_schema,
                    table_id,
                )?);
            }
        }
    }
    for key in insert_keys {
        indexes
            .insert(key.as_slice(), EMPTY_TABLE_VALUE)
            .map_err(map_redb_error)?;
    }
    Ok(())
}

fn remove_documents_for_table_id(
    write_txn: &redb::WriteTransaction,
    table_id: &nimbus_core::TableId,
) -> Result<()> {
    remove_prefixed_binary_rows(write_txn, DOCUMENTS, table_prefix(table_id))
}

fn remove_indexes_for_table_id(
    write_txn: &redb::WriteTransaction,
    table_id: &nimbus_core::TableId,
) -> Result<()> {
    remove_prefixed_binary_rows(write_txn, INDEXES, table_index_prefix(table_id))
}

fn remove_schema_for_table(
    write_txn: &redb::WriteTransaction,
    table: &nimbus_core::TableName,
) -> Result<()> {
    let mut schemas = match write_txn.open_table(SCHEMAS) {
        Ok(schemas) => schemas,
        Err(TableError::TableDoesNotExist(_)) => return Ok(()),
        Err(error) => return Err(map_redb_error(error)),
    };
    schemas.remove(table.as_str()).map_err(map_redb_error)?;
    Ok(())
}

fn remove_prefixed_binary_rows(
    write_txn: &redb::WriteTransaction,
    table_definition: redb::TableDefinition<&[u8], &[u8]>,
    prefix: Vec<u8>,
) -> Result<()> {
    let mut table = match write_txn.open_table(table_definition) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let keys = prefixed_keys(&table, prefix.as_slice())?;
    for key in keys {
        table.remove(key.as_slice()).map_err(map_redb_error)?;
    }
    Ok(())
}

fn prefixed_keys(table: &redb::Table<'_, &[u8], &[u8]>, prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut keys = Vec::new();
    if let Some(end) = prefix_end(prefix) {
        for item in table
            .range(prefix..end.as_slice())
            .map_err(map_redb_error)?
        {
            let (key, _) = item.map_err(map_redb_error)?;
            if !key.value().starts_with(prefix) {
                break;
            }
            keys.push(key.value().to_vec());
        }
    } else {
        for item in table.range(prefix..).map_err(map_redb_error)? {
            let (key, _) = item.map_err(map_redb_error)?;
            if !key.value().starts_with(prefix) {
                break;
            }
            keys.push(key.value().to_vec());
        }
    }
    Ok(keys)
}

fn write_applied_sequence(
    write_txn: &redb::WriteTransaction,
    sequence: SequenceNumber,
) -> Result<()> {
    let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
    metadata
        .insert(APPLIED_SEQUENCE_KEY, encode_u64(sequence.0).as_slice())
        .map_err(map_redb_error)?;
    Ok(())
}

fn write_next_sequence(write_txn: &redb::WriteTransaction, sequence: u64) -> Result<()> {
    let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
    metadata
        .insert(NEXT_SEQUENCE_KEY, encode_u64(sequence).as_slice())
        .map_err(map_redb_error)?;
    Ok(())
}

fn commit_journal_txn(
    fault_injector: &dyn FaultInjector,
    write_txn: redb::WriteTransaction,
) -> Result<()> {
    fault_injector.check(FaultPoint::JournalAppendBeforeDurableFlush)?;
    write_txn.commit().map_err(map_redb_error)?;
    fault_injector.check(FaultPoint::JournalFlushBeforeVisibility)?;
    Ok(())
}

pub(crate) fn commit_write_txn_cancellable<Check>(
    fault_injector: &dyn FaultInjector,
    check_cancel: Check,
    write_txn: redb::WriteTransaction,
) -> Result<()>
where
    Check: Fn() -> Result<()>,
{
    fault_injector.check(FaultPoint::StorageCommitBeforeVisibility)?;
    check_cancel()?;
    write_txn.commit().map_err(map_redb_error)?;
    fault_injector.check(FaultPoint::StorageCommitAfterVisibilityBeforeReturn)?;
    Ok(())
}

pub(crate) fn begin_scheduled_execution(
    write_txn: &redb::WriteTransaction,
    execution_id: Option<&str>,
) -> Result<bool> {
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };

    let mut executions = write_txn
        .open_table(SCHEDULED_JOB_EXECUTIONS)
        .map_err(map_redb_error)?;
    if executions
        .get(execution_id)
        .map_err(map_redb_error)?
        .is_some()
    {
        return Ok(false);
    }
    executions
        .insert(execution_id, EMPTY_TABLE_VALUE)
        .map_err(map_redb_error)?;
    Ok(true)
}

fn next_sequence(write_txn: &redb::WriteTransaction) -> Result<u64> {
    let metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
    Ok(
        match metadata.get(NEXT_SEQUENCE_KEY).map_err(map_redb_error)? {
            Some(value) => decode_u64(value.value())?,
            None => 1,
        },
    )
}

pub(super) fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

pub(super) fn decode_u64(bytes: &[u8]) -> Result<u64> {
    let array: [u8; 8] = bytes
        .try_into()
        .map_err(|_| Error::Internal("expected 8 bytes when decoding u64 metadata".to_string()))?;
    Ok(u64::from_be_bytes(array))
}
