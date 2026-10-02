//! Dialect-shared table identity lifecycle.
//!
//! A logical table moves through hidden, active, and deleting identities in the
//! `table_catalog` table. The catalog statements, the state checks, and the
//! tenant events are the same for PostgreSQL and MySQL, so they live here once
//! over [`SqlSession`]. Two steps stay with each provider:
//!
//! - **Index entry purge.** MySQL keeps index entries in a side table that a
//!   hard delete must clear. PostgreSQL indexes are native and need no purge.
//! - **Schema cleanup.** When the last identity of a logical table goes, each
//!   transaction drops its own schema state and native indexes.

use std::future::Future;
use std::str::FromStr;

use nimbus_core::{
    Error, Result, SequenceNumber, TableId, TableLifecycleEvent, TableName, TableState,
    TenantEventKind,
};

use crate::sql::dialect::{SqlRow, SqlSession, SqlSessionTransaction, SqlValue};
use crate::sql::store_core::SqlStoreCore;
use crate::sql::write_core::{SqlWriteBackend, sql_record_tenant_event};
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};

/// Session hook for the provider-owned part of a hard delete.
pub(crate) trait SqlTableLifecycleSession: SqlSession {
    /// Remove index entries that the provider stores outside `documents`.
    fn purge_table_index_entries(
        &mut self,
        tenant_schema: &str,
        table_id: &TableId,
    ) -> impl Future<Output = Result<()>> + Send;
}

/// Store-level table lifecycle operations. Each runs in one write transaction.
///
/// As elsewhere in [`crate::sql`], a default method here shares a name with the
/// inherent method that [`sql_table_lifecycle_facade`] generates. Inherent
/// methods win method-call resolution, so the facade is not recursive.
pub(crate) trait SqlTableLifecycleStore:
    SqlStoreCore<Transaction: SqlTableLifecycleTransaction>
{
    fn latest_sequence(&self) -> Result<SequenceNumber>;

    fn stage_hidden_table_identity(&self, table: &TableName, table_id: &TableId) -> Result<()> {
        let table = table.clone();
        let table_id = table_id.clone();
        self.execute_write(move |transaction| {
            transaction.stage_hidden_table_identity(&table, &table_id)
        })?;
        Ok(())
    }

    fn activate_hidden_table_identity(
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

    fn mark_table_deleting(&self, table: &TableName) -> Result<Option<TableId>> {
        let table = table.clone();
        Ok(self
            .execute_write(move |transaction| transaction.mark_table_deleting(&table))?
            .value)
    }

    fn hard_delete_table_identity(&self, table_id: &TableId) -> Result<bool> {
        self.retention_floor()
            .ensure_hard_delete_allowed(table_id, self.latest_sequence()?)?;
        let table_id = table_id.clone();
        Ok(self
            .execute_write(move |transaction| transaction.hard_delete_table_identity(&table_id))?
            .value)
    }
}

/// Transaction-level table lifecycle operations. Each records its tenant event.
pub(crate) trait SqlTableLifecycleTransaction:
    SqlWriteBackend + SqlSessionTransaction + Sized
{
    /// Drop the schema state of a logical table whose last identity is gone.
    fn after_table_identity_removed(&mut self, table: &TableName) -> Result<()>;

    fn stage_hidden_table_identity(&mut self, table: &TableName, table_id: &TableId) -> Result<()> {
        self.check_cancel()?;
        let (mut session, tenant_schema, runtime_handle) = self.session_parts()?;
        Self::block_on_session(&runtime_handle, async move {
            stage_hidden_table_identity_in_session(&mut session, &tenant_schema, table, table_id)
                .await
        })?;
        sql_record_tenant_event(
            self,
            TenantEventKind::TableLifecycle {
                lifecycle: TableLifecycleEvent::StageHidden {
                    table: table.clone(),
                    table_id: table_id.clone(),
                },
            },
        );
        Ok(())
    }

    fn activate_hidden_table_identity(
        &mut self,
        table: &TableName,
        table_id: &TableId,
    ) -> Result<Option<TableId>> {
        self.check_cancel()?;
        let (mut session, tenant_schema, runtime_handle) = self.session_parts()?;
        let replaced_table_id = Self::block_on_session(&runtime_handle, async move {
            activate_hidden_table_identity_in_session(&mut session, &tenant_schema, table, table_id)
                .await
        })?;
        sql_record_tenant_event(
            self,
            TenantEventKind::TableLifecycle {
                lifecycle: TableLifecycleEvent::ActivateHidden {
                    table: table.clone(),
                    table_id: table_id.clone(),
                    replaced_table_id: replaced_table_id.clone(),
                },
            },
        );
        Ok(replaced_table_id)
    }

    fn mark_table_deleting(&mut self, table: &TableName) -> Result<Option<TableId>> {
        self.check_cancel()?;
        let (mut session, tenant_schema, runtime_handle) = self.session_parts()?;
        let table_id = Self::block_on_session(&runtime_handle, async move {
            mark_table_deleting_in_session(&mut session, &tenant_schema, table).await
        })?;
        if let Some(table_id) = table_id.as_ref() {
            sql_record_tenant_event(
                self,
                TenantEventKind::TableLifecycle {
                    lifecycle: TableLifecycleEvent::MarkDeleting {
                        table: table.clone(),
                        table_id: table_id.clone(),
                    },
                },
            );
        }
        Ok(table_id)
    }

    fn hard_delete_table_identity(&mut self, table_id: &TableId) -> Result<bool> {
        self.check_cancel()?;
        let (mut session, tenant_schema, runtime_handle) = self.session_parts()?;
        let Some(table) = Self::block_on_session(&runtime_handle, async move {
            hard_delete_table_identity_in_session(&mut session, &tenant_schema, table_id).await
        })?
        else {
            return Ok(false);
        };

        if self.load_table_id(&table)?.is_none() {
            self.after_table_identity_removed(&table)?;
        }
        sql_record_tenant_event(
            self,
            TenantEventKind::TableLifecycle {
                lifecycle: TableLifecycleEvent::HardDelete {
                    table,
                    table_id: table_id.clone(),
                },
            },
        );
        Ok(true)
    }
}

pub(crate) async fn stage_hidden_table_identity_in_session<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
    table: &TableName,
    table_id: &TableId,
) -> Result<()> {
    if let Some((namespace, existing_table, state)) =
        table_identity_row_for_table_id(session, tenant_schema, table_id).await?
    {
        return Err(Error::conflict(format!(
            "table id {} is already assigned to logical table {} in namespace {} with {} state",
            table_id, existing_table, namespace, state
        )));
    }

    let query = format!(
        "INSERT INTO {} (namespace, table_name, table_id, state) VALUES (?, ?, ?, ?)",
        S::table(tenant_schema, "table_catalog")
    );
    let namespace = hidden_table_namespace(table_id);
    session
        .execute_sql(
            &query,
            &[
                SqlValue::Text(&namespace),
                SqlValue::Text(table.as_str()),
                SqlValue::Text(table_id.as_str()),
                SqlValue::Text(TableState::Hidden.as_str()),
            ],
        )
        .await?;
    Ok(())
}

pub(crate) async fn activate_hidden_table_identity_in_session<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
    table: &TableName,
    table_id: &TableId,
) -> Result<Option<TableId>> {
    let hidden_namespace = hidden_table_namespace(table_id);
    let Some((hidden_table_id, hidden_state)) =
        catalog_row(session, tenant_schema, &hidden_namespace, table).await?
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
        match catalog_row(session, tenant_schema, DEFAULT_TABLE_NAMESPACE, table).await? {
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
        move_active_identity_to_deleting(session, tenant_schema, table, old_table_id).await?;
    }

    let query = format!(
        "UPDATE {}
         SET namespace = ?, state = ?
         WHERE namespace = ? AND table_name = ? AND table_id = ?",
        S::table(tenant_schema, "table_catalog")
    );
    session
        .execute_sql(
            &query,
            &[
                SqlValue::Text(DEFAULT_TABLE_NAMESPACE),
                SqlValue::Text(TableState::Active.as_str()),
                SqlValue::Text(&hidden_namespace),
                SqlValue::Text(table.as_str()),
                SqlValue::Text(table_id.as_str()),
            ],
        )
        .await?;
    Ok(old_table_id)
}

pub(crate) async fn mark_table_deleting_in_session<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
    table: &TableName,
) -> Result<Option<TableId>> {
    let Some((table_id, state)) =
        catalog_row(session, tenant_schema, DEFAULT_TABLE_NAMESPACE, table).await?
    else {
        return Ok(None);
    };
    if state != TableState::Active {
        return Err(Error::conflict(format!(
            "logical table {} is already in {} lifecycle state",
            table, state
        )));
    }

    move_active_identity_to_deleting(session, tenant_schema, table, &table_id).await?;
    Ok(Some(table_id))
}

pub(crate) async fn hard_delete_table_identity_in_session<S: SqlTableLifecycleSession>(
    session: &mut S,
    tenant_schema: &str,
    table_id: &TableId,
) -> Result<Option<TableName>> {
    let Some((_, table_name, state)) =
        table_identity_row_for_table_id(session, tenant_schema, table_id).await?
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
        "DELETE FROM {} WHERE table_id = ?",
        S::table(tenant_schema, "documents")
    );
    session
        .execute_sql(&delete_documents, &[SqlValue::Text(table_id.as_str())])
        .await?;
    session
        .purge_table_index_entries(tenant_schema, table_id)
        .await?;
    let delete_catalog = format!(
        "DELETE FROM {} WHERE table_id = ?",
        S::table(tenant_schema, "table_catalog")
    );
    session
        .execute_sql(&delete_catalog, &[SqlValue::Text(table_id.as_str())])
        .await?;
    Ok(Some(TableName::new(table_name)?))
}

/// Move the active identity of `table` into its deleting namespace.
async fn move_active_identity_to_deleting<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
    table: &TableName,
    active_table_id: &TableId,
) -> Result<()> {
    let deleting_namespace = deleting_table_namespace(active_table_id);
    ensure_namespace_is_free(session, tenant_schema, &deleting_namespace, table).await?;
    let query = format!(
        "UPDATE {}
         SET namespace = ?, state = ?
         WHERE namespace = ? AND table_name = ?",
        S::table(tenant_schema, "table_catalog")
    );
    session
        .execute_sql(
            &query,
            &[
                SqlValue::Text(&deleting_namespace),
                SqlValue::Text(TableState::Deleting.as_str()),
                SqlValue::Text(DEFAULT_TABLE_NAMESPACE),
                SqlValue::Text(table.as_str()),
            ],
        )
        .await?;
    Ok(())
}

async fn catalog_row<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
    namespace: &str,
    table: &TableName,
) -> Result<Option<(TableId, TableState)>> {
    let query = format!(
        "SELECT table_id, state
         FROM {}
         WHERE namespace = ? AND table_name = ?",
        S::table(tenant_schema, "table_catalog")
    );
    session
        .fetch_optional(
            &query,
            &[SqlValue::Text(namespace), SqlValue::Text(table.as_str())],
        )
        .await?
        .map(|row| {
            Ok((
                TableId::from_str(&row.text(0)?)?,
                TableState::from_str(&row.text(1)?)?,
            ))
        })
        .transpose()
}

async fn table_identity_row_for_table_id<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
    table_id: &TableId,
) -> Result<Option<(String, String, TableState)>> {
    let query = format!(
        "SELECT namespace, table_name, state
         FROM {}
         WHERE table_id = ?",
        S::table(tenant_schema, "table_catalog")
    );
    session
        .fetch_optional(&query, &[SqlValue::Text(table_id.as_str())])
        .await?
        .map(|row| {
            Ok((
                row.text(0)?,
                row.text(1)?,
                TableState::from_str(&row.text(2)?)?,
            ))
        })
        .transpose()
}

async fn ensure_namespace_is_free<S: SqlSession>(
    session: &mut S,
    tenant_schema: &str,
    namespace: &str,
    table: &TableName,
) -> Result<()> {
    let query = format!(
        "SELECT 1
         FROM {}
         WHERE namespace = ? AND table_name = ?",
        S::table(tenant_schema, "table_catalog")
    );
    if session
        .fetch_optional(
            &query,
            &[SqlValue::Text(namespace), SqlValue::Text(table.as_str())],
        )
        .await?
        .is_some()
    {
        return Err(Error::conflict(format!(
            "table identity already exists for logical table {} in namespace {}",
            table, namespace
        )));
    }
    Ok(())
}

/// Re-exposes the [`SqlTableLifecycleStore`] and
/// [`SqlTableLifecycleTransaction`] entry points as inherent methods, keeping
/// the public API of each store and write transaction unchanged.
macro_rules! sql_table_lifecycle_facade {
    ($store:ty, $transaction:ty) => {
        impl $store {
            pub fn stage_hidden_table_identity(
                &self,
                table: &nimbus_core::TableName,
                table_id: &nimbus_core::TableId,
            ) -> nimbus_core::Result<()> {
                <Self as crate::sql::table_lifecycle::SqlTableLifecycleStore>::stage_hidden_table_identity(self, table, table_id)
            }

            pub fn activate_hidden_table_identity(
                &self,
                table: &nimbus_core::TableName,
                table_id: &nimbus_core::TableId,
            ) -> nimbus_core::Result<Option<nimbus_core::TableId>> {
                <Self as crate::sql::table_lifecycle::SqlTableLifecycleStore>::activate_hidden_table_identity(self, table, table_id)
            }

            pub fn mark_table_deleting(
                &self,
                table: &nimbus_core::TableName,
            ) -> nimbus_core::Result<Option<nimbus_core::TableId>> {
                <Self as crate::sql::table_lifecycle::SqlTableLifecycleStore>::mark_table_deleting(self, table)
            }

            pub fn hard_delete_table_identity(
                &self,
                table_id: &nimbus_core::TableId,
            ) -> nimbus_core::Result<bool> {
                <Self as crate::sql::table_lifecycle::SqlTableLifecycleStore>::hard_delete_table_identity(self, table_id)
            }
        }

        impl $transaction {
            pub fn stage_hidden_table_identity(
                &mut self,
                table: &nimbus_core::TableName,
                table_id: &nimbus_core::TableId,
            ) -> nimbus_core::Result<()> {
                <Self as crate::sql::table_lifecycle::SqlTableLifecycleTransaction>::stage_hidden_table_identity(self, table, table_id)
            }

            pub fn activate_hidden_table_identity(
                &mut self,
                table: &nimbus_core::TableName,
                table_id: &nimbus_core::TableId,
            ) -> nimbus_core::Result<Option<nimbus_core::TableId>> {
                <Self as crate::sql::table_lifecycle::SqlTableLifecycleTransaction>::activate_hidden_table_identity(self, table, table_id)
            }

            pub fn mark_table_deleting(
                &mut self,
                table: &nimbus_core::TableName,
            ) -> nimbus_core::Result<Option<nimbus_core::TableId>> {
                <Self as crate::sql::table_lifecycle::SqlTableLifecycleTransaction>::mark_table_deleting(self, table)
            }

            pub fn hard_delete_table_identity(
                &mut self,
                table_id: &nimbus_core::TableId,
            ) -> nimbus_core::Result<bool> {
                <Self as crate::sql::table_lifecycle::SqlTableLifecycleTransaction>::hard_delete_table_identity(self, table_id)
            }
        }
    };
}

pub(crate) use sql_table_lifecycle_facade;
