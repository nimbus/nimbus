use super::*;
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, TableIdentitySnapshotEntry, deleting_table_namespace,
    hidden_table_namespace,
};

pub(super) fn expect_write_commit(
    commit: Option<CommitEntry>,
    expectation: &str,
) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

pub(super) fn table_has_entries(conn: &Connection, table_name: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {table_name} LIMIT 1");
    Ok(conn
        .query_row(sql.as_str(), [], |_| Ok(()))
        .optional()
        .map_err(map_sqlite_error)?
        .is_some())
}

pub(super) fn serialize_json<T>(value: &T) -> Result<String>
where
    T: Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn serialize_document_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.fields).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn serialize_document_typed_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.typed_fields)
        .map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

pub(super) fn decode_u64(bytes: &[u8]) -> Result<u64> {
    let array: [u8; 8] = bytes.try_into().map_err(|_| {
        Error::Internal("expected 8 bytes when decoding sqlite metadata".to_string())
    })?;
    Ok(u64::from_be_bytes(array))
}

pub(super) fn row_to_document(
    table: &TableName,
    id: &DocumentId,
    creation_time: u64,
    update_time: u64,
    data_json: String,
    typed_fields_json: String,
) -> Result<Document> {
    Ok(Document {
        id: id.clone(),
        table: table.clone(),
        creation_time: Timestamp(creation_time),
        update_time: Timestamp(update_time),
        fields: serde_json::from_str(&data_json)
            .map_err(|error| Error::Serialization(error.to_string()))?,
        typed_fields: serde_json::from_str(&typed_fields_json)
            .map_err(|error| Error::Serialization(error.to_string()))?,
    })
}

pub(super) fn load_document_from_conn(
    conn: &Connection,
    table: &TableName,
    id: &DocumentId,
) -> Result<Option<Document>> {
    let Some(table_id) = resolve_table_id_in_conn(conn, table)? else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT creation_time, update_time, data_json, typed_fields_json
         FROM documents
         WHERE table_id = ?1 AND id = ?2",
        params![table_id.as_str(), id.to_string()],
        |row| {
            Ok(row_to_document(
                table,
                id,
                row.get(0)?,
                row.get(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )
    .optional()
    .map_err(map_sqlite_error)?
    .transpose()
}

pub(super) fn load_document_by_table_id_from_conn(
    conn: &Connection,
    table: &TableName,
    table_id: &TableId,
    id: &DocumentId,
) -> Result<Option<Document>> {
    conn.query_row(
        "SELECT creation_time, update_time, data_json, typed_fields_json
         FROM documents
         WHERE table_id = ?1 AND id = ?2",
        params![table_id.as_str(), id.to_string()],
        |row| {
            Ok(row_to_document(
                table,
                id,
                row.get(0)?,
                row.get(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        },
    )
    .optional()
    .map_err(map_sqlite_error)?
    .transpose()
}

pub(super) fn resolve_table_id_in_conn(
    conn: &Connection,
    table: &TableName,
) -> Result<Option<TableId>> {
    let Some((table_id, state)) = conn
        .query_row(
            "SELECT table_id, state
         FROM table_catalog
         WHERE namespace = ?1 AND table_name = ?2",
            params![DEFAULT_TABLE_NAMESPACE, table.as_str()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )
        .optional()
        .map_err(map_sqlite_error)?
    else {
        return Ok(None);
    };
    let state = TableState::from_str(state.as_str())?;
    if state != TableState::Active {
        return Err(Error::conflict(format!(
            "logical table {} is in {} lifecycle state",
            table, state
        )));
    }
    Ok(Some(TableId::from_str(table_id.as_str())?))
}

pub(super) fn resolve_or_create_table_id_in_conn(
    conn: &Connection,
    table: &TableName,
    id_source: &dyn IdSource,
) -> Result<TableId> {
    if let Some(table_id) = resolve_table_id_in_conn(conn, table)? {
        return Ok(table_id);
    }

    let table_id = id_source.next_table_id();
    conn.execute(
        "INSERT OR IGNORE INTO table_catalog (namespace, table_name, table_id, state)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            DEFAULT_TABLE_NAMESPACE,
            table.as_str(),
            table_id.as_str(),
            TableState::Active.as_str()
        ],
    )
    .map_err(map_sqlite_error)?;
    resolve_table_id_in_conn(conn, table)?.ok_or_else(|| {
        Error::Internal(format!(
            "failed to resolve table id for logical table {} after catalog insert",
            table
        ))
    })
}

pub(super) fn ensure_table_id_in_conn(
    conn: &Connection,
    table: &TableName,
    table_id: &TableId,
) -> Result<()> {
    let hidden_namespace = hidden_table_namespace(table_id);
    let staged_hidden = match catalog_identity_row_in_conn(conn, hidden_namespace.as_str(), table)?
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

    match catalog_identity_row_in_conn(conn, DEFAULT_TABLE_NAMESPACE, table)? {
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
            ensure_table_id_available_in_conn(
                conn,
                table_id,
                Some((hidden_namespace.as_str(), table)),
            )?;
            conn.execute(
                "UPDATE table_catalog
                 SET namespace = ?1, state = ?2
                 WHERE namespace = ?3 AND table_name = ?4",
                params![
                    deleting_table_namespace(&existing),
                    TableState::Deleting.as_str(),
                    DEFAULT_TABLE_NAMESPACE,
                    table.as_str()
                ],
            )
            .map_err(map_sqlite_error)?;
            if staged_hidden {
                conn.execute(
                    "DELETE FROM table_catalog WHERE namespace = ?1 AND table_name = ?2",
                    params![hidden_namespace.as_str(), table.as_str()],
                )
                .map_err(map_sqlite_error)?;
            }
            conn.execute(
                "INSERT INTO table_catalog (namespace, table_name, table_id, state)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    DEFAULT_TABLE_NAMESPACE,
                    table.as_str(),
                    table_id.as_str(),
                    TableState::Active.as_str()
                ],
            )
            .map_err(map_sqlite_error)?;
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

    ensure_table_id_available_in_conn(conn, table_id, Some((hidden_namespace.as_str(), table)))?;
    if staged_hidden {
        conn.execute(
            "DELETE FROM table_catalog WHERE namespace = ?1 AND table_name = ?2",
            params![hidden_namespace.as_str(), table.as_str()],
        )
        .map_err(map_sqlite_error)?;
    }
    conn.execute(
        "INSERT INTO table_catalog (namespace, table_name, table_id, state)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            DEFAULT_TABLE_NAMESPACE,
            table.as_str(),
            table_id.as_str(),
            TableState::Active.as_str()
        ],
    )
    .map_err(map_sqlite_error)?;
    Ok(())
}

pub(super) fn ensure_table_identity_in_conn(
    conn: &Connection,
    identity: &TableIdentitySnapshotEntry,
) -> Result<()> {
    if let Some((existing_id, existing_state)) =
        catalog_identity_row_in_conn(conn, identity.namespace.as_str(), &identity.table)?
    {
        if existing_id == identity.table_id && existing_state == identity.state {
            return Ok(());
        }
        return Err(Error::conflict(format!(
            "logical table {} in namespace {} is already assigned table id {} in {} state, snapshot references {} in {} state",
            identity.table,
            identity.namespace,
            existing_id,
            existing_state,
            identity.table_id,
            identity.state
        )));
    }
    ensure_table_id_available_in_conn(conn, &identity.table_id, None)?;
    conn.execute(
        "INSERT INTO table_catalog (namespace, table_name, table_id, state)
         VALUES (?1, ?2, ?3, ?4)",
        params![
            identity.namespace.as_str(),
            identity.table.as_str(),
            identity.table_id.as_str(),
            identity.state.as_str()
        ],
    )
    .map_err(map_sqlite_error)?;
    Ok(())
}

fn catalog_identity_row_in_conn(
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

fn ensure_table_id_available_in_conn(
    conn: &Connection,
    table_id: &TableId,
    allowed_key: Option<(&str, &TableName)>,
) -> Result<()> {
    let Some((namespace, table_name, state)) = conn
        .query_row(
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
    else {
        return Ok(());
    };
    let table_name =
        TableName::new(table_name).map_err(|error| Error::Serialization(error.to_string()))?;
    if allowed_key
        .map(|(allowed_namespace, allowed_table)| {
            allowed_namespace == namespace && allowed_table == &table_name
        })
        .unwrap_or(false)
    {
        return Ok(());
    }
    Err(Error::conflict(format!(
        "table id {} is already assigned to logical table {} in namespace {} with {} state",
        table_id,
        table_name,
        namespace,
        TableState::from_str(state.as_str())?
    )))
}

pub(super) fn sql_value_from_json(value: &serde_json::Value) -> Result<SqlValue> {
    match value {
        serde_json::Value::Null => Ok(SqlValue::Null),
        serde_json::Value::Bool(value) => Ok(SqlValue::Integer(i64::from(*value))),
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_i64() {
                Ok(SqlValue::Integer(value))
            } else if let Some(value) = number.as_u64() {
                i64::try_from(value)
                    .map(SqlValue::Integer)
                    .map_err(|_| Error::InvalidInput(format!("numeric value exceeds i64: {value}")))
            } else if let Some(value) = number.as_f64() {
                Ok(SqlValue::Real(value))
            } else {
                Err(Error::InvalidInput(format!(
                    "unsupported numeric value: {number}"
                )))
            }
        }
        serde_json::Value::String(value) => Ok(SqlValue::Text(value.clone())),
        serde_json::Value::Array(_) | serde_json::Value::Object(_) => Err(Error::InvalidInput(
            "SQLite index scans do not support array or object comparison values".to_string(),
        )),
    }
}

pub(super) fn map_sqlite_error(error: rusqlite::Error) -> Error {
    let message = error.to_string();
    match error {
        rusqlite::Error::SqliteFailure(code, _) => match code.code {
            rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked => {
                Error::storage(StorageErrorKind::Busy, message)
            }
            rusqlite::ErrorCode::OutOfMemory | rusqlite::ErrorCode::DiskFull => {
                Error::ResourceExhausted(message)
            }
            rusqlite::ErrorCode::PermissionDenied
            | rusqlite::ErrorCode::ReadOnly
            | rusqlite::ErrorCode::AuthorizationForStatementDenied => {
                Error::PermissionDenied(message)
            }
            rusqlite::ErrorCode::CannotOpen => {
                Error::storage(StorageErrorKind::Unavailable, message)
            }
            rusqlite::ErrorCode::SystemIoFailure => Error::storage(StorageErrorKind::Io, message),
            rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase => {
                Error::storage(StorageErrorKind::Corruption, message)
            }
            rusqlite::ErrorCode::OperationAborted
            | rusqlite::ErrorCode::OperationInterrupted
            | rusqlite::ErrorCode::SchemaChanged
            | rusqlite::ErrorCode::FileLockingProtocolFailed => {
                Error::storage(StorageErrorKind::Transient, message)
            }
            _ => Error::storage(StorageErrorKind::Other, message),
        },
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}
