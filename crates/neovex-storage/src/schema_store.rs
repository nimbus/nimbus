use neovex_core::{Error, Result, Schema, TableName, TableSchema};
use redb::{ReadableTable, TableError};

use crate::index::{encode_index_value, index_key};
use crate::keys::{prefix_end, table_prefix};
use crate::store::{DOCUMENTS, INDEXES, SCHEMAS, TenantStore, map_redb_error};

impl TenantStore {
    /// Loads the tenant schema from storage.
    pub fn load_schema(&self) -> Result<Schema> {
        let read_txn = self.db.begin_read().map_err(map_redb_error)?;
        let table_handle = match read_txn.open_table(SCHEMAS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Schema::default()),
            Err(error) => return Err(map_redb_error(error)),
        };

        let mut schema = Schema::default();
        for item in table_handle.iter().map_err(map_redb_error)? {
            let (_, value) = item.map_err(map_redb_error)?;
            let table_schema: TableSchema = rmp_serde::from_slice(value.value())
                .map_err(|error| Error::Serialization(error.to_string()))?;
            schema
                .tables
                .insert(table_schema.table.clone(), table_schema);
        }

        Ok(schema)
    }

    /// Saves a single table schema.
    pub fn save_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        let payload = rmp_serde::to_vec(table_schema)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut table_handle = write_txn.open_table(SCHEMAS).map_err(map_redb_error)?;
            table_handle
                .insert(table_schema.table.as_str(), payload.as_slice())
                .map_err(map_redb_error)?;
        }
        write_txn.commit().map_err(map_redb_error)?;
        Ok(())
    }

    /// Replaces a table schema and its index contents in one transaction.
    pub fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        let payload = rmp_serde::to_vec(table_schema)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let keys_to_remove = collect_table_index_keys(&write_txn, &table_schema.table)?;
        let keys_to_insert = collect_rebuilt_index_keys(&write_txn, table_schema)?;

        {
            let mut index_table = write_txn.open_table(INDEXES).map_err(map_redb_error)?;
            for key in keys_to_remove {
                index_table.remove(key.as_slice()).map_err(map_redb_error)?;
            }
            for key in keys_to_insert {
                index_table
                    .insert(key.as_slice(), &[] as &[u8])
                    .map_err(map_redb_error)?;
            }
        }
        {
            let mut schema_table = write_txn.open_table(SCHEMAS).map_err(map_redb_error)?;
            schema_table
                .insert(table_schema.table.as_str(), payload.as_slice())
                .map_err(map_redb_error)?;
        }

        write_txn.commit().map_err(map_redb_error)?;
        Ok(())
    }

    /// Deletes only the persisted schema entry for a table.
    pub fn delete_table_schema_entry(&self, table: &TableName) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        {
            let mut table_handle = match write_txn.open_table(SCHEMAS) {
                Ok(table_handle) => table_handle,
                Err(TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(error) => return Err(map_redb_error(error)),
            };
            table_handle
                .remove(table.as_str())
                .map_err(map_redb_error)?;
        }
        write_txn.commit().map_err(map_redb_error)?;
        Ok(())
    }

    /// Deletes a table schema and its index contents in one transaction.
    pub fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        let write_txn = self.db.begin_write().map_err(map_redb_error)?;
        let keys_to_remove = collect_table_index_keys(&write_txn, table)?;

        {
            let mut index_table = write_txn.open_table(INDEXES).map_err(map_redb_error)?;
            for key in keys_to_remove {
                index_table.remove(key.as_slice()).map_err(map_redb_error)?;
            }
        }
        {
            let mut schema_table = match write_txn.open_table(SCHEMAS) {
                Ok(schema_table) => schema_table,
                Err(TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(error) => return Err(map_redb_error(error)),
            };
            schema_table
                .remove(table.as_str())
                .map_err(map_redb_error)?;
        }

        write_txn.commit().map_err(map_redb_error)?;
        Ok(())
    }
}

fn table_index_prefix(table: &TableName) -> Vec<u8> {
    let mut prefix = Vec::with_capacity(table.as_str().len() + 1);
    prefix.extend_from_slice(table.as_str().as_bytes());
    prefix.push(0x00);
    prefix
}

fn collect_table_index_keys(
    write_txn: &redb::WriteTransaction,
    table: &TableName,
) -> Result<Vec<Vec<u8>>> {
    let prefix = table_index_prefix(table);
    let index_table = match write_txn.open_table(INDEXES) {
        Ok(index_table) => index_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };

    let mut keys = Vec::new();
    match prefix_end(&prefix) {
        Some(end) => {
            for item in index_table
                .range(prefix.as_slice()..end.as_slice())
                .map_err(map_redb_error)?
            {
                let (key, _) = item.map_err(map_redb_error)?;
                keys.push(key.value().to_vec());
            }
        }
        None => {
            for item in index_table
                .range(prefix.as_slice()..)
                .map_err(map_redb_error)?
            {
                let (key, _) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(&prefix) {
                    break;
                }
                keys.push(key.value().to_vec());
            }
        }
    }

    Ok(keys)
}

fn collect_rebuilt_index_keys(
    write_txn: &redb::WriteTransaction,
    table_schema: &TableSchema,
) -> Result<Vec<Vec<u8>>> {
    let documents_table = match write_txn.open_table(DOCUMENTS) {
        Ok(documents_table) => documents_table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let start = table_prefix(&table_schema.table);
    let mut keys = Vec::new();

    match prefix_end(&start) {
        Some(end) => {
            for item in documents_table
                .range(start.as_slice()..end.as_slice())
                .map_err(map_redb_error)?
            {
                let (_, value) = item.map_err(map_redb_error)?;
                let document = neovex_core::Document::from_msgpack(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                push_document_index_keys(&mut keys, &document, table_schema)?;
            }
        }
        None => {
            for item in documents_table
                .range(start.as_slice()..)
                .map_err(map_redb_error)?
            {
                let (key, value) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(&start) {
                    break;
                }
                let document = neovex_core::Document::from_msgpack(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                push_document_index_keys(&mut keys, &document, table_schema)?;
            }
        }
    }

    Ok(keys)
}

fn push_document_index_keys(
    keys: &mut Vec<Vec<u8>>,
    document: &neovex_core::Document,
    table_schema: &TableSchema,
) -> Result<()> {
    for index in &table_schema.indexes {
        if let Some(value) = document.get_field(&index.field) {
            let encoded = encode_index_value(value)?;
            keys.push(index_key(
                &table_schema.table,
                &index.name,
                &encoded,
                &document.id,
            ));
        }
    }
    Ok(())
}
