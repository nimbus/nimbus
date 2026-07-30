//! Dialect-independent document predicates shared by the SQL backends.
//!
//! Filter evaluation, value ordering, and index prefix/range matching operate on
//! already-materialized [`Document`]s, so they carry no dialect weight: no
//! placeholder style, no locking order, no type binding. PostgreSQL and MySQL
//! each used to carry a private copy in their own `query_helpers.rs`; the copies
//! had drifted only in formatting (`Ordering` imported vs. spelled out,
//! `matches!` vs. `==`), never in behavior.
//!
//! The two index-scan argument validators live here for the same reason: their
//! rejection messages are observable, and they were spelled out identically in
//! both stores and again in [`crate::sql::read_snapshot`].
//!
//! Two neighbours in those files stay per-backend on purpose because their
//! observable error text differs between the dialects, and this step is
//! behavior-preserving:
//!
//! - `field_type_for_table_schema` — PostgreSQL says `indexed field '{}' not
//!   found for table '{}'`, MySQL says `field '{}' not found in schema for
//!   table '{}'`.
//! - `validate_durable_journal_stream_limit` — PostgreSQL says `journal stream
//!   limit ...`, MySQL says `durable journal stream limit ...`.

use std::cmp::Ordering;
use std::ops::Bound;

use nimbus_core::{Document, Error, Filter, FilterOp, Result, TableName, TableSchema};
use serde_json::Value;

use crate::IndexRangeBound;

/// Rejects an index scan whose exact prefix is longer than the index itself.
///
/// Both SQL stores and the shared read snapshot enforce this rule, so the
/// message lives here rather than in three copies that can drift apart.
pub(crate) fn validate_index_prefix_len(
    index_name: &str,
    prefix_len: usize,
    index_field_count: usize,
) -> Result<()> {
    if prefix_len > index_field_count {
        return Err(Error::InvalidInput(format!(
            "index prefix length {prefix_len} exceeds index '{index_name}' field count {index_field_count}"
        )));
    }
    Ok(())
}

/// Rejects a bounded range scan whose exact prefix consumes every index field,
/// leaving no field for the range to constrain. A scan unbounded on both sides
/// is not a range scan and is exempt.
pub(crate) fn validate_index_range_prefix(
    index_name: &str,
    prefix_len: usize,
    index_field_count: usize,
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
) -> Result<()> {
    if (!matches!(start, Bound::Unbounded) || !matches!(end, Bound::Unbounded))
        && prefix_len >= index_field_count
    {
        return Err(Error::InvalidInput(format!(
            "composite range prefix length {prefix_len} leaves no range field for index '{index_name}'"
        )));
    }
    Ok(())
}

/// Evaluates every filter against `document`, short-circuiting on the first
/// mismatch. A document missing a filtered field never matches.
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

pub(crate) fn filter_documents_with_predicate<F>(
    documents: Vec<Document>,
    filters: &[Filter],
    check_cancel: &mut dyn FnMut() -> Result<()>,
    mut include_document: F,
) -> Result<Vec<Document>>
where
    F: FnMut(&Document) -> Result<bool>,
{
    let mut filtered = Vec::new();
    for document in documents {
        check_cancel()?;
        if matches_filters(&document, filters)? && include_document(&document)? {
            filtered.push(document);
        }
    }
    Ok(filtered)
}

/// Orders two JSON scalars. Only strings and numbers are comparable; anything
/// else is rejected rather than given an arbitrary order.
pub(crate) fn compare_values(left: &Value, right: &Value) -> Result<Ordering> {
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

/// Checks the equality-constrained leading fields of an index. `exact_prefix`
/// shorter than `index_fields` leaves the remaining fields unconstrained.
pub(crate) fn document_matches_exact_prefix(
    document: &Document,
    index_fields: &[String],
    exact_prefix: &[Value],
) -> bool {
    index_fields
        .iter()
        .zip(exact_prefix.iter())
        .all(|(field, value)| document.get_field(field) == Some(value))
}

pub(crate) fn filter_index_documents_with_cancel(
    documents: Vec<Document>,
    table: &TableName,
    index_fields: &[String],
    exact_prefix: &[Value],
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
    check_cancel: &mut dyn FnMut() -> Result<()>,
) -> Result<Vec<Document>> {
    let range_field = index_fields.get(exact_prefix.len());
    let mut filtered = Vec::new();
    for document in documents {
        check_cancel()?;
        if &document.table != table {
            continue;
        }
        if !document_matches_exact_prefix(&document, index_fields, exact_prefix) {
            continue;
        }
        if let Some(range_field) = range_field
            && !document_matches_range_bounds(&document, range_field, start, end)?
        {
            continue;
        }
        filtered.push(document);
    }
    Ok(filtered)
}

pub(crate) fn index_fields_for_table_schema(
    table_schema: &TableSchema,
    index_name: &str,
) -> Result<Vec<String>> {
    table_schema
        .queryable_indexes()
        .find(|index| index.name == index_name)
        .map(|index| index.fields.clone())
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "index '{}' not found for table '{}'",
                index_name,
                table_schema.table.as_str()
            ))
        })
}

/// Checks the single range-constrained field that follows an index's exact
/// prefix. A document missing that field never matches.
pub(crate) fn document_matches_range_bounds(
    document: &Document,
    field: &str,
    start: IndexRangeBound<'_>,
    end: IndexRangeBound<'_>,
) -> Result<bool> {
    let Some(value) = document.get_field(field) else {
        return Ok(false);
    };

    match start {
        Bound::Included(start) => {
            let ordering = compare_values(value, start)?;
            if matches!(ordering, Ordering::Less) {
                return Ok(false);
            }
        }
        Bound::Excluded(start) => {
            let ordering = compare_values(value, start)?;
            if !matches!(ordering, Ordering::Greater) {
                return Ok(false);
            }
        }
        Bound::Unbounded => {}
    }

    match end {
        Bound::Included(end) => {
            let ordering = compare_values(value, end)?;
            if matches!(ordering, Ordering::Greater) {
                return Ok(false);
            }
        }
        Bound::Excluded(end) => {
            let ordering = compare_values(value, end)?;
            if !matches!(ordering, Ordering::Less) {
                return Ok(false);
            }
        }
        Bound::Unbounded => {}
    }

    Ok(true)
}
