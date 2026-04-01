use std::cmp::Ordering;

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use neovex_core::{
    Cursor, Document, DocumentId, Error, Filter, FilterOp, OrderBy, OrderDirection, Page,
    PaginatedQuery, Query, Result,
};
use neovex_storage::TenantStore;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Evaluates a query against a tenant store.
pub fn evaluate_query(store: &TenantStore, query: &Query) -> Result<Vec<Document>> {
    evaluate_query_cancellable(store, query, &mut || Ok(()))
}

/// Evaluates a query against a tenant store while checking for cancellation between rows.
pub fn evaluate_query_cancellable(
    store: &TenantStore,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let filtered =
        store.scan_table_matching_cancellable(&query.table, check_cancel, |document| {
            matches_filters(document, &query.filters)
        })?;
    finalize_query_documents(filtered, query, check_cancel)
}

/// Evaluates a query using preloaded documents instead of scanning the store.
pub fn evaluate_query_with_docs(documents: Vec<Document>, query: &Query) -> Result<Vec<Document>> {
    evaluate_query_with_docs_cancellable(documents, query, &mut || Ok(()))
}

/// Evaluates a query using preloaded documents while checking for cancellation between rows.
pub fn evaluate_query_with_docs_cancellable(
    documents: Vec<Document>,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let filtered = filter_documents_cancellable(documents, &query.filters, check_cancel)?;
    finalize_query_documents(filtered, query, check_cancel)
}

fn filter_documents_cancellable(
    documents: Vec<Document>,
    filters: &[Filter],
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let mut filtered = Vec::with_capacity(documents.len());
    for document in documents {
        check_cancel()?;
        if matches_filters(&document, filters)? {
            filtered.push(document);
        }
    }
    Ok(filtered)
}

fn finalize_query_documents(
    mut filtered: Vec<Document>,
    query: &Query,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    check_cancel()?;
    sort_documents(&mut filtered, query.order.as_ref())?;
    check_cancel()?;
    if let Some(limit) = query.limit {
        filtered.truncate(limit);
    }
    Ok(filtered)
}

#[derive(Debug, Serialize, Deserialize)]
struct CursorPayload {
    query_signature: String,
    sort_value: Option<Value>,
    doc_id: String,
}

/// Encodes a pagination cursor.
pub fn encode_cursor(
    sort_value: Option<&Value>,
    doc_id: &DocumentId,
    query: &Query,
) -> Result<Cursor> {
    let payload = CursorPayload {
        query_signature: query_signature(query)?,
        sort_value: sort_value.cloned(),
        doc_id: doc_id.to_string(),
    };
    let json = serde_json::to_vec(&payload)
        .map_err(|error| Error::Internal(format!("cursor should serialize: {error}")))?;
    Ok(Cursor(URL_SAFE_NO_PAD.encode(&json)))
}

/// Decodes a pagination cursor.
pub fn decode_cursor(cursor: &Cursor, query: &Query) -> Result<(Option<Value>, DocumentId)> {
    let bytes = URL_SAFE_NO_PAD
        .decode(&cursor.0)
        .map_err(|_| Error::InvalidInput("invalid cursor".to_string()))?;
    let payload: CursorPayload = serde_json::from_slice(&bytes)
        .map_err(|_| Error::InvalidInput("invalid cursor".to_string()))?;
    if payload.query_signature != query_signature(query)? {
        return Err(Error::InvalidInput("invalid cursor".to_string()));
    }
    let doc_id = payload
        .doc_id
        .parse::<DocumentId>()
        .map_err(|_| Error::InvalidInput("invalid cursor document id".to_string()))?;
    Ok((payload.sort_value, doc_id))
}

/// Evaluates a paginated query.
pub fn evaluate_paginated(store: &TenantStore, paginated: &PaginatedQuery) -> Result<Page> {
    evaluate_paginated_cancellable(store, paginated, &mut || Ok(()))
}

/// Evaluates a paginated query while checking for cancellation between rows.
pub fn evaluate_paginated_cancellable(
    store: &TenantStore,
    paginated: &PaginatedQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Page> {
    let filtered = store.scan_table_matching_cancellable(
        &paginated.query.table,
        check_cancel,
        |document| matches_filters(document, &paginated.query.filters),
    )?;
    evaluate_paginated_with_filtered_docs_cancellable(filtered, paginated, check_cancel)
}

/// Evaluates a paginated query using preloaded documents instead of scanning the store.
pub fn evaluate_paginated_with_docs(
    documents: Vec<Document>,
    paginated: &PaginatedQuery,
) -> Result<Page> {
    evaluate_paginated_with_docs_cancellable(documents, paginated, &mut || Ok(()))
}

/// Evaluates a paginated query using preloaded documents while checking for cancellation.
pub fn evaluate_paginated_with_docs_cancellable(
    documents: Vec<Document>,
    paginated: &PaginatedQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Page> {
    if paginated.page_size == 0 {
        return Err(Error::InvalidInput(
            "page_size must be greater than zero".to_string(),
        ));
    }

    let mut unbounded_query = paginated.query.clone();
    unbounded_query.limit = None;
    let filtered = filter_documents_cancellable(documents, &unbounded_query.filters, check_cancel)?;
    evaluate_paginated_with_filtered_docs_cancellable(filtered, paginated, check_cancel)
}

fn evaluate_paginated_with_filtered_docs_cancellable(
    mut filtered: Vec<Document>,
    paginated: &PaginatedQuery,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Page> {
    if paginated.page_size == 0 {
        return Err(Error::InvalidInput(
            "page_size must be greater than zero".to_string(),
        ));
    }

    let mut unbounded_query = paginated.query.clone();
    unbounded_query.limit = None;
    check_cancel()?;
    sort_documents(&mut filtered, unbounded_query.order.as_ref())?;

    let start_index = if let Some(cursor) = &paginated.after {
        let (cursor_sort_value, cursor_doc_id) = decode_cursor(cursor, &unbounded_query)?;
        let mut start = filtered.len();
        for (index, document) in filtered.iter().enumerate() {
            check_cancel()?;
            if compare_document_to_cursor(
                document,
                paginated.query.order.as_ref(),
                cursor_sort_value.as_ref(),
                &cursor_doc_id,
            )? == Ordering::Greater
            {
                start = index;
                break;
            }
        }
        start
    } else {
        0
    };

    let remaining = &filtered[start_index..];
    let window: Vec<_> = remaining.iter().take(paginated.page_size + 1).collect();
    let has_more = window.len() > paginated.page_size;
    let page_docs = window
        .into_iter()
        .take(paginated.page_size)
        .collect::<Vec<_>>();

    let next_cursor = if has_more {
        check_cancel()?;
        page_docs
            .last()
            .map(|document| {
                let sort_value = paginated
                    .query
                    .order
                    .as_ref()
                    .and_then(|order| document.get_field(&order.field));
                encode_cursor(sort_value, &document.id, &unbounded_query)
            })
            .transpose()?
    } else {
        None
    };

    check_cancel()?;
    Ok(Page {
        data: page_docs
            .into_iter()
            .map(|document| document.to_json())
            .collect(),
        next_cursor,
        has_more,
    })
}

fn query_signature(query: &Query) -> Result<String> {
    let mut normalized = query.clone();
    normalized.limit = None;
    let bytes = serde_json::to_vec(&normalized).map_err(|error| {
        Error::Internal(format!("query signature serialization failed: {error}"))
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub(crate) fn matches_filters(document: &Document, filters: &[Filter]) -> Result<bool> {
    for filter in filters {
        let Some(field_value) = document.get_field(&filter.field) else {
            return Ok(false);
        };
        let matched = match filter.op {
            FilterOp::Eq => field_value == &filter.value,
            FilterOp::Neq => field_value != &filter.value,
            FilterOp::Gt => compare_values(field_value, &filter.value)? == Ordering::Greater,
            FilterOp::Gte => {
                matches!(
                    compare_values(field_value, &filter.value)?,
                    Ordering::Greater | Ordering::Equal
                )
            }
            FilterOp::Lt => compare_values(field_value, &filter.value)? == Ordering::Less,
            FilterOp::Lte => {
                matches!(
                    compare_values(field_value, &filter.value)?,
                    Ordering::Less | Ordering::Equal
                )
            }
        };

        if !matched {
            return Ok(false);
        }
    }

    Ok(true)
}

fn sort_documents(documents: &mut [Document], order: Option<&OrderBy>) -> Result<()> {
    match order {
        Some(order) => {
            let field = order.field.clone();
            let direction = order.direction;
            validate_order_domain(documents.iter(), &field)?;
            documents.sort_by(|left, right| {
                let ordering = compare_order_field(left.get_field(&field), right.get_field(&field))
                    .expect("ordering inputs should be validated before sorting");
                let ordering = match direction {
                    OrderDirection::Asc => ordering,
                    OrderDirection::Desc => ordering.reverse(),
                };
                ordering.then_with(|| left.id.cmp(&right.id))
            });
        }
        None => {
            documents.sort_by(|left, right| left.id.cmp(&right.id));
        }
    }
    Ok(())
}

fn compare_order_field(left: Option<&Value>, right: Option<&Value>) -> Result<Ordering> {
    match (left, right) {
        (Some(left), Some(right)) => compare_values(left, right),
        (Some(_), None) => Ok(Ordering::Less),
        (None, Some(_)) => Ok(Ordering::Greater),
        (None, None) => Ok(Ordering::Equal),
    }
}

fn compare_document_to_cursor(
    document: &Document,
    order: Option<&OrderBy>,
    cursor_sort_value: Option<&Value>,
    cursor_doc_id: &DocumentId,
) -> Result<Ordering> {
    let ordering = match order {
        Some(order) => {
            let ordering =
                compare_order_field(document.get_field(&order.field), cursor_sort_value)?;
            match order.direction {
                OrderDirection::Asc => ordering,
                OrderDirection::Desc => ordering.reverse(),
            }
        }
        None => Ordering::Equal,
    };

    Ok(ordering.then_with(|| document.id.cmp(cursor_doc_id)))
}

fn validate_order_domain<'a>(
    documents: impl Iterator<Item = &'a Document>,
    field: &str,
) -> Result<()> {
    let mut observed_kind = None;
    for document in documents {
        let Some(value) = document.get_field(field) else {
            continue;
        };
        let kind = order_value_kind(value)?;
        if let Some(previous) = observed_kind {
            if previous != kind {
                return Err(Error::InvalidInput(
                    "ordering cannot mix string and number values in the same field".to_string(),
                ));
            }
        } else {
            observed_kind = Some(kind);
        }
    }
    Ok(())
}

fn order_value_kind(value: &Value) -> Result<OrderValueKind> {
    match value {
        Value::String(_) => Ok(OrderValueKind::String),
        Value::Number(number) if number.as_f64().is_some() => Ok(OrderValueKind::Number),
        _ => Err(Error::InvalidInput(
            "ordering only supports string and number fields in phase 1".to_string(),
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OrderValueKind {
    String,
    Number,
}

fn compare_values(left: &Value, right: &Value) -> Result<Ordering> {
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
