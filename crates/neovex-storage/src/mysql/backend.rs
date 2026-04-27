use super::*;

pub(super) fn validate_identifier_input(value: &str, label: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(format!("{label} cannot be empty")));
    }
    if value.len() >= MYSQL_IDENTIFIER_LIMIT {
        return Err(Error::InvalidInput(format!(
            "{label} must be shorter than {MYSQL_IDENTIFIER_LIMIT} bytes for MySQL"
        )));
    }
    Ok(())
}

pub(super) fn cached_schema(schema_cache: &RwLock<Option<Schema>>) -> Option<Schema> {
    schema_cache.read().ok().and_then(|guard| guard.clone())
}

pub(super) fn publish_schema_cache(schema_cache: &RwLock<Option<Schema>>, schema: &Schema) {
    if let Ok(mut guard) = schema_cache.write() {
        *guard = Some(schema.clone());
    }
}

pub(super) fn invalidate_schema_cache_handle(schema_cache: &RwLock<Option<Schema>>) {
    if let Ok(mut guard) = schema_cache.write() {
        *guard = None;
    }
}

pub(super) fn qualified_table(database_name: &str, table_name: &str) -> String {
    format!(
        "{}.{}",
        quote_identifier(database_name),
        quote_identifier(table_name)
    )
}

pub(super) fn quote_identifier(identifier: &str) -> String {
    let mut quoted = String::with_capacity(identifier.len() + 2);
    quoted.push('`');
    for character in identifier.chars() {
        if character == '`' {
            quoted.push('`');
        }
        quoted.push(character);
    }
    quoted.push('`');
    quoted
}

pub(super) fn mysql_index_key_prefix_chars(key_part_count: usize) -> usize {
    let part_count = key_part_count.max(1);
    let max_chars = MYSQL_MAX_INDEX_KEY_BYTES / MYSQL_INDEX_KEY_BYTES_PER_CHAR;
    (max_chars / part_count).clamp(1, MYSQL_INDEX_KEY_VALUE_LEN)
}

pub(super) fn mysql_index_key_part(identifier: &str, prefix_chars: usize) -> String {
    format!("{}({prefix_chars})", quote_identifier(identifier))
}

pub(super) async fn initialize_tenant_database(conn: &mut Conn, database_name: &str) -> Result<()> {
    for statement in tenant_init_statements(database_name) {
        conn.query_drop(statement).await.map_err(map_mysql_error)?;
    }
    Ok(())
}

pub(super) fn tenant_init_statements(database_name: &str) -> Vec<String> {
    vec![
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                table_name VARCHAR(191) NOT NULL,\
                id VARCHAR(191) NOT NULL,\
                data_json LONGTEXT NOT NULL,\
                typed_fields_json LONGTEXT NOT NULL,\
                creation_time BIGINT UNSIGNED NOT NULL,\
                update_time BIGINT UNSIGNED NOT NULL,\
                PRIMARY KEY (table_name, id)\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "documents")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                table_name VARCHAR(191) PRIMARY KEY,\
                schema_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "schemas")
        ),
        // Firestore path keys can exceed InnoDB's practical indexed-byte
        // budget, so MySQL indexes fixed SHA-256 digests while the
        // authoritative raw keys and binding payload remain in blobs.
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                locator_hash BINARY(32) PRIMARY KEY,\
                locator_key LONGBLOB NOT NULL,\
                document_path_hash BINARY(32) NOT NULL UNIQUE,\
                document_path_key LONGBLOB NOT NULL,\
                collection_group_hash BINARY(32) NOT NULL,\
                binding_blob LONGBLOB NOT NULL,\
                locator_blob LONGBLOB NOT NULL,\
                KEY idx_resource_path_bindings_collection_group_hash (collection_group_hash)\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "resource_path_bindings")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                execution_id VARCHAR(191) PRIMARY KEY\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "scheduled_job_executions")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                id VARCHAR(191) PRIMARY KEY,\
                run_at BIGINT UNSIGNED NOT NULL,\
                data_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "scheduled_jobs")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                id VARCHAR(191) PRIMARY KEY,\
                data_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "running_scheduled_jobs")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                job_id VARCHAR(191) PRIMARY KEY,\
                data_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "scheduled_job_results")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                registration_id VARCHAR(255) NOT NULL,\
                event_id VARCHAR(255) NOT NULL,\
                data_blob LONGBLOB NOT NULL,\
                PRIMARY KEY (registration_id, event_id)\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "trigger_invocations")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                name VARCHAR(191) PRIMARY KEY,\
                next_run BIGINT UNSIGNED NOT NULL,\
                enabled BOOLEAN NOT NULL,\
                data_json LONGTEXT NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "cron_jobs")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                sequence BIGINT UNSIGNED NOT NULL AUTO_INCREMENT PRIMARY KEY,\
                record_blob LONGBLOB NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "commit_log")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                key_name VARCHAR(191) PRIMARY KEY,\
                value_u64 BIGINT UNSIGNED NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "metadata")
        ),
        format!(
            "INSERT IGNORE INTO {} (key_name, value_u64) VALUES ('{}', 0)",
            qualified_table(database_name, "metadata"),
            APPLIED_SEQUENCE_KEY
        ),
    ]
}

pub(super) async fn database_exists(conn: &mut Conn, database_name: &str) -> Result<bool> {
    let row = conn
        .exec_first::<Row, _, _>(
            "SELECT SCHEMA_NAME FROM INFORMATION_SCHEMA.SCHEMATA WHERE SCHEMA_NAME = ?",
            (database_name,),
        )
        .await
        .map_err(map_mysql_error)?;
    Ok(row.is_some())
}

pub(super) fn map_mysql_error(error: mysql_async::Error) -> Error {
    let message = error.to_string();
    match error {
        mysql_async::Error::Server(server) => match server.code {
            1040 | 1041 | 1206 | 1226 => Error::ResourceExhausted(message),
            1044 | 1045 | 1142 | 1143 | 1227 => Error::PermissionDenied(message),
            1062 => Error::AlreadyExists(message),
            1205 => Error::storage(StorageErrorKind::Busy, message),
            1213 => Error::storage(StorageErrorKind::Transient, message),
            2006 | 2013 => Error::storage(StorageErrorKind::Unavailable, message),
            _ => Error::storage(StorageErrorKind::Other, message),
        },
        mysql_async::Error::Io(_) => Error::storage(StorageErrorKind::Io, message),
        mysql_async::Error::Url(_) => Error::InvalidInput(message),
        mysql_async::Error::Driver(driver) => match driver {
            mysql_async::DriverError::ConnectionClosed
            | mysql_async::DriverError::PoolDisconnected => {
                Error::storage(StorageErrorKind::Unavailable, message)
            }
            mysql_async::DriverError::PacketOutOfOrder
            | mysql_async::DriverError::UnexpectedPacket { .. } => {
                Error::storage(StorageErrorKind::Corruption, message)
            }
            _ => Error::storage(StorageErrorKind::Other, message),
        },
        mysql_async::Error::Other(_) => Error::storage(StorageErrorKind::Other, message),
    }
}

pub(super) fn mysql_server_error_code(error: &mysql_async::Error) -> Option<u16> {
    match error {
        mysql_async::Error::Server(error) => Some(error.code),
        _ => None,
    }
}

pub(super) fn map_join_error(error: tokio::task::JoinError) -> Error {
    if error.is_cancelled() {
        Error::Cancelled
    } else {
        Error::Internal(error.to_string())
    }
}

pub(super) fn map_permit_error(error: tokio::sync::AcquireError) -> Error {
    Error::Internal(error.to_string())
}

pub(super) async fn load_schema_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<Schema>
where
    C: Queryable,
{
    let query = format!(
        "SELECT schema_json FROM {} ORDER BY table_name",
        qualified_table(database_name, "schemas")
    );
    let rows: Vec<Row> = session.query(query).await.map_err(map_mysql_error)?;
    let mut schema = Schema::default();
    for row in rows {
        let (schema_json,): (String,) = mysql_async::from_row(row);
        let table_schema: TableSchema = deserialize_json(schema_json.as_str())?;
        schema
            .tables
            .insert(table_schema.table.clone(), table_schema);
    }
    Ok(schema)
}

pub(super) async fn load_journal_progress_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<JournalProgress>
where
    C: Queryable,
{
    let durable_head = load_latest_sequence_from_session(session, database_name).await?;
    let applied_head = load_metadata_u64_from_session(session, database_name, APPLIED_SEQUENCE_KEY)
        .await?
        .map(SequenceNumber)
        .unwrap_or(SequenceNumber(0));
    Ok(JournalProgress {
        durable_head,
        applied_head,
    })
}

pub(super) async fn load_latest_sequence_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<SequenceNumber>
where
    C: Queryable,
{
    let query = format!(
        "SELECT COALESCE(MAX(sequence), 0) FROM {}",
        qualified_table(database_name, "commit_log")
    );
    let value = session
        .query_first::<Option<u64>, _>(query)
        .await
        .map_err(map_mysql_error)?
        .flatten()
        .unwrap_or(0);
    Ok(SequenceNumber(value))
}

pub(super) async fn load_metadata_u64_from_session<C>(
    session: &mut C,
    database_name: &str,
    key: &str,
) -> Result<Option<u64>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT value_u64 FROM {} WHERE key_name = ?",
        qualified_table(database_name, "metadata")
    );
    session
        .exec_first::<Row, _, _>(query, (key,))
        .await
        .map_err(map_mysql_error)
        .map(|row| row.map(|row| mysql_async::from_row::<(u64,)>(row).0))
}

pub(super) async fn load_documents_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: Option<&TableName>,
) -> Result<Vec<Document>>
where
    C: Queryable,
{
    let (query, params_table) = if let Some(table) = table {
        (
            format!(
                "SELECT table_name, id, creation_time, update_time, data_json, typed_fields_json \
                 FROM {} WHERE table_name = ? ORDER BY id",
                qualified_table(database_name, "documents")
            ),
            Some(table.as_str().to_string()),
        )
    } else {
        (
            format!(
                "SELECT table_name, id, creation_time, update_time, data_json, typed_fields_json \
                 FROM {} ORDER BY table_name, id",
                qualified_table(database_name, "documents")
            ),
            None,
        )
    };
    let rows: Vec<Row> = if let Some(table_name) = params_table {
        session
            .exec(query, (table_name,))
            .await
            .map_err(map_mysql_error)?
    } else {
        session.query(query).await.map_err(map_mysql_error)?
    };
    rows.into_iter()
        .map(|row| {
            let (table_name, id, creation_time, update_time, data_json, typed_fields_json): (
                String,
                String,
                u64,
                u64,
                String,
                String,
            ) = mysql_async::from_row(row);
            let table = TableName::new(table_name)?;
            let id = DocumentId::from_str(&id)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            row_to_document(
                &table,
                &id,
                creation_time,
                update_time,
                data_json,
                typed_fields_json,
            )
        })
        .collect()
}

pub(super) async fn load_scheduled_execution_ids_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<Vec<String>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT execution_id FROM {} ORDER BY execution_id",
        qualified_table(database_name, "scheduled_job_executions")
    );
    let rows: Vec<Row> = session.query(query).await.map_err(map_mysql_error)?;
    Ok(rows
        .into_iter()
        .map(|row| mysql_async::from_row::<(String,)>(row).0)
        .collect())
}

pub(super) async fn load_durable_records_from_session<C>(
    session: &mut C,
    database_name: &str,
    sequence: SequenceNumber,
) -> Result<Vec<DurableMutationRecord>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT record_blob FROM {} WHERE sequence >= ? ORDER BY sequence",
        qualified_table(database_name, "commit_log")
    );
    let rows: Vec<Row> = session
        .exec(query, (sequence.0,))
        .await
        .map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| {
            let (record_blob,): (Vec<u8>,) = mysql_async::from_row(row);
            deserialize_durable_record(record_blob.as_slice())
        })
        .collect()
}

pub(super) async fn load_document_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    id: &DocumentId,
) -> Result<Option<Document>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT creation_time, update_time, data_json, typed_fields_json FROM {} WHERE table_name = ? AND id = ?",
        qualified_table(database_name, "documents")
    );
    session
        .exec_first::<Row, _, _>(query, (table.as_str(), id.to_string()))
        .await
        .map_err(map_mysql_error)?
        .map(|row| {
            let (creation_time, update_time, data_json, typed_fields_json): (
                u64,
                u64,
                String,
                String,
            ) = mysql_async::from_row(row);
            row_to_document(
                table,
                id,
                creation_time,
                update_time,
                data_json,
                typed_fields_json,
            )
        })
        .transpose()
}

pub(super) async fn load_table_schema_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
) -> Result<Option<TableSchema>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT schema_json FROM {} WHERE table_name = ?",
        qualified_table(database_name, "schemas")
    );
    session
        .exec_first::<Row, _, _>(query, (table.as_str(),))
        .await
        .map_err(map_mysql_error)?
        .map(|row| {
            deserialize_json::<TableSchema>(mysql_async::from_row::<(String,)>(row).0.as_str())
        })
        .transpose()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn load_index_candidate_documents_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    table_schema: &TableSchema,
    index_name: &str,
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<Vec<Document>>
where
    C: Queryable,
{
    let index_fields = index_fields_for_table_schema(table_schema, index_name)?;
    let range_field = index_fields.get(exact_prefix.len());

    let mut clauses = vec!["table_name = ?".to_string()];
    let mut params = vec![MySqlValue::Bytes(table.as_str().as_bytes().to_vec())];

    for (field, value) in index_fields.iter().zip(exact_prefix.iter()) {
        clauses.push(format!(
            "{} = ?",
            quote_identifier(&mysql_generated_column_name(table, field))
        ));
        params.push(mysql_index_text_value(value)?);
    }

    if let Some(range_field) = range_field {
        let field_type = field_type_for_table_schema(table_schema, range_field)?;
        match field_type {
            FieldType::String => {
                append_mysql_range_clause(
                    &mut clauses,
                    &mut params,
                    quote_identifier(&mysql_generated_column_name(table, range_field)),
                    start.map(mysql_index_text_value).transpose()?,
                    end.map(mysql_index_text_value).transpose()?,
                    start_inclusive,
                    end_inclusive,
                );
            }
            FieldType::Number => {
                append_mysql_range_clause(
                    &mut clauses,
                    &mut params,
                    mysql_numeric_column_expr(table, range_field),
                    start.map(mysql_numeric_value).transpose()?,
                    end.map(mysql_numeric_value).transpose()?,
                    start_inclusive,
                    end_inclusive,
                );
            }
            _ if start.is_some() || end.is_some() => {
                return Err(Error::InvalidInput(
                    "range scans only support string and number indexed fields".to_string(),
                ));
            }
            _ => {}
        }
    }

    let sql = format!(
        "SELECT table_name, id, creation_time, update_time, data_json, typed_fields_json \
         FROM {} \
         WHERE {} \
         ORDER BY id",
        qualified_table(database_name, "documents"),
        clauses.join(" AND ")
    );
    let rows: Vec<Row> = session
        .exec(sql, Params::Positional(params))
        .await
        .map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| {
            let (table_name, id, creation_time, update_time, data_json, typed_fields_json): (
                String,
                String,
                u64,
                u64,
                String,
                String,
            ) = mysql_async::from_row(row);
            let table = TableName::new(table_name)?;
            let id = DocumentId::from_str(&id)
                .map_err(|error| Error::Serialization(error.to_string()))?;
            row_to_document(
                &table,
                &id,
                creation_time,
                update_time,
                data_json,
                typed_fields_json,
            )
        })
        .collect()
}

pub(super) async fn load_scheduled_jobs_from_session<C>(
    session: &mut C,
    database_name: &str,
    table_name: &str,
) -> Result<Vec<ScheduledJob>>
where
    C: Queryable,
{
    let order_by = if table_name == "scheduled_jobs" {
        "run_at, id"
    } else {
        "id"
    };
    let query = format!(
        "SELECT data_json FROM {} ORDER BY {}",
        qualified_table(database_name, table_name),
        order_by
    );
    let rows: Vec<Row> = session.query(query).await.map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| {
            deserialize_json::<ScheduledJob>(mysql_async::from_row::<(String,)>(row).0.as_str())
        })
        .collect()
}

pub(super) async fn load_scheduled_job_result_from_session<C>(
    session: &mut C,
    database_name: &str,
    job_id: &str,
) -> Result<Option<ScheduledJobResult>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT data_json FROM {} WHERE job_id = ?",
        qualified_table(database_name, "scheduled_job_results")
    );
    session
        .exec_first::<Row, _, _>(query, (job_id,))
        .await
        .map_err(map_mysql_error)?
        .map(|row| {
            deserialize_json::<ScheduledJobResult>(
                mysql_async::from_row::<(String,)>(row).0.as_str(),
            )
        })
        .transpose()
}

pub(super) async fn load_cron_jobs_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<Vec<CronJob>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT data_json FROM {} ORDER BY name",
        qualified_table(database_name, "cron_jobs")
    );
    let rows: Vec<Row> = session.query(query).await.map_err(map_mysql_error)?;
    rows.into_iter()
        .map(|row| deserialize_json::<CronJob>(mysql_async::from_row::<(String,)>(row).0.as_str()))
        .collect()
}

pub(super) async fn begin_scheduled_execution_in_session<C>(
    session: &mut C,
    database_name: &str,
    execution_id: Option<&str>,
) -> Result<bool>
where
    C: Queryable,
{
    let Some(execution_id) = execution_id else {
        return Ok(true);
    };
    let exists_query = format!(
        "SELECT execution_id FROM {} WHERE execution_id = ?",
        qualified_table(database_name, "scheduled_job_executions")
    );
    if session
        .exec_first::<Row, _, _>(exists_query, (execution_id,))
        .await
        .map_err(map_mysql_error)?
        .is_some()
    {
        return Ok(false);
    }
    let query = format!(
        "INSERT INTO {} (execution_id) VALUES (?)",
        qualified_table(database_name, "scheduled_job_executions")
    );
    session
        .exec_drop(query, (execution_id,))
        .await
        .map_err(map_mysql_error)?;
    Ok(true)
}

pub(super) async fn apply_durable_record_in_session<C>(
    session: &mut C,
    database_name: &str,
    record: &DurableMutationRecord,
) -> Result<()>
where
    C: Queryable,
{
    if !begin_scheduled_execution_in_session(
        session,
        database_name,
        record.scheduled_execution_id.as_deref(),
    )
    .await?
    {
        return Ok(());
    }

    for write in &record.writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                let existing =
                    load_document_from_session(session, database_name, &write.table, &write.doc_id)
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
                        let query = format!(
                            "INSERT INTO {} (table_name, id, data_json, typed_fields_json, creation_time, update_time) VALUES (?, ?, ?, ?, ?, ?)",
                            qualified_table(database_name, "documents")
                        );
                        session
                            .exec_drop(
                                query,
                                (
                                    write.table.as_str(),
                                    write.doc_id.to_string(),
                                    serialize_document_fields(current)?,
                                    serialize_document_typed_fields(current)?,
                                    current.creation_time.0,
                                    current.update_time.0,
                                ),
                            )
                            .await
                            .map_err(map_mysql_error)?;
                    }
                }
            }
            (Some(previous), Some(current)) => {
                let existing =
                    load_document_from_session(session, database_name, &write.table, &write.doc_id)
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
                let query = format!(
                    "UPDATE {} SET data_json = ?, typed_fields_json = ?, creation_time = ?, update_time = ? WHERE table_name = ? AND id = ?",
                    qualified_table(database_name, "documents")
                );
                session
                    .exec_drop(
                        query,
                        (
                            serialize_document_fields(current)?,
                            serialize_document_typed_fields(current)?,
                            current.creation_time.0,
                            current.update_time.0,
                            write.table.as_str(),
                            write.doc_id.to_string(),
                        ),
                    )
                    .await
                    .map_err(map_mysql_error)?;
            }
            (Some(previous), None) => {
                match load_document_from_session(
                    session,
                    database_name,
                    &write.table,
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
                        let query = format!(
                            "DELETE FROM {} WHERE table_name = ? AND id = ?",
                            qualified_table(database_name, "documents")
                        );
                        session
                            .exec_drop(query, (write.table.as_str(), write.doc_id.to_string()))
                            .await
                            .map_err(map_mysql_error)?;
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

pub(super) async fn table_has_entries<C>(
    session: &mut C,
    database_name: &str,
    table_name: &str,
) -> Result<bool>
where
    C: Queryable,
{
    let query = format!(
        "SELECT 1 FROM {} LIMIT 1",
        qualified_table(database_name, table_name)
    );
    Ok(session
        .query_first::<Row, _>(query)
        .await
        .map_err(map_mysql_error)?
        .is_some())
}

pub(super) async fn create_mysql_indexes_for_table_schema<C>(
    session: &mut C,
    database_name: &str,
    table_schema: &TableSchema,
) -> Result<()>
where
    C: Queryable,
{
    for field in unique_index_fields(table_schema) {
        let column_name = mysql_generated_column_name(&table_schema.table, field);
        if !mysql_document_column_exists(session, database_name, &column_name).await? {
            let sql = format!(
                "ALTER TABLE {} ADD COLUMN {} VARCHAR({}) GENERATED ALWAYS AS ({}) VIRTUAL",
                qualified_table(database_name, "documents"),
                quote_identifier(&column_name),
                MYSQL_INDEX_KEY_VALUE_LEN,
                mysql_generated_column_expr(&table_schema.table, field),
            );
            session.query_drop(sql).await.map_err(map_mysql_error)?;
        }
    }
    for index in &table_schema.indexes {
        let index_name = mysql_index_name(&table_schema.table, &index.name);
        if mysql_document_index_exists(session, database_name, &index_name).await? {
            continue;
        }
        let key_part_prefix = mysql_index_key_prefix_chars(index.fields.len() + 2);
        let mut columns = Vec::with_capacity(index.fields.len() + 2);
        columns.push(mysql_index_key_part("table_name", key_part_prefix));
        columns.extend(index.fields.iter().map(|field| {
            mysql_index_key_part(
                &mysql_generated_column_name(&table_schema.table, field),
                key_part_prefix,
            )
        }));
        columns.push(mysql_index_key_part("id", key_part_prefix));
        let sql = format!(
            "CREATE INDEX {} ON {} ({})",
            quote_identifier(&index_name),
            qualified_table(database_name, "documents"),
            columns.join(", ")
        );
        session.query_drop(sql).await.map_err(map_mysql_error)?;
    }
    Ok(())
}

pub(super) async fn drop_mysql_indexes_for_table_schema<C>(
    session: &mut C,
    database_name: &str,
    table_schema: &TableSchema,
) -> Result<()>
where
    C: Queryable,
{
    for index in &table_schema.indexes {
        let index_name = mysql_index_name(&table_schema.table, &index.name);
        if mysql_document_index_exists(session, database_name, &index_name).await? {
            let sql = format!(
                "DROP INDEX {} ON {}",
                quote_identifier(&index_name),
                qualified_table(database_name, "documents")
            );
            session.query_drop(sql).await.map_err(map_mysql_error)?;
        }
    }
    for field in unique_index_fields(table_schema) {
        let column_name = mysql_generated_column_name(&table_schema.table, field);
        if mysql_document_column_exists(session, database_name, &column_name).await? {
            let sql = format!(
                "ALTER TABLE {} DROP COLUMN {}",
                qualified_table(database_name, "documents"),
                quote_identifier(&column_name),
            );
            session.query_drop(sql).await.map_err(map_mysql_error)?;
        }
    }
    Ok(())
}

pub(super) async fn mysql_document_column_exists<C>(
    session: &mut C,
    database_name: &str,
    column_name: &str,
) -> Result<bool>
where
    C: Queryable,
{
    let row = session
        .exec_first::<Row, _, _>(
            "SELECT COLUMN_NAME \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'documents' AND COLUMN_NAME = ?",
            (database_name, column_name),
        )
        .await
        .map_err(map_mysql_error)?;
    Ok(row.is_some())
}

pub(super) async fn mysql_document_index_exists<C>(
    session: &mut C,
    database_name: &str,
    index_name: &str,
) -> Result<bool>
where
    C: Queryable,
{
    let row = session
        .exec_first::<Row, _, _>(
            "SELECT INDEX_NAME \
             FROM INFORMATION_SCHEMA.STATISTICS \
             WHERE TABLE_SCHEMA = ? AND TABLE_NAME = 'documents' AND INDEX_NAME = ?",
            (database_name, index_name),
        )
        .await
        .map_err(map_mysql_error)?;
    Ok(row.is_some())
}

pub(super) fn expect_write_commit(
    commit: Option<CommitEntry>,
    expectation: &str,
) -> Result<CommitEntry> {
    commit.ok_or_else(|| Error::Internal(expectation.to_string()))
}

pub(super) fn apply_schedule_ops_in_transaction(
    transaction: &mut MySqlWriteTransaction,
    schedule_ops: &[ResolvedScheduleOp],
) -> Result<()> {
    for schedule_op in schedule_ops {
        match schedule_op {
            ResolvedScheduleOp::Insert { job } => transaction.insert_scheduled_job(job)?,
            ResolvedScheduleOp::Cancel { job_id } => {
                if !transaction.cancel_scheduled_job(job_id)? {
                    return Err(Error::ScheduledJobNotFound(job_id.clone()));
                }
            }
        }
    }
    Ok(())
}

pub(super) fn serialize_json<T>(value: &T) -> Result<String>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn serialize_document_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.fields).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn matches_filters(document: &Document, filters: &[Filter]) -> Result<bool> {
    for filter in filters {
        let Some(field_value) = document.get_field(&filter.field) else {
            return Ok(false);
        };
        let matched = match filter.op {
            FilterOp::Eq => field_value == &filter.value,
            FilterOp::Neq => field_value != &filter.value,
            FilterOp::Gt => {
                compare_values(field_value, &filter.value)? == std::cmp::Ordering::Greater
            }
            FilterOp::Gte => matches!(
                compare_values(field_value, &filter.value)?,
                std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
            ),
            FilterOp::Lt => compare_values(field_value, &filter.value)? == std::cmp::Ordering::Less,
            FilterOp::Lte => matches!(
                compare_values(field_value, &filter.value)?,
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            ),
        };
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn filter_documents_with_predicate<F>(
    documents: Vec<Document>,
    filters: &[Filter],
    check_cancel: &mut dyn FnMut() -> Result<()>,
    mut include_document: F,
) -> Result<Vec<Document>>
where
    F: FnMut(&Document) -> Result<bool>,
{
    let mut filtered = Vec::new();
    for document in documents {
        check_cancel()?;
        if matches_filters(&document, filters)? && include_document(&document)? {
            filtered.push(document);
        }
    }
    Ok(filtered)
}

pub(super) fn compare_values(left: &Value, right: &Value) -> Result<std::cmp::Ordering> {
    match (left, right) {
        (Value::String(left), Value::String(right)) => Ok(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => {
            let left = left
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            let right = right
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            left.partial_cmp(&right).ok_or_else(|| {
                Error::InvalidInput("invalid numeric ordering comparison".to_string())
            })
        }
        _ => Err(Error::InvalidInput(
            "comparisons only support string and number fields in phase 1".to_string(),
        )),
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn filter_index_documents_with_cancel(
    documents: Vec<Document>,
    table: &TableName,
    index_fields: &[String],
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let range_field = index_fields.get(exact_prefix.len());
    let mut filtered = Vec::new();
    for document in documents {
        check_cancel()?;
        if &document.table != table {
            continue;
        }
        if !document_matches_exact_prefix(&document, index_fields, exact_prefix) {
            continue;
        }
        if let Some(range_field) = range_field
            && !document_matches_range_bounds(
                &document,
                range_field,
                start,
                end,
                start_inclusive,
                end_inclusive,
            )?
        {
            continue;
        }
        filtered.push(document);
    }
    Ok(filtered)
}

pub(super) fn index_fields_for_table_schema(
    table_schema: &TableSchema,
    index_name: &str,
) -> Result<Vec<String>> {
    let index = table_schema
        .indexes
        .iter()
        .find(|index| index.name == index_name)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "index '{}' not found for table '{}'",
                index_name,
                table_schema.table.as_str()
            ))
        })?;
    Ok(index.fields.clone())
}

pub(super) fn field_type_for_table_schema(
    table_schema: &TableSchema,
    field_name: &str,
) -> Result<FieldType> {
    table_schema
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .map(|field| field.field_type)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "field '{}' not found in schema for table '{}'",
                field_name,
                table_schema.table.as_str()
            ))
        })
}

pub(super) fn mysql_index_text_value(value: &Value) -> Result<MySqlValue> {
    match value {
        Value::String(value) => Ok(MySqlValue::Bytes(value.as_bytes().to_vec())),
        Value::Number(number) => Ok(MySqlValue::Bytes(number.to_string().into_bytes())),
        _ => Err(Error::InvalidInput(
            "index equality and prefix scans only support string and number values".to_string(),
        )),
    }
}

pub(super) fn mysql_numeric_value(value: &Value) -> Result<MySqlValue> {
    let number = value.as_f64().ok_or_else(|| {
        Error::InvalidInput("numeric range bounds require number values".to_string())
    })?;
    Ok(MySqlValue::Double(number))
}

pub(super) fn mysql_numeric_column_expr(table: &TableName, field: &str) -> String {
    format!(
        "CAST({} AS DOUBLE)",
        quote_identifier(&mysql_generated_column_name(table, field))
    )
}

pub(super) fn append_mysql_range_clause(
    clauses: &mut Vec<String>,
    params: &mut Vec<MySqlValue>,
    expr: String,
    start: Option<MySqlValue>,
    end: Option<MySqlValue>,
    start_inclusive: bool,
    end_inclusive: bool,
) {
    if let Some(start) = start {
        let operator = if start_inclusive { ">=" } else { ">" };
        clauses.push(format!("{expr} {operator} ?"));
        params.push(start);
    }
    if let Some(end) = end {
        let operator = if end_inclusive { "<=" } else { "<" };
        clauses.push(format!("{expr} {operator} ?"));
        params.push(end);
    }
}

pub(super) fn document_matches_exact_prefix(
    document: &Document,
    index_fields: &[String],
    exact_prefix: &[Value],
) -> bool {
    index_fields
        .iter()
        .zip(exact_prefix.iter())
        .all(|(field, expected)| document.get_field(field) == Some(expected))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn document_matches_range_bounds(
    document: &Document,
    field: &str,
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<bool> {
    if let Some(start) = start {
        let Some(value) = document.get_field(field) else {
            return Ok(false);
        };
        let ordering = compare_values(value, start)?;
        if start_inclusive {
            if ordering == std::cmp::Ordering::Less {
                return Ok(false);
            }
        } else if !matches!(ordering, std::cmp::Ordering::Greater) {
            return Ok(false);
        }
    }
    if let Some(end) = end {
        let Some(value) = document.get_field(field) else {
            return Ok(false);
        };
        let ordering = compare_values(value, end)?;
        if end_inclusive {
            if ordering == std::cmp::Ordering::Greater {
                return Ok(false);
            }
        } else if !matches!(ordering, std::cmp::Ordering::Less) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(Error::InvalidInput(
            "durable journal stream limit must be greater than zero".to_string(),
        ));
    }
    if limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "durable journal stream limit {limit} exceeds maximum {MAX_DURABLE_JOURNAL_STREAM_LIMIT}"
        )));
    }
    Ok(())
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

pub(super) fn serialize_document_typed_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.typed_fields)
        .map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn claim_due_jobs_upper_bound(timestamp: Timestamp) -> u64 {
    timestamp.0
}

pub(super) fn mysql_index_name(table: &TableName, index_name: &str) -> String {
    let digest = Sha256::digest(format!("{}:{index_name}", table.as_str()).as_bytes());
    let mut suffix = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("idx_{suffix}")
}

pub(super) fn mysql_generated_column_name(table: &TableName, field: &str) -> String {
    let digest = Sha256::digest(format!("{}:{field}", table.as_str()).as_bytes());
    let mut suffix = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("gcol_{suffix}")
}

pub(super) fn unique_index_fields(table_schema: &TableSchema) -> Vec<&str> {
    let mut fields = Vec::new();
    for index in &table_schema.indexes {
        for field in &index.fields {
            if !fields.contains(&field.as_str()) {
                fields.push(field.as_str());
            }
        }
    }
    fields
}

pub(super) fn mysql_generated_column_expr(table: &TableName, field: &str) -> String {
    format!(
        "CASE WHEN table_name = {} THEN JSON_UNQUOTE(JSON_EXTRACT(data_json, '$.\"{}\"')) ELSE NULL END",
        mysql_string_literal(table.as_str()),
        field.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

pub(super) fn mysql_string_literal(value: &str) -> String {
    let mut quoted = String::with_capacity(value.len() + 2);
    quoted.push('\'');
    for character in value.chars() {
        if character == '\'' {
            quoted.push('\'');
        }
        quoted.push(character);
    }
    quoted.push('\'');
    quoted
}
