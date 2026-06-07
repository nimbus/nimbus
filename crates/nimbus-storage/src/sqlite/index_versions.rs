use nimbus_core::{
    Document, DocumentId, Error, HistoricalIndexCursor, HistoricalIndexQuery,
    HistoricalIndexScalar, HistoricalIndexTuple, HistoricalReadShape, IndexDefinition, IndexId,
    Result, SequenceNumber, StorageErrorKind, TableId, TenantEventKind, WriteOp,
};
use rusqlite::OptionalExtension;
use rusqlite::types::Value as SqlValue;
use serde_json::Value;

use crate::diagnostics::IndexVersionStorageDiagnostic;
use crate::index::{encode_index_tuple, encode_index_value, encoded_index_tuple_for_document};
use crate::keys::prefix_end;
use crate::store::HistoricalIndexDocumentPage;
use crate::{
    CURRENT_INDEX_VERSION_STORAGE_FORMAT, INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
    StorageFormatVersion, storage_format_version_from_u64, validate_index_version_storage_format,
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

struct HistoricalIndexDocumentEntry {
    tuple: HistoricalIndexTuple,
    document: Document,
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
        let index = queryable_historical_index(read_shape, index_name)?;
        let encoded = encode_index_value(value)?;
        let end_key = prefix_end(&encoded);
        let query = HistoricalIndexQuery::Equal(HistoricalIndexTuple::from_values(
            std::slice::from_ref(value),
        )?);
        self.historical_index_scan_page_for_tuple_bounds(
            read_shape,
            &index,
            query,
            &encoded,
            Some(&encoded),
            end_key.as_deref(),
            after,
            limit,
            check_cancel,
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
        let index = queryable_historical_index(read_shape, index_name)?;
        let encoded_prefix = encode_index_tuple(prefix_values)?;
        let end_key = prefix_end(&encoded_prefix);
        let prefix = prefix_values
            .iter()
            .map(HistoricalIndexScalar::from_json)
            .collect::<Result<Vec<_>>>()?;
        self.historical_index_scan_page_for_tuple_bounds(
            read_shape,
            &index,
            HistoricalIndexQuery::Prefix(prefix),
            &encoded_prefix,
            Some(&encoded_prefix),
            end_key.as_deref(),
            after,
            limit,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn historical_index_scan_range_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_range_page_cancellable(
                read_shape,
                index_name,
                start,
                end,
                start_inclusive,
                end_inclusive,
                None,
                usize::MAX,
                check_cancel,
            )?
            .documents)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn historical_index_scan_range_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let index = queryable_historical_index(read_shape, index_name)?;
        let start_encoded = start.map(encode_index_value).transpose()?;
        let end_encoded = end.map(encode_index_value).transpose()?;
        let start_key = historical_range_start_key(start_encoded.as_deref(), start_inclusive);
        let end_key = historical_range_end_key(end_encoded.as_deref(), end_inclusive);
        self.historical_index_scan_page_for_tuple_bounds(
            read_shape,
            &index,
            historical_range_query(start, end, start_inclusive, end_inclusive)?,
            &[],
            start_key.as_deref(),
            end_key.as_deref(),
            after,
            limit,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn historical_index_scan_composite_range_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        Ok(self
            .historical_index_scan_composite_range_page_cancellable(
                read_shape,
                index_name,
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
                None,
                usize::MAX,
                check_cancel,
            )?
            .documents)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn historical_index_scan_composite_range_page_cancellable(
        &self,
        read_shape: &HistoricalReadShape,
        index_name: &str,
        exact_prefix: &[Value],
        start: Option<&Value>,
        end: Option<&Value>,
        start_inclusive: bool,
        end_inclusive: bool,
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let index = queryable_historical_index(read_shape, index_name)?;
        let encoded_prefix = encode_index_tuple(exact_prefix)?;
        let start_key = historical_composite_start_key(&encoded_prefix, start, start_inclusive)?;
        let end_key = historical_composite_end_key(&encoded_prefix, end, end_inclusive)?;
        self.historical_index_scan_page_for_tuple_bounds(
            read_shape,
            &index,
            historical_composite_range_query(
                exact_prefix,
                start,
                end,
                start_inclusive,
                end_inclusive,
            )?,
            &encoded_prefix,
            Some(&start_key),
            end_key.as_deref(),
            after,
            limit,
            check_cancel,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn historical_index_scan_page_for_tuple_bounds(
        &self,
        read_shape: &HistoricalReadShape,
        index: &IndexDefinition,
        query: HistoricalIndexQuery,
        match_prefix: &[u8],
        start_key: Option<&[u8]>,
        end_key: Option<&[u8]>,
        after: Option<&HistoricalIndexCursor>,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<HistoricalIndexDocumentPage> {
        if limit == 0 {
            return Err(Error::InvalidInput(
                "historical index page limit must be greater than zero".to_string(),
            ));
        }
        if let Some(cursor) = after {
            cursor.validate_context(read_shape, index, &query)?;
        }
        let mut entries = self.visible_historical_index_entries_for_tuple_bounds(
            read_shape,
            index,
            match_prefix,
            start_key,
            end_key,
            check_cancel,
        )?;
        entries.sort_by(|left, right| {
            left.tuple
                .cmp(&right.tuple)
                .then_with(|| left.document.id.cmp(&right.document.id))
        });
        let start = after
            .and_then(|cursor| {
                entries.iter().position(|entry| {
                    &entry.tuple == cursor.last_tuple()
                        && &entry.document.id == cursor.last_document_id()
                })
            })
            .map_or(0, |position| position.saturating_add(1));
        let selected = entries
            .into_iter()
            .skip(start)
            .take(limit)
            .collect::<Vec<_>>();
        let next_cursor = if selected.len() == limit {
            selected.last().map(|entry| {
                HistoricalIndexCursor::new(
                    read_shape,
                    index,
                    query,
                    entry.tuple.clone(),
                    entry.document.id.clone(),
                )
            })
        } else {
            None
        };
        Ok(HistoricalIndexDocumentPage {
            documents: selected.into_iter().map(|entry| entry.document).collect(),
            next_cursor,
        })
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

fn queryable_historical_index(
    read_shape: &HistoricalReadShape,
    index_name: &str,
) -> Result<IndexDefinition> {
    read_shape
        .queryable_indexes()
        .iter()
        .find(|index| index.name == index_name)
        .cloned()
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "enabled historical index not found for table {}: {}",
                read_shape.table(),
                index_name
            ))
        })
}

fn historical_range_start_key(start: Option<&[u8]>, start_inclusive: bool) -> Option<Vec<u8>> {
    let start = start?;
    if start_inclusive {
        Some(start.to_vec())
    } else {
        prefix_end(start).or_else(|| Some(Vec::new()))
    }
}

fn historical_range_end_key(end: Option<&[u8]>, end_inclusive: bool) -> Option<Vec<u8>> {
    let end = end?;
    if end_inclusive {
        prefix_end(end)
    } else {
        Some(end.to_vec())
    }
}

fn historical_composite_start_key(
    exact_prefix: &[u8],
    start: Option<&Value>,
    start_inclusive: bool,
) -> Result<Vec<u8>> {
    let Some(start) = start else {
        return Ok(exact_prefix.to_vec());
    };
    let mut key = exact_prefix.to_vec();
    key.extend_from_slice(&encode_index_value(start)?);
    if start_inclusive {
        Ok(key)
    } else {
        Ok(prefix_end(&key).unwrap_or_default())
    }
}

fn historical_composite_end_key(
    exact_prefix: &[u8],
    end: Option<&Value>,
    end_inclusive: bool,
) -> Result<Option<Vec<u8>>> {
    let Some(end) = end else {
        return Ok(prefix_end(exact_prefix));
    };
    let mut key = exact_prefix.to_vec();
    key.extend_from_slice(&encode_index_value(end)?);
    if end_inclusive {
        Ok(prefix_end(&key))
    } else {
        Ok(Some(key))
    }
}

fn historical_range_query(
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<HistoricalIndexQuery> {
    Ok(HistoricalIndexQuery::Range {
        start: start
            .map(|value| HistoricalIndexTuple::from_values(std::slice::from_ref(value)))
            .transpose()?,
        start_inclusive,
        end: end
            .map(|value| HistoricalIndexTuple::from_values(std::slice::from_ref(value)))
            .transpose()?,
        end_inclusive,
    })
}

fn historical_composite_range_query(
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<HistoricalIndexQuery> {
    if start.is_none() && end.is_none() {
        return Ok(HistoricalIndexQuery::Prefix(
            exact_prefix
                .iter()
                .map(HistoricalIndexScalar::from_json)
                .collect::<Result<Vec<_>>>()?,
        ));
    }
    Ok(HistoricalIndexQuery::Range {
        start: composite_bound_tuple(exact_prefix, start)?,
        start_inclusive,
        end: composite_bound_tuple(exact_prefix, end)?,
        end_inclusive,
    })
}

fn composite_bound_tuple(
    exact_prefix: &[Value],
    bound: Option<&Value>,
) -> Result<Option<HistoricalIndexTuple>> {
    if exact_prefix.is_empty() && bound.is_none() {
        return Ok(None);
    }
    let mut values = exact_prefix.to_vec();
    if let Some(bound) = bound {
        values.push(bound.clone());
    }
    HistoricalIndexTuple::from_values(&values).map(Some)
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
) -> Result<()> {
    for event in events {
        if let TenantEventKind::DocumentWrite { writes } = event {
            record_index_versions_for_writes_in_conn(conn, sequence, writes)?;
        }
    }
    Ok(())
}

pub(super) fn record_index_versions_for_writes_in_conn(
    conn: &rusqlite::Connection,
    sequence: SequenceNumber,
    writes: &[WriteOp],
) -> Result<()> {
    if writes.is_empty() {
        return Ok(());
    }

    let mutations = index_version_mutations_for_writes(conn, writes)?;
    if mutations.is_empty() {
        return Ok(());
    }

    ensure_index_version_storage_format_in_conn(conn)?;
    let sequence_i64 = i64_from_sequence(sequence)?;
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
) -> Result<Vec<IndexVersionMutation>> {
    let mut mutations = Vec::new();
    for write in writes {
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

fn ensure_index_version_storage_format_in_conn(conn: &rusqlite::Connection) -> Result<()> {
    if let Some(format_version) = load_index_version_storage_format_in_conn(conn)? {
        validate_index_version_storage_format(format_version)?;
        return Ok(());
    }

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
