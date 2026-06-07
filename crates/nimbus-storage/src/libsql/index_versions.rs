use super::*;
use crate::diagnostics::IndexVersionStorageDiagnostic;
use crate::index::encoded_index_tuple_for_document;
use crate::{
    CURRENT_INDEX_VERSION_STORAGE_FORMAT, INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
    StorageFormatVersion, storage_format_version_from_u64, validate_index_version_storage_format,
};

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexVersionInterval {
    pub document_id: DocumentId,
    pub visible_from: SequenceNumber,
    pub visible_until: Option<SequenceNumber>,
}

struct IndexVersionMutation {
    table_id: String,
    index_id: String,
    document_id: String,
    close_tuple: Option<Vec<u8>>,
    open_tuple: Option<Vec<u8>>,
}

impl LibsqlReplicaTenantStore {
    pub fn index_version_storage_diagnostic(&self) -> Result<IndexVersionStorageDiagnostic> {
        let conn = self.remote_connection()?;
        self.block_on(index_version_storage_diagnostic_remote(&conn))
    }

    #[cfg(test)]
    pub(crate) fn index_version_intervals_for_testing(
        &self,
        table_id: &TableId,
        index_id: &nimbus_core::IndexId,
    ) -> Result<Vec<IndexVersionInterval>> {
        let conn = self.remote_connection()?;
        self.block_on(index_version_intervals_from_remote_conn(
            &conn, table_id, index_id,
        ))
    }
}

pub(super) async fn record_index_versions_for_events_remote(
    conn: &Connection,
    sequence: SequenceNumber,
    events: &[TenantEventKind],
) -> Result<()> {
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_index_versions_for_writes_remote(conn, sequence, writes).await?;
        }
    }
    Ok(())
}

pub(super) async fn record_index_versions_for_writes_remote(
    conn: &Connection,
    sequence: SequenceNumber,
    writes: &[WriteOp],
) -> Result<()> {
    if writes.is_empty() {
        return Ok(());
    }

    let mutations = index_version_mutations_for_writes(conn, writes).await?;
    if mutations.is_empty() {
        return Ok(());
    }

    ensure_index_version_storage_format_remote(conn).await?;
    let sequence = i64_from_u64(sequence.0)?;
    for mutation in mutations {
        if let Some(close_tuple) = mutation.close_tuple {
            conn.execute(
                "UPDATE index_versions
                 SET visible_until = ?5
                 WHERE table_id = ?1
                   AND index_id = ?2
                   AND encoded_tuple = ?3
                   AND document_id = ?4
                   AND visible_until IS NULL",
                libsql::params![
                    mutation.table_id.as_str(),
                    mutation.index_id.as_str(),
                    close_tuple,
                    mutation.document_id.as_str(),
                    sequence,
                ],
            )
            .await
            .map_err(map_libsql_error)?;
        }
        if let Some(open_tuple) = mutation.open_tuple {
            conn.execute(
                "INSERT INTO index_versions (
                    table_id,
                    index_id,
                    encoded_tuple,
                    document_id,
                    visible_from,
                    visible_until
                 ) VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                libsql::params![
                    mutation.table_id.as_str(),
                    mutation.index_id.as_str(),
                    open_tuple,
                    mutation.document_id.as_str(),
                    sequence,
                ],
            )
            .await
            .map_err(map_libsql_error)?;
        }
    }
    Ok(())
}

pub(super) async fn prune_index_versions_before_remote(
    conn: &Connection,
    prune_before: SequenceNumber,
) -> Result<u64> {
    if prune_before.0 == 0 {
        return Ok(0);
    }
    validate_index_version_storage_format_remote(conn).await?;
    let deleted = conn
        .execute(
            "DELETE FROM index_versions
             WHERE visible_until IS NOT NULL AND visible_until <= ?1",
            libsql::params![i64_from_u64(prune_before.0)?],
        )
        .await
        .map_err(map_libsql_error)?;
    Ok(deleted)
}

async fn index_version_mutations_for_writes(
    conn: &Connection,
    writes: &[WriteOp],
) -> Result<Vec<IndexVersionMutation>> {
    let mut mutations = Vec::new();
    for write in writes {
        let Some(table_schema) = load_remote_table_schema_from_conn(conn, &write.table).await?
        else {
            continue;
        };
        for index in table_schema.maintained_indexes() {
            let close_tuple = write
                .previous
                .as_ref()
                .map(|previous| encoded_index_tuple_for_document(previous, index))
                .transpose()?
                .flatten();
            let open_tuple = write
                .current
                .as_ref()
                .map(|current| encoded_index_tuple_for_document(current, index))
                .transpose()?
                .flatten();
            if close_tuple.is_some() || open_tuple.is_some() {
                mutations.push(IndexVersionMutation {
                    table_id: write.table_id.as_str().to_string(),
                    index_id: index.id.as_str().to_string(),
                    document_id: write.doc_id.to_string(),
                    close_tuple,
                    open_tuple,
                });
            }
        }
    }
    Ok(mutations)
}

#[cfg(test)]
async fn index_version_intervals_from_remote_conn(
    conn: &Connection,
    table_id: &TableId,
    index_id: &nimbus_core::IndexId,
) -> Result<Vec<IndexVersionInterval>> {
    validate_index_version_storage_format_remote(conn).await?;
    let mut rows = conn
        .query(
            "SELECT document_id, visible_from, visible_until
             FROM index_versions
             WHERE table_id = ?1 AND index_id = ?2
             ORDER BY encoded_tuple, document_id, visible_from",
            libsql::params![table_id.as_str(), index_id.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    let mut intervals = Vec::new();
    while let Some(row) = rows.next().await.map_err(map_libsql_error)? {
        let visible_from = row.get::<i64>(1).map_err(map_libsql_error)?;
        let visible_until = row.get::<Option<i64>>(2).map_err(map_libsql_error)?;
        intervals.push(IndexVersionInterval {
            document_id: DocumentId::from_key(row.get::<String>(0).map_err(map_libsql_error)?)?,
            visible_from: SequenceNumber(u64_from_i64(visible_from)?),
            visible_until: visible_until
                .map(u64_from_i64)
                .transpose()?
                .map(SequenceNumber),
        });
    }
    Ok(intervals)
}

async fn validate_index_version_storage_format_remote(conn: &Connection) -> Result<()> {
    let format_version = load_index_version_storage_format_remote(conn).await?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_index_version_storage_format(format_version)?;
            false
        }
        None => index_versions_have_rows_remote(conn).await?,
    };
    crate::validate_index_version_storage_format_state(format_version, has_versions)
}

async fn index_version_storage_diagnostic_remote(
    conn: &Connection,
) -> Result<IndexVersionStorageDiagnostic> {
    let format_version = load_index_version_storage_format_remote(conn).await?;
    let mut rows = conn
        .query(
            "SELECT COUNT(*), MIN(visible_from), MAX(MAX(visible_from, COALESCE(visible_until, visible_from))) FROM index_versions",
            (),
        )
        .await
        .map_err(map_libsql_error)?;
    let row = rows
        .next()
        .await
        .map_err(map_libsql_error)?
        .ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "libSQL index version aggregate query returned no row",
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
    crate::validate_index_version_storage_format_state(format_version, version_count > 0)?;

    Ok(IndexVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence,
        max_sequence,
    })
}

async fn ensure_index_version_storage_format_remote(conn: &Connection) -> Result<()> {
    if let Some(format_version) = load_index_version_storage_format_remote(conn).await? {
        validate_index_version_storage_format(format_version)?;
        return Ok(());
    }

    put_remote_metadata_u64(
        conn,
        INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
        u64::from(CURRENT_INDEX_VERSION_STORAGE_FORMAT.0),
    )
    .await
}

async fn load_index_version_storage_format_remote(
    conn: &Connection,
) -> Result<Option<StorageFormatVersion>> {
    load_remote_metadata_u64(conn, INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY)
        .await?
        .map(storage_format_version_from_u64)
        .transpose()
}

async fn index_versions_have_rows_remote(conn: &Connection) -> Result<bool> {
    let mut rows = conn
        .query("SELECT EXISTS(SELECT 1 FROM index_versions LIMIT 1)", ())
        .await
        .map_err(map_libsql_error)?;
    let row = rows
        .next()
        .await
        .map_err(map_libsql_error)?
        .ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "libSQL index version existence query returned no row",
            )
        })?;
    Ok(row.get::<i64>(0).map_err(map_libsql_error)? != 0)
}

async fn load_remote_table_schema_from_conn(
    conn: &Connection,
    table: &TableName,
) -> Result<Option<TableSchema>> {
    let mut rows = conn
        .query(
            "SELECT schema_json FROM schemas WHERE table_name = ?1",
            libsql::params![table.as_str()],
        )
        .await
        .map_err(map_libsql_error)?;
    let Some(row) = rows.next().await.map_err(map_libsql_error)? else {
        return Ok(None);
    };
    deserialize_json(row.get::<String>(0).map_err(map_libsql_error)?.as_str()).map(Some)
}

fn u64_from_i64(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("libSQL index version sequence is negative: {value}"),
        )
    })
}
