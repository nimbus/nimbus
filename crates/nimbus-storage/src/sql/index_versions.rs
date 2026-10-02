//! Dialect-shared index-version history.
//!
//! `index_versions` keeps one visibility interval per index tuple and
//! document. Historical index reads, the storage-format marker, the
//! diagnostic, and retention pruning use the same statements on PostgreSQL and
//! MySQL, so they live here once. Recording intervals stays with each
//! provider: MySQL keys the table by a tuple hash and derives the mutations
//! from its index-entry keyspace, and PostgreSQL derives them from the writes.
//!
//! [`SqlIndexVersionSession`] carries the provider-owned metadata and
//! document-version access that the shared statements need.

use std::future::Future;

use nimbus_core::{
    Document, DocumentId, Error, HistoricalIndexTuple, HistoricalReadShape, IndexDefinition,
    Result, SequenceNumber, StorageErrorKind, TableId, TableName,
};

use crate::diagnostics::IndexVersionStorageDiagnostic;
use crate::index::history_scan::HistoricalIndexDocumentEntry;
use crate::sql::dialect::{Dialect, SqlRow, SqlSession, SqlSessionStore, SqlValue};
use crate::sql::store_core::SqlStoreCore;
use crate::{
    CURRENT_INDEX_VERSION_STORAGE_FORMAT, INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
    RetentionReadFloors, StorageFormatVersion, storage_format_version_from_u64,
    validate_index_version_storage_format,
};

const INDEX_VERSIONS_TABLE: &str = "index_versions";

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexVersionInterval {
    pub document_id: DocumentId,
    pub visible_from: SequenceNumber,
    pub visible_until: Option<SequenceNumber>,
}

/// Session hooks for the provider-owned tables that index-version reads touch.
pub(crate) trait SqlIndexVersionSession: SqlSession {
    fn load_metadata_u64(
        &mut self,
        tenant_schema: &str,
        key: &str,
    ) -> impl Future<Output = Result<Option<u64>>> + Send;

    fn upsert_metadata_u64(
        &mut self,
        tenant_schema: &str,
        key: &str,
        value: u64,
    ) -> impl Future<Output = Result<()>> + Send;

    fn load_retention_read_floors(
        &mut self,
        tenant_schema: &str,
    ) -> impl Future<Output = Result<RetentionReadFloors>> + Send;

    fn document_version_at(
        &mut self,
        tenant_schema: &str,
        table: &TableName,
        table_id: &TableId,
        document_id: &DocumentId,
        sequence: SequenceNumber,
    ) -> impl Future<Output = Result<Option<Document>>> + Send;
}

/// Store-level index-version reads over a pooled session.
pub(crate) trait SqlIndexVersionStore: SqlStoreCore + SqlSessionStore {
    fn index_version_storage_diagnostic(&self) -> Result<IndexVersionStorageDiagnostic> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        self.block_on_read(async move {
            index_version_storage_diagnostic_from_session(&mut session.await?, &tenant_schema).await
        })
    }

    #[cfg(test)]
    fn index_version_intervals_for_testing(
        &self,
        table_id: &TableId,
        index_id: &nimbus_core::IndexId,
    ) -> Result<Vec<IndexVersionInterval>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let table_id = table_id.clone();
        let index_id = index_id.clone();
        self.block_on_read(async move {
            index_version_intervals_from_session(
                &mut session.await?,
                &tenant_schema,
                &table_id,
                &index_id,
            )
            .await
        })
    }

    /// Stored retention read floors, raised to the floors this process has
    /// already published.
    fn effective_retention_read_floors(&self) -> Result<RetentionReadFloors> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        self.block_on_read(async move {
            session
                .await?
                .load_retention_read_floors(&tenant_schema)
                .await
        })
        .map(|floors| floors.max(self.retention_floor().published_read_floors()))
    }

    fn load_visible_historical_index_entries(
        &self,
        read_shape: &HistoricalReadShape,
        index: &IndexDefinition,
        match_prefix: &[u8],
        start_key: Option<&[u8]>,
        end_key: Option<&[u8]>,
    ) -> Result<Vec<HistoricalIndexDocumentEntry>> {
        let session = self.open_read_session();
        let tenant_schema = self.tenant_schema();
        let read_shape = read_shape.clone();
        let index = index.clone();
        let match_prefix = match_prefix.to_vec();
        let start_key = start_key.map(<[u8]>::to_vec);
        let end_key = end_key.map(<[u8]>::to_vec);
        self.block_on_read(async move {
            visible_historical_index_entries_for_tuple_bounds(
                &mut session.await?,
                &tenant_schema,
                &read_shape,
                &index,
                &match_prefix,
                start_key.as_deref(),
                end_key.as_deref(),
            )
            .await
        })
    }
}

async fn visible_historical_index_entries_for_tuple_bounds<S: SqlIndexVersionSession>(
    session: &mut S,
    tenant_schema: &str,
    read_shape: &HistoricalReadShape,
    index: &IndexDefinition,
    match_prefix: &[u8],
    start_key: Option<&[u8]>,
    end_key: Option<&[u8]>,
) -> Result<Vec<HistoricalIndexDocumentEntry>> {
    validate_index_version_storage_format_in_session(session, tenant_schema).await?;
    let read_sequence = read_shape.read_snapshot().sequence().sequence();
    let mut query = format!(
        "SELECT encoded_tuple, document_id, visible_from, visible_until
         FROM {}
         WHERE table_id = ? AND index_id = ?",
        S::table(tenant_schema, INDEX_VERSIONS_TABLE)
    );
    let mut params = vec![
        SqlValue::Text(read_shape.table_id().as_str()),
        SqlValue::Text(index.id.as_str()),
    ];
    if let Some(start_key) = start_key.filter(|key| !key.is_empty()) {
        query.push_str(" AND encoded_tuple >= ?");
        params.push(SqlValue::Bytes(start_key));
    }
    if let Some(end_key) = end_key {
        query.push_str(" AND encoded_tuple < ?");
        params.push(SqlValue::Bytes(end_key));
    }
    query.push_str(" ORDER BY encoded_tuple, document_id, visible_from");
    let rows = session.fetch_all(&query, &params).await?;
    let mut entries = Vec::new();
    for row in rows {
        if !row.bytes(0)?.starts_with(match_prefix) {
            if !match_prefix.is_empty() {
                break;
            }
            continue;
        }
        let visible_from = SequenceNumber(row.u64(2)?);
        let visible_until = row.optional_u64(3)?.map(SequenceNumber);
        if visible_from > read_sequence || visible_until.is_some_and(|until| read_sequence >= until)
        {
            continue;
        }
        let document_id = DocumentId::from_key(row.text(1)?.as_str())?;
        entries.push(
            visible_historical_entry(session, tenant_schema, read_shape, index, document_id)
                .await?,
        );
    }
    Ok(entries)
}

async fn visible_historical_entry<S: SqlIndexVersionSession>(
    session: &mut S,
    tenant_schema: &str,
    read_shape: &HistoricalReadShape,
    index: &IndexDefinition,
    document_id: DocumentId,
) -> Result<HistoricalIndexDocumentEntry> {
    let read_sequence = read_shape.read_snapshot().sequence().sequence();
    let Some(document) = session
        .document_version_at(
            tenant_schema,
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
                "visible historical {} index row for document {} has no document version at sequence {}",
                S::Dialect::NAME,
                document_id,
                read_sequence.0
            ),
        ));
    };
    let tuple = HistoricalIndexTuple::from_document(&document, index)?.ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "visible historical {} index row for document {} has no tuple for index {}",
                S::Dialect::NAME,
                document.id,
                index.name
            ),
        )
    })?;
    Ok(HistoricalIndexDocumentEntry { tuple, document })
}

/// Delete every interval that closed at or before `prune_before` and return
/// how many were deleted.
pub(crate) async fn prune_index_versions_before_in_session<S: SqlIndexVersionSession>(
    session: &mut S,
    tenant_schema: &str,
    prune_before: SequenceNumber,
) -> Result<u64> {
    if prune_before.0 == 0 {
        return Ok(0);
    }
    validate_index_version_storage_format_in_session(session, tenant_schema).await?;
    let query = format!(
        "DELETE FROM {}
         WHERE visible_until IS NOT NULL AND visible_until <= ?",
        S::table(tenant_schema, INDEX_VERSIONS_TABLE)
    );
    session
        .execute_sql(&query, &[SqlValue::Sequence(prune_before)])
        .await
}

#[cfg(test)]
async fn index_version_intervals_from_session<S: SqlIndexVersionSession>(
    session: &mut S,
    tenant_schema: &str,
    table_id: &TableId,
    index_id: &nimbus_core::IndexId,
) -> Result<Vec<IndexVersionInterval>> {
    validate_index_version_storage_format_in_session(session, tenant_schema).await?;
    let query = format!(
        "SELECT document_id, visible_from, visible_until
         FROM {}
         WHERE table_id = ? AND index_id = ?
         ORDER BY encoded_tuple, document_id, visible_from",
        S::table(tenant_schema, INDEX_VERSIONS_TABLE)
    );
    let rows = session
        .fetch_all(
            &query,
            &[
                SqlValue::Text(table_id.as_str()),
                SqlValue::Text(index_id.as_str()),
            ],
        )
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(IndexVersionInterval {
                document_id: DocumentId::from_key(row.text(0)?)?,
                visible_from: SequenceNumber(row.u64(1)?),
                visible_until: row.optional_u64(2)?.map(SequenceNumber),
            })
        })
        .collect()
}

async fn validate_index_version_storage_format_in_session<S: SqlIndexVersionSession>(
    session: &mut S,
    tenant_schema: &str,
) -> Result<()> {
    let format_version =
        load_index_version_storage_format_from_session(session, tenant_schema).await?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_index_version_storage_format(format_version)?;
            false
        }
        None => {
            let query = format!(
                "SELECT 1 FROM {} LIMIT 1",
                S::table(tenant_schema, INDEX_VERSIONS_TABLE)
            );
            session.fetch_optional(&query, &[]).await?.is_some()
        }
    };
    crate::validate_index_version_storage_format_state(format_version, has_versions)
}

async fn index_version_storage_diagnostic_from_session<S: SqlIndexVersionSession>(
    session: &mut S,
    tenant_schema: &str,
) -> Result<IndexVersionStorageDiagnostic> {
    let format_version =
        load_index_version_storage_format_from_session(session, tenant_schema).await?;
    let query = format!(
        "SELECT COUNT(*), MIN(visible_from), MAX(GREATEST(visible_from, COALESCE(visible_until, visible_from))) FROM {}",
        S::table(tenant_schema, INDEX_VERSIONS_TABLE)
    );
    let row = session.fetch_optional(&query, &[]).await?.ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "{} index version aggregate query returned no row",
                S::Dialect::NAME
            ),
        )
    })?;
    let version_count = row.u64(0)?;
    crate::validate_index_version_storage_format_state(format_version, version_count > 0)?;

    Ok(IndexVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence: row.optional_u64(1)?.map(SequenceNumber),
        max_sequence: row.optional_u64(2)?.map(SequenceNumber),
    })
}

/// Record the current storage format before the first interval is written.
pub(crate) async fn ensure_index_version_storage_format_in_session<S: SqlIndexVersionSession>(
    session: &mut S,
    tenant_schema: &str,
) -> Result<()> {
    if let Some(format_version) =
        load_index_version_storage_format_from_session(session, tenant_schema).await?
    {
        return validate_index_version_storage_format(format_version);
    }
    session
        .upsert_metadata_u64(
            tenant_schema,
            INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
            u64::from(CURRENT_INDEX_VERSION_STORAGE_FORMAT.0),
        )
        .await
}

async fn load_index_version_storage_format_from_session<S: SqlIndexVersionSession>(
    session: &mut S,
    tenant_schema: &str,
) -> Result<Option<StorageFormatVersion>> {
    session
        .load_metadata_u64(tenant_schema, INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY)
        .await?
        .map(storage_format_version_from_u64)
        .transpose()
}

/// Implements [`crate::sql::index_history::SqlHistoricalIndexStore`] over
/// [`SqlIndexVersionStore`] and re-exposes the diagnostic entry points as
/// inherent methods, keeping each store's public API unchanged.
macro_rules! sql_index_version_facade {
    ($store:ty) => {
        impl crate::sql::index_history::SqlHistoricalIndexStore for $store {
            fn retention_read_floors(&self) -> nimbus_core::Result<crate::RetentionReadFloors> {
                <Self as crate::sql::index_versions::SqlIndexVersionStore>::effective_retention_read_floors(self)
            }

            fn check_retention_read_page(&self) -> nimbus_core::Result<()> {
                self.check_fault(crate::FaultPoint::RetentionReadAfterPage)
            }

            fn visible_historical_index_entries(
                &self,
                read_shape: &nimbus_core::HistoricalReadShape,
                index: &nimbus_core::IndexDefinition,
                match_prefix: &[u8],
                start_key: Option<&[u8]>,
                end_key: Option<&[u8]>,
            ) -> nimbus_core::Result<Vec<crate::index::history_scan::HistoricalIndexDocumentEntry>>
            {
                <Self as crate::sql::index_versions::SqlIndexVersionStore>::load_visible_historical_index_entries(
                    self,
                    read_shape,
                    index,
                    match_prefix,
                    start_key,
                    end_key,
                )
            }
        }

        impl $store {
            pub fn index_version_storage_diagnostic(
                &self,
            ) -> nimbus_core::Result<crate::diagnostics::IndexVersionStorageDiagnostic> {
                <Self as crate::sql::index_versions::SqlIndexVersionStore>::index_version_storage_diagnostic(self)
            }

            #[cfg(test)]
            pub(crate) fn index_version_intervals_for_testing(
                &self,
                table_id: &nimbus_core::TableId,
                index_id: &nimbus_core::IndexId,
            ) -> nimbus_core::Result<Vec<crate::sql::index_versions::IndexVersionInterval>> {
                <Self as crate::sql::index_versions::SqlIndexVersionStore>::index_version_intervals_for_testing(self, table_id, index_id)
            }
        }
    };
}

pub(crate) use sql_index_version_facade;
