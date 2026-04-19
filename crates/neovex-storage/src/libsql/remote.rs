use super::*;

#[derive(Debug, Clone)]
pub(super) struct RemoteNamespaceSnapshot {
    pub(super) schemas: Vec<RemoteSchemaRow>,
    pub(super) documents: Vec<RemoteDocumentRow>,
    pub(super) scheduled_jobs: Vec<RemoteJsonRow>,
    pub(super) running_scheduled_jobs: Vec<RemoteJsonRow>,
    pub(super) scheduled_job_results: Vec<RemoteJsonRow>,
    pub(super) scheduled_job_executions: Vec<String>,
    pub(super) cron_jobs: Vec<RemoteNamedJsonRow>,
    pub(super) commit_log: Vec<RemoteCommitLogRow>,
    pub(super) metadata: Vec<RemoteMetadataRow>,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteSchemaRow {
    pub(super) table_name: String,
    pub(super) schema_json: String,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteDocumentRow {
    table_name: String,
    id: String,
    creation_time: u64,
    data_json: String,
}

#[derive(Debug, Clone)]
pub(super) struct RemoteJsonRow {
    key: String,
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
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<RemoteNamespaceSnapshot> {
    let database = open_remote_database(primary_url, auth_token, namespace).await?;
    let conn = database.connect().map_err(map_libsql_error)?;
    conn.execute_batch("BEGIN")
        .await
        .map_err(map_libsql_error)?;
    let snapshot = async {
        Ok(RemoteNamespaceSnapshot {
            schemas: query_remote_schema_rows(&conn).await?,
            documents: query_remote_document_rows(&conn).await?,
            scheduled_jobs: query_remote_json_rows(&conn, "scheduled_jobs", "id").await?,
            running_scheduled_jobs: query_remote_json_rows(&conn, "running_scheduled_jobs", "id")
                .await?,
            scheduled_job_results: query_remote_json_rows(&conn, "scheduled_job_results", "job_id")
                .await?,
            scheduled_job_executions: query_remote_execution_ids(&conn).await?,
            cron_jobs: query_remote_named_json_rows(&conn, "cron_jobs", "name").await?,
            commit_log: query_remote_commit_log_rows(&conn).await?,
            metadata: query_remote_metadata_rows(&conn).await?,
        })
    }
    .await;
    let _ = conn.execute_batch("ROLLBACK").await;
    snapshot
}

pub(super) async fn query_remote_schema_rows(conn: &Connection) -> Result<Vec<RemoteSchemaRow>> {
    let mut rows = conn
        .query(
            "SELECT table_name, schema_json FROM schemas ORDER BY table_name",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteSchemaRow {
            table_name: row.get::<String>(0).map_err(map_libsql_error)?,
            schema_json: row.get::<String>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn query_remote_document_rows(conn: &Connection) -> Result<Vec<RemoteDocumentRow>> {
    let mut rows = conn
        .query(
            "SELECT table_name, id, creation_time, data_json
             FROM documents
             ORDER BY table_name, id",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        let creation_time = row.get::<i64>(2).map_err(map_libsql_error)?;
        result.push(RemoteDocumentRow {
            table_name: row.get::<String>(0).map_err(map_libsql_error)?,
            id: row.get::<String>(1).map_err(map_libsql_error)?,
            creation_time: u64::try_from(creation_time).map_err(|_| {
                Error::storage(
                    StorageErrorKind::Corruption,
                    format!(
                        "remote libsql creation_time {creation_time} is negative for namespace snapshot"
                    ),
                )
            })?,
            data_json: row.get::<String>(3).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn query_remote_json_rows(
    conn: &Connection,
    table: &str,
    key_column: &str,
) -> Result<Vec<RemoteJsonRow>> {
    let sql = format!("SELECT {key_column}, data_json FROM {table} ORDER BY {key_column}");
    let mut rows = conn
        .query(sql.as_str(), ())
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteJsonRow {
            key: row.get::<String>(0).map_err(map_libsql_error)?,
            data_json: row.get::<String>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn query_remote_named_json_rows(
    conn: &Connection,
    table: &str,
    name_column: &str,
) -> Result<Vec<RemoteNamedJsonRow>> {
    let sql = format!("SELECT {name_column}, data_json FROM {table} ORDER BY {name_column}");
    let mut rows = conn
        .query(sql.as_str(), ())
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(RemoteNamedJsonRow {
            name: row.get::<String>(0).map_err(map_libsql_error)?,
            data_json: row.get::<String>(1).map_err(map_libsql_error)?,
        });
    }
    Ok(result)
}

async fn query_remote_execution_ids(conn: &Connection) -> Result<Vec<String>> {
    let mut rows = conn
        .query(
            "SELECT execution_id FROM scheduled_job_executions ORDER BY execution_id",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    let mut result = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        result.push(row.get::<String>(0).map_err(map_libsql_error)?);
    }
    Ok(result)
}

async fn query_remote_commit_log_rows(conn: &Connection) -> Result<Vec<RemoteCommitLogRow>> {
    let mut rows = conn
        .query(
            "SELECT sequence, record_blob FROM commit_log ORDER BY sequence",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
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

async fn query_remote_metadata_rows(conn: &Connection) -> Result<Vec<RemoteMetadataRow>> {
    let mut rows = conn
        .query("SELECT key, value_blob FROM metadata ORDER BY key", ())
        .await
        .map_err(map_libsql_error)?;
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
) -> Result<()> {
    std::fs::create_dir_all(replica_dir).map_err(storage_io_error)?;
    let staging_path = staged_replica_path(replica_path);
    remove_sqlite_artifacts(staging_path.as_path())?;

    let conn =
        LocalSqliteConnection::open(staging_path.as_path()).map_err(map_local_sqlite_error)?;
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
                "INSERT INTO documents (table_name, id, data_json, creation_time)
                 VALUES (?1, ?2, ?3, ?4)",
            )
            .map_err(map_local_sqlite_error)?;
        for row in &snapshot.documents {
            statement
                .execute(params![
                    row.table_name.as_str(),
                    row.id.as_str(),
                    row.data_json.as_str(),
                    row.creation_time
                ])
                .map_err(map_local_sqlite_error)?;
        }
    }
    insert_json_rows(conn, "scheduled_jobs", "id", &snapshot.scheduled_jobs)?;
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
    let database = open_remote_database(primary_url, auth_token, namespace).await?;
    let conn = database.connect().map_err(map_libsql_error)?;
    conn.execute_batch(SQLITE_INIT_SQL)
        .await
        .map_err(map_libsql_error)?;
    Ok(())
}

pub(super) async fn clear_tenant_namespace(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let database = open_remote_database(primary_url, auth_token, namespace).await?;
    let conn = database.connect().map_err(map_libsql_error)?;
    conn.execute_batch(LIBSQL_DROP_TENANT_SQL)
        .await
        .map_err(map_libsql_error)?;
    Ok(())
}

pub(super) async fn tenant_namespace_has_foundation(
    primary_url: &str,
    auth_token: Option<&str>,
    namespace: &str,
) -> Result<bool> {
    let database = open_remote_database(primary_url, auth_token, namespace).await?;
    let conn = database.connect().map_err(map_libsql_error)?;
    let mut rows = conn
        .query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'metadata'",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(rows.next().await.map_err(map_libsql_error)?.is_some())
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
    let response = apply_admin_auth(
        HttpClient::new()
            .post(namespace_create_endpoint(admin_api_url, namespace))
            .json(&serde_json::json!({})),
        admin_auth_header,
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

pub(super) async fn drop_remote_namespace(
    admin_api_url: &str,
    admin_auth_header: Option<&str>,
    namespace: &str,
) -> Result<()> {
    let response = apply_admin_auth(
        HttpClient::new().delete(namespace_endpoint(admin_api_url, namespace)),
        admin_auth_header,
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
