use nimbus_core::{Document, Result, TableName};
use redb::{ReadTransaction, TableError};

use crate::document_codec::decode_document_msgpack;
use crate::keys::document_key;
use crate::store::{DOCUMENTS, INDEXES, map_redb_error};

use super::super::keyspace::doc_id_from_index_key;

pub(super) fn decode_document(bytes: &[u8]) -> Result<Document> {
    decode_document_msgpack(bytes)
        .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))
}

pub(super) fn scan_documents_for_index_key_bounds_in_read_txn(
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
            let doc_key = document_key(table, &doc_id);
            if let Some(value) = documents_table
                .get(doc_key.as_slice())
                .map_err(map_redb_error)?
            {
                documents.push(decode_document(value.value())?);
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
            let doc_key = document_key(table, &doc_id);
            if let Some(value) = documents_table
                .get(doc_key.as_slice())
                .map_err(map_redb_error)?
            {
                documents.push(decode_document(value.value())?);
            }
        }
    }
    Ok(documents)
}
