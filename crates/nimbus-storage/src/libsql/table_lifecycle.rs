use super::*;
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};

impl LibsqlReplicaTenantStore {
    pub fn stage_hidden_table_identity(&self, table: &TableName, table_id: &TableId) -> Result<()> {
        let table = table.clone();
        let table_id = table_id.clone();
        self.execute_write(move |transaction| {
            transaction.stage_hidden_table_identity(&table, &table_id)
        })?;
        Ok(())
    }

    pub fn activate_hidden_table_identity(
        &self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<Option<TableId>> {
        let table = table.clone();
        let table_id = table_id.clone();
        Ok(self
            .execute_write(move |transaction| {
                transaction.activate_hidden_table_identity(&table, &table_id)
            })?
            .value)
    }

    pub fn mark_table_deleting(&self, table: &TableName) -> Result<Option<TableId>> {
        let table = table.clone();
        Ok(self
            .execute_write(move |transaction| transaction.mark_table_deleting(&table))?
            .value)
    }

    pub fn hard_delete_table_identity(&self, table_id: &TableId) -> Result<bool> {
        self.retention_floor
            .ensure_hard_delete_allowed(table_id, self.latest_sequence()?)?;
        let table_id = table_id.clone();
        Ok(self
            .execute_write(move |transaction| transaction.hard_delete_table_identity(&table_id))?
            .value)
    }
}

impl LibsqlReplicaWriteTransaction {
    pub fn stage_hidden_table_identity(
        &mut self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<()> {
        self.check_cancel()?;
        self.store.block_on(async {
            stage_hidden_table_identity_in_session(self.session()?, table, table_id).await
        })?;
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::StageHidden {
                table: table.clone(),
                table_id: table_id.clone(),
            },
        });
        self.refresh_cache_after_commit = true;
        Ok(())
    }

    pub fn activate_hidden_table_identity(
        &mut self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<Option<TableId>> {
        self.check_cancel()?;
        let retired = self.store.block_on(async {
            activate_hidden_table_identity_in_session(self.session()?, table, table_id).await
        })?;
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::ActivateHidden {
                table: table.clone(),
                table_id: table_id.clone(),
                replaced_table_id: retired.clone(),
            },
        });
        self.refresh_cache_after_commit = true;
        Ok(retired)
    }

    pub fn mark_table_deleting(&mut self, table: &TableName) -> Result<Option<TableId>> {
        self.check_cancel()?;
        let marked = self
            .store
            .block_on(async { mark_table_deleting_in_session(self.session()?, table).await })?;
        if let Some(table_id) = marked.as_ref() {
            self.record_tenant_event(TenantEventKind::TableLifecycle {
                lifecycle: TableLifecycleEvent::MarkDeleting {
                    table: table.clone(),
                    table_id: table_id.clone(),
                },
            });
        }
        self.refresh_cache_after_commit = true;
        Ok(marked)
    }

    pub fn hard_delete_table_identity(&mut self, table_id: &TableId) -> Result<bool> {
        self.check_cancel()?;
        let Some(table) = self.store.block_on(async {
            hard_delete_table_identity_in_session(self.session()?, table_id).await
        })?
        else {
            return Ok(false);
        };

        if self
            .store
            .block_on(async { load_remote_table_id_from_session(self.session()?, &table).await })?
            .is_none()
        {
            self.store.block_on(async {
                self.session()?
                    .execute(
                        "DELETE FROM schemas WHERE table_name = ?1",
                        libsql::params![table.as_str()],
                    )
                    .await
                    .map_err(map_libsql_error)?;
                Ok(())
            })?;
        }
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::HardDelete {
                table,
                table_id: table_id.clone(),
            },
        });
        self.refresh_cache_after_commit = true;
        Ok(true)
    }
}

pub(super) async fn stage_hidden_table_identity_in_session(
    session: &Transaction,
    table: &TableName,
    table_id: &TableId,
) -> Result<()> {
    if let Some((namespace, existing_table, state)) =
        table_identity_row_for_table_id(session, table_id).await?
    {
        return Err(Error::Conflict(format!(
            "table id {} is already assigned to logical table {} in namespace {} with {} state",
            table_id, existing_table, namespace, state
        )));
    }

    session
        .execute(
            "INSERT INTO table_catalog (namespace, table_name, table_id, state)
             VALUES (?1, ?2, ?3, ?4)",
            libsql::params![
                hidden_table_namespace(table_id),
                table.as_str(),
                table_id.as_str(),
                TableState::Hidden.as_str()
            ],
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(())
}

pub(super) async fn activate_hidden_table_identity_in_session(
    session: &Transaction,
    table: &TableName,
    table_id: &TableId,
) -> Result<Option<TableId>> {
    let hidden_namespace = hidden_table_namespace(table_id);
    let Some((hidden_table_id, hidden_state)) =
        catalog_row(session, hidden_namespace.as_str(), table).await?
    else {
        return Err(Error::InvalidInput(format!(
            "hidden table identity {} for logical table {} does not exist",
            table_id, table
        )));
    };
    if &hidden_table_id != table_id || hidden_state != TableState::Hidden {
        return Err(Error::Conflict(format!(
            "hidden table identity {} for logical table {} is cataloged as {} in {} state",
            table_id, table, hidden_table_id, hidden_state
        )));
    }

    let old_table_id = match catalog_row(session, DEFAULT_TABLE_NAMESPACE, table).await? {
        Some((active_table_id, TableState::Active)) => Some(active_table_id),
        Some((_, state)) => {
            return Err(Error::Conflict(format!(
                "logical table {} is already in {} lifecycle state",
                table, state
            )));
        }
        None => None,
    };

    if let Some(old_table_id) = old_table_id.as_ref() {
        ensure_namespace_is_free(
            session,
            deleting_table_namespace(old_table_id).as_str(),
            table,
        )
        .await?;
        session
            .execute(
                "UPDATE table_catalog
                 SET namespace = ?1, state = ?2
                 WHERE namespace = ?3 AND table_name = ?4",
                libsql::params![
                    deleting_table_namespace(old_table_id),
                    TableState::Deleting.as_str(),
                    DEFAULT_TABLE_NAMESPACE,
                    table.as_str()
                ],
            )
            .await
            .map_err(map_libsql_error)?;
    }

    session
        .execute(
            "UPDATE table_catalog
             SET namespace = ?1, state = ?2
             WHERE namespace = ?3 AND table_name = ?4 AND table_id = ?5",
            libsql::params![
                DEFAULT_TABLE_NAMESPACE,
                TableState::Active.as_str(),
                hidden_namespace,
                table.as_str(),
                table_id.as_str()
            ],
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(old_table_id)
}

pub(super) async fn mark_table_deleting_in_session(
    session: &Transaction,
    table: &TableName,
) -> Result<Option<TableId>> {
    let Some((table_id, state)) = catalog_row(session, DEFAULT_TABLE_NAMESPACE, table).await?
    else {
        return Ok(None);
    };
    if state != TableState::Active {
        return Err(Error::Conflict(format!(
            "logical table {} is already in {} lifecycle state",
            table, state
        )));
    }

    ensure_namespace_is_free(session, deleting_table_namespace(&table_id).as_str(), table).await?;
    session
        .execute(
            "UPDATE table_catalog
             SET namespace = ?1, state = ?2
             WHERE namespace = ?3 AND table_name = ?4",
            libsql::params![
                deleting_table_namespace(&table_id),
                TableState::Deleting.as_str(),
                DEFAULT_TABLE_NAMESPACE,
                table.as_str()
            ],
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(Some(table_id))
}

pub(super) async fn hard_delete_table_identity_in_session(
    session: &Transaction,
    table_id: &TableId,
) -> Result<Option<TableName>> {
    let Some((_, table_name, state)) = table_identity_row_for_table_id(session, table_id).await?
    else {
        return Ok(None);
    };
    if state != TableState::Deleting {
        return Err(Error::Conflict(format!(
            "table id {} for logical table {} is in {} lifecycle state, not deleting",
            table_id, table_name, state
        )));
    }

    session
        .execute(
            "DELETE FROM documents WHERE table_id = ?1",
            libsql::params![table_id.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    session
        .execute(
            "DELETE FROM table_catalog WHERE table_id = ?1",
            libsql::params![table_id.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(Some(TableName::new(table_name)?))
}

async fn catalog_row(
    session: &Transaction,
    namespace: &str,
    table: &TableName,
) -> Result<Option<(TableId, TableState)>> {
    let mut rows = session
        .query(
            "SELECT table_id, state
             FROM table_catalog
             WHERE namespace = ?1 AND table_name = ?2",
            libsql::params![namespace, table.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    Ok(Some((
        TableId::from_str(row.get::<String>(0).map_err(map_libsql_error)?.as_str())?,
        TableState::from_str(row.get::<String>(1).map_err(map_libsql_error)?.as_str())?,
    )))
}

async fn table_identity_row_for_table_id(
    session: &Transaction,
    table_id: &TableId,
) -> Result<Option<(String, String, TableState)>> {
    let mut rows = session
        .query(
            "SELECT namespace, table_name, state
             FROM table_catalog
             WHERE table_id = ?1",
            libsql::params![table_id.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    Ok(Some((
        row.get::<String>(0).map_err(map_libsql_error)?,
        row.get::<String>(1).map_err(map_libsql_error)?,
        TableState::from_str(row.get::<String>(2).map_err(map_libsql_error)?.as_str())?,
    )))
}

async fn ensure_namespace_is_free(
    session: &Transaction,
    namespace: &str,
    table: &TableName,
) -> Result<()> {
    let mut rows = session
        .query(
            "SELECT 1
             FROM table_catalog
             WHERE namespace = ?1 AND table_name = ?2",
            libsql::params![namespace, table.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    if rows.next().await.map_err(map_libsql_error)?.is_some() {
        return Err(Error::Conflict(format!(
            "table identity already exists for logical table {} in namespace {}",
            table, namespace
        )));
    }
    Ok(())
}
