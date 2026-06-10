use super::document_versions::get_document_version_at_from_session;
use super::*;
use crate::diagnostics::IndexVersionStorageDiagnostic;
use crate::index::encoded_index_tuple_for_document;
use crate::index::history_scan::{
    HistoricalIndexDocumentEntry, HistoricalIndexPageRequest, HistoricalIndexScanPlan,
    finish_historical_index_page,
};
use crate::store::HistoricalIndexDocumentPage;
use crate::{
    CURRENT_INDEX_VERSION_STORAGE_FORMAT, INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
    IndexRangeBound, StorageFormatVersion, storage_format_version_from_u64,
    validate_index_version_storage_format,
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

impl PostgresTenantStore {
    pub fn index_version_storage_diagnostic(&self) -> Result<IndexVersionStorageDiagnostic> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            index_version_storage_diagnostic_from_session(&client, &schema_name).await
        })
    }

    pub fn historical_index_scan_eq_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_eq_page_cancellable(
                read_shape,
                index_name,
                value,
                None,
                usize::MAX,
                check_cancel,
            )?
            .documents)
    }

    pub fn historical_index_scan_eq_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        value: &Value,
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let plan = HistoricalIndexScanPlan::equal(read_shape, index_name, value)?;
        self.historical_index_scan_page_for_plan(
            read_shape,
            &plan,
            HistoricalIndexPageRequest {
                after,
                limit,
                check_cancel,
            },
        )
    }

    pub fn historical_index_scan_prefix_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_prefix_page_cancellable(
                read_shape,
                index_name,
                prefix_values,
                None,
                usize::MAX,
                check_cancel,
            )?
            .documents)
    }

    pub fn historical_index_scan_prefix_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        prefix_values: &[Value],
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let plan = HistoricalIndexScanPlan::prefix(read_shape, index_name, prefix_values)?;
        self.historical_index_scan_page_for_plan(
            read_shape,
            &plan,
            HistoricalIndexPageRequest {
                after,
                limit,
                check_cancel,
            },
        )
    }

    pub fn historical_index_scan_range_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_range_page_cancellable(
                read_shape,
                index_name,
                start,
                end,
                HistoricalIndexPageRequest {
                    after: None,
                    limit: usize::MAX,
                    check_cancel,
                },
            )?
            .documents)
    }

    pub(crate) fn historical_index_scan_range_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        page: HistoricalIndexPageRequest<'_, '_>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let plan = HistoricalIndexScanPlan::range(read_shape, index_name, start, end)?;
        self.historical_index_scan_page_for_plan(read_shape, &plan, page)
    }

    pub fn historical_index_scan_composite_range_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_composite_range_page_cancellable(
                read_shape,
                index_name,
                exact_prefix,
                start,
                end,
                HistoricalIndexPageRequest {
                    after: None,
                    limit: usize::MAX,
                    check_cancel,
                },
            )?
            .documents)
    }

    pub(crate) fn historical_index_scan_composite_range_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        page: HistoricalIndexPageRequest<'_, '_>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let plan = HistoricalIndexScanPlan::composite_range(
            read_shape,
            index_name,
            exact_prefix,
            start,
            end,
        )?;
        self.historical_index_scan_page_for_plan(read_shape, &plan, page)
    }

    fn historical_index_scan_page_for_plan(
        &self,
        read_shape: &HistoricalReadShape,
        plan: &HistoricalIndexScanPlan,
        page: HistoricalIndexPageRequest<'_, '_>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let HistoricalIndexPageRequest {
            after,
            limit,
            check_cancel,
        } = page;
        plan.validate_page_request(read_shape, after, limit)?;
        if plan.empty {
            return finish_historical_index_page(read_shape, plan, after, limit, Vec::new());
        }
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let read_shape_for_query = read_shape.clone();
        let index_for_query = plan.index.clone();
        let match_prefix = plan.match_prefix.clone();
        let start_key = plan.start_key.clone();
        let end_key = plan.end_key.clone();
        let entries = self.block_on(async move {
            let client = provider.client().await?;
            visible_historical_index_entries_for_tuple_bounds(
                &client,
                &schema_name,
                &read_shape_for_query,
                &index_for_query,
                &match_prefix,
                start_key.as_deref(),
                end_key.as_deref(),
            )
            .await
        })?;
        for _ in &entries {
            check_cancel()?;
        }
        finish_historical_index_page(read_shape, plan, after, limit, entries)
    }

    #[cfg(test)]
    pub(crate) fn index_version_intervals_for_testing(
        &self,
        table_id: &TableId,
        index_id: &nimbus_core::IndexId,
    ) -> Result<Vec<IndexVersionInterval>> {
        let provider = self.provider.clone();
        let schema_name = self.schema_name.clone();
        let table_id = table_id.clone();
        let index_id = index_id.clone();
        self.block_on(async move {
            let client = provider.client().await?;
            index_version_intervals_from_session(&client, &schema_name, &table_id, &index_id).await
        })
    }
}

async fn visible_historical_index_entries_for_tuple_bounds<C>(
    session: &C,
    schema_name: &str,
    read_shape: &HistoricalReadShape,
    index: &IndexDefinition,
    match_prefix: &[u8],
    start_key: Option<&[u8]>,
    end_key: Option<&[u8]>,
) -> Result<Vec<HistoricalIndexDocumentEntry>>
where
    C: GenericClient + Sync,
{
    validate_index_version_storage_format_in_session(session, schema_name).await?;
    let read_sequence = read_shape.read_snapshot().sequence().sequence();
    let table_id_param = read_shape.table_id().as_str().to_string();
    let index_id_param = index.id.as_str().to_string();
    let start_param = start_key
        .filter(|key| !key.is_empty())
        .map(ToOwned::to_owned);
    let end_param = end_key.map(ToOwned::to_owned);
    let mut params: Vec<&(dyn ToSql + Sync)> = vec![&table_id_param, &index_id_param];
    let mut query = format!(
        "SELECT encoded_tuple, document_id, visible_from, visible_until
         FROM {}
         WHERE table_id = $1 AND index_id = $2",
        qualified_table(schema_name, "index_versions")
    );
    if let Some(start_key) = &start_param {
        let ordinal = params.len() + 1;
        query.push_str(format!(" AND encoded_tuple >= ${ordinal}").as_str());
        params.push(start_key);
    }
    if let Some(end_key) = &end_param {
        let ordinal = params.len() + 1;
        query.push_str(format!(" AND encoded_tuple < ${ordinal}").as_str());
        params.push(end_key);
    }
    query.push_str(" ORDER BY encoded_tuple, document_id, visible_from");
    let rows = session
        .query(query.as_str(), params.as_slice())
        .await
        .map_err(map_postgres_error)?;
    let mut entries = Vec::new();
    for row in rows {
        let encoded_tuple = row.get::<_, Vec<u8>>(0);
        if !encoded_tuple.starts_with(match_prefix) {
            if !match_prefix.is_empty() {
                break;
            }
            continue;
        }
        let value = PostgresIndexVersionValue {
            document_id: row.get::<_, String>(1),
            visible_from: row.get::<_, i64>(2),
            visible_until: row.get::<_, Option<i64>>(3),
        };
        maybe_push_visible_historical_entry(
            session,
            schema_name,
            read_shape,
            index,
            read_sequence,
            value,
            &mut entries,
        )
        .await?;
    }
    Ok(entries)
}

struct PostgresIndexVersionValue {
    document_id: String,
    visible_from: i64,
    visible_until: Option<i64>,
}

async fn maybe_push_visible_historical_entry<C>(
    session: &C,
    schema_name: &str,
    read_shape: &HistoricalReadShape,
    index: &IndexDefinition,
    read_sequence: SequenceNumber,
    value: PostgresIndexVersionValue,
    entries: &mut Vec<HistoricalIndexDocumentEntry>,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    if !postgres_index_version_visible_at(&value, read_sequence)? {
        return Ok(());
    }
    let document_id = DocumentId::from_key(value.document_id.as_str())?;
    let Some(document) = get_document_version_at_from_session(
        session,
        schema_name,
        read_shape.table(),
        read_shape.table_id(),
        &document_id,
        read_sequence,
    )
    .await?
    else {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "visible historical Postgres index row for document {} has no document version at sequence {}",
                document_id, read_sequence.0
            ),
        ));
    };
    let tuple = HistoricalIndexTuple::from_document(&document, index)?.ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "visible historical Postgres index row for document {} has no tuple for index {}",
                document.id, index.name
            ),
        )
    })?;
    entries.push(HistoricalIndexDocumentEntry { tuple, document });
    Ok(())
}

fn postgres_index_version_visible_at(
    value: &PostgresIndexVersionValue,
    sequence: SequenceNumber,
) -> Result<bool> {
    let visible_from = sequence_number_from_i64(value.visible_from)?;
    let visible_until = value
        .visible_until
        .map(sequence_number_from_i64)
        .transpose()?;
    Ok(visible_from <= sequence && visible_until.is_none_or(|until| sequence < until))
}

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

    ensure_index_version_storage_format_in_session(session, schema_name).await?;
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

pub(super) async fn prune_index_versions_before_in_session<C>(
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
    validate_index_version_storage_format_in_session(session, schema_name).await?;
    let query = format!(
        "WITH deleted AS (
            DELETE FROM {}
            WHERE visible_until IS NOT NULL AND visible_until <= $1
            RETURNING 1
         )
         SELECT COUNT(*) FROM deleted",
        qualified_table(schema_name, "index_versions")
    );
    let row = session
        .query_one(query.as_str(), &[&i64_from_sequence(prune_before)?])
        .await
        .map_err(map_postgres_error)?;
    u64::try_from(row.get::<_, i64>(0)).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "PostgreSQL index-version prune count is negative",
        )
    })
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

#[cfg(test)]
async fn index_version_intervals_from_session<C>(
    session: &C,
    schema_name: &str,
    table_id: &TableId,
    index_id: &nimbus_core::IndexId,
) -> Result<Vec<IndexVersionInterval>>
where
    C: GenericClient + Sync,
{
    validate_index_version_storage_format_in_session(session, schema_name).await?;
    let query = format!(
        "SELECT document_id, visible_from, visible_until
         FROM {}
         WHERE table_id = $1 AND index_id = $2
         ORDER BY encoded_tuple, document_id, visible_from",
        qualified_table(schema_name, "index_versions")
    );
    let rows = session
        .query(query.as_str(), &[&table_id.as_str(), &index_id.as_str()])
        .await
        .map_err(map_postgres_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(IndexVersionInterval {
                document_id: DocumentId::from_key(row.get::<_, String>(0))?,
                visible_from: sequence_number_from_i64(row.get::<_, i64>(1))?,
                visible_until: row
                    .get::<_, Option<i64>>(2)
                    .map(sequence_number_from_i64)
                    .transpose()?,
            })
        })
        .collect()
}

async fn validate_index_version_storage_format_in_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    let format_version =
        load_index_version_storage_format_from_session(session, schema_name).await?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_index_version_storage_format(format_version)?;
            false
        }
        None => index_versions_have_rows_in_session(session, schema_name).await?,
    };
    crate::validate_index_version_storage_format_state(format_version, has_versions)
}

async fn index_version_storage_diagnostic_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<IndexVersionStorageDiagnostic>
where
    C: GenericClient + Sync,
{
    let format_version =
        load_index_version_storage_format_from_session(session, schema_name).await?;
    let query = format!(
        "SELECT COUNT(*), MIN(visible_from), MAX(GREATEST(visible_from, COALESCE(visible_until, visible_from))) FROM {}",
        qualified_table(schema_name, "index_versions")
    );
    let row = session
        .query_one(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    let version_count = u64::try_from(row.get::<_, i64>(0)).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "PostgreSQL index version count is negative",
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
    crate::validate_index_version_storage_format_state(format_version, version_count > 0)?;

    Ok(IndexVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence,
        max_sequence,
    })
}

async fn ensure_index_version_storage_format_in_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<()>
where
    C: GenericClient + Sync,
{
    if let Some(format_version) =
        load_index_version_storage_format_from_session(session, schema_name).await?
    {
        validate_index_version_storage_format(format_version)?;
        return Ok(());
    }

    let query = format!(
        "INSERT INTO {} (key, value_blob) VALUES ($1, $2)
         ON CONFLICT(key) DO UPDATE SET value_blob = EXCLUDED.value_blob",
        qualified_table(schema_name, "metadata")
    );
    let key = INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY.to_string();
    let value = encode_u64(CURRENT_INDEX_VERSION_STORAGE_FORMAT.0.into()).to_vec();
    session
        .execute(query.as_str(), &[&key, &value])
        .await
        .map_err(map_postgres_error)?;
    Ok(())
}

async fn load_index_version_storage_format_from_session<C>(
    session: &C,
    schema_name: &str,
) -> Result<Option<StorageFormatVersion>>
where
    C: GenericClient + Sync,
{
    load_metadata_u64_from_session(
        session,
        schema_name,
        INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
    )
    .await?
    .map(storage_format_version_from_u64)
    .transpose()
}

async fn index_versions_have_rows_in_session<C>(session: &C, schema_name: &str) -> Result<bool>
where
    C: GenericClient + Sync,
{
    let query = format!(
        "SELECT EXISTS(SELECT 1 FROM {} LIMIT 1)",
        qualified_table(schema_name, "index_versions")
    );
    let row = session
        .query_one(query.as_str(), &[])
        .await
        .map_err(map_postgres_error)?;
    Ok(row.get::<_, bool>(0))
}
