use super::dialect::{PostgresHandle, PostgresSession};
use super::document_versions::get_document_version_at_from_session;
use super::*;
use crate::index::encoded_index_tuple_for_document;
use crate::sql::index_history::sql_historical_index_facade;
use crate::sql::index_versions::{
    SqlIndexVersionSession, SqlIndexVersionStore, ensure_index_version_storage_format_in_session,
    sql_index_version_facade,
};

struct IndexVersionMutation {
    table_id: String,
    index_id: String,
    document_id: String,
    close_tuple: Option<Vec<u8>>,
    open_tuple: Option<Vec<u8>>,
}

impl<H: PostgresHandle> SqlIndexVersionSession for PostgresSession<H> {
    async fn load_metadata_u64(&mut self, schema_name: &str, key: &str) -> Result<Option<u64>> {
        load_metadata_u64_from_session(self.driver(), schema_name, key).await
    }

    async fn upsert_metadata_u64(
        &mut self,
        schema_name: &str,
        key: &str,
        value: u64,
    ) -> Result<()> {
        let query = format!(
            "INSERT INTO {} (key, value_blob) VALUES ($1, $2)
             ON CONFLICT(key) DO UPDATE SET value_blob = EXCLUDED.value_blob",
            qualified_table(schema_name, "metadata")
        );
        self.driver()
            .execute(query.as_str(), &[&key, &encode_u64(value).as_slice()])
            .await
            .map_err(map_postgres_error)?;
        Ok(())
    }

    async fn load_retention_read_floors(
        &mut self,
        schema_name: &str,
    ) -> Result<crate::RetentionReadFloors> {
        load_retention_read_floors_from_session(self.driver(), schema_name).await
    }

    async fn document_version_at(
        &mut self,
        schema_name: &str,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> Result<Option<Document>> {
        get_document_version_at_from_session(
            self.driver(),
            schema_name,
            table,
            table_id,
            document_id,
            sequence,
        )
        .await
    }
}

impl SqlIndexVersionStore for PostgresTenantStore {}

sql_index_version_facade!(PostgresTenantStore);
sql_historical_index_facade!(PostgresTenantStore);

pub(super) async fn record_index_versions_for_events_in_session<C>(
    session: &C,
    schema_name: &str,
    sequence: SequenceNumber,
    events: &[TenantEventKind],
) -> Result<()>
where
    C: GenericClient + Sync,
{
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_index_versions_for_writes_in_session(session, schema_name, sequence, writes)
                .await?;
        }
    }
    Ok(())
}

pub(super) async fn record_index_versions_for_writes_in_session<C>(
    session: &C,
    schema_name: &str,
    sequence: SequenceNumber,
    writes: &[WriteOp],
) -> Result<()>
where
    C: GenericClient + Sync,
{
    if writes.is_empty() {
        return Ok(());
    }

    let mutations = index_version_mutations_for_writes(session, schema_name, writes).await?;
    if mutations.is_empty() {
        return Ok(());
    }

    ensure_index_version_storage_format_in_session(&mut PostgresSession(session), schema_name)
        .await?;
    let sequence = i64_from_sequence(sequence)?;
    let close_query = format!(
        "UPDATE {}
         SET visible_until = $5
         WHERE table_id = $1
           AND index_id = $2
           AND encoded_tuple = $3
           AND document_id = $4
           AND visible_until IS NULL",
        qualified_table(schema_name, "index_versions")
    );
    let open_query = format!(
        "INSERT INTO {} (
            table_id,
            index_id,
            encoded_tuple,
            document_id,
            visible_from,
            visible_until
         ) VALUES ($1, $2, $3, $4, $5, NULL)",
        qualified_table(schema_name, "index_versions")
    );

    for mutation in mutations {
        if let Some(close_tuple) = mutation.close_tuple {
            session
                .execute(
                    close_query.as_str(),
                    &[
                        &mutation.table_id,
                        &mutation.index_id,
                        &close_tuple,
                        &mutation.document_id,
                        &sequence,
                    ],
                )
                .await
                .map_err(map_postgres_error)?;
        }
        if let Some(open_tuple) = mutation.open_tuple {
            session
                .execute(
                    open_query.as_str(),
                    &[
                        &mutation.table_id,
                        &mutation.index_id,
                        &open_tuple,
                        &mutation.document_id,
                        &sequence,
                    ],
                )
                .await
                .map_err(map_postgres_error)?;
        }
    }
    Ok(())
}

async fn index_version_mutations_for_writes<C>(
    session: &C,
    schema_name: &str,
    writes: &[WriteOp],
) -> Result<Vec<IndexVersionMutation>>
where
    C: GenericClient + Sync,
{
    let mut mutations = Vec::new();
    for write in writes {
        let Some(table_schema) =
            load_table_schema_from_session(session, schema_name, &write.table).await?
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
