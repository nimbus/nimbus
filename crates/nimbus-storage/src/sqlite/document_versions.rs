use std::collections::BTreeSet;

#[cfg(test)]
use super::config::{
    SqliteWriteStatementConcept, observe_sqlite_current_document_encode,
    observe_sqlite_format_check, observe_sqlite_uncached_statement,
};
use super::*;
use crate::diagnostics::DocumentVersionStorageDiagnostic;
use crate::{
    CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT, DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
    StorageFormatVersion, storage_format_version_from_u64,
    validate_document_version_storage_format, validate_document_version_storage_format_state,
};

impl SqliteTenantStore {
    pub fn get_document_version_at(
        &self,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>> {
        self.read_snapshot()?
            .get_document_version_at(table, table_id, document_id, sequence)
    }

    pub fn document_version_storage_diagnostic(&self) -> Result<DocumentVersionStorageDiagnostic> {
        self.read_snapshot()?.document_version_storage_diagnostic()
    }
}

impl SqliteReadSnapshot {
    pub fn get_document_version_at(
        &self,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>> {
        get_document_version_at_in_conn(&self.conn, table, table_id, document_id, sequence)
    }

    pub fn document_version_storage_diagnostic(&self) -> Result<DocumentVersionStorageDiagnostic> {
        document_version_storage_diagnostic_in_conn(&self.conn)
    }
}

pub(super) fn record_document_versions_for_events_in_conn(
    conn: &Connection,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    events: &[TenantEventKind],
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_document_versions_for_writes_in_conn(
                conn,
                sequence,
                timestamp,
                writes,
                #[cfg(test)]
                observation_path,
            )?;
        }
    }
    Ok(())
}

pub(super) fn record_document_versions_for_writes_in_conn(
    conn: &Connection,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    writes: &[WriteOp],
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    if writes.is_empty() {
        return Ok(());
    }

    #[cfg(test)]
    observe_sqlite_format_check(observation_path);
    ensure_document_version_storage_format_in_conn(
        conn,
        #[cfg(test)]
        observation_path,
    )?;
    for write in writes {
        match &write.current {
            Some(current) => {
                #[cfg(test)]
                {
                    observe_sqlite_current_document_encode(observation_path);
                    observe_sqlite_uncached_statement(
                        observation_path,
                        SqliteWriteStatementConcept::DocumentVersionInsert,
                    );
                }
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
                    params![
                        write.table_id.as_str(),
                        write.doc_id.to_string(),
                        sequence.0,
                        timestamp.0,
                        serialize_document_fields(current)?,
                        serialize_document_typed_fields(current)?,
                        current.creation_time.0,
                        current.update_time.0,
                    ],
                )
                .map_err(map_sqlite_error)?;
            }
            None => {
                #[cfg(test)]
                observe_sqlite_uncached_statement(
                    observation_path,
                    SqliteWriteStatementConcept::DocumentVersionInsert,
                );
                conn.execute(
                    "INSERT INTO document_versions (
                        table_id,
                        id,
                        commit_sequence,
                        commit_time,
                        tombstone
                     ) VALUES (?1, ?2, ?3, ?4, 1)",
                    params![
                        write.table_id.as_str(),
                        write.doc_id.to_string(),
                        sequence.0,
                        timestamp.0,
                    ],
                )
                .map_err(map_sqlite_error)?;
            }
        }
    }
    Ok(())
}

pub(super) fn prune_document_versions_before_in_conn(
    conn: &Connection,
    prune_before: SequenceNumber,
) -> Result<u64> {
    if prune_before.0 == 0 {
        return Ok(0);
    }
    validate_document_version_storage_format_in_conn(conn)?;
    let prune_before_sql = i64_from_sequence(prune_before)?;
    let mut anchor_stmt = conn
        .prepare(
            "SELECT table_id, id, MAX(commit_sequence)
             FROM document_versions
             WHERE commit_sequence <= ?1
             GROUP BY table_id, id",
        )
        .map_err(map_sqlite_error)?;
    let anchors = anchor_stmt
        .query_map(params![prune_before_sql], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(map_sqlite_error)?
        .collect::<std::result::Result<BTreeSet<_>, _>>()
        .map_err(map_sqlite_error)?;

    let mut candidate_stmt = conn
        .prepare(
            "SELECT table_id, id, commit_sequence
             FROM document_versions
             WHERE commit_sequence < ?1
             ORDER BY table_id, id, commit_sequence",
        )
        .map_err(map_sqlite_error)?;
    let candidates = candidate_stmt
        .query_map(params![prune_before_sql], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(map_sqlite_error)?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(map_sqlite_error)?;

    let mut pruned = 0_u64;
    for (table_id, document_id, commit_sequence) in candidates {
        if anchors.contains(&(table_id.clone(), document_id.clone(), commit_sequence)) {
            continue;
        }
        let deleted = conn
            .execute(
                "DELETE FROM document_versions
                 WHERE table_id = ?1 AND id = ?2 AND commit_sequence = ?3",
                params![table_id, document_id, commit_sequence],
            )
            .map_err(map_sqlite_error)?;
        pruned = pruned.saturating_add(u64::try_from(deleted).map_err(|_| {
            Error::storage(
                StorageErrorKind::Corruption,
                "SQLite document-version prune count exceeds u64",
            )
        })?);
    }
    Ok(pruned)
}

fn get_document_version_at_in_conn(
    conn: &Connection,
    table: &TableName,
    table_id: &TableId,
    document_id: &DocumentId,
    sequence: SequenceNumber,
) -> Result<Option<Document>> {
    validate_document_version_storage_format_in_conn(conn)?;
    let Some(row) = conn
        .query_row(
            "SELECT tombstone, creation_time, update_time, data_json, typed_fields_json
             FROM document_versions
             WHERE table_id = ?1 AND id = ?2 AND commit_sequence <= ?3
             ORDER BY commit_sequence DESC
             LIMIT 1",
            params![table_id.as_str(), document_id.to_string(), sequence.0],
            |row| {
                Ok(DocumentVersionRow {
                    tombstone: row.get::<_, i64>(0)?,
                    creation_time: row.get::<_, Option<u64>>(1)?,
                    update_time: row.get::<_, Option<u64>>(2)?,
                    data_json: row.get::<_, Option<String>>(3)?,
                    typed_fields_json: row.get::<_, Option<String>>(4)?,
                })
            },
        )
        .optional()
        .map_err(map_sqlite_error)?
    else {
        return Ok(None);
    };

    if row.tombstone == 1 {
        return Ok(None);
    }
    if row.tombstone != 0 {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "document version tombstone marker is invalid: {}",
                row.tombstone
            ),
        ));
    }

    Ok(Some(row_to_document(
        table,
        document_id,
        row.creation_time.ok_or_else(missing_live_version_field)?,
        row.update_time.ok_or_else(missing_live_version_field)?,
        row.data_json.ok_or_else(missing_live_version_field)?,
        row.typed_fields_json
            .ok_or_else(missing_live_version_field)?,
    )?))
}

fn document_version_storage_diagnostic_in_conn(
    conn: &Connection,
) -> Result<DocumentVersionStorageDiagnostic> {
    let format_version = load_document_version_storage_format_in_conn(conn)?;
    let (version_count, min_sequence, max_sequence) = conn
        .query_row(
            "SELECT COUNT(*), MIN(commit_sequence), MAX(commit_sequence) FROM document_versions",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<i64>>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .map_err(map_sqlite_error)?;
    let version_count = u64::try_from(version_count).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "SQLite document version count is negative",
        )
    })?;
    let min_sequence = min_sequence.map(sqlite_i64_to_sequence).transpose()?;
    let max_sequence = max_sequence.map(sqlite_i64_to_sequence).transpose()?;
    validate_document_version_storage_format_state(format_version, version_count > 0)?;

    Ok(DocumentVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence,
        max_sequence,
    })
}

fn validate_document_version_storage_format_in_conn(conn: &Connection) -> Result<()> {
    let format_version = load_document_version_storage_format_in_conn(conn)?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_document_version_storage_format(format_version)?;
            false
        }
        None => document_versions_have_rows_in_conn(conn)?,
    };
    validate_document_version_storage_format_state(format_version, has_versions)
}

fn ensure_document_version_storage_format_in_conn(
    conn: &Connection,
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    #[cfg(test)]
    observe_sqlite_uncached_statement(
        observation_path,
        SqliteWriteStatementConcept::DocumentVersionFormatRead,
    );
    if let Some(format_version) = load_document_version_storage_format_in_conn(conn)? {
        validate_document_version_storage_format(format_version)?;
        return Ok(());
    }

    #[cfg(test)]
    observe_sqlite_uncached_statement(
        observation_path,
        SqliteWriteStatementConcept::DocumentVersionFormatWrite,
    );
    conn.execute(
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
        params![
            DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
            encode_u64(CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT.0.into()).as_slice()
        ],
    )
    .map_err(map_sqlite_error)?;
    Ok(())
}

fn load_document_version_storage_format_in_conn(
    conn: &Connection,
) -> Result<Option<StorageFormatVersion>> {
    conn.query_row(
        "SELECT value_blob FROM metadata WHERE key = ?1",
        params![DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY],
        |row| row.get::<_, Vec<u8>>(0),
    )
    .optional()
    .map_err(map_sqlite_error)?
    .map(|bytes| storage_format_version_from_u64(decode_u64(bytes.as_slice())?))
    .transpose()
}

fn document_versions_have_rows_in_conn(conn: &Connection) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM document_versions LIMIT 1)",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
    .map_err(map_sqlite_error)
}

fn sqlite_i64_to_sequence(value: i64) -> Result<SequenceNumber> {
    Ok(SequenceNumber(u64::try_from(value).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("SQLite document version sequence is negative: {value}"),
        )
    })?))
}

fn i64_from_sequence(sequence: SequenceNumber) -> Result<i64> {
    i64::try_from(sequence.0).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "sequence {} exceeds SQLite document-version integer range",
                sequence.0
            ),
        )
    })
}

struct DocumentVersionRow {
    tombstone: i64,
    creation_time: Option<u64>,
    update_time: Option<u64>,
    data_json: Option<String>,
    typed_fields_json: Option<String>,
}

fn missing_live_version_field() -> Error {
    Error::storage(
        StorageErrorKind::Corruption,
        "live document version row is missing payload fields",
    )
}
