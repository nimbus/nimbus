use super::table_lifecycle::{
    activate_hidden_table_identity_in_session, hard_delete_table_identity_in_session,
    mark_table_deleting_in_session, stage_hidden_table_identity_in_session,
};
use super::*;
use crate::keys::prefix_end;
use crate::mysql::document_versions::{
    record_document_versions_for_events_in_session, record_document_versions_for_writes_in_session,
};
use crate::mysql::index_versions::{
    record_index_versions_for_events_in_session, record_index_versions_for_writes_in_session,
};

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
                namespace VARCHAR(191) NOT NULL DEFAULT 'default',\
                table_name VARCHAR(191) NOT NULL,\
                table_id VARCHAR(191) NOT NULL UNIQUE,\
                state VARCHAR(32) NOT NULL DEFAULT 'active',\
                PRIMARY KEY (namespace, table_name)\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "table_catalog")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                table_id VARCHAR(191) NOT NULL,\
                id VARCHAR(191) NOT NULL,\
                data_json LONGTEXT NOT NULL,\
                typed_fields_json LONGTEXT NOT NULL,\
                creation_time BIGINT UNSIGNED NOT NULL,\
                update_time BIGINT UNSIGNED NOT NULL,\
                PRIMARY KEY (table_id, id),\
                CONSTRAINT fk_documents_table_id FOREIGN KEY (table_id) REFERENCES {} (table_id)\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "documents"),
            qualified_table(database_name, "table_catalog")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                table_id VARCHAR(191) NOT NULL,\
                id VARCHAR(191) NOT NULL,\
                commit_sequence BIGINT UNSIGNED NOT NULL,\
                commit_time BIGINT UNSIGNED NOT NULL,\
                tombstone BOOLEAN NOT NULL,\
                data_json LONGTEXT NULL,\
                typed_fields_json LONGTEXT NULL,\
                creation_time BIGINT UNSIGNED NULL,\
                update_time BIGINT UNSIGNED NULL,\
                PRIMARY KEY (table_id, id, commit_sequence),\
                CHECK (\
                    (\
                        tombstone = TRUE \
                        AND data_json IS NULL \
                        AND typed_fields_json IS NULL \
                        AND creation_time IS NULL \
                        AND update_time IS NULL \
                    )\
                    OR (\
                        tombstone = FALSE \
                        AND data_json IS NOT NULL \
                        AND typed_fields_json IS NOT NULL \
                        AND creation_time IS NOT NULL \
                        AND update_time IS NOT NULL \
                    )\
                )\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "document_versions")
        ),
        format!(
            "CREATE TABLE IF NOT EXISTS {} (\
                table_id VARCHAR(191) NOT NULL,\
                index_id VARCHAR(191) NOT NULL,\
                encoded_tuple_hash BINARY(32) NOT NULL,\
                encoded_tuple LONGBLOB NOT NULL,\
                document_id VARCHAR(191) NOT NULL,\
                visible_from BIGINT UNSIGNED NOT NULL,\
                visible_until BIGINT UNSIGNED NULL,\
                PRIMARY KEY (table_id, index_id, encoded_tuple_hash, document_id, visible_from),\
                KEY idx_index_versions_visibility (table_id, index_id, encoded_tuple_hash, document_id, visible_from)\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "index_versions")
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
            "CREATE TABLE IF NOT EXISTS {} (\
                singleton BOOLEAN NOT NULL PRIMARY KEY DEFAULT TRUE CHECK (singleton = TRUE),\
                owner_id VARCHAR(191) NOT NULL,\
                epoch BIGINT UNSIGNED NOT NULL CHECK (epoch >= 1),\
                expires_at DATETIME(6) NOT NULL,\
                durable_sequence BIGINT UNSIGNED NOT NULL\
            ) ENGINE=InnoDB",
            qualified_table(database_name, "committer_lease")
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

pub(super) const MYSQL_EXECUTOR_CONTEXT: &str = "mysql executor";

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

pub(super) async fn load_durable_journal_cursor_floor_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<SequenceNumber>
where
    C: Queryable,
{
    let query = format!(
        "SELECT MIN(sequence) FROM {}",
        qualified_table(database_name, "commit_log")
    );
    let value = session
        .query_first::<Option<u64>, _>(query)
        .await
        .map_err(map_mysql_error)?
        .flatten()
        .unwrap_or(0);
    Ok(SequenceNumber(value.saturating_sub(1)))
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
                "SELECT c.table_name, d.id, d.creation_time, d.update_time, d.data_json, d.typed_fields_json \
                 FROM {} AS d \
                 JOIN {} AS c ON c.table_id = d.table_id \
                 WHERE c.namespace = 'default' AND c.table_name = ? \
                 ORDER BY d.id",
                qualified_table(database_name, "documents"),
                qualified_table(database_name, "table_catalog")
            ),
            Some(table.as_str().to_string()),
        )
    } else {
        (
            format!(
                "SELECT c.table_name, d.id, d.creation_time, d.update_time, d.data_json, d.typed_fields_json \
                 FROM {} AS d \
                 JOIN {} AS c ON c.table_id = d.table_id \
                 WHERE c.namespace = 'default' \
                 ORDER BY c.table_name, d.id",
                qualified_table(database_name, "documents"),
                qualified_table(database_name, "table_catalog")
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
) -> Result<Vec<TenantEventRecord>>
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
            deserialize_tenant_event_record(record_blob.as_slice())
        })
        .collect()
}

pub(super) async fn load_documents_by_id_prefix_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    id_prefix: &str,
) -> Result<Vec<Document>>
where
    C: Queryable,
{
    let Some(table_id) = load_table_id_from_session(session, database_name, table).await? else {
        return Ok(Vec::new());
    };
    let start = id_prefix.to_owned();
    let table_name = table.as_str().to_owned();
    let rows: Vec<Row> = if let Some(end) =
        prefix_end(id_prefix.as_bytes()).and_then(|bytes| String::from_utf8(bytes).ok())
    {
        let query = format!(
            "SELECT ? AS table_name, id, creation_time, update_time, data_json, typed_fields_json \
             FROM {} \
             WHERE table_id = ? AND id >= ? AND id < ? \
             ORDER BY id",
            qualified_table(database_name, "documents")
        );
        session
            .exec(
                query,
                (table_name, table_id.as_str().to_owned(), start, end),
            )
            .await
            .map_err(map_mysql_error)?
    } else {
        let query = format!(
            "SELECT ? AS table_name, id, creation_time, update_time, data_json, typed_fields_json \
             FROM {} \
             WHERE table_id = ? AND id >= ? \
             ORDER BY id",
            qualified_table(database_name, "documents")
        );
        session
            .exec(query, (table_name, table_id.as_str().to_owned(), start))
            .await
            .map_err(map_mysql_error)?
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

pub(super) async fn load_documents_starting_at_id_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    start_id: &str,
    limit: usize,
) -> Result<Vec<Document>>
where
    C: Queryable,
{
    if limit == 0 {
        return Ok(Vec::new());
    }
    let Some(table_id) = load_table_id_from_session(session, database_name, table).await? else {
        return Ok(Vec::new());
    };
    let query = format!(
        "SELECT ? AS table_name, id, creation_time, update_time, data_json, typed_fields_json \
         FROM {} \
         WHERE table_id = ? AND id >= ? \
         ORDER BY id \
         LIMIT ?",
        qualified_table(database_name, "documents")
    );
    let rows: Vec<Row> = session
        .exec(
            query,
            (
                table.as_str().to_owned(),
                table_id.as_str().to_owned(),
                start_id.to_owned(),
                u64::try_from(limit).unwrap_or(u64::MAX),
            ),
        )
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

pub(super) async fn load_document_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    id: &DocumentId,
) -> Result<Option<Document>>
where
    C: Queryable,
{
    let Some(table_id) = load_table_id_from_session(session, database_name, table).await? else {
        return Ok(None);
    };
    let query = format!(
        "SELECT creation_time, update_time, data_json, typed_fields_json FROM {} WHERE table_id = ? AND id = ?",
        qualified_table(database_name, "documents")
    );
    session
        .exec_first::<Row, _, _>(query, (table_id.as_str(), id.to_string()))
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

pub(super) async fn load_document_by_table_id_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    table_id: &TableId,
    id: &DocumentId,
) -> Result<Option<Document>>
where
    C: Queryable,
{
    let query = format!(
        "SELECT creation_time, update_time, data_json, typed_fields_json FROM {} WHERE table_id = ? AND id = ?",
        qualified_table(database_name, "documents")
    );
    session
        .exec_first::<Row, _, _>(query, (table_id.as_str(), id.to_string()))
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

pub(super) async fn load_index_candidate_documents_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    table_schema: &TableSchema,
    index_name: &str,
    exact_prefix: &[Value],
    bounds: crate::range_bound::OwnedIndexRangeBounds,
) -> Result<Vec<Document>>
where
    C: Queryable,
{
    let crate::range_bound::OwnedIndexRangeBounds { start, end } = bounds;
    let index_fields = index_fields_for_table_schema(table_schema, index_name)?;
    let range_field = index_fields.get(exact_prefix.len());

    let Some(table_id) = load_table_id_from_session(session, database_name, table).await? else {
        return Ok(Vec::new());
    };
    let mut clauses = vec!["d.table_id = ?".to_string()];
    let mut params = vec![MySqlValue::Bytes(table_id.to_string().into_bytes())];

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
                    crate::range_bound::map_owned_index_range_bound(
                        start.clone(),
                        mysql_index_text_value,
                    )?,
                    crate::range_bound::map_owned_index_range_bound(
                        end.clone(),
                        mysql_index_text_value,
                    )?,
                );
            }
            FieldType::Number => {
                append_mysql_range_clause(
                    &mut clauses,
                    &mut params,
                    mysql_numeric_column_expr(table, range_field),
                    crate::range_bound::map_owned_index_range_bound(
                        start.clone(),
                        mysql_numeric_value,
                    )?,
                    crate::range_bound::map_owned_index_range_bound(
                        end.clone(),
                        mysql_numeric_value,
                    )?,
                );
            }
            _ if !matches!(start, std::ops::Bound::Unbounded)
                || !matches!(end, std::ops::Bound::Unbounded) =>
            {
                return Err(Error::InvalidInput(
                    "range scans only support string and number indexed fields".to_string(),
                ));
            }
            _ => {}
        }
    }

    let sql = format!(
        "SELECT c.table_name, d.id, d.creation_time, d.update_time, d.data_json, d.typed_fields_json \
         FROM {} AS d \
         JOIN {} AS c ON c.table_id = d.table_id \
         WHERE {} \
         ORDER BY d.id",
        qualified_table(database_name, "documents"),
        qualified_table(database_name, "table_catalog"),
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
    record: &TenantEventRecord,
) -> Result<()>
where
    C: Queryable,
{
    if record.events.is_empty() {
        if !begin_scheduled_execution_in_session(
            session,
            database_name,
            record.scheduled_execution_id.as_deref(),
        )
        .await?
        {
            return Ok(());
        }
        record_document_versions_for_writes_in_session(
            session,
            database_name,
            record.sequence,
            record.timestamp,
            &record.writes,
        )
        .await?;
        record_index_versions_for_writes_in_session(
            session,
            database_name,
            record.sequence,
            &record.writes,
        )
        .await?;
        return apply_document_writes_in_session(
            session,
            database_name,
            record.sequence,
            &record.writes,
        )
        .await;
    }

    record_document_versions_for_events_in_session(
        session,
        database_name,
        record.sequence,
        record.timestamp,
        &record.events,
    )
    .await?;
    record_index_versions_for_events_in_session(
        session,
        database_name,
        record.sequence,
        &record.events,
    )
    .await?;
    for event in &record.events {
        apply_tenant_event_in_session(session, database_name, record.sequence, event).await?;
    }

    Ok(())
}

async fn apply_tenant_event_in_session<C>(
    session: &mut C,
    database_name: &str,
    sequence: SequenceNumber,
    event: &TenantEventKind,
) -> Result<()>
where
    C: Queryable,
{
    match event {
        TenantEventKind::DocumentWrite { writes } => {
            apply_document_writes_in_session(session, database_name, sequence, writes).await
        }
        TenantEventKind::SchemaChange { change } => {
            apply_schema_change_in_session(session, database_name, change).await
        }
        TenantEventKind::TableLifecycle { lifecycle } => {
            apply_table_lifecycle_in_session(session, database_name, lifecycle).await
        }
        TenantEventKind::IndexLifecycle { .. } | TenantEventKind::Barrier { .. } => Ok(()),
        TenantEventKind::ScheduledExecution { execution_id } => {
            let _ =
                begin_scheduled_execution_in_session(session, database_name, Some(execution_id))
                    .await?;
            Ok(())
        }
        TenantEventKind::TriggerDelivery { cursor } => {
            let query = format!(
                "INSERT INTO {} (key_name, value_u64) VALUES (?, ?)
                 ON DUPLICATE KEY UPDATE value_u64 = VALUES(value_u64)",
                qualified_table(database_name, "metadata")
            );
            session
                .exec_drop(
                    query,
                    (TRIGGER_DELIVERY_CURSOR_KEY, cursor.materialized_through.0),
                )
                .await
                .map_err(map_mysql_error)?;
            Ok(())
        }
    }
}

async fn apply_document_writes_in_session<C>(
    session: &mut C,
    database_name: &str,
    sequence: SequenceNumber,
    writes: &[WriteOp],
) -> Result<()>
where
    C: Queryable,
{
    for write in writes {
        match (&write.previous, &write.current) {
            (None, Some(current)) => {
                ensure_table_id_from_session(session, database_name, &write.table, &write.table_id)
                    .await?;
                let existing = load_document_by_table_id_from_session(
                    session,
                    database_name,
                    &write.table,
                    &write.table_id,
                    &write.doc_id,
                )
                .await?;
                match existing {
                    Some(existing) if existing == *current => {}
                    Some(_) => {
                        return Err(crate::commit_log::durable_replay_preimage_corruption(
                            sequence,
                            "insert",
                            write.doc_id.as_str(),
                            "found unexpected state",
                        ));
                    }
                    None => {
                        let query = format!(
                            "INSERT INTO {} (table_id, id, data_json, typed_fields_json, creation_time, update_time) VALUES (?, ?, ?, ?, ?, ?)",
                            qualified_table(database_name, "documents")
                        );
                        session
                            .exec_drop(
                                query,
                                (
                                    write.table_id.as_str(),
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
                ensure_table_id_from_session(session, database_name, &write.table, &write.table_id)
                    .await?;
                let existing = load_document_by_table_id_from_session(
                    session,
                    database_name,
                    &write.table,
                    &write.table_id,
                    &write.doc_id,
                )
                .await?
                .ok_or_else(|| {
                    crate::commit_log::durable_replay_preimage_corruption(
                        sequence,
                        "update",
                        write.doc_id.as_str(),
                        "is missing the expected pre-image",
                    )
                })?;
                if existing != *current && existing != *previous {
                    return Err(crate::commit_log::durable_replay_preimage_corruption(
                        sequence,
                        "update",
                        write.doc_id.as_str(),
                        "found a pre-image mismatch",
                    ));
                }
                if existing != *current {
                    let query = format!(
                        "UPDATE {} SET data_json = ?, typed_fields_json = ?, creation_time = ?, update_time = ? WHERE table_id = ? AND id = ?",
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
                                write.table_id.as_str(),
                                write.doc_id.to_string(),
                            ),
                        )
                        .await
                        .map_err(map_mysql_error)?;
                }
            }
            (Some(previous), None) => {
                ensure_table_id_from_session(session, database_name, &write.table, &write.table_id)
                    .await?;
                match load_document_by_table_id_from_session(
                    session,
                    database_name,
                    &write.table,
                    &write.table_id,
                    &write.doc_id,
                )
                .await?
                {
                    Some(existing) if existing != *previous => {
                        return Err(crate::commit_log::durable_replay_preimage_corruption(
                            sequence,
                            "delete",
                            write.doc_id.as_str(),
                            "found a pre-image mismatch",
                        ));
                    }
                    Some(_) => {
                        let query = format!(
                            "DELETE FROM {} WHERE table_id = ? AND id = ?",
                            qualified_table(database_name, "documents")
                        );
                        session
                            .exec_drop(query, (write.table_id.as_str(), write.doc_id.to_string()))
                            .await
                            .map_err(map_mysql_error)?;
                    }
                    None => {}
                }
            }
            (None, None) => {
                return Err(Error::Internal(
                    "durable journal write must include a previous or current document".to_string(),
                ));
            }
        }
        match (&write.current, write.resource_path_binding.as_ref()) {
            (Some(_), Some(binding)) => {
                super::resource_paths::upsert_resource_path_binding_in_session(
                    session,
                    database_name,
                    binding,
                )
                .await?;
            }
            (None, _) => {
                super::resource_paths::remove_resource_path_binding_in_session(
                    session,
                    database_name,
                    &nimbus_core::DocumentLocator::new(write.table.clone(), write.doc_id.clone()),
                )
                .await?;
            }
            (Some(_), None) => {}
        }
    }

    Ok(())
}

async fn apply_schema_change_in_session<C>(
    session: &mut C,
    database_name: &str,
    change: &SchemaChangeEvent,
) -> Result<()>
where
    C: Queryable,
{
    match change {
        SchemaChangeEvent::SetTable {
            table,
            table_id,
            previous,
            current,
        } => {
            ensure_table_id_from_session(session, database_name, table, table_id).await?;
            if let Some(previous) = previous {
                drop_mysql_indexes_for_table_schema(session, database_name, previous).await?;
            }
            let query = format!(
                "INSERT INTO {} (table_name, schema_json) VALUES (?, ?)
                 ON DUPLICATE KEY UPDATE schema_json = VALUES(schema_json)",
                qualified_table(database_name, "schemas")
            );
            session
                .exec_drop(query, (table.as_str(), serialize_json(current)?))
                .await
                .map_err(map_mysql_error)?;
            create_mysql_indexes_for_table_schema(session, database_name, current).await
        }
        SchemaChangeEvent::DeleteTable {
            table, previous, ..
        } => {
            if let Some(previous) = previous {
                drop_mysql_indexes_for_table_schema(session, database_name, previous).await?;
            }
            let query = format!(
                "DELETE FROM {} WHERE table_name = ?",
                qualified_table(database_name, "schemas")
            );
            session
                .exec_drop(query, (table.as_str(),))
                .await
                .map_err(map_mysql_error)?;
            Ok(())
        }
    }
}

async fn apply_table_lifecycle_in_session<C>(
    session: &mut C,
    database_name: &str,
    lifecycle: &TableLifecycleEvent,
) -> Result<()>
where
    C: Queryable,
{
    match lifecycle {
        TableLifecycleEvent::StageHidden { table, table_id } => {
            stage_hidden_table_identity_in_session(session, database_name, table, table_id).await
        }
        TableLifecycleEvent::ActivateHidden {
            table, table_id, ..
        } => {
            let _ =
                activate_hidden_table_identity_in_session(session, database_name, table, table_id)
                    .await?;
            Ok(())
        }
        TableLifecycleEvent::MarkDeleting { table, .. } => {
            let _ = mark_table_deleting_in_session(session, database_name, table).await?;
            Ok(())
        }
        TableLifecycleEvent::HardDelete { table, table_id } => {
            if hard_delete_table_identity_in_session(session, database_name, table_id)
                .await?
                .is_some()
                && load_table_id_from_session(session, database_name, table)
                    .await?
                    .is_none()
            {
                if let Some(schema) =
                    load_table_schema_from_session(session, database_name, table).await?
                {
                    drop_mysql_indexes_for_table_schema(session, database_name, &schema).await?;
                }
                let query = format!(
                    "DELETE FROM {} WHERE table_name = ?",
                    qualified_table(database_name, "schemas")
                );
                session
                    .exec_drop(query, (table.as_str(),))
                    .await
                    .map_err(map_mysql_error)?;
            }
            Ok(())
        }
    }
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
    load_table_id_from_session(session, database_name, &table_schema.table)
        .await?
        .ok_or_else(|| {
            Error::Internal(format!(
                "cannot create indexes for logical table {} before its table identity exists",
                table_schema.table
            ))
        })?;
    for field in unique_index_fields(table_schema) {
        let column_name = mysql_generated_column_name(&table_schema.table, field);
        if !mysql_document_column_exists(session, database_name, &column_name).await? {
            let sql = format!(
                "ALTER TABLE {} ADD COLUMN {} VARCHAR({}) GENERATED ALWAYS AS ({}) VIRTUAL",
                qualified_table(database_name, "documents"),
                quote_identifier(&column_name),
                MYSQL_INDEX_KEY_VALUE_LEN,
                mysql_generated_column_expr(field),
            );
            session.query_drop(sql).await.map_err(map_mysql_error)?;
        }
    }
    for index in table_schema.maintained_indexes() {
        let index_name = mysql_index_name(&index.id);
        if mysql_document_index_exists(session, database_name, &index_name).await? {
            continue;
        }
        let key_part_prefix = mysql_index_key_prefix_chars(index.fields.len() + 2);
        let mut columns = Vec::with_capacity(index.fields.len() + 2);
        columns.push(mysql_index_key_part("table_id", key_part_prefix));
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
    for index in table_schema.maintained_indexes() {
        let index_name = mysql_index_name(&index.id);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_init_statements_keep_document_version_boolean_predicates_tokenized() {
        let sql = tenant_init_statements("tenant_test_database").join(";");

        assert!(sql.contains("tombstone = TRUE AND data_json IS NULL"));
        assert!(sql.contains("tombstone = FALSE AND data_json IS NOT NULL"));
        assert!(!sql.contains("TRUEAND"));
        assert!(!sql.contains("FALSEAND"));
    }
}
