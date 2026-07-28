use nimbus_core::{
    Document, DocumentId, Error, HistoricalIndexCursor, HistoricalIndexTuple, HistoricalReadShape,
    IndexDefinition, IndexId, Result, SequenceNumber, StorageErrorKind, TableId, TenantEventKind,
    WriteOp,
};
use rusqlite::OptionalExtension;
use rusqlite::types::Value as SqlValue;
use serde_json::Value;
#[cfg(test)]
use std::path::Path;

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
use super::config::{
    SqliteWriteStatementConcept, observe_sqlite_schema_check, observe_sqlite_uncached_statement,
};
use super::{
    SqliteReadSnapshot, SqliteTenantStore, decode_u64, encode_u64, load_table_schema_from_conn,
    map_sqlite_error,
};

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexVersionInterval {
    pub document_id: DocumentId,
    pub visible_from: SequenceNumber,
    pub visible_until: Option<SequenceNumber>,
}

struct IndexVersionMutation {
    close_tuple: Option<Vec<u8>>,
    open_tuple: Option<Vec<u8>>,
    document_id: DocumentId,
    index_id: IndexId,
    table_id: TableId,
}

impl SqliteTenantStore {
    pub fn index_version_storage_diagnostic(&self) -> Result<IndexVersionStorageDiagnostic> {
        self.read_snapshot()?.index_version_storage_diagnostic()
    }

    #[cfg(test)]
    pub(crate) fn index_version_intervals_for_testing(
        &self,
        table_id: &TableId,
        index_id: &IndexId,
    ) -> Result<Vec<IndexVersionInterval>> {
        self.read_snapshot()?
            .index_version_intervals_for_testing(table_id, index_id)
    }
}

impl SqliteReadSnapshot {
    pub fn index_version_storage_diagnostic(&self) -> Result<IndexVersionStorageDiagnostic> {
        index_version_storage_diagnostic_in_conn(&self.conn)
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
        let entries = self.visible_historical_index_entries_for_tuple_bounds(
            read_shape,
            &plan.index,
            plan.match_prefix.as_slice(),
            plan.start_key.as_deref(),
            plan.end_key.as_deref(),
            check_cancel,
        )?;
        finish_historical_index_page(read_shape, plan, after, limit, entries)
    }

    fn visible_historical_index_entries_for_tuple_bounds(
        &self,
        read_shape: &HistoricalReadShape,
        index: &IndexDefinition,
        match_prefix: &[u8],
        start_key: Option<&[u8]>,
        end_key: Option<&[u8]>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<HistoricalIndexDocumentEntry>> {
        validate_index_version_storage_format_in_conn(&self.conn)?;
        let read_sequence = read_shape.read_snapshot().sequence().sequence();
        let mut sql = String::from(
            "SELECT encoded_tuple, document_id, visible_from, visible_until
             FROM index_versions
             WHERE table_id = ?1 AND index_id = ?2",
        );
        let mut params = vec![
            SqlValue::Text(read_shape.table_id().as_str().to_string()),
            SqlValue::Text(index.id.as_str().to_string()),
        ];
        if let Some(start_key) = start_key.filter(|key| !key.is_empty()) {
            sql.push_str(" AND encoded_tuple >= ?");
            params.push(SqlValue::Blob(start_key.to_vec()));
        }
        if let Some(end_key) = end_key {
            sql.push_str(" AND encoded_tuple < ?");
            params.push(SqlValue::Blob(end_key.to_vec()));
        }
        sql.push_str(" ORDER BY encoded_tuple, document_id, visible_from");
        let mut stmt = self.conn.prepare_cached(&sql).map_err(map_sqlite_error)?;
        let mut rows = stmt
            .query(rusqlite::params_from_iter(params))
            .map_err(map_sqlite_error)?;
        let mut entries = Vec::new();
        while let Some(row) = rows.next().map_err(map_sqlite_error)? {
            check_cancel()?;
            let encoded_tuple = row.get::<_, Vec<u8>>(0).map_err(map_sqlite_error)?;
            if !encoded_tuple.starts_with(match_prefix) {
                if !match_prefix.is_empty() {
                    break;
                }
                continue;
            }
            let value = SqliteIndexVersionValue {
                document_id: row.get::<_, String>(1).map_err(map_sqlite_error)?,
                visible_from: row.get::<_, i64>(2).map_err(map_sqlite_error)?,
                visible_until: row.get::<_, Option<i64>>(3).map_err(map_sqlite_error)?,
            };
            maybe_push_visible_historical_entry(
                self,
                read_shape,
                index,
                read_sequence,
                value,
                &mut entries,
            )?;
        }
        Ok(entries)
    }

    #[cfg(test)]
    pub(crate) fn index_version_intervals_for_testing(
        &self,
        table_id: &TableId,
        index_id: &IndexId,
    ) -> Result<Vec<IndexVersionInterval>> {
        validate_index_version_storage_format_in_conn(&self.conn)?;
        let mut stmt = self
            .conn
            .prepare_cached(
                "SELECT document_id, visible_from, visible_until
                 FROM index_versions
                 WHERE table_id = ?1 AND index_id = ?2
                 ORDER BY encoded_tuple, document_id, visible_from",
            )
            .map_err(map_sqlite_error)?;
        let rows = stmt
            .query_map(
                rusqlite::params![table_id.as_str(), index_id.as_str()],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                    ))
                },
            )
            .map_err(map_sqlite_error)?;
        let mut intervals = Vec::new();
        for row in rows {
            let (document_id, visible_from, visible_until) = row.map_err(map_sqlite_error)?;
            intervals.push(IndexVersionInterval {
                document_id: DocumentId::from_key(document_id.as_str())?,
                visible_from: sqlite_i64_to_sequence(visible_from)?,
                visible_until: visible_until.map(sqlite_i64_to_sequence).transpose()?,
            });
        }
        Ok(intervals)
    }
}

struct SqliteIndexVersionValue {
    document_id: String,
    visible_from: i64,
    visible_until: Option<i64>,
}

fn maybe_push_visible_historical_entry(
    snapshot: &SqliteReadSnapshot,
    read_shape: &HistoricalReadShape,
    index: &IndexDefinition,
    read_sequence: SequenceNumber,
    value: SqliteIndexVersionValue,
    entries: &mut Vec<HistoricalIndexDocumentEntry>,
) -> Result<()> {
    if !sqlite_index_version_visible_at(&value, read_sequence)? {
        return Ok(());
    }
    let document_id = DocumentId::from_key(value.document_id.as_str())?;
    let Some(document) = snapshot.get_document_version_at(
        read_shape.table(),
        read_shape.table_id(),
        &document_id,
        read_sequence,
    )?
    else {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "visible historical SQLite index row for document {} has no document version at sequence {}",
                document_id, read_sequence.0
            ),
        ));
    };
    let tuple = HistoricalIndexTuple::from_document(&document, index)?.ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "visible historical SQLite index row for document {} has no tuple for index {}",
                document.id, index.name
            ),
        )
    })?;
    entries.push(HistoricalIndexDocumentEntry { tuple, document });
    Ok(())
}

fn sqlite_index_version_visible_at(
    value: &SqliteIndexVersionValue,
    sequence: SequenceNumber,
) -> Result<bool> {
    let visible_from = sqlite_i64_to_sequence(value.visible_from)?;
    let visible_until = value
        .visible_until
        .map(sqlite_i64_to_sequence)
        .transpose()?;
    Ok(visible_from <= sequence && visible_until.is_none_or(|until| sequence < until))
}

pub(super) fn record_index_versions_for_events_in_conn(
    conn: &rusqlite::Connection,
    sequence: SequenceNumber,
    events: &[TenantEventKind],
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_index_versions_for_writes_in_conn(
                conn,
                sequence,
                writes,
                #[cfg(test)]
                observation_path,
            )?;
        }
    }
    Ok(())
}

pub(super) fn record_index_versions_for_writes_in_conn(
    conn: &rusqlite::Connection,
    sequence: SequenceNumber,
    writes: &[WriteOp],
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    if writes.is_empty() {
        return Ok(());
    }

    let mutations = index_version_mutations_for_writes(
        conn,
        writes,
        #[cfg(test)]
        observation_path,
    )?;
    if mutations.is_empty() {
        return Ok(());
    }

    ensure_index_version_storage_format_in_conn(
        conn,
        #[cfg(test)]
        observation_path,
    )?;
    let sequence_i64 = i64_from_sequence(sequence)?;
    for mutation in mutations {
        if let Some(close_tuple) = mutation.close_tuple {
            #[cfg(test)]
            observe_sqlite_uncached_statement(
                observation_path,
                SqliteWriteStatementConcept::IndexVersionClose,
            );
            conn.execute(
                "UPDATE index_versions
                 SET visible_until = ?5
                 WHERE table_id = ?1
                   AND index_id = ?2
                   AND encoded_tuple = ?3
                   AND document_id = ?4
                   AND visible_until IS NULL",
                rusqlite::params![
                    mutation.table_id.as_str(),
                    mutation.index_id.as_str(),
                    close_tuple,
                    mutation.document_id.to_string(),
                    sequence_i64,
                ],
            )
            .map_err(map_sqlite_error)?;
        }
        if let Some(open_tuple) = mutation.open_tuple {
            #[cfg(test)]
            observe_sqlite_uncached_statement(
                observation_path,
                SqliteWriteStatementConcept::IndexVersionOpen,
            );
            conn.execute(
                "INSERT INTO index_versions (
                    table_id,
                    index_id,
                    encoded_tuple,
                    document_id,
                    visible_from,
                    visible_until
                 ) VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                rusqlite::params![
                    mutation.table_id.as_str(),
                    mutation.index_id.as_str(),
                    open_tuple,
                    mutation.document_id.to_string(),
                    sequence_i64,
                ],
            )
            .map_err(map_sqlite_error)?;
        }
    }
    Ok(())
}

pub(super) fn prune_index_versions_before_in_conn(
    conn: &rusqlite::Connection,
    prune_before: SequenceNumber,
) -> Result<u64> {
    if prune_before.0 == 0 {
        return Ok(0);
    }
    validate_index_version_storage_format_in_conn(conn)?;
    let deleted = conn
        .execute(
            "DELETE FROM index_versions
             WHERE visible_until IS NOT NULL AND visible_until <= ?1",
            rusqlite::params![i64_from_sequence(prune_before)?],
        )
        .map_err(map_sqlite_error)?;
    u64::try_from(deleted).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            "SQLite index-version prune count exceeds u64",
        )
    })
}

fn index_version_mutations_for_writes(
    conn: &rusqlite::Connection,
    writes: &[WriteOp],
    #[cfg(test)] observation_path: &Path,
) -> Result<Vec<IndexVersionMutation>> {
    let mut mutations = Vec::new();
    for write in writes {
        #[cfg(test)]
        {
            observe_sqlite_schema_check(observation_path);
            observe_sqlite_uncached_statement(
                observation_path,
                SqliteWriteStatementConcept::IndexSchemaRead,
            );
        }
        let Some(table_schema) = load_table_schema_from_conn(conn, &write.table)? else {
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
                    close_tuple,
                    open_tuple,
                    document_id: write.doc_id.clone(),
                    index_id: index.id.clone(),
                    table_id: write.table_id.clone(),
                });
            }
        }
    }
    Ok(mutations)
}

fn validate_index_version_storage_format_in_conn(conn: &rusqlite::Connection) -> Result<()> {
    let format_version = load_index_version_storage_format_in_conn(conn)?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_index_version_storage_format(format_version)?;
            false
        }
        None => index_versions_have_rows_in_conn(conn)?,
    };
    crate::validate_index_version_storage_format_state(format_version, has_versions)
}

fn index_version_storage_diagnostic_in_conn(
    conn: &rusqlite::Connection,
) -> Result<IndexVersionStorageDiagnostic> {
    let format_version = load_index_version_storage_format_in_conn(conn)?;
    let (version_count, min_sequence, max_sequence) = conn
        .query_row(
            "SELECT COUNT(*), MIN(visible_from), MAX(COALESCE(visible_until, visible_from))
             FROM index_versions",
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
            "SQLite index version count is negative",
        )
    })?;
    let min_sequence = min_sequence.map(sqlite_i64_to_sequence).transpose()?;
    let max_sequence = max_sequence.map(sqlite_i64_to_sequence).transpose()?;
    crate::validate_index_version_storage_format_state(format_version, version_count > 0)?;
    Ok(IndexVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence,
        max_sequence,
    })
}

fn ensure_index_version_storage_format_in_conn(
    conn: &rusqlite::Connection,
    #[cfg(test)] observation_path: &Path,
) -> Result<()> {
    #[cfg(test)]
    observe_sqlite_uncached_statement(
        observation_path,
        SqliteWriteStatementConcept::IndexVersionFormatRead,
    );
    if let Some(format_version) = load_index_version_storage_format_in_conn(conn)? {
        validate_index_version_storage_format(format_version)?;
        return Ok(());
    }

    #[cfg(test)]
    observe_sqlite_uncached_statement(
        observation_path,
        SqliteWriteStatementConcept::IndexVersionFormatWrite,
    );
    conn.execute(
        "INSERT INTO metadata (key, value_blob) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_blob = excluded.value_blob",
        rusqlite::params![
            INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
            encode_u64(CURRENT_INDEX_VERSION_STORAGE_FORMAT.0.into()).as_slice()
        ],
    )
    .map_err(map_sqlite_error)?;
    Ok(())
}

fn load_index_version_storage_format_in_conn(
    conn: &rusqlite::Connection,
) -> Result<Option<StorageFormatVersion>> {
    conn.query_row(
        "SELECT value_blob FROM metadata WHERE key = ?1",
        rusqlite::params![INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY],
        |row| row.get::<_, Vec<u8>>(0),
    )
    .optional()
    .map_err(map_sqlite_error)?
    .map(|bytes| storage_format_version_from_u64(decode_u64(bytes.as_slice())?))
    .transpose()
}

fn index_versions_have_rows_in_conn(conn: &rusqlite::Connection) -> Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM index_versions LIMIT 1)",
        [],
        |row| row.get::<_, i64>(0),
    )
    .map(|value| value != 0)
    .map_err(map_sqlite_error)
}

fn i64_from_sequence(sequence: SequenceNumber) -> Result<i64> {
    i64::try_from(sequence.0).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "sequence {} exceeds SQLite index-version integer range",
                sequence.0
            ),
        )
    })
}

fn sqlite_i64_to_sequence(value: i64) -> Result<SequenceNumber> {
    Ok(SequenceNumber(u64::try_from(value).map_err(|_| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!("SQLite index version sequence is negative: {value}"),
        )
    })?))
}
