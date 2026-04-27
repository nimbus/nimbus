use neovex_core::{Document, DocumentId, IndexDefinition, Result, TableName, WriteOp, WriteOpType};
use redb::ReadableTable;
use serde_json::Value;

use crate::document_codec::{decode_document_msgpack, encode_document_msgpack};
use crate::keys::document_key;
use crate::store::{INDEXES, TenantWriteTransaction, map_redb_error};

use super::super::keyspace::index_key_for_document;
use super::EMPTY_INDEX_VALUE;

impl TenantWriteTransaction {
    pub fn insert_document_with_indexes(
        &mut self,
        document: &Document,
        indexes: &[IndexDefinition],
    ) -> Result<()> {
        self.insert_document(document)?;
        self.check_cancel()?;
        let mut index_table = self
            .write_txn()?
            .open_table(INDEXES)
            .map_err(map_redb_error)?;
        for index in indexes {
            if let Some(key) = index_key_for_document(document, index)? {
                index_table
                    .insert(key.as_slice(), EMPTY_INDEX_VALUE)
                    .map_err(map_redb_error)?;
            }
        }
        Ok(())
    }

    pub fn update_document_with_indexes_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<()>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        self.check_cancel()?;
        let key = document_key(table, id);
        let (old_document, new_document, payload) = {
            let documents = self
                .write_txn()?
                .open_table(crate::store::DOCUMENTS)
                .map_err(map_redb_error)?;
            let old_document = {
                let existing = documents
                    .get(key.as_slice())
                    .map_err(map_redb_error)?
                    .ok_or(neovex_core::Error::DocumentNotFound(id.clone()))?;
                decode_document_msgpack(existing.value())
                    .map_err(|error| neovex_core::Error::Serialization(error.to_string()))?
            };
            let mut new_document = old_document.clone();
            for (field, value) in patch {
                new_document.fields.insert(field.clone(), value.clone());
            }
            validate(&old_document, &new_document)?;

            let payload = encode_document_msgpack(&new_document)
                .map_err(|error| neovex_core::Error::Serialization(error.to_string()))?;
            (old_document, new_document, payload)
        };

        {
            let mut documents = self
                .write_txn()?
                .open_table(crate::store::DOCUMENTS)
                .map_err(map_redb_error)?;
            documents
                .insert(key.as_slice(), payload.as_slice())
                .map_err(map_redb_error)?;
        }

        {
            self.check_cancel()?;
            let mut index_table = self
                .write_txn()?
                .open_table(INDEXES)
                .map_err(map_redb_error)?;
            for index in indexes {
                let old_key = index_key_for_document(&old_document, index)?;
                let new_key = index_key_for_document(&new_document, index)?;

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
                        .insert(new_key.as_slice(), EMPTY_INDEX_VALUE)
                        .map_err(map_redb_error)?;
                }
            }
        }

        self.record_commit_write(WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Update,
            doc_id: id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(old_document),
            current: Some(new_document),
        });
        Ok(())
    }

    pub fn delete_document_with_indexes_validated<F>(
        &mut self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<Document>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.check_cancel()?;
        let key = document_key(table, id);
        let old_document = {
            let mut documents = self
                .write_txn()?
                .open_table(crate::store::DOCUMENTS)
                .map_err(map_redb_error)?;
            let removed = documents.remove(key.as_slice()).map_err(map_redb_error)?;
            let removed = removed.ok_or(neovex_core::Error::DocumentNotFound(id.clone()))?;
            decode_document_msgpack(removed.value())
                .map_err(|error| neovex_core::Error::Serialization(error.to_string()))?
        };
        validate(&old_document)?;

        {
            self.check_cancel()?;
            let mut index_table = self
                .write_txn()?
                .open_table(INDEXES)
                .map_err(map_redb_error)?;
            for index in indexes {
                if let Some(key) = index_key_for_document(&old_document, index)? {
                    index_table.remove(key.as_slice()).map_err(map_redb_error)?;
                }
            }
        }

        self.record_commit_write(WriteOp {
            table: table.clone(),
            op_type: WriteOpType::Delete,
            doc_id: id.clone(),
            resource_path_binding: None,
            trigger_write_origin: None,
            previous: Some(old_document.clone()),
            current: None,
        });
        Ok(old_document)
    }
}
