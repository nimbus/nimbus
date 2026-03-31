use std::collections::HashSet;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use neovex_core::{
    CommitEntry, Cursor, Document, DocumentId, Error, Filter, FilterOp, OrderBy, OrderDirection,
    Query, TableName, TenantId, WriteOpType,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub(crate) struct ConvexRuntimeReadSet {
    tables: HashSet<TableName>,
    documents: HashSet<(TableName, DocumentId)>,
    index_ranges: Vec<ConvexRuntimeIndexRangeRead>,
    predicates: Vec<ConvexRuntimePredicateRead>,
    paginated_windows: Vec<ConvexRuntimePaginatedWindowRead>,
}

impl ConvexRuntimeReadSet {
    pub(crate) fn record_table(&mut self, table: &TableName) {
        self.tables.insert(table.clone());
    }

    pub(crate) fn record_document(&mut self, table: &TableName, document_id: &DocumentId) {
        self.documents.insert((table.clone(), *document_id));
    }

    pub(crate) fn record_index_range(&mut self, read: ConvexRuntimeIndexRangeRead) {
        if !self.index_ranges.iter().any(|existing| existing == &read) {
            self.index_ranges.push(read);
        }
    }

    pub(crate) fn record_predicate(&mut self, table: &TableName, filters: &[Filter]) {
        if filters.is_empty() {
            return;
        }

        let read = ConvexRuntimePredicateRead {
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
        let read = ConvexRuntimePaginatedWindowRead {
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
pub(crate) struct ConvexRuntimeIndexRangeRead {
    pub(crate) table: TableName,
    pub(crate) index_name: String,
    pub(crate) field: String,
    pub(crate) start: Option<Value>,
    pub(crate) end: Option<Value>,
    pub(crate) start_inclusive: bool,
    pub(crate) end_inclusive: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct ConvexRuntimePredicateRead {
    table: TableName,
    filters: Vec<Filter>,
}

#[derive(Debug, Clone, PartialEq)]
struct ConvexRuntimePaginatedWindowRead {
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
    read_set: &ConvexRuntimeReadSet,
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
    read_set: &ConvexRuntimeReadSet,
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

pub(crate) fn commit_intersects_runtime_read_set(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    commit: &CommitEntry,
    read_set: &ConvexRuntimeReadSet,
    deleted_documents: &[Document],
) -> bool {
    commit.writes.iter().any(|write| {
        write_intersects_runtime_read_set(service, tenant_id, write, read_set, deleted_documents)
    })
}

fn write_intersects_runtime_read_set(
    service: &neovex_engine::Service,
    tenant_id: &TenantId,
    write: &neovex_core::WriteOp,
    read_set: &ConvexRuntimeReadSet,
    deleted_documents: &[Document],
) -> bool {
    if read_set.tables.contains(&write.table) {
        return true;
    }

    if read_set
        .documents
        .contains(&(write.table.clone(), write.doc_id))
    {
        return true;
    }

    let relevant_predicates = read_set
        .predicates
        .iter()
        .filter(|read| read.table == write.table)
        .collect::<Vec<_>>();
    let relevant_paginated_windows = read_set
        .paginated_windows
        .iter()
        .filter(|read| read.table == write.table)
        .collect::<Vec<_>>();
    let mut relevant_index_reads = read_set
        .index_ranges
        .iter()
        .filter(|read| read.table == write.table);

    match write.op_type {
        WriteOpType::Delete => {
            if let Some(document) =
                deleted_document_snapshot(&write.table, &write.doc_id, deleted_documents)
            {
                if relevant_paginated_windows
                    .iter()
                    .any(|read| document_may_affect_paginated_window(document, read))
                {
                    return true;
                }
                if relevant_predicates
                    .iter()
                    .any(|read| document_matches_predicate_read(document, read))
                {
                    return true;
                }
                return relevant_index_reads.any(|read| {
                    document_matches_index_read(document.get_field(&read.field), read)
                });
            }

            !relevant_predicates.is_empty()
                || !relevant_paginated_windows.is_empty()
                || relevant_index_reads.count() > 0
        }
        WriteOpType::Insert | WriteOpType::Update => {
            let Ok(document) = service.get_document(tenant_id, &write.table, write.doc_id) else {
                return true;
            };
            if relevant_paginated_windows
                .iter()
                .any(|read| document_may_affect_paginated_window(&document, read))
            {
                return true;
            }
            if relevant_predicates
                .iter()
                .any(|read| document_matches_predicate_read(&document, read))
            {
                return true;
            }
            relevant_index_reads
                .any(|read| document_matches_index_read(document.get_field(&read.field), read))
        }
    }
}

fn filters_from_runtime_index_read(read: &ConvexRuntimeIndexRangeRead) -> Vec<Filter> {
    let mut filters = Vec::new();
    if let Some(start) = read.start.clone() {
        filters.push(Filter {
            field: read.field.clone(),
            op: if read.start_inclusive {
                FilterOp::Gte
            } else {
                FilterOp::Gt
            },
            value: start,
        });
    }
    if let Some(end) = read.end.clone() {
        filters.push(Filter {
            field: read.field.clone(),
            op: if read.end_inclusive {
                FilterOp::Lte
            } else {
                FilterOp::Lt
            },
            value: end,
        });
    }
    filters
}

fn deleted_document_snapshot<'a>(
    table: &TableName,
    document_id: &DocumentId,
    deleted_documents: &'a [Document],
) -> Option<&'a Document> {
    deleted_documents
        .iter()
        .find(|document| &document.table == table && document.id == *document_id)
}

fn document_matches_predicate_read(document: &Document, read: &ConvexRuntimePredicateRead) -> bool {
    filters_match_document(document, &read.filters).unwrap_or(true)
}

fn document_matches_index_read(value: Option<&Value>, read: &ConvexRuntimeIndexRangeRead) -> bool {
    let Some(value) = value else {
        return false;
    };
    value_matches_bounds(value, read)
}

fn document_may_affect_paginated_window(
    document: &Document,
    read: &ConvexRuntimePaginatedWindowRead,
) -> bool {
    if !filters_match_document(document, &read.filters).unwrap_or(true) {
        return false;
    }

    if let Some(start_doc_id) = read.start_doc_id.as_ref() {
        match compare_document_to_runtime_boundary(
            document,
            read.order.as_ref(),
            read.start_sort_value.as_ref(),
            start_doc_id,
        ) {
            Ok(std::cmp::Ordering::Greater) => {}
            Ok(_) => return false,
            Err(_) => return true,
        }
    }

    if read.result_count >= read.page_size
        && let Some(end_doc_id) = read.end_doc_id.as_ref()
    {
        match compare_document_to_runtime_boundary(
            document,
            read.order.as_ref(),
            read.end_sort_value.as_ref(),
            end_doc_id,
        ) {
            Ok(std::cmp::Ordering::Greater) => return false,
            Ok(_) => {}
            Err(_) => return true,
        }
    }

    true
}

fn filters_match_document(document: &Document, filters: &[Filter]) -> Result<bool, Error> {
    for filter in filters {
        let Some(field_value) = document.get_field(&filter.field) else {
            return Ok(false);
        };
        let matched = match filter.op {
            FilterOp::Eq => field_value == &filter.value,
            FilterOp::Neq => field_value != &filter.value,
            FilterOp::Gt => {
                compare_filter_values(field_value, &filter.value)? == std::cmp::Ordering::Greater
            }
            FilterOp::Gte => matches!(
                compare_filter_values(field_value, &filter.value)?,
                std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
            ),
            FilterOp::Lt => {
                compare_filter_values(field_value, &filter.value)? == std::cmp::Ordering::Less
            }
            FilterOp::Lte => matches!(
                compare_filter_values(field_value, &filter.value)?,
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            ),
        };

        if !matched {
            return Ok(false);
        }
    }

    Ok(true)
}

fn compare_filter_values(left: &Value, right: &Value) -> Result<std::cmp::Ordering, Error> {
    match (left, right) {
        (Value::String(left), Value::String(right)) => Ok(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => {
            let left = left
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            let right = right
                .as_f64()
                .ok_or_else(|| Error::InvalidInput("unsupported numeric comparison".to_string()))?;
            left.partial_cmp(&right).ok_or_else(|| {
                Error::InvalidInput("invalid numeric ordering comparison".to_string())
            })
        }
        _ => Err(Error::InvalidInput(
            "comparisons only support string and number fields in phase 1".to_string(),
        )),
    }
}

#[derive(Debug, Deserialize)]
struct ConvexRuntimeCursorBoundaryPayload {
    sort_value: Option<Value>,
    doc_id: String,
}

fn decode_runtime_cursor_boundary(cursor: &Cursor) -> Option<(Option<Value>, DocumentId)> {
    let bytes = URL_SAFE_NO_PAD.decode(&cursor.0).ok()?;
    let payload: ConvexRuntimeCursorBoundaryPayload = serde_json::from_slice(&bytes).ok()?;
    let document_id = payload.doc_id.parse().ok()?;
    Some((payload.sort_value, document_id))
}

fn extract_runtime_cursor_boundary(
    order: Option<&OrderBy>,
    value: &Value,
) -> Option<(Option<Value>, DocumentId)> {
    let Value::Object(object) = value else {
        return None;
    };
    let document_id = object
        .get("_id")
        .and_then(Value::as_str)
        .and_then(|value| value.parse().ok())?;
    let sort_value = order.and_then(|order| object.get(&order.field).cloned());
    Some((sort_value, document_id))
}

fn compare_runtime_order_field(
    left: Option<&Value>,
    right: Option<&Value>,
) -> Result<std::cmp::Ordering, Error> {
    match (left, right) {
        (Some(left), Some(right)) => compare_filter_values(left, right),
        (Some(_), None) => Ok(std::cmp::Ordering::Less),
        (None, Some(_)) => Ok(std::cmp::Ordering::Greater),
        (None, None) => Ok(std::cmp::Ordering::Equal),
    }
}

fn compare_document_to_runtime_boundary(
    document: &Document,
    order: Option<&OrderBy>,
    boundary_sort_value: Option<&Value>,
    boundary_doc_id: &DocumentId,
) -> Result<std::cmp::Ordering, Error> {
    let ordering = match order {
        Some(order) => {
            let ordering =
                compare_runtime_order_field(document.get_field(&order.field), boundary_sort_value)?;
            match order.direction {
                OrderDirection::Asc => ordering,
                OrderDirection::Desc => ordering.reverse(),
            }
        }
        None => std::cmp::Ordering::Equal,
    };

    Ok(ordering.then_with(|| document.id.cmp(boundary_doc_id)))
}

fn value_matches_bounds(value: &Value, read: &ConvexRuntimeIndexRangeRead) -> bool {
    if let Some(start) = read.start.as_ref() {
        let Some(ordering) = compare_index_values(value, start) else {
            return true;
        };
        if ordering == std::cmp::Ordering::Less
            || (ordering == std::cmp::Ordering::Equal && !read.start_inclusive)
        {
            return false;
        }
    }

    if let Some(end) = read.end.as_ref() {
        let Some(ordering) = compare_index_values(value, end) else {
            return true;
        };
        if ordering == std::cmp::Ordering::Greater
            || (ordering == std::cmp::Ordering::Equal && !read.end_inclusive)
        {
            return false;
        }
    }

    true
}

fn compare_index_values(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        (Value::Number(left), Value::Number(right)) => left
            .as_f64()
            .zip(right.as_f64())
            .and_then(|(left, right)| left.partial_cmp(&right)),
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn synthesize_runtime_subscription_base_queries_keeps_disjoint_same_table_predicates() {
        let table = TableName::new("messages").expect("table should be valid");
        let mut read_set = ConvexRuntimeReadSet::default();
        read_set.record_predicate(
            &table,
            &[Filter {
                field: "author".to_string(),
                op: FilterOp::Eq,
                value: Value::String("Ada".to_string()),
            }],
        );
        read_set.record_predicate(
            &table,
            &[Filter {
                field: "author".to_string(),
                op: FilterOp::Eq,
                value: Value::String("Bob".to_string()),
            }],
        );

        let queries = synthesize_runtime_subscription_base_queries(&read_set)
            .expect("queries should synthesize");

        assert_eq!(queries.len(), 2);
        assert!(queries.iter().all(|query| query.table == table));
        assert!(queries.iter().any(|query| query.filters
            == vec![Filter {
                field: "author".to_string(),
                op: FilterOp::Eq,
                value: Value::String("Ada".to_string()),
            }]));
        assert!(queries.iter().any(|query| query.filters
            == vec![Filter {
                field: "author".to_string(),
                op: FilterOp::Eq,
                value: Value::String("Bob".to_string()),
            }]));
    }

    #[test]
    fn synthesize_runtime_subscription_base_queries_prefers_broad_query_for_full_table_reads() {
        let table = TableName::new("messages").expect("table should be valid");
        let mut read_set = ConvexRuntimeReadSet::default();
        read_set.record_table(&table);
        read_set.record_predicate(
            &table,
            &[Filter {
                field: "author".to_string(),
                op: FilterOp::Eq,
                value: Value::String("Ada".to_string()),
            }],
        );

        let queries = synthesize_runtime_subscription_base_queries(&read_set)
            .expect("queries should synthesize");

        assert_eq!(
            queries,
            vec![Query {
                table,
                filters: Vec::new(),
                order: None,
                limit: None,
            }]
        );
    }
}
