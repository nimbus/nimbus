use neovex_core::{
    CommitEntry, Document, DurableMutationRecord, Error, Result, SequenceNumber, Timestamp, WriteOp,
};
use redb::{ReadableTable, TableError};

use crate::commit_log::serialize_commit;
use crate::keys::document_key;
use crate::simulation::{FaultInjector, FaultPoint};

use super::schema_rewrite::rewrite_document_indexes_in_write_txn;
use super::{
    APPLIED_SEQUENCE_KEY, COMMIT_LOG, DOCUMENTS, EMPTY_TABLE_VALUE, JournalProgress, METADATA,
    NEXT_SEQUENCE_KEY, SCHEDULED_JOB_EXECUTIONS, TenantStore, map_redb_error,
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
    ) -> Result<Vec<DurableMutationRecord>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table_handle = match read_txn.open_table(COMMIT_LOG) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut entries = Vec::new();
        for item in table_handle.range(sequence.0..).map_err(map_redb_error)? {
            let (_, value) = item.map_err(map_redb_error)?;
            entries.push(crate::commit_log::deserialize_durable_record(
                value.value(),
            )?);
        }
        Ok(entries)
    }

    pub(crate) fn append_commit_entry(
        &self,
        write_txn: &redb::WriteTransaction,
        writes: Vec<WriteOp>,
    ) -> Result<CommitEntry> {
        append_commit(write_txn, self.now(), writes)
    }

    pub fn append_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
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
                let payload = crate::commit_log::serialize_durable_record(record)?;
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

    pub fn apply_durable_records_batch(&self, records: &[DurableMutationRecord]) -> Result<()> {
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
) -> Result<CommitEntry> {
    let sequence = next_sequence(write_txn)?;
    let entry = CommitEntry {
        sequence: SequenceNumber(sequence),
        timestamp,
        writes,
    };

    let mut log = write_txn.open_table(COMMIT_LOG).map_err(map_redb_error)?;
    let payload = serialize_commit(&entry)?;
    log.insert(sequence, payload.as_slice())
        .map_err(map_redb_error)?;
    write_applied_sequence(write_txn, entry.sequence)?;

    Ok(entry)
}

fn apply_durable_record_in_write_txn(
    write_txn: &redb::WriteTransaction,
    record: &DurableMutationRecord,
) -> Result<()> {
    if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
        let _ = begin_scheduled_execution(write_txn, Some(execution_id))?;
    }

    let mut documents = write_txn.open_table(DOCUMENTS).map_err(map_redb_error)?;
    for write in &record.writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                let key = document_key(&write.table, &write.doc_id);
                let already_applied = {
                    let existing = documents.get(key.as_slice()).map_err(map_redb_error)?;
                    if let Some(existing) = existing {
                        let existing = Document::from_msgpack(existing.value())
                            .map_err(|error| Error::Serialization(error.to_string()))?;
                        if existing != *current {
                            return Err(Error::Conflict(format!(
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
                    let payload = current
                        .to_msgpack()
                        .map_err(|error| Error::Serialization(error.to_string()))?;
                    documents
                        .insert(key.as_slice(), payload.as_slice())
                        .map_err(map_redb_error)?;
                }
            }
            (Some(previous), Some(current)) => {
                let key = document_key(&write.table, &write.doc_id);
                let existing = {
                    let existing = documents
                        .get(key.as_slice())
                        .map_err(map_redb_error)?
                        .ok_or(Error::Conflict(format!(
                            "durable journal update replay missing document {}",
                            write.doc_id
                        )))?;
                    Document::from_msgpack(existing.value())
                        .map_err(|error| Error::Serialization(error.to_string()))?
                };
                if existing == *current {
                    continue;
                }
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "durable journal update replay found conflicting state for document {}",
                        write.doc_id
                    )));
                }
                let payload = current
                    .to_msgpack()
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                documents
                    .insert(key.as_slice(), payload.as_slice())
                    .map_err(map_redb_error)?;
            }
            (Some(previous), None) => {
                let key = document_key(&write.table, &write.doc_id);
                match documents.remove(key.as_slice()).map_err(map_redb_error)? {
                    Some(removed) => {
                        let removed = Document::from_msgpack(removed.value())
                            .map_err(|error| Error::Serialization(error.to_string()))?;
                        if removed != *previous {
                            return Err(Error::Conflict(format!(
                                "durable journal delete replay found conflicting state for document {}",
                                write.doc_id
                            )));
                        }
                    }
                    None => continue,
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
    }
    Ok(())
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
    let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
    let current = match metadata.get(NEXT_SEQUENCE_KEY).map_err(map_redb_error)? {
        Some(value) => decode_u64(value.value())?,
        None => 1,
    };
    let next = current + 1;
    metadata
        .insert(NEXT_SEQUENCE_KEY, encode_u64(next).as_slice())
        .map_err(map_redb_error)?;
    Ok(current)
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
