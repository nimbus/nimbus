use neovex_core::{Document, DocumentId, IndexDefinition, Result, TableName};

use super::encoding::encode_index_value;

/// Builds a full index key for a specific value and document.
fn index_key(
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
fn encoded_index_key_for_document(
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
pub(crate) fn index_key_for_document(
    document: &Document,
    index: &IndexDefinition,
) -> Result<Option<Vec<u8>>> {
    Ok(encoded_index_key_for_document(document, index)?
        .map(|encoded| index_key(&document.table, &index.name, &encoded, &document.id)))
}

/// Builds the prefix for all entries of an index.
pub(super) fn index_prefix(table: &TableName, index_name: &str) -> Vec<u8> {
    let mut prefix = table_index_prefix(table);
    prefix.extend_from_slice(index_name.as_bytes());
    prefix.push(0x00);
    prefix
}

/// Builds the prefix for a specific indexed value.
pub(super) fn index_value_prefix(
    table: &TableName,
    index_name: &str,
    encoded_value: &[u8],
) -> Vec<u8> {
    let mut prefix = index_prefix(table, index_name);
    prefix.extend_from_slice(encoded_value);
    prefix
}

/// Extracts the document id from an index key.
pub(super) fn doc_id_from_index_key(key: &[u8]) -> DocumentId {
    let bytes: [u8; 16] = key[key.len() - 16..]
        .try_into()
        .expect("index key should end with a document id");
    DocumentId::from_bytes(bytes)
}

pub(super) fn table_index_prefix(table: &TableName) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(table.as_str().len() + 1);
    prefix.extend_from_slice(table.as_str().as_bytes());
    prefix.push(0x00);
    prefix
}

pub(super) fn encoded_value_from_index_key(key: &[u8], prefix_len: usize) -> &[u8] {
    &key[prefix_len..key.len() - 16]
}
