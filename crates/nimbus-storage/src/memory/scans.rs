use std::ops::Bound;
use std::sync::Arc;

use nimbus_core::{
    Document, DocumentId, Error, Filter, IndexDefinition, Result, SequenceNumber, TableId,
    TableName,
};
use serde_json::Value;

use crate::IndexRangeBound;
use crate::index::{encode_index_tuple, encode_index_value};

use super::MemoryTenantStore;
use super::state::MemoryState;

/// Immutable point-in-time view of one memory tenant.
pub struct MemoryTenantSnapshot {
    pub(super) state: Arc<MemoryState>,
}

fn table_documents(state: &MemoryState, table: &TableName) -> Vec<Document> {
    state
        .active_tables
        .get(table)
        .and_then(|table_id| state.documents.get(table_id))
        .map(|documents| documents.values().cloned().collect())
        .unwrap_or_default()
}

fn scan_table_matching<F>(
    state: &MemoryState,
    table: &TableName,
    check_cancel: &mut dyn FnMut() -> Result<()>,
    mut include_document: F,
) -> Result<Vec<Document>>
where
    F: FnMut(&Document) -> Result<bool>,
{
    let mut results = Vec::new();
    for document in table_documents(state, table) {
        check_cancel()?;
        if include_document(&document)? {
            results.push(document);
        }
    }
    Ok(results)
}

fn resolve_queryable_index(
    state: &MemoryState,
    table: &TableName,
    index_name: &str,
) -> Result<Option<IndexDefinition>> {
    if !state.active_tables.contains_key(table) {
        return Ok(None);
    }
    let table_schema = state
        .schema
        .get_table(table)
        .ok_or_else(|| Error::SchemaNotFound(table.clone()))?;
    table_schema
        .queryable_indexes()
        .find(|index| index.name == index_name)
        .cloned()
        .map(Some)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "enabled index not found for table {table}: {index_name}"
            ))
        })
}

fn indexed_values(document: &Document, index: &IndexDefinition) -> Option<Vec<Value>> {
    index
        .fields
        .iter()
        .map(|field| document.fields.get(field).cloned())
        .collect()
}

fn sorted_index_matches(
    state: &MemoryState,
    table: &TableName,
    index: &IndexDefinition,
    check_cancel: &mut dyn FnMut() -> Result<()>,
    mut matches: impl FnMut(&[Value], &[u8]) -> Result<bool>,
) -> Result<Vec<Document>> {
    let mut rows = Vec::new();
    for document in table_documents(state, table) {
        check_cancel()?;
        let Some(values) = indexed_values(&document, index) else {
            continue;
        };
        let encoded = encode_index_tuple(&values)?;
        if matches(&values, &encoded)? {
            rows.push((encoded, document.id.clone(), document));
        }
    }
    rows.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
    Ok(rows.into_iter().map(|(_, _, document)| document).collect())
}

fn encoded_bound(bound: IndexRangeBound<'_>) -> Result<Option<(Vec<u8>, bool)>> {
    match bound {
        Bound::Included(value) => Ok(Some((encode_index_value(value)?, true))),
        Bound::Excluded(value) => Ok(Some((encode_index_value(value)?, false))),
        Bound::Unbounded => Ok(None),
    }
}

fn range_matches(
    encoded: &[u8],
    start: Option<&(Vec<u8>, bool)>,
    end: Option<&(Vec<u8>, bool)>,
) -> bool {
    let lower = start.is_none_or(|(bound, inclusive)| {
        if *inclusive {
            encoded >= bound.as_slice()
        } else {
            encoded > bound.as_slice()
        }
    });
    let upper = end.is_none_or(|(bound, inclusive)| {
        if *inclusive {
            encoded <= bound.as_slice()
        } else {
            encoded < bound.as_slice()
        }
    });
    lower && upper
}

fn index_range_scan(
    state: &MemoryState,
    table: &TableName,
    index_name: &str,
    exact_prefix: &[Value],
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let Some(index) = resolve_queryable_index(state, table, index_name)? else {
        return Ok(Vec::new());
    };
    let encoded_prefix = exact_prefix
        .iter()
        .map(encode_index_value)
        .collect::<Result<Vec<_>>>()?;
    let start = encoded_bound(start)?;
    let end = encoded_bound(end)?;
    let range_type = match (start.as_ref(), end.as_ref()) {
        (Some((start, _)), Some((end, _))) if start[0] != end[0] => return Ok(Vec::new()),
        (Some((start, _)), _) => Some(start[0]),
        (_, Some((end, _))) => Some(end[0]),
        (None, None) => None,
    };
    sorted_index_matches(
        state,
        table,
        &index,
        check_cancel,
        |values, _encoded_tuple| {
            if values.len() <= exact_prefix.len() {
                return Ok(false);
            }
            for (value, expected) in values.iter().zip(&encoded_prefix) {
                if encode_index_value(value)? != *expected {
                    return Ok(false);
                }
            }
            let encoded_value = encode_index_value(&values[exact_prefix.len()])?;
            if range_type.is_some_and(|tag| encoded_value[0] != tag) {
                return Ok(false);
            }
            Ok(range_matches(&encoded_value, start.as_ref(), end.as_ref()))
        },
    )
}

macro_rules! impl_memory_scans {
    ($ty:ty, $state:ident) => {
        impl $ty {
            pub fn get(&self, table: &TableName, id: &DocumentId) -> Result<Option<Document>> {
                Ok(self.$state.as_ref().get(table, id))
            }

            pub fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
                Ok(self.$state.as_ref().table_id(table))
            }

            pub fn applied_sequence(&self) -> Result<SequenceNumber> {
                Ok(self.$state.as_ref().applied_head)
            }

            pub fn scan_table_matching_cancellable<F>(
                &self,
                table: &TableName,
                check_cancel: &mut dyn FnMut() -> Result<()>,
                include_document: F,
            ) -> Result<Vec<Document>>
            where
                F: FnMut(&Document) -> Result<bool>,
            {
                scan_table_matching(self.$state.as_ref(), table, check_cancel, include_document)
            }

            pub fn scan_table_matching_with_filters_cancellable<F>(
                &self,
                table: &TableName,
                _filters: &[Filter],
                check_cancel: &mut dyn FnMut() -> Result<()>,
                include_document: F,
            ) -> Result<Vec<Document>>
            where
                F: FnMut(&Document) -> Result<bool>,
            {
                scan_table_matching(self.$state.as_ref(), table, check_cancel, include_document)
            }

            pub fn scan_table_id_prefix_cancellable(
                &self,
                table: &TableName,
                id_prefix: &str,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                scan_table_matching(self.$state.as_ref(), table, check_cancel, |document| {
                    Ok(document.id.as_str().starts_with(id_prefix))
                })
            }

            pub fn scan_table_id_starting_at_cancellable(
                &self,
                table: &TableName,
                start_id: &str,
                limit: usize,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                if limit == 0 {
                    return Ok(Vec::new());
                }
                let mut documents =
                    scan_table_matching(self.$state.as_ref(), table, check_cancel, |document| {
                        Ok(document.id.as_str() >= start_id)
                    })?;
                documents.truncate(limit);
                Ok(documents)
            }

            pub fn index_scan_eq_cancellable(
                &self,
                table: &TableName,
                index_name: &str,
                value: &Value,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                let Some(index) = resolve_queryable_index(self.$state.as_ref(), table, index_name)?
                else {
                    return Ok(Vec::new());
                };
                let expected = encode_index_value(value)?;
                sorted_index_matches(
                    self.$state.as_ref(),
                    table,
                    &index,
                    check_cancel,
                    |values, _| {
                        values
                            .first()
                            .map(encode_index_value)
                            .transpose()
                            .map(|actual| actual.as_deref() == Some(expected.as_slice()))
                    },
                )
            }

            pub fn index_scan_prefix_cancellable(
                &self,
                table: &TableName,
                index_name: &str,
                prefix_values: &[Value],
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                let Some(index) = resolve_queryable_index(self.$state.as_ref(), table, index_name)?
                else {
                    return Ok(Vec::new());
                };
                let prefix = encode_index_tuple(prefix_values)?;
                sorted_index_matches(
                    self.$state.as_ref(),
                    table,
                    &index,
                    check_cancel,
                    |_values, encoded| Ok(encoded.starts_with(&prefix)),
                )
            }

            pub fn index_scan_range_cancellable(
                &self,
                table: &TableName,
                index_name: &str,
                start: IndexRangeBound<'_>,
                end: IndexRangeBound<'_>,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                index_range_scan(
                    self.$state.as_ref(),
                    table,
                    index_name,
                    &[],
                    start,
                    end,
                    check_cancel,
                )
            }

            pub fn index_scan_composite_range_cancellable(
                &self,
                table: &TableName,
                index_name: &str,
                exact_prefix: &[Value],
                start: IndexRangeBound<'_>,
                end: IndexRangeBound<'_>,
                check_cancel: &mut dyn FnMut() -> Result<()>,
            ) -> Result<Vec<Document>> {
                index_range_scan(
                    self.$state.as_ref(),
                    table,
                    index_name,
                    exact_prefix,
                    start,
                    end,
                    check_cancel,
                )
            }
        }
    };
}

impl_memory_scans!(MemoryTenantSnapshot, state);

impl MemoryTenantStore {
    pub fn read_snapshot(&self) -> Result<MemoryTenantSnapshot> {
        Ok(MemoryTenantSnapshot {
            state: Arc::new(self.read_state()?.clone()),
        })
    }

    pub fn table_id(&self, table: &TableName) -> Result<Option<TableId>> {
        self.read_snapshot()?.table_id(table)
    }

    pub fn scan_table(&self, table: &TableName) -> Result<Vec<Document>> {
        self.scan_table_matching_cancellable(table, &mut || Ok(()), |_| Ok(true))
    }

    pub fn scan_table_matching_cancellable<F>(
        &self,
        table: &TableName,
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.read_snapshot()?
            .scan_table_matching_cancellable(table, check_cancel, include_document)
    }

    pub fn scan_table_matching_with_filters_cancellable<F>(
        &self,
        table: &TableName,
        filters: &[Filter],
        check_cancel: &mut dyn FnMut() -> Result<()>,
        include_document: F,
    ) -> Result<Vec<Document>>
    where
        F: FnMut(&Document) -> Result<bool>,
    {
        self.read_snapshot()?
            .scan_table_matching_with_filters_cancellable(
                table,
                filters,
                check_cancel,
                include_document,
            )
    }

    pub fn scan_table_id_prefix_cancellable(
        &self,
        table: &TableName,
        id_prefix: &str,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?
            .scan_table_id_prefix_cancellable(table, id_prefix, check_cancel)
    }

    pub fn scan_table_id_starting_at_cancellable(
        &self,
        table: &TableName,
        start_id: &str,
        limit: usize,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?.scan_table_id_starting_at_cancellable(
            table,
            start_id,
            limit,
            check_cancel,
        )
    }

    pub fn index_scan_eq_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        value: &Value,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?
            .index_scan_eq_cancellable(table, index_name, value, check_cancel)
    }

    pub fn index_scan_prefix_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        prefix_values: &[Value],
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?.index_scan_prefix_cancellable(
            table,
            index_name,
            prefix_values,
            check_cancel,
        )
    }

    pub fn index_scan_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?.index_scan_range_cancellable(
            table,
            index_name,
            start,
            end,
            check_cancel,
        )
    }

    pub fn index_scan_composite_range_cancellable(
        &self,
        table: &TableName,
        index_name: &str,
        exact_prefix: &[Value],
        start: IndexRangeBound<'_>,
        end: IndexRangeBound<'_>,
        check_cancel: &mut dyn FnMut() -> Result<()>,
    ) -> Result<Vec<Document>> {
        self.read_snapshot()?
            .index_scan_composite_range_cancellable(
                table,
                index_name,
                exact_prefix,
                start,
                end,
                check_cancel,
            )
    }
}
