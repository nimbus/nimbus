use super::*;

pub(super) fn apply_schedule_ops_in_libsql_transaction(
    transaction: &mut LibsqlReplicaWriteTransaction,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for op in schedule_ops {
        match op {
            ResolvedScheduleOp::Insert { job } => transaction.insert_scheduled_job(job)?,
            ResolvedScheduleOp::Cancel { job_id } => {
                transaction.cancel_scheduled_job(job_id)?;
            }
        }
    }
    Ok(())
}

pub(super) async fn table_has_entries_remote(conn: &Connection, table: &str) -> Result<bool> {
    let sql = format!("SELECT 1 FROM {table} LIMIT 1");
    let mut rows = conn
        .query(sql.as_str(), ())
        .await
        .map_err(map_libsql_error)?;
    Ok(rows.next().await.map_err(map_libsql_error)?.is_some())
}

pub(super) async fn load_remote_document_from_session(
    conn: &Connection,
    table: TableName,
    id: DocumentId,
) -> Result<Option<Document>> {
    let mut rows = conn
        .query(
            "SELECT creation_time, update_time, data_json, typed_fields_json
             FROM documents
             WHERE table_name = ?1 AND id = ?2",
            libsql::params![table.as_str(), id.to_string()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    let creation_time = row.get::<i64>(0).map_err(map_libsql_error)?;
    let update_time = row.get::<i64>(1).map_err(map_libsql_error)?;
    let data_json = row.get::<String>(2).map_err(map_libsql_error)?;
    let typed_fields_json = row.get::<String>(3).map_err(map_libsql_error)?;
    Ok(Some(row_to_document(
        &table,
        &id,
        creation_time,
        update_time,
        data_json.as_str(),
        typed_fields_json.as_str(),
    )?))
}

pub(super) async fn load_next_sequence_from_session(conn: &Connection) -> Result<u64> {
    if let Some(stored) = load_remote_metadata_u64(conn, NEXT_SEQUENCE_KEY).await? {
        return Ok(stored);
    }
    let mut rows = conn
        .query("SELECT MAX(sequence) FROM commit_log", ())
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(1);
    };
    let latest = row.get::<Option<i64>>(0).map_err(map_libsql_error)?;
    Ok(latest
        .map(sequence_from_i64)
        .transpose()?
        .unwrap_or(SequenceNumber(0))
        .0
        .saturating_add(1))
}

pub(super) async fn load_remote_metadata_u64(conn: &Connection, key: &str) -> Result<Option<u64>> {
    let mut rows = conn
        .query(
            "SELECT value_blob FROM metadata WHERE key = ?1",
            libsql::params![key.to_string()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    let bytes = row.get::<Vec<u8>>(0).map_err(map_libsql_error)?;
    Ok(Some(decode_u64(bytes.as_slice())?))
}

pub(super) async fn put_remote_metadata_u64(
    conn: &Connection,
    key: &str,
    value: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
        libsql::params![key.to_string(), encode_u64(value).to_vec()],
    )
    .await
    .map_err(map_libsql_error)?;
    Ok(())
}

pub(super) async fn apply_durable_record_in_remote_conn(
    conn: &Connection,
    record: &DurableMutationRecord,
) -> Result<()> {
    if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
        let _ = begin_scheduled_execution_remote(conn, Some(execution_id)).await?;
    }

    for write in &record.writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                let existing = load_remote_document_from_session(
                    conn,
                    write.table.clone(),
                    write.doc_id.clone(),
                )
                .await?;
                match existing {
                    Some(existing) if existing == *current => continue,
                    Some(_) => {
                        return Err(Error::Conflict(format!(
                            "durable journal insert replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    None => {
                        conn.execute(
                            "INSERT INTO documents (
                                table_name,
                                id,
                                data_json,
                                typed_fields_json,
                                creation_time,
                                update_time
                             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            libsql::params![
                                write.table.as_str(),
                                write.doc_id.to_string(),
                                serialize_document_fields(current)?,
                                serialize_document_typed_fields(current)?,
                                i64_from_u64(current.creation_time.0)?,
                                i64_from_u64(current.update_time.0)?
                            ],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    }
                }
            }
            (Some(previous), Some(current)) => {
                let existing = load_remote_document_from_session(
                    conn,
                    write.table.clone(),
                    write.doc_id.clone(),
                )
                .await?
                .ok_or(Error::Conflict(format!(
                    "durable journal update replay missing document {}",
                    write.doc_id
                )))?;
                if existing == *current {
                    continue;
                }
                if existing != *previous {
                    return Err(Error::Conflict(format!(
                        "durable journal update replay found conflicting state for document {}",
                        write.doc_id
                    )));
                }
                conn.execute(
                    "UPDATE documents
                     SET data_json = ?3, typed_fields_json = ?4, creation_time = ?5, update_time = ?6
                     WHERE table_name = ?1 AND id = ?2",
                    libsql::params![
                        write.table.as_str(),
                        write.doc_id.to_string(),
                        serialize_document_fields(current)?,
                        serialize_document_typed_fields(current)?,
                        i64_from_u64(current.creation_time.0)?,
                        i64_from_u64(current.update_time.0)?
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            }
            (Some(previous), None) => {
                match load_remote_document_from_session(
                    conn,
                    write.table.clone(),
                    write.doc_id.clone(),
                )
                .await?
                {
                    Some(existing) if existing != *previous => {
                        return Err(Error::Conflict(format!(
                            "durable journal delete replay found conflicting state for document {}",
                            write.doc_id
                        )));
                    }
                    Some(_) => {
                        conn.execute(
                            "DELETE FROM documents WHERE table_name = ?1 AND id = ?2",
                            libsql::params![write.table.as_str(), write.doc_id.to_string()],
                        )
                        .await
                        .map_err(map_libsql_error)?;
                    }
                    None => continue,
                }
            }
            (None, None) => {
                return Err(Error::Internal(
                    "durable journal write must include a previous or current document".to_string(),
                ));
            }
        }
    }

    Ok(())
}

pub(super) async fn begin_scheduled_execution_remote(
    conn: &Connection,
    execution_id: Option<&str>,
) -> Result<bool> {
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };
    let inserted = conn
        .execute(
            "INSERT OR IGNORE INTO scheduled_job_executions (execution_id) VALUES (?1)",
            libsql::params![execution_id],
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(inserted == 1)
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
    serialize_json(&document.fields)
}

pub(super) fn serialize_document_typed_fields(document: &Document) -> Result<String> {
    serialize_json(&document.typed_fields)
}

pub(super) fn encode_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

pub(super) fn decode_u64(bytes: &[u8]) -> Result<u64> {
    <[u8; 8]>::try_from(bytes)
        .map(u64::from_be_bytes)
        .map_err(|_| Error::Serialization("invalid u64 encoding".to_string()))
}

pub(super) fn row_to_document(
    table: &TableName,
    id: &DocumentId,
    creation_time: i64,
    update_time: i64,
    data_json: &str,
    typed_fields_json: &str,
) -> Result<Document> {
    Ok(Document {
        id: id.clone(),
        table: table.clone(),
        creation_time: Timestamp(u64::try_from(creation_time).map_err(|_| {
            Error::storage(
                StorageErrorKind::Corruption,
                format!("negative creation_time in libsql row: {creation_time}"),
            )
        })?),
        update_time: Timestamp(u64::try_from(update_time).map_err(|_| {
            Error::storage(
                StorageErrorKind::Corruption,
                format!("negative update_time in libsql row: {update_time}"),
            )
        })?),
        fields: deserialize_json(data_json)?,
        typed_fields: deserialize_json(typed_fields_json)?,
    })
}

pub(super) fn sequence_from_i64(value: i64) -> Result<SequenceNumber> {
    Ok(SequenceNumber(u64::try_from(value).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("negative libsql sequence value: {value}"),
        )
    })?))
}

pub(super) fn i64_from_u64(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| Error::InvalidInput(format!("value {value} exceeds SQLite INTEGER")))
}

pub(super) fn map_libsql_error(error: libsql::Error) -> Error {
    let message = error.to_string();
    match error {
        libsql::Error::ConnectionFailed(_)
        | libsql::Error::Hrana(_)
        | libsql::Error::WriteDelegation(_)
        | libsql::Error::Replication(_)
        | libsql::Error::Sync(_)
        | libsql::Error::InvalidTlsConfiguration(_) => {
            Error::storage(StorageErrorKind::Unavailable, message)
        }
        libsql::Error::WalConflict => Error::storage(StorageErrorKind::Busy, message),
        libsql::Error::SqliteFailure(code, _) | libsql::Error::RemoteSqliteFailure(_, code, _) => {
            map_sqlite_result_code(code, message)
        }
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}

pub(super) fn map_local_sqlite_error(error: rusqlite::Error) -> Error {
    let message = error.to_string();
    match error {
        rusqlite::Error::SqliteFailure(code, _) => {
            map_sqlite_result_code(code.extended_code, message)
        }
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}

pub(super) fn map_permit_error(_error: tokio::sync::AcquireError) -> Error {
    Error::Internal("libsql replica executor unexpectedly closed".to_string())
}

pub(super) fn map_join_error(error: tokio::task::JoinError) -> Error {
    Error::Internal(format!("libsql replica read task failed: {error}"))
}

pub(super) fn storage_io_error(error: impl std::fmt::Display) -> Error {
    Error::storage(StorageErrorKind::Io, error.to_string())
}

pub(super) fn map_sqlite_result_code(code: i32, message: String) -> Error {
    match code & 0xff {
        5 | 6 => Error::storage(StorageErrorKind::Busy, message),
        3 | 8 | 23 => Error::PermissionDenied(message),
        7 | 13 => Error::ResourceExhausted(message),
        10 => Error::storage(StorageErrorKind::Io, message),
        11 | 26 => Error::storage(StorageErrorKind::Corruption, message),
        14 => Error::storage(StorageErrorKind::Unavailable, message),
        9 | 15 | 17 => Error::storage(StorageErrorKind::Transient, message),
        _ => Error::storage(StorageErrorKind::Other, message),
    }
}
