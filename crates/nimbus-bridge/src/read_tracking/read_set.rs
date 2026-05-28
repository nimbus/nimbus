use std::collections::HashSet;

use nimbus_core::{
    Cursor, DependencySet, DocumentId, Filter, IndexId, IndexRangeDependency, OrderBy,
    PaginatedWindowDependency, PredicateDependency, Query, TableId, TableName,
};
use serde_json::Value;

use super::intersection::{decode_runtime_cursor_boundary, extract_runtime_cursor_boundary};

#[derive(Debug, Clone, Default)]
pub struct RuntimeReadSet {
    pub(super) tables: HashSet<RuntimeTableRead>,
    pub(super) documents: HashSet<RuntimeDocumentRead>,
    pub(super) index_ranges: Vec<RuntimeIndexRangeRead>,
    pub(super) predicates: Vec<RuntimePredicateRead>,
    pub(super) paginated_windows: Vec<RuntimePaginatedWindowRead>,
}

impl RuntimeReadSet {
    pub fn dependency_set(&self) -> DependencySet {
        let mut dependencies = DependencySet::default();
        for read in &self.tables {
            match read.table_id.as_ref() {
                Some(table_id) => dependencies.record_table(&read.table, table_id),
                None => dependencies.record_missing_table(&read.table),
            }
        }
        for read in &self.documents {
            match read.table_id.as_ref() {
                Some(table_id) => {
                    dependencies.record_document(&read.table, table_id, read.document_id.clone())
                }
                None => dependencies.record_missing_table(&read.table),
            }
        }
        for read in &self.index_ranges {
            if let (Some(table_id), Some(index_id)) =
                (read.table_id.as_ref(), read.index_id.as_ref())
            {
                dependencies.record_index_range(IndexRangeDependency {
                    table: read.table.clone(),
                    table_id: table_id.clone(),
                    index_id: index_id.clone(),
                    index_name: read.index_name.clone(),
                    field: read.field.clone(),
                    start: read.start.clone(),
                    end: read.end.clone(),
                    start_inclusive: read.start_inclusive,
                    end_inclusive: read.end_inclusive,
                });
            } else {
                dependencies.record_missing_table(&read.table);
            }
        }
        for read in &self.predicates {
            if let Some(table_id) = read.table_id.as_ref() {
                dependencies.record_predicate(PredicateDependency {
                    table: read.table.clone(),
                    table_id: table_id.clone(),
                    filters: read.filters.clone(),
                });
            } else {
                dependencies.record_missing_predicate(&read.table, read.filters.clone());
            }
        }
        for read in &self.paginated_windows {
            if let Some(table_id) = read.table_id.as_ref() {
                dependencies.record_paginated_window(PaginatedWindowDependency {
                    table: read.table.clone(),
                    table_id: table_id.clone(),
                    filters: read.filters.clone(),
                    order: read.order.clone(),
                    start_sort_values: read.start_sort_values.clone(),
                    start_doc_id: read.start_doc_id.clone(),
                    end_sort_values: read.end_sort_values.clone(),
                    end_doc_id: read.end_doc_id.clone(),
                    result_count: read.result_count,
                    page_size: read.page_size,
                });
            } else {
                dependencies.record_missing_predicate(&read.table, read.filters.clone());
            }
        }
        dependencies
    }

    pub fn record_table(&mut self, table: &TableName, table_id: Option<&TableId>) {
        self.tables.insert(RuntimeTableRead {
            table: table.clone(),
            table_id: table_id.cloned(),
        });
    }

    pub fn record_document(
        &mut self,
        table: &TableName,
        table_id: Option<&TableId>,
        document_id: &DocumentId,
    ) {
        self.documents.insert(RuntimeDocumentRead {
            table: table.clone(),
            table_id: table_id.cloned(),
            document_id: document_id.clone(),
        });
    }

    pub fn record_index_range(&mut self, read: RuntimeIndexRangeRead) {
        if !self.index_ranges.iter().any(|existing| existing == &read) {
            self.index_ranges.push(read);
        }
    }

    pub fn record_predicate(
        &mut self,
        table: &TableName,
        table_id: Option<&TableId>,
        filters: &[Filter],
    ) {
        if filters.is_empty() {
            return;
        }

        let read = RuntimePredicateRead {
            table: table.clone(),
            table_id: table_id.cloned(),
            filters: filters.to_vec(),
        };
        if !self.predicates.iter().any(|existing| existing == &read) {
            self.predicates.push(read);
        }
    }

    pub fn record_paginated_window(
        &mut self,
        query: &Query,
        table_id: Option<&TableId>,
        page_size: usize,
        after: Option<&Cursor>,
        page: &nimbus_core::Page,
    ) {
        let (start_sort_values, start_doc_id) = after
            .and_then(decode_runtime_cursor_boundary)
            .map_or((Vec::new(), None), |(sort_values, doc_id)| {
                (sort_values, Some(doc_id))
            });
        let (end_sort_values, end_doc_id) = page
            .data
            .last()
            .and_then(|value| extract_runtime_cursor_boundary(query.order.as_ref(), value))
            .map_or((Vec::new(), None), |(sort_values, doc_id)| {
                (sort_values, Some(doc_id))
            });
        let read = RuntimePaginatedWindowRead {
            table: query.table.clone(),
            table_id: table_id.cloned(),
            filters: query.filters.clone(),
            order: query.order.clone(),
            start_sort_values,
            start_doc_id,
            end_sort_values,
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
        let mut tables = self
            .tables
            .iter()
            .map(|read| read.table.clone())
            .collect::<HashSet<_>>();
        for read in &self.documents {
            tables.insert(read.table.clone());
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct RuntimeTableRead {
    pub(super) table: TableName,
    pub(super) table_id: Option<TableId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct RuntimeDocumentRead {
    pub(super) table: TableName,
    pub(super) table_id: Option<TableId>,
    pub(super) document_id: DocumentId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RuntimeIndexRangeRead {
    pub table: TableName,
    pub table_id: Option<TableId>,
    pub index_id: Option<IndexId>,
    pub index_name: String,
    pub field: String,
    pub start: Option<Value>,
    pub end: Option<Value>,
    pub start_inclusive: bool,
    pub end_inclusive: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RuntimePredicateRead {
    pub(super) table: TableName,
    pub(super) table_id: Option<TableId>,
    pub(super) filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct RuntimePaginatedWindowRead {
    pub(super) table: TableName,
    pub(super) table_id: Option<TableId>,
    pub(super) filters: Vec<Filter>,
    pub(super) order: Option<OrderBy>,
    pub(super) start_sort_values: Vec<Option<Value>>,
    pub(super) start_doc_id: Option<DocumentId>,
    pub(super) end_sort_values: Vec<Option<Value>>,
    pub(super) end_doc_id: Option<DocumentId>,
    pub(super) result_count: usize,
    pub(super) page_size: usize,
}
