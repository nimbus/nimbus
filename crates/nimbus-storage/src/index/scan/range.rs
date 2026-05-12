use std::cmp::Ordering;

use nimbus_core::{Document, Result, TableName};
use redb::{ReadTransaction, TableError};
use serde_json::Value;

use crate::keys::document_key;
use crate::store::{DOCUMENTS, INDEXES, map_redb_error};

use super::super::bounds::composite_range_scan_bounds;
use super::super::encoding::encode_index_value;
use super::super::keyspace::{doc_id_from_index_key, encoded_value_from_index_key, index_prefix};
use super::read::{decode_document, scan_documents_for_index_key_bounds_in_read_txn};

#[allow(clippy::too_many_arguments)]
pub(super) fn index_scan_range_in_read_txn(
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
        let doc_key = document_key(table, &doc_id);
        if let Some(value) = documents_table
            .get(doc_key.as_slice())
            .map_err(map_redb_error)?
        {
            documents.push(decode_document(value.value())?);
        }
    }
    Ok(documents)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn index_scan_composite_range_in_read_txn(
    read_txn: &ReadTransaction,
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
        read_txn,
        table,
        &match_prefix,
        &start_key,
        end_key.as_deref(),
        check_cancel,
    )
}
