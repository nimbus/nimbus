use std::cmp::Ordering;

use neovex_core::{
    CommitEntry, Document, DocumentId, IndexDefinition, Result, TableName, WriteOp, WriteOpType,
};
use redb::{ReadTransaction, ReadableTable, TableError};
use serde_json::Value;

use crate::keys::prefix_end;
use crate::store::{
    DOCUMENTS, INDEXES, TenantReadSnapshot, TenantStore, TenantWriteTransaction, map_redb_error,
};

const EMPTY_INDEX_VALUE: &[u8] = &[];

/// Encodes a scalar JSON value to bytes that preserve lexicographic order.
pub fn encode_index_value(value: &Value) -> Result<Vec<u8>> {
    match value {
        Value::Null => Ok(vec![0x00]),
        Value::Bool(false) => Ok(vec![0x01, 0x00]),
        Value::Bool(true) => Ok(vec![0x01, 0x01]),
        Value::Number(number) => {
            let float = number.as_f64().ok_or_else(|| {
                neovex_core::Error::InvalidInput("unsupported numeric index value".to_string())
            })?;
            let mut bytes = float.to_bits().to_be_bytes();
            if float.is_sign_positive() || float == 0.0 {
                bytes[0] ^= 0x80;
            } else {
                for byte in &mut bytes {
                    *byte = !*byte;
                }
            }
            let mut encoded = vec![0x02];
            encoded.extend_from_slice(&bytes);
            Ok(encoded)
        }
        Value::String(string) => {
            let mut encoded = Vec::with_capacity(2 + string.len());
            encoded.push(0x03);
            for byte in string.as_bytes() {
                match byte {
                    0x00 => encoded.extend_from_slice(&[0x00, 0xFF]),
                    other => encoded.push(*other),
                }
            }
            encoded.extend_from_slice(&[0x00, 0x00]);
            Ok(encoded)
        }
        _ => Err(neovex_core::Error::InvalidInput(
            "only null, boolean, number, and string fields are indexable in phase 2".to_string(),
        )),
    }
}

/// Encodes an ordered tuple of scalar JSON values for composite index scans.
pub fn encode_index_tuple(values: &[Value]) -> Result<Vec<u8>> {
    let mut encoded = Vec::new();
    for value in values {
        encoded.extend_from_slice(&encode_index_value(value)?);
    }
    Ok(encoded)
}

/// Builds a full index key for a specific value and document.
pub fn index_key(
    table: &TableName,
    index_name: &str,
    encoded_value: &[u8],
    doc_id: &DocumentId,
) -> Vec<u8> {
    let mut key = index_prefix(table, index_name);
    key.extend_from_slice(encoded_value);
    key.extend_from_slice(&doc_id.to_bytes());
    key
}

/// Builds the encoded tuple payload for one document and index definition.
pub fn encoded_index_key_for_document(
    document: &Document,
    index: &IndexDefinition,
) -> Result<Option<Vec<u8>>> {
    let mut encoded = Vec::new();
    for field in &index.fields {
        let Some(value) = document.get_field(field) else {
            return Ok(None);
        };
        encoded.extend_from_slice(&encode_index_value(value)?);
    }
    Ok(Some(encoded))
}

/// Builds the full index key for one document and index definition.
pub fn index_key_for_document(
    document: &Document,
    index: &IndexDefinition,
) -> Result<Option<Vec<u8>>> {
    Ok(encoded_index_key_for_document(document, index)?
        .map(|encoded| index_key(&document.table, &index.name, &encoded, &document.id)))
}

/// Builds the prefix for all entries of an index.
pub fn index_prefix(table: &TableName, index_name: &str) -> Vec<u8> {
    let mut prefix = table_index_prefix(table);
    prefix.extend_from_slice(index_name.as_bytes());
    prefix.push(0x00);
    prefix
}

/// Builds the prefix for a specific indexed value.
pub fn index_value_prefix(table: &TableName, index_name: &str, encoded_value: &[u8]) -> Vec<u8> {
    let mut prefix = index_prefix(table, index_name);
    prefix.extend_from_slice(encoded_value);
    prefix
}

/// Extracts the document id from an index key.
pub fn doc_id_from_index_key(key: &[u8]) -> DocumentId {
    let bytes: [u8; 16] = key[key.len() - 16..]
        .try_into()
        .expect("index key should end with a document id");
    DocumentId::from_bytes(bytes)
}

fn table_index_prefix(table: &TableName) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(table.as_str().len() + 1);
    prefix.extend_from_slice(table.as_str().as_bytes());
    prefix.push(0x00);
    prefix
}

fn encoded_value_from_index_key(key: &[u8], prefix_len: usize) -> &[u8] {
    &key[prefix_len..key.len() - 16]
}

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

fn scan_documents_for_index_key_bounds_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    match_prefix: &[u8],
    start_key: &[u8],
    end_key: Option<&[u8]>,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let index_table = match read_txn.open_table(INDEXES) {
        Ok(index_table) => index_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let documents_table = match read_txn.open_table(DOCUMENTS) {
        Ok(documents_table) => documents_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };

    let mut documents = Vec::new();
    if let Some(end_key) = end_key {
        for item in index_table
            .range(start_key..end_key)
            .map_err(map_redb_error)?
        {
            check_cancel()?;
            let (key, _) = item.map_err(map_redb_error)?;
            if !key.value().starts_with(match_prefix) {
                break;
            }
            let doc_id = doc_id_from_index_key(key.value());
            let doc_key = crate::keys::document_key(table, &doc_id);
            if let Some(value) = documents_table
                .get(doc_key.as_slice())
                .map_err(map_redb_error)?
            {
                documents.push(
                    Document::from_msgpack(value.value())
                        .map_err(|error| neovex_core::Error::Serialization(error.to_string()))?,
                );
            }
        }
    } else {
        for item in index_table.range(start_key..).map_err(map_redb_error)? {
            check_cancel()?;
            let (key, _) = item.map_err(map_redb_error)?;
            if !key.value().starts_with(match_prefix) {
                break;
            }
            let doc_id = doc_id_from_index_key(key.value());
            let doc_key = crate::keys::document_key(table, &doc_id);
            if let Some(value) = documents_table
                .get(doc_key.as_slice())
                .map_err(map_redb_error)?
            {
                documents.push(
                    Document::from_msgpack(value.value())
                        .map_err(|error| neovex_core::Error::Serialization(error.to_string()))?,
                );
            }
        }
    }
    Ok(documents)
}

fn scan_documents_for_index_key_bounds(
    store: &TenantStore,
    table: &TableName,
    match_prefix: &[u8],
    start_key: &[u8],
    end_key: Option<&[u8]>,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let read_txn = store.db.begin_read().map_err(map_redb_error)?;
    scan_documents_for_index_key_bounds_in_read_txn(
        &read_txn,
        table,
        match_prefix,
        start_key,
        end_key,
        check_cancel,
    )
}

fn index_scan_eq_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    index_name: &str,
    value: &Value,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let index_table = match read_txn.open_table(INDEXES) {
        Ok(index_table) => index_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let documents_table = match read_txn.open_table(DOCUMENTS) {
        Ok(documents_table) => documents_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };

    let encoded = encode_index_value(value)?;
    let prefix = index_value_prefix(table, index_name, &encoded);
    let mut documents = Vec::new();
    match prefix_end(&prefix) {
        Some(end) => {
            for item in index_table
                .range(prefix.as_slice()..end.as_slice())
                .map_err(map_redb_error)?
            {
                check_cancel()?;
                let (key, _) = item.map_err(map_redb_error)?;
                let doc_id = doc_id_from_index_key(key.value());
                let doc_key = crate::keys::document_key(table, &doc_id);
                if let Some(value) = documents_table
                    .get(doc_key.as_slice())
                    .map_err(map_redb_error)?
                {
                    documents.push(
                        Document::from_msgpack(value.value()).map_err(|error| {
                            neovex_core::Error::Serialization(error.to_string())
                        })?,
                    );
                }
            }
        }
        None => {
            for item in index_table
                .range(prefix.as_slice()..)
                .map_err(map_redb_error)?
            {
                check_cancel()?;
                let (key, _) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(&prefix) {
                    break;
                }
                let doc_id = doc_id_from_index_key(key.value());
                let doc_key = crate::keys::document_key(table, &doc_id);
                if let Some(value) = documents_table
                    .get(doc_key.as_slice())
                    .map_err(map_redb_error)?
                {
                    documents.push(
                        Document::from_msgpack(value.value()).map_err(|error| {
                            neovex_core::Error::Serialization(error.to_string())
                        })?,
                    );
                }
            }
        }
    }
    Ok(documents)
}

#[allow(clippy::too_many_arguments)]
fn index_scan_range_in_read_txn(
    read_txn: &ReadTransaction,
    table: &TableName,
    index_name: &str,
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let index_table = match read_txn.open_table(INDEXES) {
        Ok(index_table) => index_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let documents_table = match read_txn.open_table(DOCUMENTS) {
        Ok(documents_table) => documents_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };

    let prefix = index_prefix(table, index_name);
    let prefix_len = prefix.len();
    let start = start.map(encode_index_value).transpose()?;
    let end = end.map(encode_index_value).transpose()?;

    let mut documents = Vec::new();
    for item in index_table
        .range(prefix.as_slice()..)
        .map_err(map_redb_error)?
    {
        check_cancel()?;
        let (key, _) = item.map_err(map_redb_error)?;
        if !key.value().starts_with(&prefix) {
            break;
        }
        let encoded_value = encoded_value_from_index_key(key.value(), prefix_len);
        if let Some(start) = start.as_ref() {
            match encoded_value.cmp(start.as_slice()) {
                Ordering::Less => continue,
                Ordering::Equal if !start_inclusive => continue,
                Ordering::Equal | Ordering::Greater => {}
            }
        }
        if let Some(end) = end.as_ref() {
            match encoded_value.cmp(end.as_slice()) {
                Ordering::Greater => continue,
                Ordering::Equal if !end_inclusive => continue,
                Ordering::Equal | Ordering::Less => {}
            }
        }

        let doc_id = doc_id_from_index_key(key.value());
        let doc_key = crate::keys::document_key(table, &doc_id);
        if let Some(value) = documents_table
            .get(doc_key.as_slice())
            .map_err(map_redb_error)?
        {
            documents.push(
                Document::from_msgpack(value.value())
                    .map_err(|error| neovex_core::Error::Serialization(error.to_string()))?,
            );
        }
    }
    Ok(documents)
}

type CompositeRangeScanBounds = (Vec<u8>, Vec<u8>, Option<Vec<u8>>);

fn composite_range_scan_bounds(
    table: &TableName,
    index_name: &str,
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<CompositeRangeScanBounds> {
    let encoded_prefix = encode_index_tuple(exact_prefix)?;
    let match_prefix = index_value_prefix(table, index_name, &encoded_prefix);
    let start_key = if let Some(start) = start {
        let mut start_key = match_prefix.clone();
        start_key.extend_from_slice(&encode_index_value(start)?);
        if start_inclusive {
            start_key
        } else {
            let Some(next_key) = prefix_end(&start_key) else {
                return Ok((match_prefix, Vec::new(), Some(Vec::new())));
            };
            next_key
        }
    } else {
        match_prefix.clone()
    };
    let end_key = if let Some(end) = end {
        let mut end_key = match_prefix.clone();
        end_key.extend_from_slice(&encode_index_value(end)?);
        if end_inclusive {
            prefix_end(&end_key)
        } else {
            Some(end_key)
        }
    } else {
        prefix_end(&match_prefix)
    };

    Ok((match_prefix, start_key, end_key))
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
        let key = crate::keys::document_key(table, id);
        let (old_document, new_document, payload) = {
            let documents = self
                .write_txn()?
                .open_table(DOCUMENTS)
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
                .open_table(DOCUMENTS)
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
        let key = crate::keys::document_key(table, id);
        let old_document = {
            let mut documents = self
                .write_txn()?
                .open_table(DOCUMENTS)
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

    /// Returns documents whose indexed field equals the provided value.
    pub fn index_scan_eq(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
    ) -> Result<Vec<Document>> {
        self.index_scan_eq_cancellable(table, index_name, value, &mut || Ok(()))
    }

    /// Returns documents whose indexed field equals the provided value, checking for cancellation between rows.
    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        index_scan_eq_in_read_txn(&read_txn, table, index_name, value, check_cancel)
    }

    /// Returns documents whose indexed field falls within the provided range.
    pub fn index_scan_range(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> Result<Vec<Document>> {
        self.index_scan_range_cancellable(
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            &mut || Ok(()),
        )
    }

    /// Returns documents whose indexed field falls within the provided range, checking for cancellation between rows.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        index_scan_range_in_read_txn(
            &read_txn,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    /// Returns documents whose indexed tuple matches the provided exact leading prefix.
    pub fn index_scan_prefix(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
    ) -> Result<Vec<Document>> {
        self.index_scan_prefix_cancellable(table, index_name, prefix_values, &mut || Ok(()))
    }

    /// Returns documents whose indexed tuple matches the provided exact leading prefix, checking for cancellation between rows.
    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let encoded_prefix = encode_index_tuple(prefix_values)?;
        let match_prefix = index_value_prefix(table, index_name, &encoded_prefix);
        let end_key = prefix_end(&match_prefix);
        scan_documents_for_index_key_bounds(
            self,
            table,
            &match_prefix,
            &match_prefix,
            end_key.as_deref(),
            check_cancel,
        )
    }

    /// Returns documents whose composite index matches an exact leading prefix and one range on the next field.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
    ) -> Result<Vec<Document>> {
        self.index_scan_composite_range_cancellable(
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
            &mut || Ok(()),
        )
    }

    /// Returns documents whose composite index matches an exact leading prefix and one range on the next field, checking for cancellation between rows.
    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let (match_prefix, start_key, end_key) = composite_range_scan_bounds(
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
        )?;
        if start_key.is_empty() {
            return Ok(Vec::new());
        }

        scan_documents_for_index_key_bounds(
            self,
            table,
            &match_prefix,
            &start_key,
            end_key.as_deref(),
            check_cancel,
        )
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

impl TenantReadSnapshot {
    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        index_scan_eq_in_read_txn(&self.read_txn, table, index_name, value, check_cancel)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        index_scan_range_in_read_txn(
            &self.read_txn,
            table,
            index_name,
            start,
            end,
            start_inclusive,
            end_inclusive,
            check_cancel,
        )
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let encoded_prefix = encode_index_tuple(prefix_values)?;
        let match_prefix = index_value_prefix(table, index_name, &encoded_prefix);
        let end_key = prefix_end(&match_prefix);
        scan_documents_for_index_key_bounds_in_read_txn(
            &self.read_txn,
            table,
            &match_prefix,
            &match_prefix,
            end_key.as_deref(),
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        let (match_prefix, start_key, end_key) = composite_range_scan_bounds(
            table,
            index_name,
            exact_prefix,
            start,
            end,
            start_inclusive,
            end_inclusive,
        )?;
        if start_key.is_empty() {
            return Ok(Vec::new());
        }

        scan_documents_for_index_key_bounds_in_read_txn(
            &self.read_txn,
            table,
            &match_prefix,
            &start_key,
            end_key.as_deref(),
            check_cancel,
        )
    }
}
