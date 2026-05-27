use nimbus_core::{Result, TableId, TableLifecycleEvent, TableName, TenantEventKind};
use redb::{ReadableTable, TableError};

use crate::index::table_index_prefix;
use crate::keys::{prefix_end, table_prefix};

use super::table_catalog::{
    activate_hidden_table_identity_in_write_txn, hard_delete_deleting_table_identity_in_write_txn,
    mark_default_table_deleting_in_write_txn, resolve_table_id_in_write_txn,
    stage_hidden_table_identity_in_write_txn,
};
use super::{DOCUMENTS, INDEXES, SCHEMAS, TenantStore, TenantWriteTransaction, map_redb_error};

impl TenantStore {
    pub fn stage_hidden_table_identity(&self, table: &TableName, table_id: &TableId) -> Result<()> {
        self.execute_write(|transaction| transaction.stage_hidden_table_identity(table, table_id))?;
        Ok(())
    }

    pub fn activate_hidden_table_identity(
        &self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<Option<TableId>> {
        Ok(self
            .execute_write(|transaction| {
                transaction.activate_hidden_table_identity(table, table_id)
            })?
            .value)
    }

    pub fn mark_table_deleting(&self, table: &TableName) -> Result<Option<TableId>> {
        Ok(self
            .execute_write(|transaction| transaction.mark_table_deleting(table))?
            .value)
    }

    pub fn hard_delete_table_identity(&self, table_id: &TableId) -> Result<bool> {
        self.retention_floor
            .ensure_hard_delete_allowed(table_id, self.latest_sequence()?)?;
        Ok(self
            .execute_write(|transaction| transaction.hard_delete_table_identity(table_id))?
            .value)
    }
}

impl TenantWriteTransaction {
    pub(crate) fn stage_hidden_table_identity(
        &mut self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<()> {
        self.check_cancel()?;
        stage_hidden_table_identity_in_write_txn(self.write_txn()?, table, table_id)?;
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::StageHidden {
                table: table.clone(),
                table_id: table_id.clone(),
            },
        });
        Ok(())
    }

    pub(crate) fn activate_hidden_table_identity(
        &mut self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<Option<TableId>> {
        self.check_cancel()?;
        let replaced_table_id =
            activate_hidden_table_identity_in_write_txn(self.write_txn()?, table, table_id)?;
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::ActivateHidden {
                table: table.clone(),
                table_id: table_id.clone(),
                replaced_table_id: replaced_table_id.clone(),
            },
        });
        Ok(replaced_table_id)
    }

    pub(crate) fn mark_table_deleting(&mut self, table: &TableName) -> Result<Option<TableId>> {
        self.check_cancel()?;
        let table_id = mark_default_table_deleting_in_write_txn(self.write_txn()?, table)?;
        if let Some(table_id) = table_id.as_ref() {
            self.record_tenant_event(TenantEventKind::TableLifecycle {
                lifecycle: TableLifecycleEvent::MarkDeleting {
                    table: table.clone(),
                    table_id: table_id.clone(),
                },
            });
        }
        Ok(table_id)
    }

    pub(crate) fn hard_delete_table_identity(&mut self, table_id: &TableId) -> Result<bool> {
        self.check_cancel()?;
        let Some(table) =
            hard_delete_deleting_table_identity_in_write_txn(self.write_txn()?, table_id)?
        else {
            return Ok(false);
        };
        remove_documents_for_table_id(self.write_txn()?, table_id)?;
        remove_indexes_for_table_id(self.write_txn()?, table_id)?;
        if resolve_table_id_in_write_txn(self.write_txn()?, &table)?.is_none() {
            remove_schema_for_table(self.write_txn()?, &table)?;
        }
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::HardDelete {
                table,
                table_id: table_id.clone(),
            },
        });
        Ok(true)
    }
}

fn remove_documents_for_table_id(
    write_txn: &redb::WriteTransaction,
    table_id: &TableId,
) -> Result<()> {
    remove_prefixed_binary_rows(write_txn, DOCUMENTS, table_prefix(table_id))
}

fn remove_indexes_for_table_id(
    write_txn: &redb::WriteTransaction,
    table_id: &TableId,
) -> Result<()> {
    remove_prefixed_binary_rows(write_txn, INDEXES, table_index_prefix(table_id))
}

fn remove_schema_for_table(write_txn: &redb::WriteTransaction, table: &TableName) -> Result<()> {
    let mut schemas = match write_txn.open_table(SCHEMAS) {
        Ok(schemas) => schemas,
        Err(TableError::TableDoesNotExist(_)) => return Ok(()),
        Err(error) => return Err(map_redb_error(error)),
    };
    schemas.remove(table.as_str()).map_err(map_redb_error)?;
    Ok(())
}

fn remove_prefixed_binary_rows(
    write_txn: &redb::WriteTransaction,
    table_definition: redb::TableDefinition<&[u8], &[u8]>,
    prefix: Vec<u8>,
) -> Result<()> {
    let mut table = match write_txn.open_table(table_definition) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => return Ok(()),
        Err(error) => return Err(map_redb_error(error)),
    };
    let keys = prefixed_keys(&table, prefix.as_slice())?;
    for key in keys {
        table.remove(key.as_slice()).map_err(map_redb_error)?;
    }
    Ok(())
}

fn prefixed_keys(table: &redb::Table<'_, &[u8], &[u8]>, prefix: &[u8]) -> Result<Vec<Vec<u8>>> {
    let mut keys = Vec::new();
    if let Some(end) = prefix_end(prefix) {
        for item in table
            .range(prefix..end.as_slice())
            .map_err(map_redb_error)?
        {
            let (key, _) = item.map_err(map_redb_error)?;
            if !key.value().starts_with(prefix) {
                break;
            }
            keys.push(key.value().to_vec());
        }
    } else {
        for item in table.range(prefix..).map_err(map_redb_error)? {
            let (key, _) = item.map_err(map_redb_error)?;
            if !key.value().starts_with(prefix) {
                break;
            }
            keys.push(key.value().to_vec());
        }
    }
    Ok(keys)
}
