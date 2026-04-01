use std::collections::HashSet;

use neovex_core::{Cursor, DocumentId, Filter, OrderBy, Query, TableName};
use serde_json::Value;

use super::intersection::{decode_runtime_cursor_boundary, extract_runtime_cursor_boundary};

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeReadSet {
    pub(super) tables: HashSet<TableName>,
    pub(super) documents: HashSet<(TableName, DocumentId)>,
    pub(super) index_ranges: Vec<RuntimeIndexRangeRead>,
    pub(super) predicates: Vec<RuntimePredicateRead>,
    pub(super) paginated_windows: Vec<RuntimePaginatedWindowRead>,
}

impl RuntimeReadSet {
    pub(crate) fn record_table(&mut self, table: &TableName) {
        self.tables.insert(table.clone());
    }

    pub(crate) fn record_document(&mut self, table: &TableName, document_id: &DocumentId) {
        self.documents.insert((table.clone(), *document_id));
    }

    pub(crate) fn record_index_range(&mut self, read: RuntimeIndexRangeRead) {
        if !self.index_ranges.iter().any(|existing| existing == &read) {
            self.index_ranges.push(read);
        }
    }

    pub(crate) fn record_predicate(&mut self, table: &TableName, filters: &[Filter]) {
        if filters.is_empty() {
            return;
        }

        let read = RuntimePredicateRead {
            table: table.clone(),
            filters: filters.to_vec(),
        };
        if !self.predicates.iter().any(|existing| existing == &read) {
            self.predicates.push(read);
        }
    }

    pub(crate) fn record_paginated_window(
        &mut self,
        query: &Query,
        page_size: usize,
        after: Option<&Cursor>,
        page: &neovex_core::Page,
    ) {
        let (start_sort_value, start_doc_id) = after
            .and_then(decode_runtime_cursor_boundary)
            .map_or((None, None), |(sort_value, doc_id)| {
                (sort_value, Some(doc_id))
            });
        let (end_sort_value, end_doc_id) = page
            .data
            .last()
            .and_then(|value| extract_runtime_cursor_boundary(query.order.as_ref(), value))
            .map_or((None, None), |(sort_value, doc_id)| {
                (sort_value, Some(doc_id))
            });
        let read = RuntimePaginatedWindowRead {
            table: query.table.clone(),
            filters: query.filters.clone(),
            order: query.order.clone(),
            start_sort_value,
            start_doc_id,
            end_sort_value,
            end_doc_id,
            result_count: page.data.len(),
            page_size,
        };
        if !self
            .paginated_windows
            .iter()
            .any(|existing| existing == &read)
        {
            self.paginated_windows.push(read);
        }
    }

    pub(super) fn tables(&self) -> HashSet<TableName> {
        let mut tables = self.tables.clone();
        for (table, _) in &self.documents {
            tables.insert(table.clone());
        }
        for read in &self.index_ranges {
            tables.insert(read.table.clone());
        }
        for read in &self.predicates {
            tables.insert(read.table.clone());
        }
        for read in &self.paginated_windows {
            tables.insert(read.table.clone());
        }
        tables
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RuntimeIndexRangeRead {
    pub(crate) table: TableName,
    pub(crate) index_name: String,
    pub(crate) field: String,
    pub(crate) start: Option<Value>,
    pub(crate) end: Option<Value>,
    pub(crate) start_inclusive: bool,
    pub(crate) end_inclusive: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RuntimePredicateRead {
    pub(super) table: TableName,
    pub(super) filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RuntimePaginatedWindowRead {
    pub(super) table: TableName,
    pub(super) filters: Vec<Filter>,
    pub(super) order: Option<OrderBy>,
    pub(super) start_sort_value: Option<Value>,
    pub(super) start_doc_id: Option<DocumentId>,
    pub(super) end_sort_value: Option<Value>,
    pub(super) end_doc_id: Option<DocumentId>,
    pub(super) result_count: usize,
    pub(super) page_size: usize,
}
