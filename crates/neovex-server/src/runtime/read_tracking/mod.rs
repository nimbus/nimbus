use std::collections::HashSet;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use neovex_core::{
    CommitEntry, Cursor, Document, DocumentId, Error, Filter, FilterOp, OrderBy, OrderDirection,
    Query, TableName, TenantId, WriteOpType,
};
use serde::Deserialize;
use serde_json::Value;

mod intersection;
#[cfg(test)]
mod tests;

use self::intersection::{
    decode_runtime_cursor_boundary, extract_runtime_cursor_boundary,
    filters_from_runtime_index_read,
};
pub(crate) use intersection::commit_intersects_runtime_read_set;

#[derive(Debug, Clone, Default)]
pub(crate) struct RuntimeReadSet {
    tables: HashSet<TableName>,
    documents: HashSet<(TableName, DocumentId)>,
    index_ranges: Vec<RuntimeIndexRangeRead>,
    predicates: Vec<RuntimePredicateRead>,
    paginated_windows: Vec<RuntimePaginatedWindowRead>,
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

    fn tables(&self) -> HashSet<TableName> {
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
struct RuntimePredicateRead {
    table: TableName,
    filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq)]
struct RuntimePaginatedWindowRead {
    table: TableName,
    filters: Vec<Filter>,
    order: Option<OrderBy>,
    start_sort_value: Option<Value>,
    start_doc_id: Option<DocumentId>,
    end_sort_value: Option<Value>,
    end_doc_id: Option<DocumentId>,
    result_count: usize,
    page_size: usize,
}

pub(crate) fn synthesize_runtime_subscription_base_queries(
    read_set: &RuntimeReadSet,
) -> Result<Vec<Query>, Error> {
    let mut tables = read_set.tables().into_iter().collect::<Vec<_>>();
    tables.sort();

    if tables.is_empty() {
        return Err(Error::InvalidInput(
            "runtime-backed live subscriptions require at least one table-backed read".to_string(),
        ));
    }

    let mut queries = Vec::new();
    for table in tables {
        for query in synthesize_runtime_subscription_base_queries_for_table(read_set, &table) {
            if !queries.contains(&query) {
                queries.push(query);
            }
        }
    }

    Ok(queries)
}

fn synthesize_runtime_subscription_base_queries_for_table(
    read_set: &RuntimeReadSet,
    table: &TableName,
) -> Vec<Query> {
    if read_set.tables.contains(table) {
        return vec![broad_runtime_subscription_query(table.clone())];
    }

    let predicates = read_set
        .predicates
        .iter()
        .filter(|predicate| &predicate.table == table)
        .collect::<Vec<_>>();
    let index_ranges = read_set
        .index_ranges
        .iter()
        .filter(|range| &range.table == table)
        .collect::<Vec<_>>();
    let paginated_windows = read_set
        .paginated_windows
        .iter()
        .filter(|read| &read.table == table)
        .collect::<Vec<_>>();

    let mut queries = Vec::new();

    for predicate in predicates {
        queries.push(Query {
            table: table.clone(),
            filters: predicate.filters.clone(),
            order: None,
            limit: None,
        });
    }

    for index_range in index_ranges {
        queries.push(Query {
            table: table.clone(),
            filters: filters_from_runtime_index_read(index_range),
            order: None,
            limit: None,
        });
    }

    for paginated_window in paginated_windows {
        queries.push(Query {
            table: table.clone(),
            filters: paginated_window.filters.clone(),
            order: None,
            limit: None,
        });
    }

    if queries.is_empty()
        && read_set
            .documents
            .iter()
            .any(|(document_table, _)| document_table == table)
    {
        queries.push(broad_runtime_subscription_query(table.clone()));
    }

    if queries.is_empty() {
        queries.push(broad_runtime_subscription_query(table.clone()));
    }

    queries
}

fn broad_runtime_subscription_query(table: TableName) -> Query {
    Query {
        table,
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}
