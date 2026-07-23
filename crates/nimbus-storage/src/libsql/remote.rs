use super::*;

const REMOTE_NAMESPACE_SNAPSHOT_SQL: &str = r#"
SELECT namespace, table_name, table_id, state
FROM table_catalog
ORDER BY namespace, table_name, table_id, state;
SELECT table_name, schema_json
FROM schemas
ORDER BY table_name;
SELECT d.table_id, d.id, d.creation_time, d.update_time, d.data_json, d.typed_fields_json
FROM documents AS d
JOIN table_catalog AS c ON c.table_id = d.table_id
WHERE c.namespace = 'default'
ORDER BY c.table_name, d.id;
SELECT table_id, id, commit_sequence, commit_time, tombstone, data_json, typed_fields_json,
       creation_time, update_time
FROM document_versions
ORDER BY table_id, id, commit_sequence;
SELECT table_id, index_id, encoded_tuple, document_id, visible_from, visible_until
FROM index_versions
ORDER BY table_id, index_id, encoded_tuple, document_id, visible_from;
SELECT locator_key, document_path_key, collection_group, binding_blob, locator_blob
FROM resource_path_bindings
ORDER BY collection_group, document_path_key;
SELECT id, run_at, data_json
FROM scheduled_jobs
ORDER BY run_at, id;
SELECT id, data_json
FROM running_scheduled_jobs
ORDER BY id;
SELECT job_id, data_json
FROM scheduled_job_results
ORDER BY job_id;
SELECT execution_id
FROM scheduled_job_executions
ORDER BY execution_id;
SELECT name, data_json
FROM cron_jobs
ORDER BY name;
SELECT sequence, record_blob
FROM commit_log
ORDER BY sequence;
SELECT key, value_blob
FROM metadata
ORDER BY key;
"#;

#[derive(Debug, Clone)]
pub(super) struct RemoteNamespaceSnapshot {
    pub(super) table_catalog: Vec<RemoteTableCatalogRow>,
    pub(super) schemas: Vec<RemoteSchemaRow>,
    pub(super) documents: Vec<RemoteDocumentRow>,
    pub(super) document_versions: Vec<RemoteDocumentVersionRow>,
    pub(super) index_versions: Vec<RemoteIndexVersionRow>,
    pub(super) resource_path_bindings: Vec<RemoteResourcePathBindingRow>,
    pub(super) scheduled_jobs: Vec<RemoteScheduledJobRow>,
    pub(super) running_scheduled_jobs: Vec<RemoteJsonRow>,
    pub(super) scheduled_job_results: Vec<RemoteJsonRow>,
    pub(super) scheduled_job_executions: Vec<String>,
    pub(super) cron_jobs: Vec<RemoteNamedJsonRow>,
    pub(super) commit_log: Vec<RemoteCommitLogRow>,
    pub(super) metadata: Vec<RemoteMetadataRow>,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteTableCatalogRow {
    namespace: String,
    table_name: String,
    table_id: String,
    state: String,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteSchemaRow {
    pub(super) table_name: String,
    pub(super) schema_json: String,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteDocumentRow {
    table_id: String,
    id: String,
    creation_time: u64,
    update_time: u64,
    data_json: String,
    typed_fields_json: String,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteDocumentVersionRow {
    table_id: String,
    id: String,
    commit_sequence: i64,
    commit_time: i64,
    tombstone: i64,
    data_json: Option<String>,
    typed_fields_json: Option<String>,
    creation_time: Option<i64>,
    update_time: Option<i64>,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteIndexVersionRow {
    table_id: String,
    index_id: String,
    encoded_tuple: Vec<u8>,
    document_id: String,
    visible_from: i64,
    visible_until: Option<i64>,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteResourcePathBindingRow {
    locator_key: Vec<u8>,
    document_path_key: Vec<u8>,
    collection_group: String,
    binding_blob: Vec<u8>,
    locator_blob: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteJsonRow {
    key: String,
    data_json: String,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteScheduledJobRow {
    id: String,
    run_at: String,
    data_json: String,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteNamedJsonRow {
    name: String,
    data_json: String,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteCommitLogRow {
    pub(super) sequence: u64,
    pub(super) record_blob: Vec<u8>,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteMetadataRow {
    key: String,
    value_blob: Vec<u8>,
}

pub(super) async fn fetch_remote_namespace_snapshot(
    session: &LibsqlRemoteSession,
) -> Result<RemoteNamespaceSnapshot> {
    retry_idempotent_remote_operation(
        session,
        "fetch libsql namespace snapshot",
        |conn| async move { fetch_remote_namespace_snapshot_once(&conn).await },
    )
    .await
}

async fn fetch_remote_namespace_snapshot_once(
    conn: &Connection,
) -> Result<RemoteNamespaceSnapshot> {
    // A snapshot is already materialized fully in memory. Use one ordered
    // transactional Hrana batch so every table belongs to the same provider
    // snapshot and one tenant reopen consumes one HTTP request rather than
    // thirteen streaming cursor connections.
    let mut batch = conn
        .execute_transactional_batch(REMOTE_NAMESPACE_SNAPSHOT_SQL)
        .await
        .map_err(map_libsql_error)?;
    let snapshot = RemoteNamespaceSnapshot {
        table_catalog: collect_remote_table_catalog_rows(take_snapshot_rows(
            &mut batch,
            "table_catalog",
        )?)
        .await?,
        schemas: collect_remote_schema_rows(take_snapshot_rows(&mut batch, "schemas")?).await?,
        documents: collect_remote_document_rows(take_snapshot_rows(&mut batch, "documents")?)
            .await?,
        document_versions: collect_remote_document_version_rows(take_snapshot_rows(
            &mut batch,
            "document_versions",
        )?)
        .await?,
        index_versions: collect_remote_index_version_rows(take_snapshot_rows(
            &mut batch,
            "index_versions",
        )?)
        .await?,
        resource_path_bindings: collect_remote_resource_path_binding_rows(take_snapshot_rows(
            &mut batch,
            "resource_path_bindings",
        )?)
        .await?,
        scheduled_jobs: collect_remote_scheduled_job_rows(take_snapshot_rows(
            &mut batch,
            "scheduled_jobs",
        )?)
        .await?,
        running_scheduled_jobs: collect_remote_json_rows(take_snapshot_rows(
            &mut batch,
            "running_scheduled_jobs",
        )?)
        .await?,
        scheduled_job_results: collect_remote_json_rows(take_snapshot_rows(
            &mut batch,
            "scheduled_job_results",
        )?)
        .await?,
        scheduled_job_executions: collect_remote_execution_ids(take_snapshot_rows(
            &mut batch,
            "scheduled_job_executions",
        )?)
        .await?,
        cron_jobs: collect_remote_named_json_rows(take_snapshot_rows(&mut batch, "cron_jobs")?)
            .await?,
        commit_log: collect_remote_commit_log_rows(take_snapshot_rows(&mut batch, "commit_log")?)
            .await?,
        metadata: collect_remote_metadata_rows(take_snapshot_rows(&mut batch, "metadata")?).await?,
    };
    if batch.next_stmt_row().is_some() {
        return Err(snapshot_batch_contract_error(
            "provider returned more than the 13 required statement results",
        ));
    }
    Ok(snapshot)
}

fn take_snapshot_rows(batch: &mut libsql::BatchRows, table: &str) -> Result<libsql::Rows> {
    match batch.next_stmt_row() {
        Some(Some(rows)) => Ok(rows),
        Some(None) => Err(snapshot_batch_contract_error(format!(
            "provider skipped required {table} statement"
        ))),
        None => Err(snapshot_batch_contract_error(format!(
            "provider omitted required {table} statement result"
        ))),
    }
}

fn snapshot_batch_contract_error(message: impl Into<String>) -> Error {
    Error::storage(
        StorageErrorKind::Corruption,
        format!(
            "libSQL namespace snapshot batch contract violation: {}",
            message.into()
        ),
    )
}

async fn collect_remote_table_catalog_rows(
    mut rows: libsql::Rows,
) -> Result<Vec<RemoteTableCatalogRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteTableCatalogRow {
            namespace: row.get::<String>(0).map_err(map_libsql_error)?,
            table_name: row.get::<String>(1).map_err(map_libsql_error)?,
            table_id: row.get::<String>(2).map_err(map_libsql_error)?,
            state: row.get::<String>(3).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

pub(super) async fn query_remote_schema_rows(conn: &Connection) -> Result<Vec<RemoteSchemaRow>> {
    let rows = conn
        .query(
            "SELECT table_name, schema_json FROM schemas ORDER BY table_name",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    collect_remote_schema_rows(rows).await
}

async fn collect_remote_schema_rows(mut rows: libsql::Rows) -> Result<Vec<RemoteSchemaRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteSchemaRow {
            table_name: row.get::<String>(0).map_err(map_libsql_error)?,
            schema_json: row.get::<String>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn collect_remote_document_rows(mut rows: libsql::Rows) -> Result<Vec<RemoteDocumentRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        let creation_time = row.get::<i64>(2).map_err(map_libsql_error)?;
        let update_time = row.get::<i64>(3).map_err(map_libsql_error)?;
        result.push(RemoteDocumentRow {
            table_id: row.get::<String>(0).map_err(map_libsql_error)?,
            id: row.get::<String>(1).map_err(map_libsql_error)?,
            creation_time: u64::try_from(creation_time).map_err(|_| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    format!(
                        "remote libsql creation_time {creation_time} is negative for namespace snapshot"
                    ),
                )
            })?,
            update_time: u64::try_from(update_time).map_err(|_| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    format!(
                        "remote libsql update_time {update_time} is negative for namespace snapshot"
                    ),
                )
            })?,
            data_json: row.get::<String>(4).map_err(map_libsql_error)?,
            typed_fields_json: row.get::<String>(5).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn collect_remote_document_version_rows(
    mut rows: libsql::Rows,
) -> Result<Vec<RemoteDocumentVersionRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteDocumentVersionRow {
            table_id: row.get::<String>(0).map_err(map_libsql_error)?,
            id: row.get::<String>(1).map_err(map_libsql_error)?,
            commit_sequence: row.get::<i64>(2).map_err(map_libsql_error)?,
            commit_time: row.get::<i64>(3).map_err(map_libsql_error)?,
            tombstone: row.get::<i64>(4).map_err(map_libsql_error)?,
            data_json: row.get::<Option<String>>(5).map_err(map_libsql_error)?,
            typed_fields_json: row.get::<Option<String>>(6).map_err(map_libsql_error)?,
            creation_time: row.get::<Option<i64>>(7).map_err(map_libsql_error)?,
            update_time: row.get::<Option<i64>>(8).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn collect_remote_index_version_rows(
    mut rows: libsql::Rows,
) -> Result<Vec<RemoteIndexVersionRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteIndexVersionRow {
            table_id: row.get::<String>(0).map_err(map_libsql_error)?,
            index_id: row.get::<String>(1).map_err(map_libsql_error)?,
            encoded_tuple: row.get::<Vec<u8>>(2).map_err(map_libsql_error)?,
            document_id: row.get::<String>(3).map_err(map_libsql_error)?,
            visible_from: row.get::<i64>(4).map_err(map_libsql_error)?,
            visible_until: row.get::<Option<i64>>(5).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn collect_remote_resource_path_binding_rows(
    mut rows: libsql::Rows,
) -> Result<Vec<RemoteResourcePathBindingRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteResourcePathBindingRow {
            locator_key: row.get::<Vec<u8>>(0).map_err(map_libsql_error)?,
            document_path_key: row.get::<Vec<u8>>(1).map_err(map_libsql_error)?,
            collection_group: row.get::<String>(2).map_err(map_libsql_error)?,
            binding_blob: row.get::<Vec<u8>>(3).map_err(map_libsql_error)?,
            locator_blob: row.get::<Vec<u8>>(4).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn collect_remote_json_rows(mut rows: libsql::Rows) -> Result<Vec<RemoteJsonRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteJsonRow {
            key: row.get::<String>(0).map_err(map_libsql_error)?,
            data_json: row.get::<String>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn collect_remote_scheduled_job_rows(
    mut rows: libsql::Rows,
) -> Result<Vec<RemoteScheduledJobRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteScheduledJobRow {
            id: row.get::<String>(0).map_err(map_libsql_error)?,
            run_at: row.get::<String>(1).map_err(map_libsql_error)?,
            data_json: row.get::<String>(2).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn collect_remote_named_json_rows(mut rows: libsql::Rows) -> Result<Vec<RemoteNamedJsonRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteNamedJsonRow {
            name: row.get::<String>(0).map_err(map_libsql_error)?,
            data_json: row.get::<String>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn collect_remote_execution_ids(mut rows: libsql::Rows) -> Result<Vec<String>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(row.get::<String>(0).map_err(map_libsql_error)?);
    }
    Ok(result)
}

async fn collect_remote_commit_log_rows(mut rows: libsql::Rows) -> Result<Vec<RemoteCommitLogRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        let sequence = row.get::<i64>(0).map_err(map_libsql_error)?;
        result.push(RemoteCommitLogRow {
            sequence: u64::try_from(sequence).map_err(|_| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    format!(
                        "remote libsql durable sequence {sequence} is negative for namespace snapshot"
                    ),
                )
            })?,
            record_blob: row.get::<Vec<u8>>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn collect_remote_metadata_rows(mut rows: libsql::Rows) -> Result<Vec<RemoteMetadataRow>> {
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteMetadataRow {
            key: row.get::<String>(0).map_err(map_libsql_error)?,
            value_blob: row.get::<Vec<u8>>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

pub(super) fn materialize_snapshot_to_replica_cache(
    replica_dir: &Path,
    replica_path: &Path,
    snapshot: RemoteNamespaceSnapshot,
    dek: Option<&[u8; 32]>,
) -> Result<()> {
    std::fs::create_dir_all(replica_dir).map_err(storage_io_error)?;
    let staging_path = staged_replica_path(replica_path);
    remove_sqlite_artifacts(staging_path.as_path())?;

    let conn =
        LocalSqliteConnection::open(staging_path.as_path()).map_err(map_local_sqlite_error)?;
    if let Some(key) = dek {
        crate::sqlite::encryption::apply_encryption_key(&conn, key)?;
        crate::sqlite::encryption::harden_temp_storage(&conn)?;
    }
    initialize_local_replica_cache(&conn)?;
    let write_result = (|| {
        conn.execute_batch("BEGIN IMMEDIATE")
            .map_err(map_local_sqlite_error)?;
        insert_snapshot_rows(&conn, &snapshot)?;
        conn.execute_batch("COMMIT")
            .map_err(map_local_sqlite_error)?;
        Ok(())
    })();
    if let Err(error) = write_result {
        let _ = conn.execute_batch("ROLLBACK");
        return Err(error);
    }
    rebuild_sqlite_indexes_from_loaded_schema(&conn)?;
    drop(conn);

    remove_sqlite_artifacts(replica_path)?;
    std::fs::rename(staging_path.as_path(), replica_path).map_err(storage_io_error)?;
    Ok(())
}

fn initialize_local_replica_cache(conn: &LocalSqliteConnection) -> Result<()> {
    conn.pragma_update(None, "foreign_keys", "ON")
        .map_err(map_local_sqlite_error)?;
    conn.pragma_update(None, "synchronous", "FULL")
        .map_err(map_local_sqlite_error)?;
    conn.execute_batch(SQLITE_INIT_SQL)
        .map_err(map_local_sqlite_error)?;
    Ok(())
}

fn insert_snapshot_rows(
    conn: &LocalSqliteConnection,
    snapshot: &RemoteNamespaceSnapshot,
) -> Result<()> {
    {
        let mut statement = conn
            .prepare(
                "INSERT INTO table_catalog (namespace, table_name, table_id, state)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.table_catalog {
            statement
                .execute(params![
                    row.namespace.as_str(),
                    row.table_name.as_str(),
                    row.table_id.as_str(),
                    row.state.as_str(),
                ])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare("INSERT INTO schemas (table_name, schema_json) VALUES (?1, ?2)")
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.schemas {
            statement
                .execute(params![row.table_name.as_str(), row.schema_json.as_str()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare(
                "INSERT INTO documents (table_id, id, data_json, typed_fields_json, creation_time, update_time)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.documents {
            statement
                .execute(params![
                    row.table_id.as_str(),
                    row.id.as_str(),
                    row.data_json.as_str(),
                    row.typed_fields_json.as_str(),
                    row.creation_time,
                    row.update_time
                ])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare(
                "INSERT INTO document_versions (
                    table_id,
                    id,
                    commit_sequence,
                    commit_time,
                    tombstone,
                    data_json,
                    typed_fields_json,
                    creation_time,
                    update_time
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.document_versions {
            statement
                .execute(params![
                    row.table_id.as_str(),
                    row.id.as_str(),
                    row.commit_sequence,
                    row.commit_time,
                    row.tombstone,
                    row.data_json.as_deref(),
                    row.typed_fields_json.as_deref(),
                    row.creation_time,
                    row.update_time,
                ])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare(
                "INSERT INTO index_versions (
                    table_id,
                    index_id,
                    encoded_tuple,
                    document_id,
                    visible_from,
                    visible_until
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.index_versions {
            statement
                .execute(params![
                    row.table_id.as_str(),
                    row.index_id.as_str(),
                    row.encoded_tuple.as_slice(),
                    row.document_id.as_str(),
                    row.visible_from,
                    row.visible_until,
                ])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare(
                "INSERT INTO resource_path_bindings (
                    locator_key,
                    document_path_key,
                    collection_group,
                    binding_blob,
                    locator_blob
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
            )
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.resource_path_bindings {
            statement
                .execute(params![
                    row.locator_key.as_slice(),
                    row.document_path_key.as_slice(),
                    row.collection_group.as_str(),
                    row.binding_blob.as_slice(),
                    row.locator_blob.as_slice(),
                ])
                .map_err(map_local_sqlite_error)?;
        }
    }
    insert_scheduled_job_rows(conn, &snapshot.scheduled_jobs)?;
    insert_json_rows(
        conn,
        "running_scheduled_jobs",
        "id",
        &snapshot.running_scheduled_jobs,
    )?;
    insert_json_rows(
        conn,
        "scheduled_job_results",
        "job_id",
        &snapshot.scheduled_job_results,
    )?;
    {
        let mut statement = conn
            .prepare("INSERT INTO scheduled_job_executions (execution_id) VALUES (?1)")
            .map_err(map_local_sqlite_error)?;
        for execution_id in &snapshot.scheduled_job_executions {
            statement
                .execute(params![execution_id.as_str()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare("INSERT INTO cron_jobs (name, data_json) VALUES (?1, ?2)")
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.cron_jobs {
            statement
                .execute(params![row.name.as_str(), row.data_json.as_str()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare("INSERT INTO commit_log (sequence, record_blob) VALUES (?1, ?2)")
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.commit_log {
            statement
                .execute(params![row.sequence, row.record_blob.as_slice()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    {
        let mut statement = conn
            .prepare("INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)")
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.metadata {
            statement
                .execute(params![row.key.as_str(), row.value_blob.as_slice()])
                .map_err(map_local_sqlite_error)?;
        }
    }
    Ok(())
}

fn insert_json_rows(
    conn: &LocalSqliteConnection,
    table: &str,
    key_column: &str,
    rows: &[RemoteJsonRow],
) -> Result<()> {
    let sql = format!("INSERT INTO {table} ({key_column}, data_json) VALUES (?1, ?2)");
    let mut statement = conn.prepare(sql.as_str()).map_err(map_local_sqlite_error)?;
    for row in rows {
        statement
            .execute(params![row.key.as_str(), row.data_json.as_str()])
            .map_err(map_local_sqlite_error)?;
    }
    Ok(())
}

fn insert_scheduled_job_rows(
    conn: &LocalSqliteConnection,
    rows: &[RemoteScheduledJobRow],
) -> Result<()> {
    let mut statement = conn
        .prepare("INSERT INTO scheduled_jobs (id, run_at, data_json) VALUES (?1, ?2, ?3)")
        .map_err(map_local_sqlite_error)?;
    for row in rows {
        statement
            .execute(params![
                row.id.as_str(),
                row.run_at.as_str(),
                row.data_json.as_str()
            ])
            .map_err(map_local_sqlite_error)?;
    }
    Ok(())
}

fn staged_replica_path(replica_path: &Path) -> PathBuf {
    let file_name = replica_path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| LIBSQL_REPLICA_FILENAME.to_string());
    replica_path.with_file_name(format!("{file_name}.staging"))
}

pub(super) fn remove_sqlite_artifacts(path: &Path) -> Result<()> {
    remove_file_if_exists(path)?;
    remove_file_if_exists(sqlite_sidecar_path(path, "-wal").as_path())?;
    remove_file_if_exists(sqlite_sidecar_path(path, "-shm").as_path())?;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(storage_io_error(error)),
    }
}

fn sqlite_sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    PathBuf::from(format!("{}{}", path.display(), suffix))
}

pub(super) async fn bootstrap_tenant_namespace(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let primary_url = primary_url.to_owned();
    let auth_token = auth_token.map(str::to_owned);
    let namespace = namespace.to_owned();
    retry_idempotent_remote_operation_without_session(
        "bootstrap libsql tenant namespace",
        move || {
            let primary_url = primary_url.clone();
            let auth_token = auth_token.clone();
            let namespace = namespace.clone();
            async move {
                bootstrap_tenant_namespace_once(
                    primary_url.as_str(),
                    auth_token.as_deref(),
                    namespace.as_str(),
                )
                .await
            }
        },
    )
    .await
}

async fn bootstrap_tenant_namespace_once(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let database = open_remote_database(primary_url, auth_token, namespace).await?;
    let conn = database.connect().map_err(map_libsql_error)?;
    conn.execute_batch(SQLITE_INIT_SQL)
        .await
        .map_err(map_libsql_error)?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS committer_lease (
            singleton INTEGER NOT NULL PRIMARY KEY DEFAULT 1 CHECK (singleton = 1),
            owner_id TEXT NOT NULL,
            epoch INTEGER NOT NULL CHECK (epoch >= 1),
            expires_at INTEGER NOT NULL,
            durable_sequence INTEGER NOT NULL CHECK (durable_sequence >= 0)
        );",
    )
    .await
    .map_err(map_libsql_error)?;
    Ok(())
}

pub(super) async fn clear_tenant_namespace(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let primary_url = primary_url.to_owned();
    let auth_token = auth_token.map(str::to_owned);
    let namespace = namespace.to_owned();
    retry_idempotent_remote_operation_without_session("clear libsql tenant namespace", move || {
        let primary_url = primary_url.clone();
        let auth_token = auth_token.clone();
        let namespace = namespace.clone();
        async move {
            let database = open_remote_database(
                primary_url.as_str(),
                auth_token.as_deref(),
                namespace.as_str(),
            )
            .await?;
            let conn = database.connect().map_err(map_libsql_error)?;
            conn.execute_batch(LIBSQL_DROP_TENANT_SQL)
                .await
                .map_err(map_libsql_error)?;
            Ok(())
        }
    })
    .await
}

pub(super) async fn tenant_namespace_has_foundation(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<bool> {
    let primary_url = primary_url.to_owned();
    let auth_token = auth_token.map(str::to_owned);
    let namespace = namespace.to_owned();
    retry_idempotent_remote_operation_without_session(
        "inspect libsql tenant namespace foundation",
        move || {
            let primary_url = primary_url.clone();
            let auth_token = auth_token.clone();
            let namespace = namespace.clone();
            async move {
                let database = open_remote_database(
                    primary_url.as_str(),
                    auth_token.as_deref(),
                    namespace.as_str(),
                )
                .await?;
                let conn = database.connect().map_err(map_libsql_error)?;
                let rows = conn
                    .query(
                        "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'metadata'",
                        (),
                    )
                    .await
                    .map_err(map_libsql_error)?;
                let exists = take_single_remote_row(rows).await?.is_some();
                conn.execute("SELECT 1", ())
                    .await
                    .map_err(map_libsql_error)?;
                Ok(exists)
            }
        },
    )
    .await
}

pub(super) async fn open_remote_database(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<Database> {
    let builder = Builder::new_remote(
        primary_url.to_string(),
        auth_token.unwrap_or_default().to_string(),
    )
    .namespace(namespace.to_string())
    .connector(libsql_transport_connector()?);
    builder.build().await.map_err(map_libsql_error)
}

pub(super) async fn ensure_remote_namespace_exists(
    admin_api_url: &str,
    admin_auth_header: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let endpoint = namespace_create_endpoint(admin_api_url, namespace);
    let auth_header = admin_auth_header.map(str::to_owned);
    let namespace = namespace.to_owned();
    retry_idempotent_remote_operation_without_session("create libsql namespace", move || {
        let endpoint = endpoint.clone();
        let auth_header = auth_header.clone();
        let namespace = namespace.clone();
        async move {
            let response = apply_admin_auth(
                libsql_admin_http_client()
                    .post(endpoint)
                    .json(&serde_json::json!({})),
                auth_header.as_deref(),
            )
            .send()
            .await
            .map_err(map_admin_api_error)?;
            let status = response.status();
            let body = response.text().await.map_err(map_admin_api_error)?;
            if status.is_success() || (status.as_u16() == 400 && body.contains("already exists")) {
                return Ok(());
            }
            Err(Error::storage(
                StorageErrorKind::Unavailable,
                format!(
                    "libsql admin namespace create failed for '{namespace}': status={status}, body={body}"
                ),
            ))
        }
    })
    .await
}

pub(super) async fn drop_remote_namespace(
    admin_api_url: &str,
    admin_auth_header: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let endpoint = namespace_endpoint(admin_api_url, namespace);
    let auth_header = admin_auth_header.map(str::to_owned);
    let namespace = namespace.to_owned();
    retry_idempotent_remote_operation_without_session("delete libsql namespace", move || {
        let endpoint = endpoint.clone();
        let auth_header = auth_header.clone();
        let namespace = namespace.clone();
        async move {
            let response = apply_admin_auth(
                libsql_admin_http_client().delete(endpoint),
                auth_header.as_deref(),
            )
            .send()
            .await
            .map_err(map_admin_api_error)?;
            let status = response.status();
            let body = response.text().await.map_err(map_admin_api_error)?;
            if status.is_success()
                || (status.as_u16() == 404 && body.contains("doesn't exist"))
                || (status.as_u16() == 500 && body.contains("Directory not empty"))
            {
                return Ok(());
            }
            Err(Error::storage(
                StorageErrorKind::Unavailable,
                format!(
                    "libsql admin namespace delete failed for '{namespace}': status={status}, body={body}"
                ),
            ))
        }
    })
    .await
}

fn libsql_admin_http_client() -> &'static HttpClient {
    LIBSQL_ADMIN_HTTP_CLIENT.get_or_init(HttpClient::new)
}

fn apply_admin_auth(
    request: reqwest::RequestBuilder,
    admin_auth_header: Option<&str>,
) -> reqwest::RequestBuilder {
    match admin_auth_header {
        Some(value) => request.header(AUTHORIZATION, value),
        None => request,
    }
}

fn namespace_create_endpoint(admin_api_url: &str, namespace: &str) -> String {
    format!(
        "{}/v1/namespaces/{namespace}/create",
        admin_api_url.trim_end_matches('/')
    )
}

fn namespace_endpoint(admin_api_url: &str, namespace: &str) -> String {
    format!(
        "{}/v1/namespaces/{namespace}",
        admin_api_url.trim_end_matches('/')
    )
}

pub(super) fn tenant_namespace_name(prefix: &str, tenant_id: &TenantId) -> Result<String> {
    let mut candidate = format!("{prefix}{}", tenant_id.as_str().replace('-', "_"));
    if candidate.len() <= LIBSQL_NAMESPACE_LIMIT {
        validate_namespace_input(&candidate, "tenant namespace")?;
        return Ok(candidate);
    }

    let hash = hex_tenant_hash(tenant_id);
    let separator = if prefix.is_empty() { "" } else { "_" };
    let max_hash_len = TARGET_TENANT_HASH_HEX_LEN.min(hash.len());
    for hash_len in (MIN_TENANT_HASH_HEX_LEN..=max_hash_len).rev() {
        candidate = format!("{prefix}{separator}{}", &hash[..hash_len]);
        if candidate.len() <= LIBSQL_NAMESPACE_LIMIT {
            validate_namespace_input(&candidate, "tenant namespace")?;
            return Ok(candidate);
        }
    }

    Err(Error::InvalidInput(format!(
        "tenant namespace prefix '{prefix}' is too long to derive a libsql namespace"
    )))
}

fn hex_tenant_hash(tenant_id: &TenantId) -> String {
    let mut hasher = Sha256::new();
    hasher.update(tenant_id.as_str().as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) fn validate_namespace_input(value: &str, field: &str) -> Result<()> {
    if value.is_empty() {
        return Err(Error::InvalidInput(format!("{field} cannot be empty")));
    }
    if value.len() > LIBSQL_NAMESPACE_LIMIT {
        return Err(Error::InvalidInput(format!(
            "{field} must be at most {LIBSQL_NAMESPACE_LIMIT} characters"
        )));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(Error::InvalidInput(format!(
            "{field} must contain only ASCII letters, digits, '_' or '-'"
        )));
    }
    Ok(())
}

fn map_admin_api_error(error: reqwest::Error) -> Error {
    let message = format!("libsql admin API request failed: {error}");
    if error.is_connect() || error.is_timeout() {
        Error::storage(StorageErrorKind::Unavailable, message)
    } else {
        Error::storage(StorageErrorKind::Transient, message)
    }
}
