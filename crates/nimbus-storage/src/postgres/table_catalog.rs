//! Postgres table identity catalog session helpers.

use super::*;
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};

pub(super) async fn load_table_id_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
) -> Result<Option<TableId>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT table_id, state FROM {} WHERE namespace = $1 AND table_name = $2",
        qualified_table(schema_name, "table_catalog")
    );
    let Some(row) = session
        .query_opt(query.as_str(), &[&"default", &table.as_str()])
        .await
        .map_err(map_postgres_error)?
    else {
        return Ok(None);
    };
    let state = TableState::from_str(row.get::<_, String>(1).as_str())?;
    if state != TableState::Active {
        return Err(Error::conflict(format!(
            "logical table {} is in {} lifecycle state",
            table, state
        )));
    }
    Ok(Some(TableId::from_str(row.get::<_, String>(0).as_str())?))
}

pub(super) async fn load_table_identities_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<Vec<crate::TableIdentitySnapshotEntry>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT namespace, table_name, table_id, state
           FROM {}
           ORDER BY namespace, table_name, table_id, state",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .query(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?
        .into_iter()
        .map(|row| {
            Ok(crate::TableIdentitySnapshotEntry {
                namespace: row.get::<_, String>(0),
                table: TableName::new(row.get::<_, String>(1))?,
                table_id: TableId::from_str(row.get::<_, String>(2).as_str())?,
                state: TableState::from_str(row.get::<_, String>(3).as_str())?,
            })
        })
        .collect()
}

pub(super) async fn resolve_or_create_table_id_in_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
) -> Result<TableId>
where
    C: GenericClient + Sync,
{
    if let Some(table_id) = load_table_id_from_session(session, schema_name, table).await? {
        return Ok(table_id);
    }
    let table_id = TableId::new();
    let query = format!(
        "INSERT INTO {} (namespace, table_name, table_id, state)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT(namespace, table_name) DO NOTHING",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .execute(
            query.as_str(),
            &[
                &"default",
                &table.as_str(),
                &table_id.as_str(),
                &TableState::Active.as_str(),
            ],
        )
        .await
        .map_err(map_postgres_error)?;
    load_table_id_from_session(session, schema_name, table)
        .await?
        .ok_or_else(|| {
            Error::Internal(format!(
                "failed to resolve table id for logical table {} after catalog insert",
                table
            ))
        })
}

pub(super) async fn ensure_table_id_in_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    table_id: &TableId,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    let hidden_namespace = hidden_table_namespace(table_id);
    let staged_hidden = match catalog_identity_row_from_session(
        session,
        schema_name,
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

    match catalog_identity_row_from_session(session, schema_name, DEFAULT_TABLE_NAMESPACE, table)
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
            ensure_table_id_available_in_session(
                session,
                schema_name,
                table_id,
                Some((hidden_namespace.as_str(), table)),
            )
            .await?;
            let query = format!(
                "UPDATE {}
                   SET namespace = $1, state = $2
                   WHERE namespace = $3 AND table_name = $4",
                qualified_table(schema_name, "table_catalog")
            );
            let deleting_namespace = deleting_table_namespace(&existing);
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
            if staged_hidden {
                let query = format!(
                    "DELETE FROM {} WHERE namespace = $1 AND table_name = $2",
                    qualified_table(schema_name, "table_catalog")
                );
                session
                    .execute(query.as_str(), &[&hidden_namespace, &table.as_str()])
                    .await
                    .map_err(map_postgres_error)?;
            }
            let query = format!(
                "INSERT INTO {} (namespace, table_name, table_id, state) VALUES ($1, $2, $3, $4)",
                qualified_table(schema_name, "table_catalog")
            );
            session
                .execute(
                    query.as_str(),
                    &[
                        &DEFAULT_TABLE_NAMESPACE,
                        &table.as_str(),
                        &table_id.as_str(),
                        &TableState::Active.as_str(),
                    ],
                )
                .await
                .map_err(map_postgres_error)?;
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
    ensure_table_id_available_in_session(
        session,
        schema_name,
        table_id,
        Some((hidden_namespace.as_str(), table)),
    )
    .await?;
    if staged_hidden {
        let query = format!(
            "DELETE FROM {} WHERE namespace = $1 AND table_name = $2",
            qualified_table(schema_name, "table_catalog")
        );
        session
            .execute(query.as_str(), &[&hidden_namespace, &table.as_str()])
            .await
            .map_err(map_postgres_error)?;
    }
    let query = format!(
        "INSERT INTO {} (namespace, table_name, table_id, state) VALUES ($1, $2, $3, $4)",
        qualified_table(schema_name, "table_catalog")
    );
    session
        .execute(
            query.as_str(),
            &[
                &"default",
                &table.as_str(),
                &table_id.as_str(),
                &TableState::Active.as_str(),
            ],
        )
        .await
        .map_err(map_postgres_error)?;
    Ok(())
}

async fn catalog_identity_row_from_session<C>(
    session: &C,
    schema_name: &str,
    namespace: &str,
    table: &TableName,
) -> Result<Option<(TableId, TableState)>>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT table_id, state FROM {} WHERE namespace = $1 AND table_name = $2",
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

async fn ensure_table_id_available_in_session<C>(
    session: &C,
    schema_name: &str,
    table_id: &TableId,
    allowed_key: Option<(&str, &TableName)>,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT namespace, table_name, state FROM {} WHERE table_id = $1",
        qualified_table(schema_name, "table_catalog")
    );
    let Some(row) = session
        .query_opt(query.as_str(), &[&table_id.as_str()])
        .await
        .map_err(map_postgres_error)?
    else {
        return Ok(());
    };
    let namespace = row.get::<_, String>(0);
    let table = TableName::new(row.get::<_, String>(1))?;
    let state = TableState::from_str(row.get::<_, String>(2).as_str())?;
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
