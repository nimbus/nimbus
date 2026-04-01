use neovex_core::{Document, DocumentId, Error, OrderBy, OrderDirection};
use serde_json::Value;

use super::super::super::read_set::{RuntimeIndexRangeRead, RuntimePaginatedWindowRead};
use super::filters::{compare_filter_values, filters_match_document};

pub(in crate::runtime::read_tracking::intersection) fn document_matches_index_read(
    value: Option<&Value>,
    read: &RuntimeIndexRangeRead,
) -> bool {
    let Some(value) = value else {
        return false;
    };
    value_matches_bounds(value, read)
}

pub(in crate::runtime::read_tracking::intersection) fn document_may_affect_paginated_window(
    document: &Document,
    read: &RuntimePaginatedWindowRead,
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

fn value_matches_bounds(value: &Value, read: &RuntimeIndexRangeRead) -> bool {
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
