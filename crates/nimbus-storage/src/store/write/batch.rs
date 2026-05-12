use nimbus_core::{CommitEntry, Document, Error, Result, WriteOp, WriteOpType};
use redb::ReadableTable;

use crate::document_codec::{decode_document_msgpack, encode_document_msgpack};
use crate::index::index_key_for_document;
use crate::keys::document_key;
use crate::store::resource_paths::{
    remove_resource_path_binding_in_write_txn, upsert_resource_path_binding_in_write_txn,
};

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
        self.apply_execution_unit_batch_with_origin(writes, schedule_ops, None)
    }

    pub fn apply_execution_unit_batch_with_origin(
        &self,
        writes: &[ResolvedWrite],
        schedule_ops: &[ResolvedScheduleOp],
        trigger_write_origin: Option<&nimbus_core::TriggerWriteOrigin>,
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
                    ResolvedWrite::Insert {
                        document,
                        indexes,
                        resource_path_binding,
                    } => apply_insert(
                        &write_txn,
                        document,
                        indexes,
                        resource_path_binding.as_ref(),
                        trigger_write_origin,
                        &mut documents,
                        &mut index_table,
                        &mut commit_writes,
                    )?,
                    ResolvedWrite::Update {
                        previous,
                        current,
                        indexes,
                        resource_path_binding,
                    } => apply_update(
                        &write_txn,
                        previous,
                        current,
                        indexes,
                        resource_path_binding.as_ref(),
                        trigger_write_origin,
                        &mut documents,
                        &mut index_table,
                        &mut commit_writes,
                    )?,
                    ResolvedWrite::Delete { previous, indexes } => apply_delete(
                        &write_txn,
                        previous,
                        indexes,
                        trigger_write_origin,
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

#[expect(
    clippy::too_many_arguments,
    reason = "execution-unit inserts need document state, index/path metadata, optional trigger origin, and commit sinks inside one storage transaction helper"
)]
fn apply_insert(
    write_txn: &redb::WriteTransaction,
    document: &Document,
    indexes: &[nimbus_core::IndexDefinition],
    resource_path_binding: Option<&nimbus_core::ResourcePathBinding>,
    trigger_write_origin: Option<&nimbus_core::TriggerWriteOrigin>,
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
    if let Some(resource_path_binding) = resource_path_binding {
        upsert_resource_path_binding_in_write_txn(write_txn, resource_path_binding)?;
    }
    commit_writes.push(WriteOp {
        table: document.table.clone(),
        op_type: WriteOpType::Insert,
        doc_id: document.id.clone(),
        resource_path_binding: resource_path_binding.cloned(),
        trigger_write_origin: trigger_write_origin.cloned(),
        previous: None,
        current: Some(document.clone()),
    });
    Ok(())
}

#[expect(
    clippy::too_many_arguments,
    reason = "execution-unit updates need the current document pair, index context, commit sink, and optional path metadata in one storage-transaction helper"
)]
fn apply_update(
    write_txn: &redb::WriteTransaction,
    previous: &Document,
    current: &Document,
    indexes: &[nimbus_core::IndexDefinition],
    resource_path_binding: Option<&nimbus_core::ResourcePathBinding>,
    trigger_write_origin: Option<&nimbus_core::TriggerWriteOrigin>,
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
    if let Some(resource_path_binding) = resource_path_binding {
        upsert_resource_path_binding_in_write_txn(write_txn, resource_path_binding)?;
    }

    commit_writes.push(WriteOp {
        table: current.table.clone(),
        op_type: WriteOpType::Update,
        doc_id: current.id.clone(),
        resource_path_binding: resource_path_binding.cloned(),
        trigger_write_origin: trigger_write_origin.cloned(),
        previous: Some(previous.clone()),
        current: Some(current.clone()),
    });
    Ok(())
}

fn apply_delete(
    write_txn: &redb::WriteTransaction,
    previous: &Document,
    indexes: &[nimbus_core::IndexDefinition],
    trigger_write_origin: Option<&nimbus_core::TriggerWriteOrigin>,
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
    let resource_path_binding = remove_resource_path_binding_in_write_txn(
        write_txn,
        &nimbus_core::DocumentLocator::new(previous.table.clone(), previous.id.clone()),
    )?;

    commit_writes.push(WriteOp {
        table: previous.table.clone(),
        op_type: WriteOpType::Delete,
        doc_id: previous.id.clone(),
        resource_path_binding,
        trigger_write_origin: trigger_write_origin.cloned(),
        previous: Some(previous.clone()),
        current: None,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use nimbus_core::{
        DocumentId, DocumentLocator, DocumentPath, FieldSchema, FieldType, IndexDefinition,
        ResourcePathBinding, SequenceNumber, TableName, TableSchema,
    };
    use serde_json::json;

    use super::*;

    fn schema(table: &TableName) -> TableSchema {
        TableSchema {
            table: table.clone(),
            fields: vec![
                FieldSchema {
                    name: "owner".to_string(),
                    field_type: FieldType::String,
                    required: true,
                },
                FieldSchema {
                    name: "body".to_string(),
                    field_type: FieldType::String,
                    required: true,
                },
            ],
            indexes: vec![IndexDefinition {
                name: "by_body".to_string(),
                fields: vec!["body".to_string()],
            }],
            access_policy: None,
        }
    }

    fn document(table: &TableName, id: &str, body: &str) -> Document {
        Document::with_id(
            DocumentId::from_key(id).expect("id should parse"),
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!(body)),
            ]),
        )
    }

    #[test]
    fn failed_batch_rolls_back_document_indexes_bindings_and_commit_log() {
        let store = TenantStore::create_in_memory().expect("store should open");
        let table = TableName::new("tasks_atomic_batch").expect("table should parse");
        let schema = schema(&table);
        store
            .replace_table_schema(&schema)
            .expect("schema should persist");

        let existing = document(&table, "existing", "existing");
        store
            .insert_with_indexes(&existing, &schema.indexes)
            .expect("seed document should insert");

        let pending = document(&table, "pending", "alpha");
        let binding = ResourcePathBinding::new(
            DocumentLocator::new(table.clone(), pending.id.clone()),
            DocumentPath::from_segments(["cities", "SF"]).expect("path should parse"),
        );
        let failed = store
            .apply_execution_unit_batch(
                &[
                    ResolvedWrite::Insert {
                        document: pending.clone(),
                        indexes: schema.indexes.clone(),
                        resource_path_binding: Some(binding.clone()),
                    },
                    ResolvedWrite::Insert {
                        document: existing.clone(),
                        indexes: schema.indexes.clone(),
                        resource_path_binding: None,
                    },
                ],
                &[],
            )
            .expect_err("conflicting sibling write should fail the batch");

        assert!(matches!(failed, Error::Conflict(_)));
        assert!(
            store
                .get(&table, &pending.id)
                .expect("document lookup should succeed")
                .is_none(),
            "failed batch must not leave the document behind"
        );
        assert!(
            store
                .index_scan_eq(&table, "by_body", &json!("alpha"))
                .expect("index scan should succeed")
                .is_empty(),
            "failed batch must not leave index entries behind"
        );
        assert!(
            store
                .resource_path_binding(&binding.locator)
                .expect("binding lookup should succeed")
                .is_none(),
            "failed batch must not leave path metadata behind"
        );
        assert_eq!(
            store
                .latest_sequence()
                .expect("latest sequence should remain readable"),
            SequenceNumber(1),
            "failed batch must not append a commit log entry"
        );
    }
}
