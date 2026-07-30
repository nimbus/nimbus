use std::fmt::Write as _;
use std::ops::Bound;

use mysql_async::Value as MySqlValue;
use nimbus_core::{Error, FieldType, Result, TableName, TableSchema, Timestamp};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::store::MAX_DURABLE_JOURNAL_STREAM_LIMIT;

// Dialect-independent row serialization and document predicates live once in
// `crate::sql`; the MySQL module re-exports them so existing call sites stay
// unchanged.
pub(super) use crate::sql::predicate::{
    filter_documents_with_predicate, filter_index_documents_with_cancel,
    index_fields_for_table_schema, validate_index_prefix_len, validate_index_range_prefix,
};
pub(super) use crate::sql::row::{
    deserialize_json, row_to_document, serialize_document_fields, serialize_document_typed_fields,
    serialize_json,
};

use super::backend::quote_identifier;

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
