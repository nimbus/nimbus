use neovex_core::{Error, Result, Schema, TableName, TableSchema};
use redb::{ReadableTable, TableError};

use crate::index::{encode_index_value, index_key};
use crate::keys::{prefix_end, table_prefix};
use crate::store::{
    DOCUMENTS, INDEXES, SCHEMAS, TenantStore, TenantWriteTransaction, map_redb_error,
};

impl TenantWriteTransaction {
    pub fn save_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        let payload = rmp_serde::to_vec(table_schema)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let mut table_handle = self
            .write_txn()?
            .open_table(SCHEMAS)
            .map_err(map_redb_error)?;
        table_handle
            .insert(table_schema.table.as_str(), payload.as_slice())
            .map_err(map_redb_error)?;
        Ok(())
    }

    pub fn replace_table_schema(&mut self, table_schema: &TableSchema) -> Result<()> {
        self.check_cancel()?;
        let payload = rmp_serde::to_vec(table_schema)
            .map_err(|error| Error::Serialization(error.to_string()))?;
        let keys_to_remove =
            collect_table_index_keys(self.write_txn()?, &table_schema.table, &mut || {
                self.check_cancel()
            })?;
        let keys_to_insert =
            collect_rebuilt_index_keys(self.write_txn()?, table_schema, &mut || {
                self.check_cancel()
            })?;

        {
            let mut index_table = self
                .write_txn()?
                .open_table(INDEXES)
                .map_err(map_redb_error)?;
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
            let mut schema_table = self
                .write_txn()?
                .open_table(SCHEMAS)
                .map_err(map_redb_error)?;
            schema_table
                .insert(table_schema.table.as_str(), payload.as_slice())
                .map_err(map_redb_error)?;
        }

        Ok(())
    }

    pub fn delete_table_schema_entry(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        let mut table_handle = match self.write_txn()?.open_table(SCHEMAS) {
            Ok(table_handle) => table_handle,
            Err(TableError::TableDoesNotExist(_)) => return Ok(()),
            Err(error) => return Err(map_redb_error(error)),
        };
        table_handle
            .remove(table.as_str())
            .map_err(map_redb_error)?;
        Ok(())
    }

    pub fn delete_table_schema(&mut self, table: &TableName) -> Result<()> {
        self.check_cancel()?;
        let keys_to_remove =
            collect_table_index_keys(self.write_txn()?, table, &mut || self.check_cancel())?;

        {
            let mut index_table = self
                .write_txn()?
                .open_table(INDEXES)
                .map_err(map_redb_error)?;
            for key in keys_to_remove {
                index_table.remove(key.as_slice()).map_err(map_redb_error)?;
            }
        }
        {
            let mut schema_table = match self.write_txn()?.open_table(SCHEMAS) {
                Ok(schema_table) => schema_table,
                Err(TableError::TableDoesNotExist(_)) => return Ok(()),
                Err(error) => return Err(map_redb_error(error)),
            };
            schema_table
                .remove(table.as_str())
                .map_err(map_redb_error)?;
        }
        Ok(())
    }
}

impl TenantStore {
    /// Loads the tenant schema from storage.
    pub fn load_schema(&self) -> Result<Schema> {
        self.read_snapshot()?.load_schema()
    }

    /// Saves a single table schema.
    pub fn save_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        self.execute_write(move |transaction| transaction.save_table_schema(table_schema))?;
        Ok(())
    }

    /// Replaces a table schema and its index contents in one transaction.
    pub fn replace_table_schema(&self, table_schema: &TableSchema) -> Result<()> {
        self.execute_write(move |transaction| transaction.replace_table_schema(table_schema))?;
        Ok(())
    }

    /// Replaces the full tenant schema and reconciles derived indexes in one pass.
    pub fn replace_schema(&self, schema: &Schema) -> Result<()> {
        let current = self.load_schema()?;
        if current == *schema {
            return Ok(());
        }

        let mut tables_to_remove = current
            .tables
            .keys()
            .filter(|table| !schema.tables.contains_key(*table))
            .cloned()
            .collect::<Vec<_>>();
        tables_to_remove.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));

        let mut tables_to_replace = schema
            .tables
            .iter()
            .filter_map(|(table, table_schema)| {
                (current.tables.get(table) != Some(table_schema)).then_some(table_schema.clone())
            })
            .collect::<Vec<_>>();
        tables_to_replace
            .sort_unstable_by(|left, right| left.table.as_str().cmp(right.table.as_str()));

        self.execute_write(move |transaction| {
            for table in &tables_to_remove {
                transaction.delete_table_schema(table)?;
            }
            for table_schema in &tables_to_replace {
                transaction.replace_table_schema(table_schema)?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Deletes only the persisted schema entry for a table.
    pub fn delete_table_schema_entry(&self, table: &TableName) -> Result<()> {
        self.execute_write(move |transaction| transaction.delete_table_schema_entry(table))?;
        Ok(())
    }

    /// Deletes a table schema and its index contents in one transaction.
    pub fn delete_table_schema(&self, table: &TableName) -> Result<()> {
        self.execute_write(move |transaction| transaction.delete_table_schema(table))?;
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
    check_cancel: &mut dyn FnMut() -> Result<()>,
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
                check_cancel()?;
                let (key, _) = item.map_err(map_redb_error)?;
                keys.push(key.value().to_vec());
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
                keys.push(key.value().to_vec());
            }
        }
    }

    Ok(keys)
}

fn collect_rebuilt_index_keys(
    write_txn: &redb::WriteTransaction,
    table_schema: &TableSchema,
    check_cancel: &mut dyn FnMut() -> Result<()>,
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
                check_cancel()?;
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
                check_cancel()?;
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
