use std::collections::BTreeSet;

use super::*;
use crate::diagnostics::DocumentVersionStorageDiagnostic;
use crate::{
    CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT, DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
    StorageFormatVersion, storage_format_version_from_u64,
    validate_document_version_storage_format, validate_document_version_storage_format_state,
};

impl LibsqlReplicaTenantStore {
    pub fn get_document_version_at(
        &self,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>> {
        let conn = self.remote_connection()?;
        self.block_on(get_document_version_at_from_remote_conn(
            &conn,
            table,
            table_id,
            document_id,
            sequence,
        ))
    }

    pub fn document_version_storage_diagnostic(&self) -> Result<DocumentVersionStorageDiagnostic> {
        let conn = self.remote_connection()?;
        self.block_on(document_version_storage_diagnostic_from_remote_conn(&conn))
    }
}

pub(super) async fn record_document_versions_for_events_remote(
    conn: &Connection,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    events: &[TenantEventKind],
) -> Result<()> {
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_document_versions_for_writes_remote(conn, sequence, timestamp, writes).await?;
        }
    }
    Ok(())
}

pub(super) async fn record_document_versions_for_writes_remote(
    conn: &Connection,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    writes: &[WriteOp],
) -> Result<()> {
    if writes.is_empty() {
        return Ok(());
    }

    ensure_document_version_storage_format_remote(conn).await?;
    let sequence = i64_from_u64(sequence.0)?;
    let timestamp = i64_from_u64(timestamp.0)?;
    for write in writes {
        match &write.current {
            Some(current) => {
                conn.execute(
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
                     ) VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, ?8)",
                    libsql::params![
                        write.table_id.as_str(),
                        write.doc_id.to_string(),
                        sequence,
                        timestamp,
                        serialize_document_fields(current)?,
                        serialize_document_typed_fields(current)?,
                        i64_from_u64(current.creation_time.0)?,
                        i64_from_u64(current.update_time.0)?,
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            }
            None => {
                conn.execute(
                    "INSERT INTO document_versions (
                        table_id,
                        id,
                        commit_sequence,
                        commit_time,
                        tombstone
                     ) VALUES (?1, ?2, ?3, ?4, 1)",
                    libsql::params![
                        write.table_id.as_str(),
                        write.doc_id.to_string(),
                        sequence,
                        timestamp,
                    ],
                )
                .await
                .map_err(map_libsql_error)?;
            }
        }
    }
    Ok(())
}

pub(super) async fn prune_document_versions_before_remote(
    conn: &Connection,
    prune_before: SequenceNumber,
) -> Result<u64> {
    if prune_before.0 == 0 {
        return Ok(0);
    }
    validate_document_version_storage_format_remote(conn).await?;
    let prune_before_sql = i64_from_u64(prune_before.0)?;
    let mut anchor_rows = conn
        .query(
            "SELECT table_id, id, MAX(commit_sequence)
             FROM document_versions
             WHERE commit_sequence <= ?1
             GROUP BY table_id, id",
            libsql::params![prune_before_sql],
        )
        .await
        .map_err(map_libsql_error)?;
    let mut anchors = BTreeSet::new();
    while let Some(row) = anchor_rows.next().await.map_err(map_libsql_error)? {
        anchors.insert((
            row.get::<String>(0).map_err(map_libsql_error)?,
            row.get::<String>(1).map_err(map_libsql_error)?,
            row.get::<i64>(2).map_err(map_libsql_error)?,
        ));
    }

    let mut candidate_rows = conn
        .query(
            "SELECT table_id, id, commit_sequence
             FROM document_versions
             WHERE commit_sequence < ?1
             ORDER BY table_id, id, commit_sequence",
            libsql::params![prune_before_sql],
        )
        .await
        .map_err(map_libsql_error)?;
    let mut candidates = Vec::new();
    while let Some(row) = candidate_rows.next().await.map_err(map_libsql_error)? {
        candidates.push((
            row.get::<String>(0).map_err(map_libsql_error)?,
            row.get::<String>(1).map_err(map_libsql_error)?,
            row.get::<i64>(2).map_err(map_libsql_error)?,
        ));
    }

    let mut pruned = 0_u64;
    for (table_id, document_id, commit_sequence) in candidates {
        if anchors.contains(&(table_id.clone(), document_id.clone(), commit_sequence)) {
            continue;
        }
        conn.execute(
            "DELETE FROM document_versions
             WHERE table_id = ?1 AND id = ?2 AND commit_sequence = ?3",
            libsql::params![table_id, document_id, commit_sequence],
        )
        .await
        .map_err(map_libsql_error)?;
        pruned = pruned.saturating_add(1);
    }
    Ok(pruned)
}

async fn get_document_version_at_from_remote_conn(
    conn: &Connection,
    table: &TableName,
    table_id: &TableId,
    document_id: &DocumentId,
    sequence: SequenceNumber,
) -> Result<Option<Document>> {
    validate_document_version_storage_format_remote(conn).await?;
    let rows = conn
        .query(
            "SELECT tombstone, creation_time, update_time, data_json, typed_fields_json
             FROM document_versions
             WHERE table_id = ?1 AND id = ?2 AND commit_sequence <= ?3
             ORDER BY commit_sequence DESC
             LIMIT 1",
            libsql::params![
                table_id.as_str(),
                document_id.to_string(),
                i64_from_u64(sequence.0)?
            ],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = take_single_remote_row(rows).await? else {
        return Ok(None);
    };
    let tombstone = row.get::<i64>(0).map_err(map_libsql_error)?;
    if tombstone == 1 {
        return Ok(None);
    }
    if tombstone != 0 {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!("document version tombstone marker is invalid: {tombstone}"),
        ));
    }

    let creation_time = row
        .get::<Option<i64>>(1)
        .map_err(map_libsql_error)?
        .ok_or_else(missing_live_version_field)?;
    let update_time = row
        .get::<Option<i64>>(2)
        .map_err(map_libsql_error)?
        .ok_or_else(missing_live_version_field)?;
    let data_json = row
        .get::<Option<String>>(3)
        .map_err(map_libsql_error)?
        .ok_or_else(missing_live_version_field)?;
    let typed_fields_json = row
        .get::<Option<String>>(4)
        .map_err(map_libsql_error)?
        .ok_or_else(missing_live_version_field)?;

    Ok(Some(row_to_document(
        table,
        document_id,
        creation_time,
        update_time,
        data_json.as_str(),
        typed_fields_json.as_str(),
    )?))
}

async fn document_version_storage_diagnostic_from_remote_conn(
    conn: &Connection,
) -> Result<DocumentVersionStorageDiagnostic> {
    let format_version = load_document_version_storage_format_remote(conn).await?;
    let rows = conn
        .query(
            "SELECT COUNT(*), MIN(commit_sequence), MAX(commit_sequence) FROM document_versions",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    let row = take_single_remote_row(rows).await?.ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            "libSQL document version aggregate query returned no row",
        )
    })?;
    let version_count = u64_from_i64(row.get::<i64>(0).map_err(map_libsql_error)?)?;
    let min_sequence = row
        .get::<Option<i64>>(1)
        .map_err(map_libsql_error)?
        .map(u64_from_i64)
        .transpose()?
        .map(SequenceNumber);
    let max_sequence = row
        .get::<Option<i64>>(2)
        .map_err(map_libsql_error)?
        .map(u64_from_i64)
        .transpose()?
        .map(SequenceNumber);
    validate_document_version_storage_format_state(format_version, version_count > 0)?;

    Ok(DocumentVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence,
        max_sequence,
    })
}

async fn validate_document_version_storage_format_remote(conn: &Connection) -> Result<()> {
    let format_version = load_document_version_storage_format_remote(conn).await?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_document_version_storage_format(format_version)?;
            false
        }
        None => document_versions_have_rows_remote(conn).await?,
    };
    validate_document_version_storage_format_state(format_version, has_versions)
}

async fn ensure_document_version_storage_format_remote(conn: &Connection) -> Result<()> {
    if let Some(format_version) = load_document_version_storage_format_remote(conn).await? {
        validate_document_version_storage_format(format_version)?;
        return Ok(());
    }

    put_remote_metadata_u64(
        conn,
        DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
        u64::from(CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT.0),
    )
    .await
}

async fn load_document_version_storage_format_remote(
    conn: &Connection,
) -> Result<Option<StorageFormatVersion>> {
    load_remote_metadata_u64(conn, DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY)
        .await?
        .map(storage_format_version_from_u64)
        .transpose()
}

async fn document_versions_have_rows_remote(conn: &Connection) -> Result<bool> {
    let rows = conn
        .query("SELECT EXISTS(SELECT 1 FROM document_versions LIMIT 1)", ())
        .await
        .map_err(map_libsql_error)?;
    let row = take_single_remote_row(rows).await?.ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            "libSQL document version existence query returned no row",
        )
    })?;
    Ok(row.get::<i64>(0).map_err(map_libsql_error)? != 0)
}

fn u64_from_i64(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("libSQL document version sequence is negative: {value}"),
        )
    })
}

fn missing_live_version_field() -> Error {
    Error::storage(
        StorageErrorKind::Corruption,
        "live libSQL document version row is missing payload fields",
    )
}
