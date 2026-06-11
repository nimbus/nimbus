use super::*;
use crate::libsql::document_versions::{
    record_document_versions_for_events_remote, record_document_versions_for_writes_remote,
};
use crate::libsql::index_versions::{
    record_index_versions_for_events_remote, record_index_versions_for_writes_remote,
};
use crate::table_identity::{
    DEFAULT_TABLE_NAMESPACE, deleting_table_namespace, hidden_table_namespace,
};

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
    let Some(table_id) = load_remote_table_id_from_session(conn, &table).await? else {
        return Ok(None);
    };
    let mut rows = conn
        .query(
            "SELECT creation_time, update_time, data_json, typed_fields_json
             FROM documents
             WHERE table_id = ?1 AND id = ?2",
            libsql::params![table_id.as_str(), id.to_string()],
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

pub(super) async fn load_remote_document_by_table_id_from_session(
    conn: &Connection,
    table: &TableName,
    table_id: &TableId,
    id: &DocumentId,
) -> Result<Option<Document>> {
    let mut rows = conn
        .query(
            "SELECT creation_time, update_time, data_json, typed_fields_json
             FROM documents
             WHERE table_id = ?1 AND id = ?2",
            libsql::params![table_id.as_str(), id.to_string()],
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
        table,
        id,
        creation_time,
        update_time,
        data_json.as_str(),
        typed_fields_json.as_str(),
    )?))
}

pub(super) async fn load_remote_table_id_from_session(
    conn: &Connection,
    table: &TableName,
) -> Result<Option<TableId>> {
    let mut rows = conn
        .query(
            "SELECT table_id, state FROM table_catalog WHERE namespace = ?1 AND table_name = ?2",
            libsql::params!["default", table.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    let table_id = row.get::<String>(0).map_err(map_libsql_error)?;
    let state = TableState::from_str(row.get::<String>(1).map_err(map_libsql_error)?.as_str())?;
    if state != TableState::Active {
        return Err(Error::Conflict(format!(
            "logical table {} is in {} lifecycle state",
            table, state
        )));
    }
    Ok(Some(TableId::from_str(table_id.as_str())?))
}

pub(super) async fn resolve_or_create_remote_table_id(
    conn: &Connection,
    table: &TableName,
) -> Result<TableId> {
    if let Some(table_id) = load_remote_table_id_from_session(conn, table).await? {
        return Ok(table_id);
    }
    let table_id = TableId::new();
    conn.execute(
        "INSERT OR IGNORE INTO table_catalog (namespace, table_name, table_id, state)
         VALUES (?1, ?2, ?3, ?4)",
        libsql::params![
            "default",
            table.as_str(),
            table_id.as_str(),
            TableState::Active.as_str()
        ],
    )
    .await
    .map_err(map_libsql_error)?;
    load_remote_table_id_from_session(conn, table)
        .await?
        .ok_or_else(|| {
            Error::Internal(format!(
                "failed to resolve table id for logical table {} after catalog insert",
                table
            ))
        })
}

pub(super) async fn ensure_remote_table_id(
    conn: &Connection,
    table: &TableName,
    table_id: &TableId,
) -> Result<()> {
    let hidden_namespace = hidden_table_namespace(table_id);
    let staged_hidden = match remote_catalog_identity_row(conn, hidden_namespace.as_str(), table)
        .await?
    {
        Some((hidden_id, TableState::Hidden)) if hidden_id == *table_id => true,
        Some((hidden_id, state)) => {
            return Err(Error::Conflict(format!(
                "hidden identity slot for logical table {} and table id {} contains {} in {} state",
                table, table_id, hidden_id, state
            )));
        }
        None => false,
    };

    match remote_catalog_identity_row(conn, DEFAULT_TABLE_NAMESPACE, table).await? {
        Some((existing, TableState::Active)) if existing == *table_id => {
            if staged_hidden {
                return Err(Error::Conflict(format!(
                    "logical table {} already has active table id {} and a duplicate hidden slot",
                    table, table_id
                )));
            }
            return Ok(());
        }
        Some((existing, state)) if existing == *table_id => {
            return Err(Error::Conflict(format!(
                "logical table {} is assigned table id {} in {} lifecycle state",
                table, table_id, state
            )));
        }
        Some((existing, TableState::Active)) => {
            ensure_remote_table_id_available(
                conn,
                table_id,
                Some((hidden_namespace.as_str(), table)),
            )
            .await?;
            conn.execute(
                "UPDATE table_catalog
                 SET namespace = ?1, state = ?2
                 WHERE namespace = ?3 AND table_name = ?4",
                libsql::params![
                    deleting_table_namespace(&existing),
                    TableState::Deleting.as_str(),
                    DEFAULT_TABLE_NAMESPACE,
                    table.as_str()
                ],
            )
            .await
            .map_err(map_libsql_error)?;
            if staged_hidden {
                conn.execute(
                    "DELETE FROM table_catalog WHERE namespace = ?1 AND table_name = ?2",
                    libsql::params![hidden_namespace.as_str(), table.as_str()],
                )
                .await
                .map_err(map_libsql_error)?;
            }
            conn.execute(
                "INSERT INTO table_catalog (namespace, table_name, table_id, state)
                 VALUES (?1, ?2, ?3, ?4)",
                libsql::params![
                    DEFAULT_TABLE_NAMESPACE,
                    table.as_str(),
                    table_id.as_str(),
                    TableState::Active.as_str()
                ],
            )
            .await
            .map_err(map_libsql_error)?;
            return Ok(());
        }
        Some((existing, state)) => {
            return Err(Error::Conflict(format!(
                "logical table {} is already assigned table id {} in {} lifecycle state, journal references {}",
                table, existing, state, table_id
            )));
        }
        None => {}
    }
    ensure_remote_table_id_available(conn, table_id, Some((hidden_namespace.as_str(), table)))
        .await?;
    if staged_hidden {
        conn.execute(
            "DELETE FROM table_catalog WHERE namespace = ?1 AND table_name = ?2",
            libsql::params![hidden_namespace.as_str(), table.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    }
    conn.execute(
        "INSERT INTO table_catalog (namespace, table_name, table_id, state)
         VALUES (?1, ?2, ?3, ?4)",
        libsql::params![
            "default",
            table.as_str(),
            table_id.as_str(),
            TableState::Active.as_str()
        ],
    )
    .await
    .map_err(map_libsql_error)?;
    Ok(())
}

async fn remote_catalog_identity_row(
    conn: &Connection,
    namespace: &str,
    table: &TableName,
) -> Result<Option<(TableId, TableState)>> {
    let mut rows = conn
        .query(
            "SELECT table_id, state FROM table_catalog WHERE namespace = ?1 AND table_name = ?2",
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

async fn ensure_remote_table_id_available(
    conn: &Connection,
    table_id: &TableId,
    allowed_key: Option<(&str, &TableName)>,
) -> Result<()> {
    let mut rows = conn
        .query(
            "SELECT namespace, table_name, state FROM table_catalog WHERE table_id = ?1",
            libsql::params![table_id.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(());
    };
    let namespace = row.get::<String>(0).map_err(map_libsql_error)?;
    let table = TableName::new(row.get::<String>(1).map_err(map_libsql_error)?)?;
    let state = TableState::from_str(row.get::<String>(2).map_err(map_libsql_error)?.as_str())?;
    if allowed_key
        .map(|(allowed_namespace, allowed_table)| {
            allowed_namespace == namespace && allowed_table == &table
        })
        .unwrap_or(false)
    {
        return Ok(());
    }
    Err(Error::Conflict(format!(
        "table id {} is already assigned to logical table {} in namespace {} with {} state",
        table_id, table, namespace, state
    )))
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
    if record.events.is_empty() {
        if let Some(execution_id) = record.scheduled_execution_id.as_deref() {
            let _ = begin_scheduled_execution_remote(conn, Some(execution_id)).await?;
        }
        record_document_versions_for_writes_remote(
            conn,
            record.sequence,
            record.timestamp,
            &record.writes,
        )
        .await?;
        record_index_versions_for_writes_remote(conn, record.sequence, &record.writes).await?;
        return apply_document_writes_in_remote_conn(conn, &record.writes).await;
    }

    record_document_versions_for_events_remote(
        conn,
        record.sequence,
        record.timestamp,
        &record.events,
    )
    .await?;
    record_index_versions_for_events_remote(conn, record.sequence, &record.events).await?;
    for event in &record.events {
        apply_tenant_event_in_remote_conn(conn, event).await?;
    }

    Ok(())
}

async fn apply_tenant_event_in_remote_conn(
    conn: &Connection,
    event: &TenantEventKind,
) -> Result<()> {
    match event {
        TenantEventKind::DocumentWrite { writes } => {
            apply_document_writes_in_remote_conn(conn, writes).await
        }
        TenantEventKind::SchemaChange { change } => {
            apply_schema_change_in_remote_conn(conn, change).await
        }
        TenantEventKind::TableLifecycle { lifecycle } => {
            apply_table_lifecycle_in_remote_conn(conn, lifecycle).await
        }
        TenantEventKind::IndexLifecycle { .. } | TenantEventKind::Barrier { .. } => Ok(()),
        TenantEventKind::ScheduledExecution { execution_id } => {
            let _ = begin_scheduled_execution_remote(conn, Some(execution_id)).await?;
            Ok(())
        }
        TenantEventKind::TriggerDelivery { cursor } => {
            put_remote_metadata_u64(
                conn,
                TRIGGER_DELIVERY_CURSOR_KEY,
                cursor.materialized_through.0,
            )
            .await
        }
    }
}

async fn apply_document_writes_in_remote_conn(conn: &Connection, writes: &[WriteOp]) -> Result<()> {
    for write in writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                ensure_remote_table_id(conn, &write.table, &write.table_id).await?;
                let existing = load_remote_document_by_table_id_from_session(
                    conn,
                    &write.table,
                    &write.table_id,
                    &write.doc_id,
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
                                table_id,
                                id,
                                data_json,
                                typed_fields_json,
                                creation_time,
                                update_time
                             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                            libsql::params![
                                write.table_id.as_str(),
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
                ensure_remote_table_id(conn, &write.table, &write.table_id).await?;
                let existing = load_remote_document_by_table_id_from_session(
                    conn,
                    &write.table,
                    &write.table_id,
                    &write.doc_id,
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
                     WHERE table_id = ?1 AND id = ?2",
                    libsql::params![
                        write.table_id.as_str(),
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
                ensure_remote_table_id(conn, &write.table, &write.table_id).await?;
                match load_remote_document_by_table_id_from_session(
                    conn,
                    &write.table,
                    &write.table_id,
                    &write.doc_id,
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
                            "DELETE FROM documents WHERE table_id = ?1 AND id = ?2",
                            libsql::params![write.table_id.as_str(), write.doc_id.to_string()],
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

async fn apply_schema_change_in_remote_conn(
    conn: &Connection,
    change: &SchemaChangeEvent,
) -> Result<()> {
    match change {
        SchemaChangeEvent::SetTable {
            table,
            table_id,
            current,
            ..
        } => {
            ensure_remote_table_id(conn, table, table_id).await?;
            conn.execute(
                "INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)
                 ON CONFLICT(table_name) DO UPDATE SET schema_json = excluded.schema_json",
                libsql::params![table.as_str(), serialize_json(current)?],
            )
            .await
            .map_err(map_libsql_error)?;
            Ok(())
        }
        SchemaChangeEvent::DeleteTable { table, .. } => {
            conn.execute(
                "DELETE FROM schemas WHERE table_name = ?1",
                libsql::params![table.as_str()],
            )
            .await
            .map_err(map_libsql_error)?;
            Ok(())
        }
    }
}

async fn apply_table_lifecycle_in_remote_conn(
    conn: &Connection,
    lifecycle: &TableLifecycleEvent,
) -> Result<()> {
    match lifecycle {
        TableLifecycleEvent::StageHidden { table, table_id } => {
            conn.execute(
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
        TableLifecycleEvent::ActivateHidden {
            table, table_id, ..
        } => {
            if let Some(active_table_id) = load_remote_table_id_from_session(conn, table).await? {
                conn.execute(
                    "UPDATE table_catalog
                     SET namespace = ?1, state = ?2
                     WHERE namespace = ?3 AND table_name = ?4",
                    libsql::params![
                        deleting_table_namespace(&active_table_id),
                        TableState::Deleting.as_str(),
                        DEFAULT_TABLE_NAMESPACE,
                        table.as_str()
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            }
            conn.execute(
                "UPDATE table_catalog
                 SET namespace = ?1, state = ?2
                 WHERE namespace = ?3 AND table_name = ?4 AND table_id = ?5",
                libsql::params![
                    DEFAULT_TABLE_NAMESPACE,
                    TableState::Active.as_str(),
                    hidden_table_namespace(table_id),
                    table.as_str(),
                    table_id.as_str()
                ],
            )
            .await
            .map_err(map_libsql_error)?;
            Ok(())
        }
        TableLifecycleEvent::MarkDeleting { table, table_id } => {
            conn.execute(
                "UPDATE table_catalog
                 SET namespace = ?1, state = ?2
                 WHERE namespace = ?3 AND table_name = ?4 AND table_id = ?5",
                libsql::params![
                    deleting_table_namespace(table_id),
                    TableState::Deleting.as_str(),
                    DEFAULT_TABLE_NAMESPACE,
                    table.as_str(),
                    table_id.as_str()
                ],
            )
            .await
            .map_err(map_libsql_error)?;
            Ok(())
        }
        TableLifecycleEvent::HardDelete { table, table_id } => {
            conn.execute(
                "DELETE FROM documents WHERE table_id = ?1",
                libsql::params![table_id.as_str()],
            )
            .await
            .map_err(map_libsql_error)?;
            conn.execute(
                "DELETE FROM table_catalog WHERE table_id = ?1",
                libsql::params![table_id.as_str()],
            )
            .await
            .map_err(map_libsql_error)?;
            if load_remote_table_id_from_session(conn, table)
                .await?
                .is_none()
            {
                conn.execute(
                    "DELETE FROM schemas WHERE table_name = ?1",
                    libsql::params![table.as_str()],
                )
                .await
                .map_err(map_libsql_error)?;
            }
            Ok(())
        }
    }
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

pub(super) const LIBSQL_REPLICA_EXECUTOR_CONTEXT: &str = "libsql replica executor";

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
