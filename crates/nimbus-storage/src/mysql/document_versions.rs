use std::collections::BTreeSet;

use super::*;
use crate::diagnostics::DocumentVersionStorageDiagnostic;
use crate::{
    CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT, DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
    StorageFormatVersion, storage_format_version_from_u64,
    validate_document_version_storage_format, validate_document_version_storage_format_state,
};

impl MySqlTenantStore {
    pub fn get_document_version_at(
        &self,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        let table = table.clone();
        let table_id = table_id.clone();
        let document_id = document_id.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            get_document_version_at_from_session(
                &mut conn,
                &database_name,
                &table,
                &table_id,
                &document_id,
                sequence,
            )
            .await
        })
    }

    pub fn document_version_storage_diagnostic(&self) -> Result<DocumentVersionStorageDiagnostic> {
        let provider = self.provider.clone();
        let database_name = self.database_name.clone();
        self.block_on(async move {
            let mut conn = provider.conn().await?;
            document_version_storage_diagnostic_from_session(&mut conn, &database_name).await
        })
    }
}

pub(super) async fn record_document_versions_for_events_in_session<C>(
    session: &mut C,
    database_name: &str,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    events: &[TenantEventKind],
) -> Result<()>
where
    C: Queryable,
{
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_document_versions_for_writes_in_session(
                session,
                database_name,
                sequence,
                timestamp,
                writes,
            )
            .await?;
        }
    }
    Ok(())
}

pub(super) async fn record_document_versions_for_writes_in_session<C>(
    session: &mut C,
    database_name: &str,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    writes: &[WriteOp],
) -> Result<()>
where
    C: Queryable,
{
    if writes.is_empty() {
        return Ok(());
    }

    ensure_document_version_storage_format_in_session(session, database_name).await?;
    let live_query = format!(
        "INSERT INTO {} (
            table_id,
            id,
            commit_sequence,
            commit_time,
            tombstone,
            data_json,
            typed_fields_json,
            creation_time,
            update_time
         ) VALUES (?, ?, ?, ?, FALSE, ?, ?, ?, ?)",
        qualified_table(database_name, "document_versions")
    );
    let tombstone_query = format!(
        "INSERT INTO {} (
            table_id,
            id,
            commit_sequence,
            commit_time,
            tombstone
         ) VALUES (?, ?, ?, ?, TRUE)",
        qualified_table(database_name, "document_versions")
    );

    for write in writes {
        let id = write.doc_id.to_string();
        match &write.current {
            Some(current) => {
                session
                    .exec_drop(
                        live_query.as_str(),
                        (
                            write.table_id.as_str(),
                            id,
                            sequence.0,
                            timestamp.0,
                            serialize_document_fields(current)?,
                            serialize_document_typed_fields(current)?,
                            current.creation_time.0,
                            current.update_time.0,
                        ),
                    )
                    .await
                    .map_err(map_mysql_error)?;
            }
            None => {
                session
                    .exec_drop(
                        tombstone_query.as_str(),
                        (write.table_id.as_str(), id, sequence.0, timestamp.0),
                    )
                    .await
                    .map_err(map_mysql_error)?;
            }
        }
    }
    Ok(())
}

pub(super) async fn prune_document_versions_before_in_session<C>(
    session: &mut C,
    database_name: &str,
    prune_before: SequenceNumber,
) -> Result<u64>
where
    C: Queryable,
{
    if prune_before.0 == 0 {
        return Ok(0);
    }
    validate_document_version_storage_format_in_session(session, database_name).await?;
    let anchor_query = format!(
        "SELECT table_id, id, MAX(commit_sequence)
         FROM {}
         WHERE commit_sequence <= ?
         GROUP BY table_id, id",
        qualified_table(database_name, "document_versions")
    );
    let anchors = session
        .exec::<Row, _, _>(anchor_query, (prune_before.0,))
        .await
        .map_err(map_mysql_error)?
        .into_iter()
        .map(|row| {
            let (table_id, id, commit_sequence): (String, String, u64) = mysql_async::from_row(row);
            (table_id, id, commit_sequence)
        })
        .collect::<BTreeSet<_>>();

    let candidate_query = format!(
        "SELECT table_id, id, commit_sequence
         FROM {}
         WHERE commit_sequence < ?
         ORDER BY table_id, id, commit_sequence",
        qualified_table(database_name, "document_versions")
    );
    let candidates = session
        .exec::<Row, _, _>(candidate_query, (prune_before.0,))
        .await
        .map_err(map_mysql_error)?
        .into_iter()
        .map(|row| {
            let (table_id, id, commit_sequence): (String, String, u64) = mysql_async::from_row(row);
            (table_id, id, commit_sequence)
        })
        .collect::<Vec<_>>();
    let delete_query = format!(
        "DELETE FROM {}
         WHERE table_id = ? AND id = ? AND commit_sequence = ?",
        qualified_table(database_name, "document_versions")
    );
    let mut pruned = 0_u64;
    for candidate in candidates {
        if anchors.contains(&candidate) {
            continue;
        }
        session
            .exec_drop(
                delete_query.as_str(),
                (candidate.0, candidate.1, candidate.2),
            )
            .await
            .map_err(map_mysql_error)?;
        pruned = pruned.saturating_add(1);
    }
    Ok(pruned)
}

pub(super) async fn get_document_version_at_from_session<C>(
    session: &mut C,
    database_name: &str,
    table: &TableName,
    table_id: &TableId,
    document_id: &DocumentId,
    sequence: SequenceNumber,
) -> Result<Option<Document>>
where
    C: Queryable,
{
    validate_document_version_storage_format_in_session(session, database_name).await?;
    let query = format!(
        "SELECT tombstone, creation_time, update_time, data_json, typed_fields_json
         FROM {}
         WHERE table_id = ? AND id = ? AND commit_sequence <= ?
         ORDER BY commit_sequence DESC
         LIMIT 1",
        qualified_table(database_name, "document_versions")
    );
    let Some(row) = session
        .exec_first::<Row, _, _>(
            query,
            (table_id.as_str(), document_id.to_string(), sequence.0),
        )
        .await
        .map_err(map_mysql_error)?
    else {
        return Ok(None);
    };
    let (tombstone, creation_time, update_time, data_json, typed_fields_json): (
        bool,
        Option<u64>,
        Option<u64>,
        Option<String>,
        Option<String>,
    ) = mysql_async::from_row(row);
    if tombstone {
        return Ok(None);
    }

    Ok(Some(row_to_document(
        table,
        document_id,
        creation_time.ok_or_else(missing_live_version_field)?,
        update_time.ok_or_else(missing_live_version_field)?,
        data_json.ok_or_else(missing_live_version_field)?,
        typed_fields_json.ok_or_else(missing_live_version_field)?,
    )?))
}

async fn document_version_storage_diagnostic_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<DocumentVersionStorageDiagnostic>
where
    C: Queryable,
{
    let format_version =
        load_document_version_storage_format_from_session(session, database_name).await?;
    let query = format!(
        "SELECT COUNT(*), MIN(commit_sequence), MAX(commit_sequence) FROM {}",
        qualified_table(database_name, "document_versions")
    );
    let row = session
        .query_first::<Row, _>(query)
        .await
        .map_err(map_mysql_error)?
        .ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "MySQL document version aggregate query returned no row",
            )
        })?;
    let (version_count, min_sequence, max_sequence): (u64, Option<u64>, Option<u64>) =
        mysql_async::from_row(row);
    validate_document_version_storage_format_state(format_version, version_count > 0)?;

    Ok(DocumentVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence: min_sequence.map(SequenceNumber),
        max_sequence: max_sequence.map(SequenceNumber),
    })
}

async fn validate_document_version_storage_format_in_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<()>
where
    C: Queryable,
{
    let format_version =
        load_document_version_storage_format_from_session(session, database_name).await?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_document_version_storage_format(format_version)?;
            false
        }
        None => document_versions_have_rows_in_session(session, database_name).await?,
    };
    validate_document_version_storage_format_state(format_version, has_versions)
}

async fn ensure_document_version_storage_format_in_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<()>
where
    C: Queryable,
{
    if let Some(format_version) =
        load_document_version_storage_format_from_session(session, database_name).await?
    {
        validate_document_version_storage_format(format_version)?;
        return Ok(());
    }

    let query = format!(
        "INSERT INTO {} (key_name, value_u64) VALUES (?, ?)
         ON DUPLICATE KEY UPDATE value_u64 = VALUES(value_u64)",
        qualified_table(database_name, "metadata")
    );
    session
        .exec_drop(
            query,
            (
                DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
                u64::from(CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT.0),
            ),
        )
        .await
        .map_err(map_mysql_error)?;
    Ok(())
}

async fn load_document_version_storage_format_from_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<Option<StorageFormatVersion>>
where
    C: Queryable,
{
    load_metadata_u64_from_session(
        session,
        database_name,
        DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
    )
    .await?
    .map(storage_format_version_from_u64)
    .transpose()
}

async fn document_versions_have_rows_in_session<C>(
    session: &mut C,
    database_name: &str,
) -> Result<bool>
where
    C: Queryable,
{
    let query = format!(
        "SELECT EXISTS(SELECT 1 FROM {} LIMIT 1)",
        qualified_table(database_name, "document_versions")
    );
    let row = session
        .query_first::<Row, _>(query)
        .await
        .map_err(map_mysql_error)?
        .ok_or_else(|| {
            Error::storage(
                StorageErrorKind::Corruption,
                "MySQL document version existence query returned no row",
            )
        })?;
    let (exists,): (u8,) = mysql_async::from_row(row);
    Ok(exists != 0)
}

fn missing_live_version_field() -> Error {
    Error::storage(
        StorageErrorKind::Corruption,
        "live MySQL document version row is missing payload fields",
    )
}
