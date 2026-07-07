use std::fmt::Write as _;
use std::ops::Bound;

use mysql_async::Value as MySqlValue;
use nimbus_core::{
    Document, Error, FieldType, Filter, FilterOp, Result, TableName, TableSchema, Timestamp,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{IndexRangeBound, store::MAX_DURABLE_JOURNAL_STREAM_LIMIT};

// Dialect-independent row serialization lives once in `crate::sql::row`; the
// MySQL module re-exports it so existing call sites stay unchanged.
pub(super) use crate::sql::row::{
    deserialize_json, row_to_document, serialize_document_fields, serialize_document_typed_fields,
    serialize_json,
};

use super::backend::quote_identifier;

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

pub(super) fn filter_index_documents_with_cancel(
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
    start: Bound<MySqlValue>,
    end: Bound<MySqlValue>,
) {
    match start {
        Bound::Included(start) => {
            clauses.push(format!("{expr} >= ?"));
            params.push(start);
        }
        Bound::Excluded(start) => {
            clauses.push(format!("{expr} > ?"));
            params.push(start);
        }
        Bound::Unbounded => {}
    }
    match end {
        Bound::Included(end) => {
            clauses.push(format!("{expr} <= ?"));
            params.push(end);
        }
        Bound::Excluded(end) => {
            clauses.push(format!("{expr} < ?"));
            params.push(end);
        }
        Bound::Unbounded => {}
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

pub(super) fn document_matches_range_bounds(
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
            if ordering == std::cmp::Ordering::Less {
                return Ok(false);
            }
        }
        Bound::Excluded(start) => {
            let ordering = compare_values(value, start)?;
            if !matches!(ordering, std::cmp::Ordering::Greater) {
                return Ok(false);
            }
        }
        Bound::Unbounded => {}
    }
    match end {
        Bound::Included(end) => {
            let ordering = compare_values(value, end)?;
            if ordering == std::cmp::Ordering::Greater {
                return Ok(false);
            }
        }
        Bound::Excluded(end) => {
            let ordering = compare_values(value, end)?;
            if !matches!(ordering, std::cmp::Ordering::Less) {
                return Ok(false);
            }
        }
        Bound::Unbounded => {}
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
    // Field names are validated as logical names before reaching here, so the
    // escaping below is defense in depth. The JSON-path key is wrapped in `"`,
    // and the whole path is a single-quoted MySQL string literal in which
    // backslash is itself an escape character. Escape backslash first, then the
    // JSON-key `"` and the SQL-literal `'` delimiter.
    let escaped = field
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\'', "\\'");
    format!("JSON_UNQUOTE(JSON_EXTRACT(data_json, '$.\"{escaped}\"'))")
}

#[cfg(test)]
mod generated_column_tests {
    use super::mysql_generated_column_expr;

    #[test]
    fn generated_column_passes_through_ordinary_field() {
        assert_eq!(
            mysql_generated_column_expr("status"),
            r#"JSON_UNQUOTE(JSON_EXTRACT(data_json, '$."status"'))"#
        );
    }

    #[test]
    fn generated_column_escapes_sql_literal_delimiter() {
        // Even if validation were bypassed, single quotes must be backslash
        // escaped so the field cannot break out of the MySQL string literal.
        let expr = mysql_generated_column_expr("name' || (SELECT secret) || '");
        assert_eq!(
            expr,
            r#"JSON_UNQUOTE(JSON_EXTRACT(data_json, '$."name\' || (SELECT secret) || \'"'))"#
        );
        // Every injected quote is backslash escaped, so no bare quote (a quote
        // not preceded by `\`) can close the literal early.
        assert!(!expr.contains("name'"), "unescaped quote leaked: {expr}");
        assert!(expr.contains(r"name\'"), "quote was not escaped: {expr}");
    }

    #[test]
    fn generated_column_escapes_backslash_before_quote() {
        // A trailing backslash must not consume the closing JSON/SQL quote.
        assert_eq!(
            mysql_generated_column_expr(r"back\slash"),
            r#"JSON_UNQUOTE(JSON_EXTRACT(data_json, '$."back\\slash"'))"#
        );
    }
}
