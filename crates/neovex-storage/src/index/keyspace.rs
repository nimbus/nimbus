use neovex_core::{Document, DocumentId, IndexDefinition, Result, TableName};
use std::str::FromStr;

use super::encoding::encode_index_value;

const INDEX_KEY_DOC_ID_LENGTH_BYTES: usize = 2;

/// Builds a full index key for a specific value and document.
fn index_key(
    table: &TableName,
    index_name: &str,
    encoded_value: &[u8],
    doc_id: &DocumentId,
) -> Vec<u8> {
    let mut key = index_prefix(table, index_name);
    key.extend_from_slice(encoded_value);
    let doc_id_bytes = doc_id.as_str().as_bytes();
    let doc_id_length =
        u16::try_from(doc_id_bytes.len()).expect("document ids should fit in index key trailer");
    key.extend_from_slice(doc_id_bytes);
    key.extend_from_slice(&doc_id_length.to_be_bytes());
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
    let doc_id_start = encoded_value_end(key);
    let doc_id_end = key.len() - INDEX_KEY_DOC_ID_LENGTH_BYTES;
    let doc_id = std::str::from_utf8(&key[doc_id_start..doc_id_end])
        .expect("index key should end with a UTF-8 document id");
    DocumentId::from_str(doc_id).expect("index key should end with a valid document id")
}

pub(super) fn table_index_prefix(table: &TableName) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(table.as_str().len() + 1);
    prefix.extend_from_slice(table.as_str().as_bytes());
    prefix.push(0x00);
    prefix
}

pub(super) fn encoded_value_from_index_key(key: &[u8], prefix_len: usize) -> &[u8] {
    &key[prefix_len..encoded_value_end(key)]
}

fn encoded_value_end(key: &[u8]) -> usize {
    let doc_id_length_offset = key
        .len()
        .checked_sub(INDEX_KEY_DOC_ID_LENGTH_BYTES)
        .expect("index key should include a document id length trailer");
    let doc_id_length_bytes: [u8; INDEX_KEY_DOC_ID_LENGTH_BYTES] = key[doc_id_length_offset..]
        .try_into()
        .expect("index key should include a document id length trailer");
    let doc_id_length = usize::from(u16::from_be_bytes(doc_id_length_bytes));
    doc_id_length_offset
        .checked_sub(doc_id_length)
        .expect("index key should contain a full document id payload")
}
