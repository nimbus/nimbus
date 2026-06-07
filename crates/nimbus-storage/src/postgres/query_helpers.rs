use std::cmp::Ordering;

use nimbus_core::{Document, Error, FieldType, Filter, FilterOp, Result, TableName, TableSchema};
use serde_json::Value;
use tokio_postgres::types::ToSql;

use crate::store::MAX_DURABLE_JOURNAL_STREAM_LIMIT;

use super::backend::postgres_json_extract_expr;

pub(super) fn matches_filters(document: &Document, filters: &[Filter]) -> Result<bool> {
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

pub(super) fn filter_documents_with_predicate<F>(
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

pub(super) fn compare_values(left: &Value, right: &Value) -> Result<Ordering> {
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

pub(super) fn document_matches_exact_prefix(
    document: &Document,
    index_fields: &[String],
    exact_prefix: &[Value],
) -> bool {
    index_fields
        .iter()
        .zip(exact_prefix.iter())
        .all(|(field, value)| document.get_field(field) == Some(value))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn filter_index_documents_with_cancel(
    documents: Vec<Document>,
    table: &TableName,
    index_fields: &[String],
    exact_prefix: &[Value],
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
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
            && !document_matches_range_bounds(
                &document,
                range_field,
                start,
                end,
                start_inclusive,
                end_inclusive,
            )?
        {
            continue;
        }
        filtered.push(document);
    }
    Ok(filtered)
}

pub(super) fn index_fields_for_table_schema(
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

pub(super) fn field_type_for_table_schema(
    table_schema: &TableSchema,
    field_name: &str,
) -> Result<FieldType> {
    table_schema
        .fields
        .iter()
        .find(|field| field.name == field_name)
        .map(|field| field.field_type)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "indexed field '{}' not found for table '{}'",
                field_name,
                table_schema.table.as_str()
            ))
        })
}

pub(super) fn postgres_index_text_value(value: &Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        _ => Err(Error::InvalidInput(
            "indexed values must be string, number, or boolean scalars".to_string(),
        )),
    }
}

pub(super) fn postgres_numeric_value(value: &Value) -> Result<f64> {
    value
        .as_f64()
        .ok_or_else(|| Error::InvalidInput("numeric indexed value expected".to_string()))
}

pub(super) fn postgres_numeric_extract_expr(field: &str) -> String {
    format!(
        "CAST({} AS DOUBLE PRECISION)",
        postgres_json_extract_expr(field)
    )
}

pub(super) fn append_postgres_range_clause<T>(
    clauses: &mut Vec<String>,
    params: &mut Vec<Box<dyn ToSql + Sync + Send>>,
    expr: String,
    start: Option<T>,
    end: Option<T>,
    start_inclusive: bool,
    end_inclusive: bool,
) where
    T: ToSql + Sync + Send + 'static,
{
    if let Some(start) = start {
        let operator = if start_inclusive { ">=" } else { ">" };
        clauses.push(format!("{expr} {operator} ${}", params.len() + 1));
        params.push(Box::new(start));
    }
    if let Some(end) = end {
        let operator = if end_inclusive { "<=" } else { "<" };
        clauses.push(format!("{expr} {operator} ${}", params.len() + 1));
        params.push(Box::new(end));
    }
}

pub(super) fn document_matches_range_bounds(
    document: &Document,
    field: &str,
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<bool> {
    let Some(value) = document.get_field(field) else {
        return Ok(false);
    };

    if let Some(start) = start {
        let ordering = compare_values(value, start)?;
        let passes = if start_inclusive {
            matches!(ordering, Ordering::Greater | Ordering::Equal)
        } else {
            ordering == Ordering::Greater
        };
        if !passes {
            return Ok(false);
        }
    }

    if let Some(end) = end {
        let ordering = compare_values(value, end)?;
        let passes = if end_inclusive {
            matches!(ordering, Ordering::Less | Ordering::Equal)
        } else {
            ordering == Ordering::Less
        };
        if !passes {
            return Ok(false);
        }
    }

    Ok(true)
}

pub(super) fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(Error::InvalidInput(
            "journal stream limit must be greater than zero".to_string(),
        ));
    }
    if limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "journal stream limit {limit} exceeds the maximum {}",
            MAX_DURABLE_JOURNAL_STREAM_LIMIT
        )));
    }
    Ok(())
}
