use nimbus_core::{
    Document, DocumentId, Error, HistoricalIndexCursor, HistoricalIndexQuery,
    HistoricalIndexScalar, HistoricalIndexTuple, HistoricalReadShape, IndexDefinition, Result,
    SequenceNumber, StorageErrorKind, WriteOp,
};
#[cfg(test)]
use nimbus_core::{IndexId, TableId};
use redb::ReadableTable;
use redb::TableError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::diagnostics::IndexVersionStorageDiagnostic;
use crate::index::history_scan::HistoricalIndexPageRequest;
use crate::index::{
    IndexRangeScanBounds, composite_range_scan_bounds, encode_index_tuple, encode_index_value,
    index_key_for_document, index_prefix, index_value_prefix,
};
use crate::keys::prefix_end;
use crate::range_bound::{index_range_bound_is_inclusive, index_range_bound_value};
use crate::store::schema_rewrite::load_table_schema_in_write_txn;
use crate::store::{INDEX_VERSIONS, METADATA, TenantReadSnapshot, TenantStore, map_redb_error};
use crate::{
    CURRENT_INDEX_VERSION_STORAGE_FORMAT, INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
    IndexRangeBound, storage_format_version_from_u64, validate_index_version_storage_format,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct IndexVersionValue {
    document_id: String,
    visible_from: u64,
    visible_until: Option<u64>,
}

#[cfg(test)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct IndexVersionInterval {
    pub document_id: DocumentId,
    pub visible_from: SequenceNumber,
    pub visible_until: Option<SequenceNumber>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HistoricalIndexDocumentPage {
    pub documents: Vec<Document>,
    pub next_cursor: Option<HistoricalIndexCursor>,
}

struct IndexVersionMutation {
    close_prefix: Option<Vec<u8>>,
    open_key: Option<Vec<u8>>,
    document_id: DocumentId,
}

struct HistoricalIndexDocumentEntry {
    tuple: HistoricalIndexTuple,
    document: Document,
}

enum HistoricalIndexKeyBounds<'a> {
    Empty,
    Bounds {
        match_prefix: &'a [u8],
        start_key: &'a [u8],
        end_key: Option<&'a [u8]>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HistoricalRangeStartKey {
    Empty,
    Seek(Vec<u8>),
}

impl TenantStore {
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

impl TenantReadSnapshot {
    pub fn index_version_storage_diagnostic(&self) -> Result<IndexVersionStorageDiagnostic> {
        index_version_storage_diagnostic_in_read_txn(&self.read_txn)
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
        let match_prefix = index_value_prefix(read_shape.table_id(), &index.id, &encoded);
        let end_key = prefix_end(&match_prefix);
        let query = HistoricalIndexQuery::Equal(HistoricalIndexTuple::from_values(
            std::slice::from_ref(value),
        )?);
        self.historical_index_scan_page_for_bounds(
            read_shape,
            &index,
            query,
            HistoricalIndexKeyBounds::Bounds {
                match_prefix: &match_prefix,
                start_key: &match_prefix,
                end_key: end_key.as_deref(),
            },
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
        let index = queryable_historical_index(read_shape, index_name)?;
        let encoded_prefix = encode_index_tuple(prefix_values)?;
        let match_prefix = index_value_prefix(read_shape.table_id(), &index.id, &encoded_prefix);
        let end_key = prefix_end(&match_prefix);
        let prefix = prefix_values
            .iter()
            .map(HistoricalIndexScalar::from_json)
            .collect::<Result<Vec<_>>>()?;
        self.historical_index_scan_page_for_bounds(
            read_shape,
            &index,
            HistoricalIndexQuery::Prefix(prefix),
            HistoricalIndexKeyBounds::Bounds {
                match_prefix: &match_prefix,
                start_key: &match_prefix,
                end_key: end_key.as_deref(),
            },
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
        let index = queryable_historical_index(read_shape, index_name)?;
        let match_prefix = index_prefix(read_shape.table_id(), &index.id);
        let start_encoded = index_range_bound_value(start)
            .map(encode_index_value)
            .transpose()?;
        let end_encoded = index_range_bound_value(end)
            .map(encode_index_value)
            .transpose()?;
        let range_start_key =
            historical_range_start_key(&match_prefix, start_encoded.as_deref(), start)?;
        let query = historical_range_query(start, end)?;
        let start_key = match range_start_key {
            HistoricalRangeStartKey::Empty => {
                return self.historical_index_scan_page_for_bounds(
                    read_shape,
                    &index,
                    query,
                    HistoricalIndexKeyBounds::Empty,
                    page,
                );
            }
            HistoricalRangeStartKey::Seek(start_key) => start_key,
        };
        let end_key = historical_range_end_key(&match_prefix, end_encoded.as_deref(), end);
        self.historical_index_scan_page_for_bounds(
            read_shape,
            &index,
            query,
            HistoricalIndexKeyBounds::Bounds {
                match_prefix: &match_prefix,
                start_key: &start_key,
                end_key: end_key.as_deref(),
            },
            page,
        )
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
        let index = queryable_historical_index(read_shape, index_name)?;
        let range_bounds = composite_range_scan_bounds(
            read_shape.table_id(),
            &index.id,
            exact_prefix,
            start,
            end,
        )?;
        let IndexRangeScanBounds::Bounds {
            match_prefix,
            start_key,
            end_key,
        } = range_bounds
        else {
            return self.historical_index_scan_page_for_bounds(
                read_shape,
                &index,
                historical_composite_range_query(exact_prefix, start, end)?,
                HistoricalIndexKeyBounds::Empty,
                page,
            );
        };
        self.historical_index_scan_page_for_bounds(
            read_shape,
            &index,
            historical_composite_range_query(exact_prefix, start, end)?,
            HistoricalIndexKeyBounds::Bounds {
                match_prefix: &match_prefix,
                start_key: &start_key,
                end_key: end_key.as_deref(),
            },
            page,
        )
    }

    fn historical_index_scan_page_for_bounds(
        &self,
        read_shape: &HistoricalReadShape,
        index: &IndexDefinition,
        query: HistoricalIndexQuery,
        key_bounds: HistoricalIndexKeyBounds<'_>,
        page: HistoricalIndexPageRequest<'_, '_>,
    ) -> Result<HistoricalIndexDocumentPage> {
        let HistoricalIndexPageRequest {
            after,
            limit,
            check_cancel,
        } = page;
        if limit == 0 {
            return Err(Error::InvalidInput(
                "historical index page limit must be greater than zero".to_string(),
            ));
        }
        if let Some(cursor) = after {
            cursor.validate_context(read_shape, index, &query)?;
        }
        let read_sequence = read_shape.read_snapshot().sequence().sequence();
        let initial_floor = self
            .retained_history_read_floors()?
            .max(self.retention_floor.published_read_floors())
            .historical_index();
        crate::retention::validate_retention_after_page(
            read_sequence,
            initial_floor,
            "historical index page",
        )?;
        let mut entries = match key_bounds {
            HistoricalIndexKeyBounds::Empty => Vec::new(),
            HistoricalIndexKeyBounds::Bounds {
                match_prefix,
                start_key,
                end_key,
            } => self.visible_historical_index_entries_for_bounds(
                read_shape,
                index,
                match_prefix,
                start_key,
                end_key,
                check_cancel,
            )?,
        };
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
        let result = HistoricalIndexDocumentPage {
            documents: selected.into_iter().map(|entry| entry.document).collect(),
            next_cursor,
        };
        self.fault_injector
            .check(crate::FaultPoint::RetentionReadAfterPage)?;
        let authoritative_floor = initial_floor.max(
            self.retention_floor
                .published_read_floors()
                .historical_index(),
        );
        crate::retention::validate_retention_after_page(
            read_sequence,
            authoritative_floor,
            "historical index page",
        )?;
        Ok(result)
    }

    fn visible_historical_index_entries_for_bounds(
        &self,
        read_shape: &HistoricalReadShape,
        index: &IndexDefinition,
        match_prefix: &[u8],
        start_key: &[u8],
        end_key: Option<&[u8]>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<HistoricalIndexDocumentEntry>> {
        validate_index_version_storage_format_for_read_txn(&self.read_txn)?;
        let versions = match self.read_txn.open_table(INDEX_VERSIONS) {
            Ok(versions) => versions,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };
        let read_sequence = read_shape.read_snapshot().sequence().sequence();
        let mut entries = Vec::new();
        if let Some(end_key) = end_key {
            for item in versions.range(start_key..end_key).map_err(map_redb_error)? {
                check_cancel()?;
                let (key, value) = item.map_err(map_redb_error)?;
                let base_key = index_version_base_key(key.value())?;
                if !base_key.starts_with(match_prefix) {
                    break;
                }
                maybe_push_visible_historical_entry(
                    self,
                    read_shape,
                    index,
                    read_sequence,
                    value.value(),
                    &mut entries,
                )?;
            }
        } else {
            for item in versions.range(start_key..).map_err(map_redb_error)? {
                check_cancel()?;
                let (key, value) = item.map_err(map_redb_error)?;
                let base_key = index_version_base_key(key.value())?;
                if !base_key.starts_with(match_prefix) {
                    break;
                }
                maybe_push_visible_historical_entry(
                    self,
                    read_shape,
                    index,
                    read_sequence,
                    value.value(),
                    &mut entries,
                )?;
            }
        }
        Ok(entries)
    }

    #[cfg(test)]
    pub(crate) fn index_version_intervals_for_testing(
        &self,
        table_id: &TableId,
        index_id: &IndexId,
    ) -> Result<Vec<IndexVersionInterval>> {
        validate_index_version_storage_format_for_read_txn(&self.read_txn)?;
        let versions = match self.read_txn.open_table(INDEX_VERSIONS) {
            Ok(versions) => versions,
            Err(TableError::TableDoesNotExist(_)) => return Ok(Vec::new()),
            Err(error) => return Err(map_redb_error(error)),
        };
        let prefix = crate::index::index_prefix(table_id, index_id);
        let mut intervals = Vec::new();
        match prefix_end(&prefix) {
            Some(end) => {
                for item in versions
                    .range(prefix.as_slice()..end.as_slice())
                    .map_err(map_redb_error)?
                {
                    let (_, value) = item.map_err(map_redb_error)?;
                    intervals.push(decode_interval(value.value())?);
                }
            }
            None => {
                for item in versions
                    .range(prefix.as_slice()..)
                    .map_err(map_redb_error)?
                {
                    let (key, value) = item.map_err(map_redb_error)?;
                    if !key.value().starts_with(prefix.as_slice()) {
                        break;
                    }
                    intervals.push(decode_interval(value.value())?);
                }
            }
        }
        Ok(intervals)
    }
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

fn historical_range_start_key(
    index_prefix: &[u8],
    start: Option<&[u8]>,
    start_bound: IndexRangeBound<'_>,
) -> Result<HistoricalRangeStartKey> {
    let Some(start) = start else {
        return Ok(HistoricalRangeStartKey::Seek(index_prefix.to_vec()));
    };
    let mut start_key = index_prefix.to_vec();
    start_key.extend_from_slice(start);
    if index_range_bound_is_inclusive(start_bound) {
        return Ok(HistoricalRangeStartKey::Seek(start_key));
    }
    Ok(prefix_end(&start_key)
        .map(HistoricalRangeStartKey::Seek)
        .unwrap_or(HistoricalRangeStartKey::Empty))
}

fn historical_range_end_key(
    index_prefix: &[u8],
    end: Option<&[u8]>,
    end_bound: IndexRangeBound<'_>,
) -> Option<Vec<u8>> {
    let Some(end) = end else {
        return prefix_end(index_prefix);
    };
    let mut end_key = index_prefix.to_vec();
    end_key.extend_from_slice(end);
    if index_range_bound_is_inclusive(end_bound) {
        prefix_end(&end_key)
    } else {
        Some(end_key)
    }
}

fn historical_range_query(
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
) -> Result<HistoricalIndexQuery> {
    Ok(HistoricalIndexQuery::Range {
        start: index_range_bound_value(start)
            .map(|value| HistoricalIndexTuple::from_values(std::slice::from_ref(value)))
            .transpose()?,
        start_inclusive: index_range_bound_is_inclusive(start),
        end: index_range_bound_value(end)
            .map(|value| HistoricalIndexTuple::from_values(std::slice::from_ref(value)))
            .transpose()?,
        end_inclusive: index_range_bound_is_inclusive(end),
    })
}

fn historical_composite_range_query(
    exact_prefix: &[Value],
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
) -> Result<HistoricalIndexQuery> {
    if index_range_bound_value(start).is_none() && index_range_bound_value(end).is_none() {
        return Ok(HistoricalIndexQuery::Prefix(
            exact_prefix
                .iter()
                .map(HistoricalIndexScalar::from_json)
                .collect::<Result<Vec<_>>>()?,
        ));
    }
    Ok(HistoricalIndexQuery::Range {
        start: composite_bound_tuple(exact_prefix, index_range_bound_value(start))?,
        start_inclusive: index_range_bound_is_inclusive(start),
        end: composite_bound_tuple(exact_prefix, index_range_bound_value(end))?,
        end_inclusive: index_range_bound_is_inclusive(end),
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
    snapshot: &TenantReadSnapshot,
    read_shape: &HistoricalReadShape,
    index: &IndexDefinition,
    read_sequence: SequenceNumber,
    value: &[u8],
    entries: &mut Vec<HistoricalIndexDocumentEntry>,
) -> Result<()> {
    let value: IndexVersionValue = rmp_serde::from_slice(value)
        .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?;
    if !index_version_visible_at(&value, read_sequence) {
        return Ok(());
    }
    let document_id = DocumentId::from_key(&value.document_id)?;
    let Some(document) =
        snapshot.get_document_version_at(read_shape.table_id(), &document_id, read_sequence)?
    else {
        return Err(Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "visible historical index row for document {} has no document version at sequence {}",
                document_id, read_sequence.0
            ),
        ));
    };
    let tuple = HistoricalIndexTuple::from_document(&document, index)?.ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            format!(
                "visible historical index row for document {} has no tuple for index {}",
                document.id, index.name
            ),
        )
    })?;
    entries.push(HistoricalIndexDocumentEntry { tuple, document });
    Ok(())
}

fn index_version_visible_at(value: &IndexVersionValue, sequence: SequenceNumber) -> bool {
    value.visible_from <= sequence.0 && value.visible_until.is_none_or(|until| sequence.0 < until)
}

fn index_version_base_key(key: &[u8]) -> Result<&[u8]> {
    let base_len = key.len().checked_sub(8).ok_or_else(|| {
        Error::storage(
            StorageErrorKind::Corruption,
            "index-version key is missing visible_from sequence suffix",
        )
    })?;
    Ok(&key[..base_len])
}

pub(super) fn record_index_versions_for_writes(
    write_txn: &redb::WriteTransaction,
    sequence: SequenceNumber,
    writes: &[WriteOp],
) -> Result<()> {
    if writes.is_empty() {
        return Ok(());
    }

    let mutations = index_version_mutations_for_writes(write_txn, sequence, writes)?;
    if mutations.is_empty() {
        return Ok(());
    }

    ensure_index_version_storage_format_in_write_txn(write_txn)?;
    let mut versions = write_txn
        .open_table(INDEX_VERSIONS)
        .map_err(map_redb_error)?;
    for mutation in mutations {
        if let Some(close_prefix) = mutation.close_prefix {
            close_open_index_version(&mut versions, close_prefix.as_slice(), sequence)?;
        }
        if let Some(open_key) = mutation.open_key {
            let value = IndexVersionValue {
                document_id: mutation.document_id.to_string(),
                visible_from: sequence.0,
                visible_until: None,
            };
            let encoded = rmp_serde::to_vec_named(&value)
                .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?;
            versions
                .insert(open_key.as_slice(), encoded.as_slice())
                .map_err(map_redb_error)?;
        }
    }
    Ok(())
}

fn index_version_mutations_for_writes(
    write_txn: &redb::WriteTransaction,
    sequence: SequenceNumber,
    writes: &[WriteOp],
) -> Result<Vec<IndexVersionMutation>> {
    let mut mutations = Vec::new();
    for write in writes {
        let Some(table_schema) = load_table_schema_in_write_txn(write_txn, &write.table)? else {
            continue;
        };
        for index in table_schema.maintained_indexes() {
            let close_prefix = write
                .previous
                .as_ref()
                .map(|previous| index_key_for_document(previous, index, &write.table_id))
                .transpose()?
                .flatten();
            let open_key = write
                .current
                .as_ref()
                .map(|current| index_key_for_document(current, index, &write.table_id))
                .transpose()?
                .flatten()
                .map(|key| index_version_key(key, sequence));
            if close_prefix.is_some() || open_key.is_some() {
                mutations.push(IndexVersionMutation {
                    close_prefix,
                    open_key,
                    document_id: write.doc_id.clone(),
                });
            }
        }
    }
    Ok(mutations)
}

fn close_open_index_version(
    versions: &mut redb::Table<'_, &[u8], &[u8]>,
    prefix: &[u8],
    sequence: SequenceNumber,
) -> Result<()> {
    let Some(key) = latest_open_index_version_key(versions, prefix)? else {
        return Ok(());
    };
    let value = versions
        .get(key.as_slice())
        .map_err(map_redb_error)?
        .ok_or_else(|| nimbus_core::Error::Internal("index-version row disappeared".to_string()))?
        .value()
        .to_vec();
    let mut value: IndexVersionValue = rmp_serde::from_slice(value.as_slice())
        .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?;
    value.visible_until = Some(sequence.0);
    let encoded = rmp_serde::to_vec_named(&value)
        .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?;
    versions
        .insert(key.as_slice(), encoded.as_slice())
        .map_err(map_redb_error)?;
    Ok(())
}

fn latest_open_index_version_key(
    versions: &redb::Table<'_, &[u8], &[u8]>,
    prefix: &[u8],
) -> Result<Option<Vec<u8>>> {
    let mut latest = None;
    match prefix_end(prefix) {
        Some(end) => {
            for item in versions
                .range(prefix..end.as_slice())
                .map_err(map_redb_error)?
            {
                let (key, value) = item.map_err(map_redb_error)?;
                let value: IndexVersionValue = rmp_serde::from_slice(value.value())
                    .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?;
                if value.visible_until.is_none() {
                    latest = Some(key.value().to_vec());
                }
            }
        }
        None => {
            for item in versions.range(prefix..).map_err(map_redb_error)? {
                let (key, value) = item.map_err(map_redb_error)?;
                if !key.value().starts_with(prefix) {
                    break;
                }
                let value: IndexVersionValue = rmp_serde::from_slice(value.value())
                    .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?;
                if value.visible_until.is_none() {
                    latest = Some(key.value().to_vec());
                }
            }
        }
    }
    Ok(latest)
}

fn validate_index_version_storage_format_for_read_txn(
    read_txn: &redb::ReadTransaction,
) -> Result<()> {
    let format_version = load_index_version_storage_format_in_read_txn(read_txn)?;
    let has_versions = match format_version {
        Some(format_version) => {
            validate_index_version_storage_format(format_version)?;
            false
        }
        None => index_versions_have_rows_in_read_txn(read_txn)?,
    };
    crate::validate_index_version_storage_format_state(format_version, has_versions)
}

fn index_version_storage_diagnostic_in_read_txn(
    read_txn: &redb::ReadTransaction,
) -> Result<IndexVersionStorageDiagnostic> {
    let format_version = load_index_version_storage_format_in_read_txn(read_txn)?;
    let mut version_count = 0_u64;
    let mut min_sequence = None;
    let mut max_sequence = None;

    match read_txn.open_table(INDEX_VERSIONS) {
        Ok(versions) => {
            for item in versions.iter().map_err(map_redb_error)? {
                let (_, value) = item.map_err(map_redb_error)?;
                let value: IndexVersionValue = rmp_serde::from_slice(value.value())
                    .map_err(|error| Error::Serialization(error.to_string()))?;
                let visible_from = SequenceNumber(value.visible_from);
                min_sequence = Some(
                    min_sequence.map_or(visible_from, |current: SequenceNumber| {
                        current.min(visible_from)
                    }),
                );
                max_sequence = Some(
                    max_sequence.map_or(visible_from, |current: SequenceNumber| {
                        current.max(visible_from)
                    }),
                );
                if let Some(visible_until) = value.visible_until {
                    let visible_until = SequenceNumber(visible_until);
                    max_sequence = Some(
                        max_sequence.map_or(visible_until, |current| current.max(visible_until)),
                    );
                }
                version_count = version_count.saturating_add(1);
            }
        }
        Err(TableError::TableDoesNotExist(_)) => {}
        Err(error) => return Err(map_redb_error(error)),
    }

    crate::validate_index_version_storage_format_state(format_version, version_count > 0)?;
    Ok(IndexVersionStorageDiagnostic {
        format_version,
        version_count,
        min_sequence,
        max_sequence,
    })
}

fn ensure_index_version_storage_format_in_write_txn(
    write_txn: &redb::WriteTransaction,
) -> Result<()> {
    let mut metadata = write_txn.open_table(METADATA).map_err(map_redb_error)?;
    let existing = metadata
        .get(INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY)
        .map_err(map_redb_error)?
        .map(|value| value.value().to_vec());
    if let Some(bytes) = existing {
        let version = storage_format_version_from_u64(decode_format_u64(bytes.as_slice())?)?;
        validate_index_version_storage_format(version)?;
        return Ok(());
    }

    metadata
        .insert(
            INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY,
            encode_format_u64(CURRENT_INDEX_VERSION_STORAGE_FORMAT.0.into()).as_slice(),
        )
        .map_err(map_redb_error)?;
    Ok(())
}

fn load_index_version_storage_format_in_read_txn(
    read_txn: &redb::ReadTransaction,
) -> Result<Option<crate::StorageFormatVersion>> {
    let metadata = match read_txn.open_table(METADATA) {
        Ok(metadata) => metadata,
        Err(TableError::TableDoesNotExist(_)) => return Ok(None),
        Err(error) => return Err(map_redb_error(error)),
    };
    metadata
        .get(INDEX_VERSION_STORAGE_FORMAT_METADATA_KEY)
        .map_err(map_redb_error)?
        .map(|value| storage_format_version_from_u64(decode_format_u64(value.value())?))
        .transpose()
}

fn index_versions_have_rows_in_read_txn(read_txn: &redb::ReadTransaction) -> Result<bool> {
    let versions = match read_txn.open_table(INDEX_VERSIONS) {
        Ok(versions) => versions,
        Err(TableError::TableDoesNotExist(_)) => return Ok(false),
        Err(error) => return Err(map_redb_error(error)),
    };
    Ok(versions
        .iter()
        .map_err(map_redb_error)?
        .next()
        .transpose()
        .map_err(map_redb_error)?
        .is_some())
}

fn index_version_key(mut index_key: Vec<u8>, sequence: SequenceNumber) -> Vec<u8> {
    index_key.extend_from_slice(&sequence.0.to_be_bytes());
    index_key
}

#[cfg(test)]
fn decode_interval(bytes: &[u8]) -> Result<IndexVersionInterval> {
    let value: IndexVersionValue = rmp_serde::from_slice(bytes)
        .map_err(|error| nimbus_core::Error::Serialization(error.to_string()))?;
    Ok(IndexVersionInterval {
        document_id: DocumentId::from_key(&value.document_id)?,
        visible_from: SequenceNumber(value.visible_from),
        visible_until: value.visible_until.map(SequenceNumber),
    })
}

fn encode_format_u64(value: u64) -> [u8; 8] {
    value.to_be_bytes()
}

fn decode_format_u64(bytes: &[u8]) -> Result<u64> {
    let array: [u8; 8] = bytes.try_into().map_err(|_| {
        nimbus_core::Error::storage(
            nimbus_core::StorageErrorKind::Corruption,
            "index-version storage format marker is not a u64",
        )
    })?;
    Ok(u64::from_be_bytes(array))
}

#[cfg(test)]
mod tests {
    use std::ops::Bound;

    use serde_json::json;

    use super::*;

    #[test]
    fn redb_historical_exclusive_start_at_max_key_is_empty() {
        let marker = json!("marker");

        assert_eq!(
            historical_range_start_key(&[0xff], Some(&[0xff]), Bound::Excluded(&marker))
                .expect("exclusive start key should compute"),
            HistoricalRangeStartKey::Empty
        );
        assert_eq!(
            historical_range_start_key(&[0xff], Some(&[0xff]), Bound::Included(&marker))
                .expect("inclusive start key should compute"),
            HistoricalRangeStartKey::Seek(vec![0xff, 0xff])
        );
        assert_eq!(
            historical_range_start_key(&[0xff], None, Bound::Excluded(&marker))
                .expect("unbounded start key should compute"),
            HistoricalRangeStartKey::Seek(vec![0xff])
        );
    }
}
