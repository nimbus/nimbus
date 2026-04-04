use neovex_core::{Document, Result, TableName, TableSchema};
use redb::{ReadableTable, TableError};

use crate::index::index_key_for_document;

use super::{EMPTY_TABLE_VALUE, INDEXES, SCHEMAS, map_redb_error};

pub(super) fn rewrite_document_indexes_in_write_txn(
    write_txn: &redb::WriteTransaction,
    previous: Option<&Document>,
    current: Option<&Document>,
) -> Result<()> {
    let table = current
        .map(|document| &document.table)
        .or_else(|| previous.map(|document| &document.table))
        .ok_or_else(|| {
            neovex_core::Error::Internal(
                "durable journal index rewrite requires a document snapshot".to_string(),
            )
        })?;
    let Some(table_schema) = load_table_schema_in_write_txn(write_txn, table)? else {
        return Ok(());
    };

    let mut index_table = write_txn.open_table(INDEXES).map_err(map_redb_error)?;
    if let Some(previous) = previous {
        for key in durable_record_index_keys(previous, &table_schema)? {
            index_table.remove(key.as_slice()).map_err(map_redb_error)?;
        }
    }
    if let Some(current) = current {
        for key in durable_record_index_keys(current, &table_schema)? {
            index_table
                .insert(key.as_slice(), EMPTY_TABLE_VALUE)
                .map_err(map_redb_error)?;
        }
    }
    Ok(())
}

fn load_table_schema_in_write_txn(
    write_txn: &redb::WriteTransaction,
    table: &TableName,
) -> Result<Option<TableSchema>> {
    let schema_table = match write_txn.open_table(SCHEMAS) {
        Ok(schema_table) => schema_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };
    let Some(value) = schema_table.get(table.as_str()).map_err(map_redb_error)? else {
        return Ok(None);
    };
    let table_schema = rmp_serde::from_slice(value.value())
        .map_err(|error| neovex_core::Error::Serialization(error.to_string()))?;
    Ok(Some(table_schema))
}

pub(super) fn durable_record_index_keys(
    document: &Document,
    table_schema: &TableSchema,
) -> Result<Vec<Vec<u8>>> {
    let mut keys = Vec::new();
    for index in &table_schema.indexes {
        if let Some(key) = index_key_for_document(document, index)? {
            keys.push(key);
        }
    }
    Ok(keys)
}
