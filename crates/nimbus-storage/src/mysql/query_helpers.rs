use std::fmt::Write as _;

use mysql_async::Value as MySqlValue;
use nimbus_core::{
    Document, DocumentId, Error, FieldType, Filter, FilterOp, Result, TableName, TableSchema,
    Timestamp,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::store::MAX_DURABLE_JOURNAL_STREAM_LIMIT;

use super::backend::quote_identifier;

pub(super) fn serialize_json<T>(value: &T) -> Result<String>
where
    T: serde::Serialize,
{
    serde_json::to_string(value).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn serialize_document_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.fields).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn deserialize_json<T>(json: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(json).map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn matches_filters(document: &Document, filters: &[Filter]) -> Result<bool> {
    for filter in filters {
        let Some(field_value) = document.get_field(&filter.field) else {
            return Ok(false);
        };
        let matched = match filter.op {
            FilterOp::Eq => field_value == &filter.value,
            FilterOp::Neq => field_value != &filter.value,
            FilterOp::Gt => {
                compare_values(field_value, &filter.value)? == std::cmp::Ordering::Greater
            }
            FilterOp::Gte => matches!(
                compare_values(field_value, &filter.value)?,
                std::cmp::Ordering::Greater | std::cmp::Ordering::Equal
            ),
            FilterOp::Lt => compare_values(field_value, &filter.value)? == std::cmp::Ordering::Less,
            FilterOp::Lte => matches!(
                compare_values(field_value, &filter.value)?,
                std::cmp::Ordering::Less | std::cmp::Ordering::Equal
            ),
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

pub(super) fn compare_values(left: &Value, right: &Value) -> Result<std::cmp::Ordering> {
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
    let index = table_schema
        .queryable_indexes()
        .find(|index| index.name == index_name)
        .ok_or_else(|| {
            Error::InvalidInput(format!(
                "index '{}' not found for table '{}'",
                index_name,
                table_schema.table.as_str()
            ))
        })?;
    Ok(index.fields.clone())
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
                "field '{}' not found in schema for table '{}'",
                field_name,
                table_schema.table.as_str()
            ))
        })
}

pub(super) fn mysql_index_text_value(value: &Value) -> Result<MySqlValue> {
    match value {
        Value::String(value) => Ok(MySqlValue::Bytes(value.as_bytes().to_vec())),
        Value::Number(number) => Ok(MySqlValue::Bytes(number.to_string().into_bytes())),
        _ => Err(Error::InvalidInput(
            "index equality and prefix scans only support string and number values".to_string(),
        )),
    }
}

pub(super) fn mysql_numeric_value(value: &Value) -> Result<MySqlValue> {
    let number = value.as_f64().ok_or_else(|| {
        Error::InvalidInput("numeric range bounds require number values".to_string())
    })?;
    Ok(MySqlValue::Double(number))
}

pub(super) fn mysql_numeric_column_expr(table: &TableName, field: &str) -> String {
    format!(
        "CAST({} AS DOUBLE)",
        quote_identifier(&mysql_generated_column_name(table, field))
    )
}

pub(super) fn append_mysql_range_clause(
    clauses: &mut Vec<String>,
    params: &mut Vec<MySqlValue>,
    expr: String,
    start: Option<MySqlValue>,
    end: Option<MySqlValue>,
    start_inclusive: bool,
    end_inclusive: bool,
) {
    if let Some(start) = start {
        let operator = if start_inclusive { ">=" } else { ">" };
        clauses.push(format!("{expr} {operator} ?"));
        params.push(start);
    }
    if let Some(end) = end {
        let operator = if end_inclusive { "<=" } else { "<" };
        clauses.push(format!("{expr} {operator} ?"));
        params.push(end);
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
        .all(|(field, expected)| document.get_field(field) == Some(expected))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn document_matches_range_bounds(
    document: &Document,
    field: &str,
    start: Option<&Value>,
    end: Option<&Value>,
    start_inclusive: bool,
    end_inclusive: bool,
) -> Result<bool> {
    if let Some(start) = start {
        let Some(value) = document.get_field(field) else {
            return Ok(false);
        };
        let ordering = compare_values(value, start)?;
        if start_inclusive {
            if ordering == std::cmp::Ordering::Less {
                return Ok(false);
            }
        } else if !matches!(ordering, std::cmp::Ordering::Greater) {
            return Ok(false);
        }
    }
    if let Some(end) = end {
        let Some(value) = document.get_field(field) else {
            return Ok(false);
        };
        let ordering = compare_values(value, end)?;
        if end_inclusive {
            if ordering == std::cmp::Ordering::Greater {
                return Ok(false);
            }
        } else if !matches!(ordering, std::cmp::Ordering::Less) {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn validate_durable_journal_stream_limit(limit: usize) -> Result<()> {
    if limit == 0 {
        return Err(Error::InvalidInput(
            "durable journal stream limit must be greater than zero".to_string(),
        ));
    }
    if limit > MAX_DURABLE_JOURNAL_STREAM_LIMIT {
        return Err(Error::InvalidInput(format!(
            "durable journal stream limit {limit} exceeds maximum {MAX_DURABLE_JOURNAL_STREAM_LIMIT}"
        )));
    }
    Ok(())
}

pub(super) fn row_to_document(
    table: &TableName,
    id: &DocumentId,
    creation_time: u64,
    update_time: u64,
    data_json: String,
    typed_fields_json: String,
) -> Result<Document> {
    Ok(Document {
        id: id.clone(),
        table: table.clone(),
        creation_time: Timestamp(creation_time),
        update_time: Timestamp(update_time),
        fields: serde_json::from_str(&data_json)
            .map_err(|error| Error::Serialization(error.to_string()))?,
        typed_fields: serde_json::from_str(&typed_fields_json)
            .map_err(|error| Error::Serialization(error.to_string()))?,
    })
}

pub(super) fn serialize_document_typed_fields(document: &Document) -> Result<String> {
    serde_json::to_string(&document.typed_fields)
        .map_err(|error| Error::Serialization(error.to_string()))
}

pub(super) fn claim_due_jobs_upper_bound(timestamp: Timestamp) -> u64 {
    timestamp.0
}

pub(super) fn mysql_index_name(index_id: &nimbus_core::IndexId) -> String {
    let digest = Sha256::digest(index_id.as_str().as_bytes());
    let mut suffix = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("idx_{suffix}")
}

pub(super) fn mysql_generated_column_name(table: &TableName, field: &str) -> String {
    let digest = Sha256::digest(format!("{}:{field}", table.as_str()).as_bytes());
    let mut suffix = String::with_capacity(24);
    for byte in digest.iter().take(12) {
        let _ = write!(&mut suffix, "{byte:02x}");
    }
    format!("gcol_{suffix}")
}

pub(super) fn unique_index_fields(table_schema: &TableSchema) -> Vec<&str> {
    let mut fields = Vec::new();
    for index in &table_schema.indexes {
        for field in &index.fields {
            if !fields.contains(&field.as_str()) {
                fields.push(field.as_str());
            }
        }
    }
    fields
}

pub(super) fn mysql_generated_column_expr(field: &str) -> String {
    format!(
        "JSON_UNQUOTE(JSON_EXTRACT(data_json, '$.\"{}\"'))",
        field.replace('\\', "\\\\").replace('"', "\\\"")
    )
}
