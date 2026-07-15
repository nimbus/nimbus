use nimbus_core::{
    Document, DocumentId, DocumentLocator, Error, IndexDefinition, ResourcePathBinding, Result,
    TableName, TriggerWriteOrigin, WriteOp, WriteOpType,
};
use redb::ReadableTable;

use crate::document_codec::{decode_document_msgpack, encode_document_msgpack};
use crate::index::index_key_for_document;
use crate::keys::document_key;
use crate::store::resource_paths::{
    remove_resource_path_binding_in_write_txn, upsert_resource_path_binding_in_write_txn,
};
use crate::store::table_catalog::{
    resolve_or_create_table_id_in_write_txn, resolve_table_id_in_write_txn,
};

use super::super::{DOCUMENTS, EMPTY_TABLE_VALUE, INDEXES, TenantWriteTransaction, map_redb_error};

#[derive(Clone, Copy)]
enum WriteExpectation {
    Point,
    Batch,
}

impl WriteExpectation {
    fn missing_document_error(self, id: &DocumentId) -> Error {
        match self {
            Self::Point => Error::DocumentNotFound(id.clone()),
            Self::Batch => changed_before_commit_error(id),
        }
    }

    fn mismatch_error(self, id: &DocumentId) -> Error {
        match self {
            Self::Point => Error::DocumentNotFound(id.clone()),
            Self::Batch => changed_before_commit_error(id),
        }
    }
}

fn changed_before_commit_error(id: &DocumentId) -> Error {
    Error::conflict(format!("document {} changed before transaction commit", id))
}

impl TenantWriteTransaction {
    pub(crate) fn read_existing_document_for_point_write(
        &mut self,
        table: &TableName,
        id: &DocumentId,
    ) -> Result<Document> {
        self.check_cancel()?;
        let table_id = resolve_table_id_in_write_txn(self.write_txn()?, table)?
            .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
        let key = document_key(&table_id, id);
        let documents = self
            .write_txn()?
            .open_table(DOCUMENTS)
            .map_err(map_redb_error)?;
        let existing = documents
            .get(key.as_slice())
            .map_err(map_redb_error)?
            .ok_or_else(|| Error::DocumentNotFound(id.clone()))?;
        decode_document_msgpack(existing.value())
            .map_err(|error| Error::Serialization(error.to_string()))
    }

    pub(crate) fn apply_document_insert(
        &mut self,
        document: &Document,
        indexes: &[IndexDefinition],
        resource_path_binding: Option<&ResourcePathBinding>,
        trigger_write_origin: Option<&TriggerWriteOrigin>,
    ) -> Result<()> {
        self.check_cancel()?;
        let table_id = resolve_or_create_table_id_in_write_txn(self.write_txn()?, &document.table)?;
        let key = document_key(&table_id, &document.id);
        {
            let mut documents = self
                .write_txn()?
                .open_table(DOCUMENTS)
                .map_err(map_redb_error)?;
            if documents
                .get(key.as_slice())
                .map_err(map_redb_error)?
                .is_some()
            {
                return Err(changed_before_commit_error(&document.id));
            }
            let payload = encode_document_msgpack(document)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            documents
                .insert(key.as_slice(), payload.as_slice())
                .map_err(map_redb_error)?;
        }
        write_insert_index_entries(self.write_txn()?, document, indexes, &table_id)?;
        if let Some(resource_path_binding) = resource_path_binding {
            upsert_resource_path_binding_in_write_txn(self.write_txn()?, resource_path_binding)?;
        }
        self.record_commit_write(WriteOp {
            table: document.table.clone(),
            table_id,
            op_type: WriteOpType::Insert,
            doc_id: document.id.clone(),
            resource_path_binding: resource_path_binding.cloned(),
            trigger_write_origin: trigger_write_origin.cloned(),
            previous: None,
            current: Some(document.clone()),
        });
        Ok(())
    }

    pub(crate) fn apply_point_document_update(
        &mut self,
        previous: &Document,
        current: &Document,
        indexes: &[IndexDefinition],
    ) -> Result<()> {
        self.apply_document_update(
            previous,
            current,
            indexes,
            None,
            None,
            WriteExpectation::Point,
        )
    }

    pub(crate) fn apply_batch_document_update(
        &mut self,
        previous: &Document,
        current: &Document,
        indexes: &[IndexDefinition],
        resource_path_binding: Option<&ResourcePathBinding>,
        trigger_write_origin: Option<&TriggerWriteOrigin>,
    ) -> Result<()> {
        self.apply_document_update(
            previous,
            current,
            indexes,
            resource_path_binding,
            trigger_write_origin,
            WriteExpectation::Batch,
        )
    }

    fn apply_document_update(
        &mut self,
        previous: &Document,
        current: &Document,
        indexes: &[IndexDefinition],
        resource_path_binding: Option<&ResourcePathBinding>,
        trigger_write_origin: Option<&TriggerWriteOrigin>,
        expectation: WriteExpectation,
    ) -> Result<()> {
        self.check_cancel()?;
        let table_id = resolve_table_id_in_write_txn(self.write_txn()?, &current.table)?
            .ok_or_else(|| expectation.missing_document_error(&current.id))?;
        let key = document_key(&table_id, &current.id);
        {
            let mut documents = self
                .write_txn()?
                .open_table(DOCUMENTS)
                .map_err(map_redb_error)?;
            let existing = {
                let existing = documents
                    .get(key.as_slice())
                    .map_err(map_redb_error)?
                    .ok_or_else(|| expectation.missing_document_error(&current.id))?;
                decode_document_msgpack(existing.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?
            };
            if existing != *previous {
                return Err(expectation.mismatch_error(&current.id));
            }

            let payload = encode_document_msgpack(current)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            documents
                .insert(key.as_slice(), payload.as_slice())
                .map_err(map_redb_error)?;
        }

        apply_update_index_diff(self.write_txn()?, previous, current, indexes, &table_id)?;
        if let Some(resource_path_binding) = resource_path_binding {
            upsert_resource_path_binding_in_write_txn(self.write_txn()?, resource_path_binding)?;
        }
        self.record_commit_write(WriteOp {
            table: current.table.clone(),
            table_id,
            op_type: WriteOpType::Update,
            doc_id: current.id.clone(),
            resource_path_binding: resource_path_binding.cloned(),
            trigger_write_origin: trigger_write_origin.cloned(),
            previous: Some(previous.clone()),
            current: Some(current.clone()),
        });
        Ok(())
    }

    pub(crate) fn apply_point_document_delete(
        &mut self,
        previous: &Document,
        indexes: &[IndexDefinition],
    ) -> Result<()> {
        self.apply_document_delete(previous, indexes, None, WriteExpectation::Point)
    }

    pub(crate) fn apply_batch_document_delete(
        &mut self,
        previous: &Document,
        indexes: &[IndexDefinition],
        trigger_write_origin: Option<&TriggerWriteOrigin>,
    ) -> Result<()> {
        self.apply_document_delete(
            previous,
            indexes,
            trigger_write_origin,
            WriteExpectation::Batch,
        )
    }

    fn apply_document_delete(
        &mut self,
        previous: &Document,
        indexes: &[IndexDefinition],
        trigger_write_origin: Option<&TriggerWriteOrigin>,
        expectation: WriteExpectation,
    ) -> Result<()> {
        self.check_cancel()?;
        let table_id = resolve_table_id_in_write_txn(self.write_txn()?, &previous.table)?
            .ok_or_else(|| expectation.missing_document_error(&previous.id))?;
        let key = document_key(&table_id, &previous.id);
        {
            let mut documents = self
                .write_txn()?
                .open_table(DOCUMENTS)
                .map_err(map_redb_error)?;
            let removed = documents
                .remove(key.as_slice())
                .map_err(map_redb_error)?
                .ok_or_else(|| expectation.missing_document_error(&previous.id))?;
            let removed = decode_document_msgpack(removed.value())
                .map_err(|error| Error::Serialization(error.to_string()))?;
            if removed != *previous {
                return Err(expectation.mismatch_error(&previous.id));
            }
        }

        remove_index_entries(self.write_txn()?, previous, indexes, &table_id)?;
        let resource_path_binding = remove_resource_path_binding_in_write_txn(
            self.write_txn()?,
            &DocumentLocator::new(previous.table.clone(), previous.id.clone()),
        )?;
        self.record_commit_write(WriteOp {
            table: previous.table.clone(),
            table_id,
            op_type: WriteOpType::Delete,
            doc_id: previous.id.clone(),
            resource_path_binding,
            trigger_write_origin: trigger_write_origin.cloned(),
            previous: Some(previous.clone()),
            current: None,
        });
        Ok(())
    }
}

fn write_insert_index_entries(
    write_txn: &redb::WriteTransaction,
    document: &Document,
    indexes: &[IndexDefinition],
    table_id: &nimbus_core::TableId,
) -> Result<()> {
    let mut index_table = None;
    for index in indexes.iter().filter(|index| index.is_maintained()) {
        if let Some(index_key) = index_key_for_document(document, index, table_id)? {
            if index_table.is_none() {
                index_table = Some(write_txn.open_table(INDEXES).map_err(map_redb_error)?);
            }
            let table = index_table
                .as_mut()
                .expect("index table should be initialized before use");
            table
                .insert(index_key.as_slice(), EMPTY_TABLE_VALUE)
                .map_err(map_redb_error)?;
        }
    }
    Ok(())
}

fn apply_update_index_diff(
    write_txn: &redb::WriteTransaction,
    previous: &Document,
    current: &Document,
    indexes: &[IndexDefinition],
    table_id: &nimbus_core::TableId,
) -> Result<()> {
    let mut index_table = None;
    for index in indexes.iter().filter(|index| index.is_maintained()) {
        let old_key = index_key_for_document(previous, index, table_id)?;
        let new_key = index_key_for_document(current, index, table_id)?;
        if old_key == new_key {
            continue;
        }
        if index_table.is_none() {
            index_table = Some(write_txn.open_table(INDEXES).map_err(map_redb_error)?);
        }
        let table = index_table
            .as_mut()
            .expect("index table should be initialized before use");
        if let Some(old_key) = old_key {
            table.remove(old_key.as_slice()).map_err(map_redb_error)?;
        }
        if let Some(new_key) = new_key {
            table
                .insert(new_key.as_slice(), EMPTY_TABLE_VALUE)
                .map_err(map_redb_error)?;
        }
    }
    Ok(())
}

fn remove_index_entries(
    write_txn: &redb::WriteTransaction,
    previous: &Document,
    indexes: &[IndexDefinition],
    table_id: &nimbus_core::TableId,
) -> Result<()> {
    let mut index_table = None;
    for index in indexes.iter().filter(|index| index.is_maintained()) {
        if let Some(index_key) = index_key_for_document(previous, index, table_id)? {
            if index_table.is_none() {
                index_table = Some(write_txn.open_table(INDEXES).map_err(map_redb_error)?);
            }
            let table = index_table
                .as_mut()
                .expect("index table should be initialized before use");
            table.remove(index_key.as_slice()).map_err(map_redb_error)?;
        }
    }
    Ok(())
}
