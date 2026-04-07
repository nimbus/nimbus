use neovex_core::{IndexDefinition, Result, TableName};
use redb::TableError;

use crate::keys::prefix_end;
use crate::store::{INDEXES, TenantStore, map_redb_error};

use super::super::keyspace::{index_key_for_document, table_index_prefix};
use super::EMPTY_INDEX_VALUE;

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

impl TenantStore {
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
