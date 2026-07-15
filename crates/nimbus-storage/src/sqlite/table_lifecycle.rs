use std::str::FromStr;

use super::*;
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};

impl SqliteTenantStore {
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

impl SqliteWriteTransaction {
    pub(crate) fn stage_hidden_table_identity(
        &mut self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<()> {
        self.check_cancel()?;
        let conn = self.connection_mut()?;
        if let Some((namespace, existing_table, state)) =
            table_identity_row_for_table_id(conn, table_id)?
        {
            return Err(Error::conflict(format!(
                "table id {} is already assigned to logical table {} in namespace {} with {} state",
                table_id, existing_table, namespace, state
            )));
        }

        conn.execute(
            "INSERT INTO table_catalog (namespace, table_name, table_id, state)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                hidden_table_namespace(table_id),
                table.as_str(),
                table_id.as_str(),
                TableState::Hidden.as_str()
            ],
        )
        .map_err(map_sqlite_error)?;
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
        let conn = self.connection_mut()?;
        let hidden_namespace = hidden_table_namespace(table_id);
        let Some((hidden_table_id, hidden_state)) =
            catalog_row(conn, hidden_namespace.as_str(), table)?
        else {
            return Err(Error::InvalidInput(format!(
                "hidden table identity {} for logical table {} does not exist",
                table_id, table
            )));
        };
        if &hidden_table_id != table_id || hidden_state != TableState::Hidden {
            return Err(Error::conflict(format!(
                "hidden table identity {} for logical table {} is cataloged as {} in {} state",
                table_id, table, hidden_table_id, hidden_state
            )));
        }

        let old_table_id = match catalog_row(conn, DEFAULT_TABLE_NAMESPACE, table)? {
            Some((active_table_id, TableState::Active)) => Some(active_table_id),
            Some((_, state)) => {
                return Err(Error::conflict(format!(
                    "logical table {} is already in {} lifecycle state",
                    table, state
                )));
            }
            None => None,
        };

        if let Some(old_table_id) = old_table_id.as_ref() {
            ensure_namespace_is_free(conn, deleting_table_namespace(old_table_id).as_str(), table)?;
            conn.execute(
                "UPDATE table_catalog
                 SET namespace = ?1, state = ?2
                 WHERE namespace = ?3 AND table_name = ?4",
                params![
                    deleting_table_namespace(old_table_id),
                    TableState::Deleting.as_str(),
                    DEFAULT_TABLE_NAMESPACE,
                    table.as_str()
                ],
            )
            .map_err(map_sqlite_error)?;
        }

        conn.execute(
            "UPDATE table_catalog
             SET namespace = ?1, state = ?2
             WHERE namespace = ?3 AND table_name = ?4 AND table_id = ?5",
            params![
                DEFAULT_TABLE_NAMESPACE,
                TableState::Active.as_str(),
                hidden_namespace,
                table.as_str(),
                table_id.as_str()
            ],
        )
        .map_err(map_sqlite_error)?;
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::ActivateHidden {
                table: table.clone(),
                table_id: table_id.clone(),
                replaced_table_id: old_table_id.clone(),
            },
        });
        Ok(old_table_id)
    }

    pub(crate) fn mark_table_deleting(&mut self, table: &TableName) -> Result<Option<TableId>> {
        self.check_cancel()?;
        let conn = self.connection_mut()?;
        let Some((table_id, state)) = catalog_row(conn, DEFAULT_TABLE_NAMESPACE, table)? else {
            return Ok(None);
        };
        if state != TableState::Active {
            return Err(Error::conflict(format!(
                "logical table {} is already in {} lifecycle state",
                table, state
            )));
        }

        ensure_namespace_is_free(conn, deleting_table_namespace(&table_id).as_str(), table)?;
        conn.execute(
            "UPDATE table_catalog
             SET namespace = ?1, state = ?2
             WHERE namespace = ?3 AND table_name = ?4",
            params![
                deleting_table_namespace(&table_id),
                TableState::Deleting.as_str(),
                DEFAULT_TABLE_NAMESPACE,
                table.as_str()
            ],
        )
        .map_err(map_sqlite_error)?;
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::MarkDeleting {
                table: table.clone(),
                table_id: table_id.clone(),
            },
        });
        Ok(Some(table_id))
    }

    pub(crate) fn hard_delete_table_identity(&mut self, table_id: &TableId) -> Result<bool> {
        self.check_cancel()?;
        let conn = self.connection_mut()?;
        let Some((_, table_name, state)) = table_identity_row_for_table_id(conn, table_id)? else {
            return Ok(false);
        };
        if state != TableState::Deleting {
            return Err(Error::conflict(format!(
                "table id {} for logical table {} is in {} lifecycle state, not deleting",
                table_id, table_name, state
            )));
        }

        conn.execute(
            "DELETE FROM documents WHERE table_id = ?1",
            params![table_id.as_str()],
        )
        .map_err(map_sqlite_error)?;
        conn.execute(
            "DELETE FROM table_catalog WHERE table_id = ?1",
            params![table_id.as_str()],
        )
        .map_err(map_sqlite_error)?;

        let table = TableName::new(table_name)?;
        if resolve_table_id_in_conn(conn, &table)?.is_none() {
            if let Some(schema) = load_table_schema_from_conn(conn, &table)? {
                drop_sqlite_indexes_for_table_schema(conn, &schema)?;
            }
            conn.execute(
                "DELETE FROM schemas WHERE table_name = ?1",
                params![table.as_str()],
            )
            .map_err(map_sqlite_error)?;
            self.schema_cache_dirty = true;
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

fn catalog_row(
    conn: &Connection,
    namespace: &str,
    table: &TableName,
) -> Result<Option<(TableId, TableState)>> {
    conn.query_row(
        "SELECT table_id, state
         FROM table_catalog
         WHERE namespace = ?1 AND table_name = ?2",
        params![namespace, table.as_str()],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
    )
    .optional()
    .map_err(map_sqlite_error)?
    .map(|(table_id, state)| {
        Ok((
            TableId::from_str(table_id.as_str())?,
            TableState::from_str(state.as_str())?,
        ))
    })
    .transpose()
}

fn table_identity_row_for_table_id(
    conn: &Connection,
    table_id: &TableId,
) -> Result<Option<(String, String, TableState)>> {
    conn.query_row(
        "SELECT namespace, table_name, state
         FROM table_catalog
         WHERE table_id = ?1",
        params![table_id.as_str()],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        },
    )
    .optional()
    .map_err(map_sqlite_error)?
    .map(|(namespace, table_name, state)| {
        Ok((namespace, table_name, TableState::from_str(state.as_str())?))
    })
    .transpose()
}

fn ensure_namespace_is_free(conn: &Connection, namespace: &str, table: &TableName) -> Result<()> {
    let exists = conn
        .query_row(
            "SELECT 1
             FROM table_catalog
             WHERE namespace = ?1 AND table_name = ?2",
            params![namespace, table.as_str()],
            |_| Ok(()),
        )
        .optional()
        .map_err(map_sqlite_error)?
        .is_some();
    if exists {
        return Err(Error::conflict(format!(
            "table identity already exists for logical table {} in namespace {}",
            table, namespace
        )));
    }
    Ok(())
}
