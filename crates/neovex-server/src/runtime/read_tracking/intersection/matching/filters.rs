use neovex_core::{Document, Error, Filter, FilterOp};
use serde_json::Value;

use super::super::super::read_set::{RuntimeIndexRangeRead, RuntimePredicateRead};

pub(in crate::runtime::read_tracking) fn filters_from_runtime_index_read(
    read: &RuntimeIndexRangeRead,
) -> Vec<Filter> {
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

pub(in crate::runtime::read_tracking::intersection) fn document_matches_predicate_read(
    document: &Document,
    read: &RuntimePredicateRead,
) -> bool {
    filters_match_document(document, &read.filters).unwrap_or(true)
}

pub(super) fn filters_match_document(
    document: &Document,
    filters: &[Filter],
) -> Result<bool, Error> {
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

pub(super) fn compare_filter_values(
    left: &Value,
    right: &Value,
) -> Result<std::cmp::Ordering, Error> {
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
