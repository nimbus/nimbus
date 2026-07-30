use std::ops::Bound;

use nimbus_core::{Error, FieldType, Result, TableSchema};
use serde_json::Value;
use tokio_postgres::types::ToSql;

use crate::store::MAX_DURABLE_JOURNAL_STREAM_LIMIT;

// Dialect-independent document predicates live once in `crate::sql::predicate`;
// the PostgreSQL module re-exports them so existing call sites stay unchanged.
pub(super) use crate::sql::predicate::{
    filter_documents_with_predicate, filter_index_documents_with_cancel,
    index_fields_for_table_schema, validate_index_prefix_len, validate_index_range_prefix,
};

use super::backend::postgres_json_extract_expr;

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
    start: Bound<T>,
    end: Bound<T>,
) where
    T: ToSql + Sync + Send + 'static,
{
    match start {
        Bound::Included(start) => {
            clauses.push(format!("{expr} >= ${}", params.len() + 1));
            params.push(Box::new(start));
        }
        Bound::Excluded(start) => {
            clauses.push(format!("{expr} > ${}", params.len() + 1));
            params.push(Box::new(start));
        }
        Bound::Unbounded => {}
    }
    match end {
        Bound::Included(end) => {
            clauses.push(format!("{expr} <= ${}", params.len() + 1));
            params.push(Box::new(end));
        }
        Bound::Excluded(end) => {
            clauses.push(format!("{expr} < ${}", params.len() + 1));
            params.push(Box::new(end));
        }
        Bound::Unbounded => {}
    }
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
