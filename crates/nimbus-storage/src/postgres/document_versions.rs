use super::*;
use crate::diagnostics::DocumentVersionStorageDiagnostic;
use crate::{
    CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT, DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
    StorageFormatVersion, storage_format_version_from_u64,
    validate_document_version_storage_format, validate_document_version_storage_format_state,
};

impl PostgresTenantStore {
    pub fn get_document_version_at(
        &self,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table = table.clone();
        let table_id = table_id.clone();
        let document_id = document_id.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            get_document_version_at_from_session(
                &client,
                &schema_name,
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
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            document_version_storage_diagnostic_from_session(&client, &schema_name).await
        })
    }
}

pub(super) async fn record_document_versions_for_events_in_session<C>(
    session: &C,
    schema_name: &str,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    events: &[TenantEventKind],
) -> Result<()>
where
    C: GenericClient + Sync,
{
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_document_versions_for_writes_in_session(
                session,
                schema_name,
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
    session: &C,
    schema_name: &str,
    sequence: SequenceNumber,
    timestamp: Timestamp,
    writes: &[WriteOp],
) -> Result<()>
where
    C: GenericClient + Sync,
{
    if writes.is_empty() {
        return Ok(());
    }

    ensure_document_version_storage_format_in_session(session, schema_name).await?;
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
         ) VALUES ($1, $2, $3, $4, FALSE, $5, $6, $7, $8)",
        qualified_table(schema_name, "document_versions")
    );
    let tombstone_query = format!(
        "INSERT INTO {} (
            table_id,
            id,
            commit_sequence,
            commit_time,
            tombstone
         ) VALUES ($1, $2, $3, $4, TRUE)",
        qualified_table(schema_name, "document_versions")
    );
    let sequence = i64_from_sequence(sequence)?;
    let timestamp = i64_from_timestamp(timestamp)?;

    for write in writes {
        let id = write.doc_id.to_string();
        match &write.current {
            Some(current) => {
                let data_json = serialize_document_fields(current)?;
                let typed_fields_json = serialize_document_typed_fields(current)?;
                let creation_time = i64_from_timestamp(current.creation_time)?;
                let update_time = i64_from_timestamp(current.update_time)?;
                session
                    .execute(
                        live_query.as_str(),
                        &[
                            &write.table_id.as_str(),
                            &id,
                            &sequence,
                            &timestamp,
                            &data_json,
                            &typed_fields_json,
                            &creation_time,
                            &update_time,
                        ],
                    )
                    .await
                    .map_err(map_postgres_error)?;
            }
            None => {
                session
                    .execute(
                        tombstone_query.as_str(),
                        &[&write.table_id.as_str(), &id, &sequence, &timestamp],
                    )
                    .await
                    .map_err(map_postgres_error)?;
            }
        }
    }
    Ok(())
}

pub(super) async fn prune_document_versions_before_in_session<C>(
    session: &C,
    schema_name: &str,
    prune_before: SequenceNumber,
) -> Result<u64>
where
    C: GenericClient + Sync,
{
    if prune_before.0 == 0 {
        return Ok(0);
    }
    validate_document_version_storage_format_in_session(session, schema_name).await?;
    let query = format!(
        "WITH anchors AS (
            SELECT table_id, id, MAX(commit_sequence) AS anchor_sequence
            FROM {}
            WHERE commit_sequence <= $1
            GROUP BY table_id, id
         ),
         deleted AS (
            DELETE FROM {} AS versions
            USING anchors
            WHERE versions.table_id = anchors.table_id
              AND versions.id = anchors.id
              AND versions.commit_sequence < $1
              AND versions.commit_sequence <> anchors.anchor_sequence
            RETURNING 1
         )
         SELECT COUNT(*) FROM deleted",
        qualified_table(schema_name, "document_versions"),
        qualified_table(schema_name, "document_versions")
    );
    let row = session
        .query_one(query.as_str(), &[&i64_from_sequence(prune_before)?])
        .await
        .map_err(map_postgres_error)?;
    u64::try_from(row.get::<_, i64>(0)).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "PostgreSQL document-version prune count is negative",
        )
    })
}

pub(super) async fn get_document_version_at_from_session<C>(
    session: &C,
    schema_name: &str,
    table: &TableName,
    table_id: &TableId,
    document_id: &DocumentId,
    sequence: SequenceNumber,
) -> Result<Option<Document>>
where
    C: GenericClient + Sync,
{
    validate_document_version_storage_format_in_session(session, schema_name).await?;
    let query = format!(
        "SELECT tombstone, creation_time, update_time, data_json, typed_fields_json
         FROM {}
         WHERE table_id = $1 AND id = $2 AND commit_sequence <= $3
         ORDER BY commit_sequence DESC
         LIMIT 1",
        qualified_table(schema_name, "document_versions")
    );
    let sequence = i64_from_sequence(sequence)?;
    let id = document_id.to_string();
    let Some(row) = session
        .query_opt(query.as_str(), &[&table_id.as_str(), &id, &sequence])
        .await
        .map_err(map_postgres_error)?
    else {
        return Ok(None);
    };

    let tombstone: bool = row.get(0);
    if tombstone {
        return Ok(None);
    }
    let creation_time: Option<i64> = row.get(1);
    let update_time: Option<i64> = row.get(2);
    let data_json: Option<String> = row.get(3);
    let typed_fields_json: Option<String> = row.get(4);

    Ok(Some(Document {
        id: document_id.clone(),
        table: table.clone(),
        creation_time: timestamp_from_i64(creation_time.ok_or_else(missing_live_version_field)?)?,
        update_time: timestamp_from_i64(update_time.ok_or_else(missing_live_version_field)?)?,
        fields: serde_json::from_str(data_json.ok_or_else(missing_live_version_field)?.as_str())
            .map_err(|error| Error::Serialization(error.to_string()))?,
        typed_fields: serde_json::from_str(
            typed_fields_json
                .ok_or_else(missing_live_version_field)?
                .as_str(),
        )
        .map_err(|error| Error::Serialization(error.to_string()))?,
    }))
}

async fn document_version_storage_diagnostic_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<DocumentVersionStorageDiagnostic>
where
    C: GenericClient + Sync,
{
    let format_version =
        load_document_version_storage_format_from_session(session, schema_name).await?;
    let query = format!(
        "SELECT COUNT(*), MIN(commit_sequence), MAX(commit_sequence) FROM {}",
        qualified_table(schema_name, "document_versions")
    );
    let row = session
        .query_one(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    let version_count = u64::try_from(row.get::<_, i64>(0)).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "PostgreSQL document version count is negative",
        )
    })?;
    let min_sequence = row
        .get::<_, Option<i64>>(1)
        .map(sequence_number_from_i64)
        .transpose()?;
    let max_sequence = row
        .get::<_, Option<i64>>(2)
        .map(sequence_number_from_i64)
        .transpose()?;
    validate_document_version_storage_format_state(format_version, version_count > 0)?;

    Ok(DocumentVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence,
        max_sequence,
    })
}

async fn validate_document_version_storage_format_in_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    let format_version =
        load_document_version_storage_format_from_session(session, schema_name).await?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_document_version_storage_format(format_version)?;
            false
        }
        None => document_versions_have_rows_in_session(session, schema_name).await?,
    };
    validate_document_version_storage_format_state(format_version, has_versions)
}

async fn ensure_document_version_storage_format_in_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    if let Some(format_version) =
        load_document_version_storage_format_from_session(session, schema_name).await?
    {
        validate_document_version_storage_format(format_version)?;
        return Ok(());
    }

    let query = format!(
        "INSERT INTO {} (key, value_blob) VALUES ($1, $2)
         ON CONFLICT(key) DO UPDATE SET value_blob = EXCLUDED.value_blob",
        qualified_table(schema_name, "metadata")
    );
    let key = DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY.to_string();
    let value = encode_u64(CURRENT_DOCUMENT_VERSION_STORAGE_FORMAT.0.into()).to_vec();
    session
        .execute(query.as_str(), &[&key, &value])
        .await
        .map_err(map_postgres_error)?;
    Ok(())
}

async fn load_document_version_storage_format_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<Option<StorageFormatVersion>>
where
    C: GenericClient + Sync,
{
    load_metadata_u64_from_session(
        session,
        schema_name,
        DOCUMENT_VERSION_STORAGE_FORMAT_METADATA_KEY,
    )
    .await?
    .map(storage_format_version_from_u64)
    .transpose()
}

async fn document_versions_have_rows_in_session<C>(session: &C, schema_name: &str) -> Result<bool>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT EXISTS(SELECT 1 FROM {} LIMIT 1)",
        qualified_table(schema_name, "document_versions")
    );
    let row = session
        .query_one(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    Ok(row.get::<_, bool>(0))
}

fn missing_live_version_field() -> Error {
    Error::storage(
        StorageErrorKind::Corruption,
        "live PostgreSQL document version row is missing payload fields",
    )
}
