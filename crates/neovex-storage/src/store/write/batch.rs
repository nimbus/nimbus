use neovex_core::{CommitEntry, Document, Error, Result, WriteOp, WriteOpType};
use redb::ReadableTable;

use crate::document_codec::{decode_document_msgpack, encode_document_msgpack};
use crate::index::index_key_for_document;
use crate::keys::document_key;

use super::super::{
    DOCUMENTS, EMPTY_TABLE_VALUE, INDEXES, ResolvedScheduleOp, ResolvedWrite, TenantStore,
    map_redb_error,
};
use super::scheduled::apply_schedule_ops;

impl TenantStore {
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
        if writes.is_empty() && schedule_ops.is_empty() {
            return Err(Error::Internal(
                "execution-unit batch must contain at least one change".to_string(),
            ));
        }

        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let mut commit_writes = Vec::with_capacity(writes.len());

        {
            let mut documents = write_txn.open_table(DOCUMENTS).map_err(map_redb_error)?;
            let mut index_table = write_txn.open_table(INDEXES).map_err(map_redb_error)?;

            for write in writes {
                match write {
                    ResolvedWrite::Insert { document, indexes } => apply_insert(
                        document,
                        indexes,
                        &mut documents,
                        &mut index_table,
                        &mut commit_writes,
                    )?,
                    ResolvedWrite::Update {
                        previous,
                        current,
                        indexes,
                    } => apply_update(
                        previous,
                        current,
                        indexes,
                        &mut documents,
                        &mut index_table,
                        &mut commit_writes,
                    )?,
                    ResolvedWrite::Delete { previous, indexes } => apply_delete(
                        previous,
                        indexes,
                        &mut documents,
                        &mut index_table,
                        &mut commit_writes,
                    )?,
                }
            }
        }

        apply_schedule_ops(&write_txn, schedule_ops)?;

        let commit = if commit_writes.is_empty() {
            None
        } else {
            Some(self.append_commit_entry(&write_txn, commit_writes)?)
        };
        self.commit_write_txn(write_txn)?;
        Ok(commit)
    }
}

fn apply_insert(
    document: &Document,
    indexes: &[neovex_core::IndexDefinition],
    documents: &mut redb::Table<&[u8], &[u8]>,
    index_table: &mut redb::Table<&[u8], &[u8]>,
    commit_writes: &mut Vec<WriteOp>,
) -> Result<()> {
    let key = document_key(&document.table, &document.id);
    if documents
        .get(key.as_slice())
        .map_err(map_redb_error)?
        .is_some()
    {
        return Err(Error::Conflict(format!(
            "document {} changed before transaction commit",
            document.id
        )));
    }

    let payload = encode_document_msgpack(document)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    documents
        .insert(key.as_slice(), payload.as_slice())
        .map_err(map_redb_error)?;
    for index in indexes {
        if let Some(index_key) = index_key_for_document(document, index)? {
            index_table
                .insert(index_key.as_slice(), EMPTY_TABLE_VALUE)
                .map_err(map_redb_error)?;
        }
    }
    commit_writes.push(WriteOp {
        table: document.table.clone(),
        op_type: WriteOpType::Insert,
        doc_id: document.id,
        previous: None,
        current: Some(document.clone()),
    });
    Ok(())
}

fn apply_update(
    previous: &Document,
    current: &Document,
    indexes: &[neovex_core::IndexDefinition],
    documents: &mut redb::Table<&[u8], &[u8]>,
    index_table: &mut redb::Table<&[u8], &[u8]>,
    commit_writes: &mut Vec<WriteOp>,
) -> Result<()> {
    let key = document_key(&current.table, &current.id);
    let existing = {
        let existing = documents
            .get(key.as_slice())
            .map_err(map_redb_error)?
            .ok_or(Error::Conflict(format!(
                "document {} changed before transaction commit",
                current.id
            )))?;
        decode_document_msgpack(existing.value())
            .map_err(|error| Error::Serialization(error.to_string()))?
    };
    if &existing != previous {
        return Err(Error::Conflict(format!(
            "document {} changed before transaction commit",
            current.id
        )));
    }

    let payload = encode_document_msgpack(current)
        .map_err(|error| Error::Serialization(error.to_string()))?;
    documents
        .insert(key.as_slice(), payload.as_slice())
        .map_err(map_redb_error)?;

    for index in indexes {
        let old_key = index_key_for_document(previous, index)?;
        let new_key = index_key_for_document(current, index)?;
        if old_key == new_key {
            continue;
        }
        if let Some(old_key) = old_key {
            index_table
                .remove(old_key.as_slice())
                .map_err(map_redb_error)?;
        }
        if let Some(new_key) = new_key {
            index_table
                .insert(new_key.as_slice(), EMPTY_TABLE_VALUE)
                .map_err(map_redb_error)?;
        }
    }

    commit_writes.push(WriteOp {
        table: current.table.clone(),
        op_type: WriteOpType::Update,
        doc_id: current.id,
        previous: Some(previous.clone()),
        current: Some(current.clone()),
    });
    Ok(())
}

fn apply_delete(
    previous: &Document,
    indexes: &[neovex_core::IndexDefinition],
    documents: &mut redb::Table<&[u8], &[u8]>,
    index_table: &mut redb::Table<&[u8], &[u8]>,
    commit_writes: &mut Vec<WriteOp>,
) -> Result<()> {
    let key = document_key(&previous.table, &previous.id);
    let removed = documents
        .remove(key.as_slice())
        .map_err(map_redb_error)?
        .ok_or(Error::Conflict(format!(
            "document {} changed before transaction commit",
            previous.id
        )))?;
    let removed = decode_document_msgpack(removed.value())
        .map_err(|error| Error::Serialization(error.to_string()))?;
    if &removed != previous {
        return Err(Error::Conflict(format!(
            "document {} changed before transaction commit",
            previous.id
        )));
    }

    for index in indexes {
        if let Some(index_key) = index_key_for_document(previous, index)? {
            index_table
                .remove(index_key.as_slice())
                .map_err(map_redb_error)?;
        }
    }

    commit_writes.push(WriteOp {
        table: previous.table.clone(),
        op_type: WriteOpType::Delete,
        doc_id: previous.id,
        previous: Some(previous.clone()),
        current: None,
    });
    Ok(())
}
