use super::*;
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};

impl PostgresTenantStore {
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

impl PostgresWriteTransaction {
    pub fn stage_hidden_table_identity(
        &mut self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<()> {
        self.check_cancel()?;
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let table_id = table_id.clone();
        let event_table = table.clone();
        let event_table_id = table_id.clone();
        let client = self.session()?;
        self.block_on(async move {
            stage_hidden_table_identity_in_session(client, &schema_name, &table, &table_id).await
        })?;
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::StageHidden {
                table: event_table,
                table_id: event_table_id,
            },
        });
        Ok(())
    }

    pub fn activate_hidden_table_identity(
        &mut self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<Option<TableId>> {
        self.check_cancel()?;
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let table_id = table_id.clone();
        let event_table = table.clone();
        let event_table_id = table_id.clone();
        let client = self.session()?;
        let replaced_table_id = self.block_on(async move {
            activate_hidden_table_identity_in_session(client, &schema_name, &table, &table_id).await
        })?;
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::ActivateHidden {
                table: event_table,
                table_id: event_table_id,
                replaced_table_id: replaced_table_id.clone(),
            },
        });
        Ok(replaced_table_id)
    }

    pub fn mark_table_deleting(&mut self, table: &TableName) -> Result<Option<TableId>> {
        self.check_cancel()?;
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let event_table = table.clone();
        let client = self.session()?;
        let table_id = self.block_on(async move {
            mark_table_deleting_in_session(client, &schema_name, &table).await
        })?;
        if let Some(table_id) = table_id.as_ref() {
            self.record_tenant_event(TenantEventKind::TableLifecycle {
                lifecycle: TableLifecycleEvent::MarkDeleting {
                    table: event_table,
                    table_id: table_id.clone(),
                },
            });
        }
        Ok(table_id)
    }

    pub fn hard_delete_table_identity(&mut self, table_id: &TableId) -> Result<bool> {
        self.check_cancel()?;
        let schema_name = self.schema_name.clone();
        let table_id = table_id.clone();
        let event_table_id = table_id.clone();
        let client = self.session()?;
        let hard_delete_table_id = table_id.clone();
        let Some(table) = self.block_on(async move {
            hard_delete_table_identity_in_session(client, &schema_name, &hard_delete_table_id).await
        })?
        else {
            return Ok(false);
        };

        if self.load_table_id(&table)?.is_none() {
            if let Some(previous) = self.load_table_schema(&table)? {
                self.drop_table_indexes(&previous)?;
            }
            self.delete_table_schema_entry(&table)?;
            self.notification.schema_changed = true;
            self.schema_cache_changed = true;
        }
        self.record_tenant_event(TenantEventKind::TableLifecycle {
            lifecycle: TableLifecycleEvent::HardDelete {
                table,
                table_id: event_table_id,
            },
        });
        Ok(true)
    }
}

pub(super) async fn stage_hidden_table_identity_in_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    table_id: &TableId,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    if let Some((namespace, existing_table, state)) =
        table_identity_row_for_table_id(session, schema_name, table_id).await?
    {
        return Err(Error::conflict(format!(
            "table id {} is already assigned to logical table {} in namespace {} with {} state",
            table_id, existing_table, namespace, state
        )));
    }

    let query = format!(
        "INSERT INTO {} (namespace, table_name, table_id, state) VALUES ($1, $2, $3, $4)",
        qualified_table(schema_name, "table_catalog")
    );
    let namespace = hidden_table_namespace(table_id);
    session
        .execute(
            query.as_str(),
            &[
                &namespace,
                &table.as_str(),
                &table_id.as_str(),
                &TableState::Hidden.as_str(),
            ],
        )
        .await
        .map_err(map_postgres_error)?;
    Ok(())
}

pub(super) async fn activate_hidden_table_identity_in_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    table_id: &TableId,
) -> Result<Option<TableId>>
where
    C: GenericClient + Sync,
{
    let hidden_namespace = hidden_table_namespace(table_id);
    let Some((hidden_table_id, hidden_state)) =
        catalog_row(session, schema_name, hidden_namespace.as_str(), table).await?
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

    let old_table_id =
        match catalog_row(session, schema_name, DEFAULT_TABLE_NAMESPACE, table).await? {
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
        ensure_namespace_is_free(
            session,
            schema_name,
            deleting_table_namespace(old_table_id).as_str(),
            table,
        )
        .await?;
        let query = format!(
            "UPDATE {}
             SET namespace = $1, state = $2
             WHERE namespace = $3 AND table_name = $4",
            qualified_table(schema_name, "table_catalog")
        );
        let deleting_namespace = deleting_table_namespace(old_table_id);
        session
            .execute(
                query.as_str(),
                &[
                    &deleting_namespace,
                    &TableState::Deleting.as_str(),
                    &DEFAULT_TABLE_NAMESPACE,
                    &table.as_str(),
                ],
            )
            .await
            .map_err(map_postgres_error)?;
    }

    let query = format!(
        "UPDATE {}
         SET namespace = $1, state = $2
         WHERE namespace = $3 AND table_name = $4 AND table_id = $5",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .execute(
            query.as_str(),
            &[
                &DEFAULT_TABLE_NAMESPACE,
                &TableState::Active.as_str(),
                &hidden_namespace,
                &table.as_str(),
                &table_id.as_str(),
            ],
        )
        .await
        .map_err(map_postgres_error)?;
    Ok(old_table_id)
}

pub(super) async fn mark_table_deleting_in_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
) -> Result<Option<TableId>>
where
    C: GenericClient + Sync,
{
    let Some((table_id, state)) =
        catalog_row(session, schema_name, DEFAULT_TABLE_NAMESPACE, table).await?
    else {
        return Ok(None);
    };
    if state != TableState::Active {
        return Err(Error::conflict(format!(
            "logical table {} is already in {} lifecycle state",
            table, state
        )));
    }

    ensure_namespace_is_free(
        session,
        schema_name,
        deleting_table_namespace(&table_id).as_str(),
        table,
    )
    .await?;
    let query = format!(
        "UPDATE {}
         SET namespace = $1, state = $2
         WHERE namespace = $3 AND table_name = $4",
        qualified_table(schema_name, "table_catalog")
    );
    let deleting_namespace = deleting_table_namespace(&table_id);
    session
        .execute(
            query.as_str(),
            &[
                &deleting_namespace,
                &TableState::Deleting.as_str(),
                &DEFAULT_TABLE_NAMESPACE,
                &table.as_str(),
            ],
        )
        .await
        .map_err(map_postgres_error)?;
    Ok(Some(table_id))
}

pub(super) async fn hard_delete_table_identity_in_session<C>(
    session: &C,
    schema_name: &str,
    table_id: &TableId,
) -> Result<Option<TableName>>
where
    C: GenericClient + Sync,
{
    let Some((_, table_name, state)) =
        table_identity_row_for_table_id(session, schema_name, table_id).await?
    else {
        return Ok(None);
    };
    if state != TableState::Deleting {
        return Err(Error::conflict(format!(
            "table id {} for logical table {} is in {} lifecycle state, not deleting",
            table_id, table_name, state
        )));
    }

    let delete_documents = format!(
        "DELETE FROM {} WHERE table_id = $1",
        qualified_table(schema_name, "documents")
    );
    session
        .execute(delete_documents.as_str(), &[&table_id.as_str()])
        .await
        .map_err(map_postgres_error)?;
    let delete_catalog = format!(
        "DELETE FROM {} WHERE table_id = $1",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .execute(delete_catalog.as_str(), &[&table_id.as_str()])
        .await
        .map_err(map_postgres_error)?;
    Ok(Some(TableName::new(table_name)?))
}

async fn catalog_row<C>(
    session: &C,
    schema_name: &str,
    namespace: &str,
    table: &TableName,
) -> Result<Option<(TableId, TableState)>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT table_id, state
         FROM {}
         WHERE namespace = $1 AND table_name = $2",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .query_opt(query.as_str(), &[&namespace, &table.as_str()])
        .await
        .map_err(map_postgres_error)?
        .map(|row| {
            Ok((
                TableId::from_str(row.get::<_, String>(0).as_str())?,
                TableState::from_str(row.get::<_, String>(1).as_str())?,
            ))
        })
        .transpose()
}

async fn table_identity_row_for_table_id<C>(
    session: &C,
    schema_name: &str,
    table_id: &TableId,
) -> Result<Option<(String, String, TableState)>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT namespace, table_name, state
         FROM {}
         WHERE table_id = $1",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .query_opt(query.as_str(), &[&table_id.as_str()])
        .await
        .map_err(map_postgres_error)?
        .map(|row| {
            Ok((
                row.get::<_, String>(0),
                row.get::<_, String>(1),
                TableState::from_str(row.get::<_, String>(2).as_str())?,
            ))
        })
        .transpose()
}

async fn ensure_namespace_is_free<C>(
    session: &C,
    schema_name: &str,
    namespace: &str,
    table: &TableName,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT 1
         FROM {}
         WHERE namespace = $1 AND table_name = $2",
        qualified_table(schema_name, "table_catalog")
    );
    if session
        .query_opt(query.as_str(), &[&namespace, &table.as_str()])
        .await
        .map_err(map_postgres_error)?
        .is_some()
    {
        return Err(Error::conflict(format!(
            "table identity already exists for logical table {} in namespace {}",
            table, namespace
        )));
    }
    Ok(())
}
