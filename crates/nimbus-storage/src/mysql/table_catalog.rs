use std::str::FromStr;

use mysql_async::Row;
use mysql_async::prelude::Queryable;
use nimbus_core::{Error, IdSource, Result, TableId, TableName, TableState};

use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};

use super::backend::{map_mysql_error, qualified_table};

pub(super) async fn load_table_id_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
) -> Result<Option<TableId>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT table_id, state FROM {} WHERE namespace = ? AND table_name = ?",
        qualified_table(database_name, "table_catalog")
    );
    let Some(row) = session
        .exec_first::<Row, _, _>(query, ("default", table.as_str()))
        .await
        .map_err(map_mysql_error)?
    else {
        return Ok(None);
    };
    let (table_id, state): (String, String) = mysql_async::from_row(row);
    let state = TableState::from_str(state.as_str())?;
    if state != TableState::Active {
        return Err(Error::conflict(format!(
            "logical table {} is in {} lifecycle state",
            table, state
        )));
    }
    Ok(Some(TableId::from_str(table_id.as_str())?))
}

pub(super) async fn resolve_or_create_table_id_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    id_source: &dyn IdSource,
) -> Result<TableId>
where
    C: Queryable,
{
    if let Some(table_id) = load_table_id_from_session(session, database_name, table).await? {
        return Ok(table_id);
    }
    let table_id = id_source.next_table_id();
    let query = format!(
        "INSERT IGNORE INTO {} (namespace, table_name, table_id, state) VALUES (?, ?, ?, ?)",
        qualified_table(database_name, "table_catalog")
    );
    session
        .exec_drop(
            query,
            (
                "default",
                table.as_str(),
                table_id.as_str(),
                TableState::Active.as_str(),
            ),
        )
        .await
        .map_err(map_mysql_error)?;
    load_table_id_from_session(session, database_name, table)
        .await?
        .ok_or_else(|| {
            Error::Internal(format!(
                "failed to resolve table id for logical table {} after catalog insert",
                table
            ))
        })
}

pub(super) async fn ensure_table_id_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    table_id: &TableId,
) -> Result<()>
where
    C: Queryable,
{
    let hidden_namespace = hidden_table_namespace(table_id);
    let staged_hidden = match catalog_identity_row_from_session(
        session,
        database_name,
        hidden_namespace.as_str(),
        table,
    )
    .await?
    {
        Some((hidden_id, TableState::Hidden)) if hidden_id == *table_id => true,
        Some((hidden_id, state)) => {
            return Err(Error::conflict(format!(
                "hidden identity slot for logical table {} and table id {} contains {} in {} state",
                table, table_id, hidden_id, state
            )));
        }
        None => false,
    };

    match catalog_identity_row_from_session(session, database_name, DEFAULT_TABLE_NAMESPACE, table)
        .await?
    {
        Some((existing, TableState::Active)) if existing == *table_id => {
            if staged_hidden {
                return Err(Error::conflict(format!(
                    "logical table {} already has active table id {} and a duplicate hidden slot",
                    table, table_id
                )));
            }
            return Ok(());
        }
        Some((existing, state)) if existing == *table_id => {
            return Err(Error::conflict(format!(
                "logical table {} is assigned table id {} in {} lifecycle state",
                table, table_id, state
            )));
        }
        Some((existing, TableState::Active)) => {
            ensure_table_id_available_from_session(
                session,
                database_name,
                table_id,
                Some((hidden_namespace.as_str(), table)),
            )
            .await?;
            let query = format!(
                "UPDATE {}
                 SET namespace = ?, state = ?
                 WHERE namespace = ? AND table_name = ?",
                qualified_table(database_name, "table_catalog")
            );
            session
                .exec_drop(
                    query,
                    (
                        deleting_table_namespace(&existing),
                        TableState::Deleting.as_str(),
                        DEFAULT_TABLE_NAMESPACE,
                        table.as_str(),
                    ),
                )
                .await
                .map_err(map_mysql_error)?;
            if staged_hidden {
                let query = format!(
                    "DELETE FROM {} WHERE namespace = ? AND table_name = ?",
                    qualified_table(database_name, "table_catalog")
                );
                session
                    .exec_drop(query, (hidden_namespace.as_str(), table.as_str()))
                    .await
                    .map_err(map_mysql_error)?;
            }
            let query = format!(
                "INSERT INTO {} (namespace, table_name, table_id, state) VALUES (?, ?, ?, ?)",
                qualified_table(database_name, "table_catalog")
            );
            session
                .exec_drop(
                    query,
                    (
                        DEFAULT_TABLE_NAMESPACE,
                        table.as_str(),
                        table_id.as_str(),
                        TableState::Active.as_str(),
                    ),
                )
                .await
                .map_err(map_mysql_error)?;
            return Ok(());
        }
        Some((existing, state)) => {
            return Err(Error::conflict(format!(
                "logical table {} is already assigned table id {} in {} lifecycle state, journal references {}",
                table, existing, state, table_id
            )));
        }
        None => {}
    }
    ensure_table_id_available_from_session(
        session,
        database_name,
        table_id,
        Some((hidden_namespace.as_str(), table)),
    )
    .await?;
    if staged_hidden {
        let query = format!(
            "DELETE FROM {} WHERE namespace = ? AND table_name = ?",
            qualified_table(database_name, "table_catalog")
        );
        session
            .exec_drop(query, (hidden_namespace.as_str(), table.as_str()))
            .await
            .map_err(map_mysql_error)?;
    }
    let query = format!(
        "INSERT INTO {} (namespace, table_name, table_id, state) VALUES (?, ?, ?, ?)",
        qualified_table(database_name, "table_catalog")
    );
    session
        .exec_drop(
            query,
            (
                "default",
                table.as_str(),
                table_id.as_str(),
                TableState::Active.as_str(),
            ),
        )
        .await
        .map_err(map_mysql_error)?;
    Ok(())
}

async fn catalog_identity_row_from_session<C>(
    session: &mut C,
    database_name: &str,
    namespace: &str,
    table: &TableName,
) -> Result<Option<(TableId, TableState)>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT table_id, state FROM {} WHERE namespace = ? AND table_name = ?",
        qualified_table(database_name, "table_catalog")
    );
    let Some(row) = session
        .exec_first::<Row, _, _>(query, (namespace, table.as_str()))
        .await
        .map_err(map_mysql_error)?
    else {
        return Ok(None);
    };
    let (table_id, state): (String, String) = mysql_async::from_row(row);
    Ok(Some((
        TableId::from_str(table_id.as_str())?,
        TableState::from_str(state.as_str())?,
    )))
}

async fn ensure_table_id_available_from_session<C>(
    session: &mut C,
    database_name: &str,
    table_id: &TableId,
    allowed_key: Option<(&str, &TableName)>,
) -> Result<()>
where
    C: Queryable,
{
    let query = format!(
        "SELECT namespace, table_name, state FROM {} WHERE table_id = ?",
        qualified_table(database_name, "table_catalog")
    );
    let Some(row) = session
        .exec_first::<Row, _, _>(query, (table_id.as_str(),))
        .await
        .map_err(map_mysql_error)?
    else {
        return Ok(());
    };
    let (namespace, table_name, state): (String, String, String) = mysql_async::from_row(row);
    let table = TableName::new(table_name)?;
    let state = TableState::from_str(state.as_str())?;
    if allowed_key
        .map(|(allowed_namespace, allowed_table)| {
            allowed_namespace == namespace && allowed_table == &table
        })
        .unwrap_or(false)
    {
        return Ok(());
    }
    Err(Error::conflict(format!(
        "table id {} is already assigned to logical table {} in namespace {} with {} state",
        table_id, table, namespace, state
    )))
}

pub(super) async fn load_table_identities_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<Vec<crate::TableIdentitySnapshotEntry>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT namespace, table_name, table_id, state
         FROM {}
         ORDER BY namespace, table_name, table_id, state",
        qualified_table(database_name, "table_catalog")
    );
    let rows = session
        .query::<Row, _>(query)
        .await
        .map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| {
            let (namespace, table_name, table_id, state): (String, String, String, String) =
                mysql_async::from_row(row);
            Ok(crate::TableIdentitySnapshotEntry {
                namespace,
                table: TableName::new(table_name)?,
                table_id: TableId::from_str(table_id.as_str())?,
                state: TableState::from_str(state.as_str())?,
            })
        })
        .collect()
}
