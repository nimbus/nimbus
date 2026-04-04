use neovex_core::{
    CommitEntry, Document, DocumentId, IndexDefinition, Result, TableName, WriteOp, WriteOpType,
};
use redb::{ReadableTable, TableError};
use serde_json::Value;

use crate::keys::{document_key, prefix_end};
use crate::store::{INDEXES, TenantStore, TenantWriteTransaction, map_redb_error};

use super::keyspace::{index_key_for_document, table_index_prefix};

const EMPTY_INDEX_VALUE: &[u8] = &[];

fn collect_index_keys_for_prefix(store: &TenantStore, prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
    let read_txn = store.db.begin_read().map_err(map_redb_error)?;
    let index_table = match read_txn.open_table(INDEXES) {
        Ok(index_table) => index_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };

    let mut keys = Vec::new();
    match prefix_end(prefix) {
        Some(end) => {
            for item in index_table
                .range(prefix..end.as_slice())
                .map_err(map_redb_error)?
            {
                let (key, _) = item.map_err(map_redb_error)?;
                keys.push(key.value().to_vec());
            }
        }
        None => {
            for item in index_table.range(prefix..).map_err(map_redb_error)? {
                let (key, _) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(prefix) {
                    break;
                }
                keys.push(key.value().to_vec());
            }
        }
    }

    Ok(keys)
}

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
                    .ok_or(neovex_core::Error::DocumentNotFound(*id))?;
                Document::from_msgpack(existing.value())
                    .map_err(|error| neovex_core::Error::Serialization(error.to_string()))?
            };
            let mut new_document = old_document.clone();
            for (field, value) in patch {
                new_document.fields.insert(field.clone(), value.clone());
            }
            validate(&old_document, &new_document)?;

            let payload = new_document
                .to_msgpack()
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
            doc_id: *id,
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
            let removed = removed.ok_or(neovex_core::Error::DocumentNotFound(*id))?;
            Document::from_msgpack(removed.value())
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
            doc_id: *id,
            previous: Some(old_document.clone()),
            current: None,
        });
        Ok(old_document)
    }
}

impl TenantStore {
    /// Inserts a document and maintains indexes atomically.
    pub fn insert_with_indexes(
        &self,
        document: &Document,
        indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        self.insert_with_indexes_once(document, indexes, None)?
            .ok_or_else(|| {
                neovex_core::Error::Internal(
                    "non-deduplicated indexed insert should commit".to_string(),
                )
            })
    }

    /// Inserts a document and maintains indexes once for the provided scheduled execution id.
    pub fn insert_with_indexes_once(
        &self,
        document: &Document,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<CommitEntry>> {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(false);
            }
            transaction.insert_document_with_indexes(document, indexes)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(committed.commit.ok_or_else(|| {
                neovex_core::Error::Internal(
                    "deduplicated indexed insert should record a commit entry".to_string(),
                )
            })?)
        } else {
            None
        })
    }

    /// Updates a document and maintains indexes atomically.
    pub fn update_with_indexes(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        self.update_with_indexes_validated(table, id, patch, indexes, |_, _| Ok(()))
    }

    /// Updates a document and maintains indexes atomically after validating the merged result.
    pub fn update_with_indexes_validated<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<CommitEntry>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        self.update_with_indexes_validated_once(table, id, patch, indexes, None, validate)?
            .ok_or_else(|| {
                neovex_core::Error::Internal(
                    "non-deduplicated indexed update should commit".to_string(),
                )
            })
    }

    /// Updates a document and maintains indexes once for the provided scheduled execution id.
    pub fn update_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        patch: &serde_json::Map<String, Value>,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<CommitEntry>>
    where
        F: FnOnce(&Document, &Document) -> Result<()>,
    {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(false);
            }
            transaction
                .update_document_with_indexes_validated(table, id, patch, indexes, validate)?;
            Ok(true)
        })?;
        Ok(if committed.value {
            Some(committed.commit.ok_or_else(|| {
                neovex_core::Error::Internal(
                    "deduplicated indexed update should record a commit entry".to_string(),
                )
            })?)
        } else {
            None
        })
    }

    /// Deletes a document and removes index entries atomically.
    pub fn delete_with_indexes(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
    ) -> Result<CommitEntry> {
        self.delete_with_indexes_validated_once(table, id, indexes, None, |_| Ok(()))?
            .map(|commit| commit.0)
            .ok_or_else(|| {
                neovex_core::Error::Internal(
                    "non-deduplicated indexed delete should commit".to_string(),
                )
            })
    }

    /// Deletes a document and removes index entries once for the provided scheduled execution id.
    pub fn delete_with_indexes_once(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
    ) -> Result<Option<(CommitEntry, Document)>> {
        self.delete_with_indexes_validated_once(table, id, indexes, execution_id, |_| Ok(()))
    }

    /// Deletes a document and removes index entries atomically, returning the removed snapshot.
    pub fn delete_with_indexes_returning_document(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
    ) -> Result<(CommitEntry, Document)> {
        self.delete_with_indexes_validated_returning_document(table, id, indexes, |_| Ok(()))
    }

    /// Deletes a document and removes index entries atomically after validating the removed snapshot.
    pub fn delete_with_indexes_validated_returning_document<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        validate: F,
    ) -> Result<(CommitEntry, Document)>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        self.delete_with_indexes_validated_once(table, id, indexes, None, validate)?
            .ok_or_else(|| {
                neovex_core::Error::Internal(
                    "non-deduplicated indexed delete should commit".to_string(),
                )
            })
    }

    /// Deletes a document and removes index entries once for the provided scheduled execution id, returning the removed snapshot.
    pub fn delete_with_indexes_validated_once<F>(
        &self,
        table: &TableName,
        id: &DocumentId,
        indexes: &[IndexDefinition],
        execution_id: Option<&str>,
        validate: F,
    ) -> Result<Option<(CommitEntry, Document)>>
    where
        F: FnOnce(&Document) -> Result<()>,
    {
        let committed = self.execute_write(move |transaction| {
            if !transaction.begin_scheduled_execution(execution_id)? {
                return Ok(None);
            }
            let removed_document =
                transaction.delete_document_with_indexes_validated(table, id, indexes, validate)?;
            Ok(Some(removed_document))
        })?;
        Ok(if let Some(removed_document) = committed.value {
            Some((
                committed.commit.ok_or_else(|| {
                    neovex_core::Error::Internal(
                        "deduplicated indexed delete should record a commit entry".to_string(),
                    )
                })?,
                removed_document,
            ))
        } else {
            None
        })
    }

    /// Clears all index entries for a table.
    pub fn clear_table_indexes(&self, table: &TableName) -> Result<()> {
        let prefix = table_index_prefix(table);
        let keys = collect_index_keys_for_prefix(self, &prefix)?;
        if keys.is_empty() {
            return Ok(());
        }

        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut index_table = write_txn.open_table(INDEXES).map_err(map_redb_error)?;
            for key in keys {
                index_table.remove(key.as_slice()).map_err(map_redb_error)?;
            }
        }
        self.commit_write_txn(write_txn)?;
        Ok(())
    }

    /// Rebuilds all indexes for a table from the current document set.
    pub fn rebuild_table_indexes(
        &self,
        table: &TableName,
        indexes: &[IndexDefinition],
    ) -> Result<()> {
        self.clear_table_indexes(table)?;
        if indexes.is_empty() {
            return Ok(());
        }

        let documents = self.scan_table(table)?;
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut index_table = write_txn.open_table(INDEXES).map_err(map_redb_error)?;
            for document in documents {
                for index in indexes {
                    if let Some(key) = index_key_for_document(&document, index)? {
                        index_table
                            .insert(key.as_slice(), EMPTY_INDEX_VALUE)
                            .map_err(map_redb_error)?;
                    }
                }
            }
        }
        self.commit_write_txn(write_txn)?;
        Ok(())
    }
}
