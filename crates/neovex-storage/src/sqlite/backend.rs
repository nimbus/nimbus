use super::*;

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
    data_json: String,
) -> Result<Document> {
    Ok(Document {
        id: *id,
        table: table.clone(),
        creation_time: Timestamp(creation_time),
        fields: serde_json::from_str(&data_json)
            .map_err(|error| Error::Serialization(error.to_string()))?,
    })
}

pub(super) fn load_document_from_conn(
    conn: &Connection,
    table: &TableName,
    id: &DocumentId,
) -> Result<Option<Document>> {
    conn.query_row(
        "SELECT creation_time, data_json
         FROM documents
         WHERE table_name = ?1 AND id = ?2",
        params![table.as_str(), id.to_string()],
        |row| {
            Ok(row_to_document(
                table,
                id,
                row.get(0)?,
                row.get::<_, String>(1)?,
            ))
        },
    )
    .optional()
    .map_err(map_sqlite_error)?
    .transpose()
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
