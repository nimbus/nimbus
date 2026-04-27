use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neovex_core::StructuredQuery;
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use super::serializer;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedRunQueryRequest {
    pub structured_query: StructuredQuery,
    pub transaction: Option<Vec<u8>>,
}

#[derive(Debug, Error)]
pub(crate) enum FirestoreRunQueryRequestError {
    #[error("invalid Firestore RunQuery request: {0}")]
    InvalidRequest(String),
    #[error("unsupported Firestore RunQuery feature: {0}")]
    Unsupported(String),
}

pub(crate) fn parse_run_query_request(
    request: &Value,
) -> Result<ParsedRunQueryRequest, FirestoreRunQueryRequestError> {
    let request: RunQueryRequestJson = serde_json::from_value(request.clone())
        .map_err(|error| invalid_request(format!("malformed JSON body: {error}")))?;
    let consistency_selector_count = usize::from(request.transaction.is_some())
        + usize::from(request.new_transaction.is_some())
        + usize::from(request.read_time.is_some());
    if consistency_selector_count > 1 {
        return Err(invalid_request(
            "RunQuery request must set at most one of `transaction`, `newTransaction`, or `readTime`",
        ));
    }
    if request.new_transaction.is_some() {
        return Err(unsupported_request("`newTransaction`"));
    }
    if request.read_time.is_some() {
        return Err(unsupported_request("`readTime`"));
    }
    if request.explain_options.is_some() {
        return Err(unsupported_request("`explainOptions`"));
    }

    let mut structured_query = request
        .structured_query
        .ok_or_else(|| invalid_request("RunQuery request must include `structuredQuery`"))?;
    decode_structured_query_values(&mut structured_query)?;
    let structured_query = serde_json::from_value(structured_query)
        .map_err(|error| invalid_request(format!("invalid `structuredQuery`: {error}")))?;
    let transaction = request
        .transaction
        .as_deref()
        .map(parse_transaction)
        .transpose()?;

    Ok(ParsedRunQueryRequest {
        structured_query,
        transaction,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunQueryRequestJson {
    structured_query: Option<Value>,
    transaction: Option<String>,
    #[serde(default)]
    new_transaction: Option<Value>,
    read_time: Option<String>,
    #[serde(default)]
    explain_options: Option<Value>,
}

pub(crate) fn decode_structured_query_values(
    structured_query: &mut Value,
) -> Result<(), FirestoreRunQueryRequestError> {
    let Value::Object(query) = structured_query else {
        return Err(invalid_request("`structuredQuery` must be an object"));
    };
    if let Some(filter) = query.get_mut("where") {
        decode_query_filter_values(filter)?;
    }
    if let Some(cursor) = query.get_mut("startAt") {
        decode_cursor_values(cursor)?;
    }
    if let Some(cursor) = query.get_mut("endAt") {
        decode_cursor_values(cursor)?;
    }
    Ok(())
}

fn decode_query_value(value: &Value) -> Result<Value, FirestoreRunQueryRequestError> {
    if let Some(value) = value.as_object() {
        if let Some(reference) = value.get("referenceValue").and_then(Value::as_str) {
            return Ok(Value::String(reference.to_string()));
        }
        if let Some(array) = value.get("arrayValue") {
            let values = array
                .get("values")
                .and_then(Value::as_array)
                .ok_or_else(|| {
                    invalid_request("invalid query value: arrayValue must include `values`")
                })?;
            return Ok(Value::Array(
                values
                    .iter()
                    .map(decode_query_value)
                    .collect::<Result<Vec<_>, _>>()?,
            ));
        }
        if let Some(map) = value.get("mapValue") {
            let fields = map
                .get("fields")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    invalid_request("invalid query value: mapValue must include `fields`")
                })?;
            return Ok(Value::Object(
                fields
                    .iter()
                    .map(|(field, value)| {
                        decode_query_value(value).map(|value| (field.clone(), value))
                    })
                    .collect::<Result<serde_json::Map<_, _>, _>>()?,
            ));
        }
    }
    serializer::decode_proto_json_value(value)
        .map_err(|error| invalid_request(format!("invalid query value: {error}")))
}

fn decode_query_filter_values(filter: &mut Value) -> Result<(), FirestoreRunQueryRequestError> {
    let Value::Object(filter) = filter else {
        return Err(invalid_request("query filters must be objects"));
    };
    if let Some(field_filter) = filter.get_mut("fieldFilter") {
        let Value::Object(field_filter) = field_filter else {
            return Err(invalid_request("field filters must be objects"));
        };
        let value = field_filter
            .get_mut("value")
            .ok_or_else(|| invalid_request("field filters must include `value`"))?;
        *value = decode_query_value(value)?;
        return Ok(());
    }
    if let Some(composite_filter) = filter.get_mut("compositeFilter") {
        let Value::Object(composite_filter) = composite_filter else {
            return Err(invalid_request("composite filters must be objects"));
        };
        let filters = composite_filter
            .get_mut("filters")
            .ok_or_else(|| invalid_request("composite filters must include `filters`"))?;
        let Value::Array(filters) = filters else {
            return Err(invalid_request(
                "composite filter `filters` must be an array",
            ));
        };
        for nested_filter in filters {
            decode_query_filter_values(nested_filter)?;
        }
    }
    Ok(())
}

fn decode_cursor_values(cursor: &mut Value) -> Result<(), FirestoreRunQueryRequestError> {
    let Value::Object(cursor) = cursor else {
        return Err(invalid_request("query cursors must be objects"));
    };
    let Some(values) = cursor.get_mut("values") else {
        return Ok(());
    };
    let Value::Array(values) = values else {
        return Err(invalid_request("cursor `values` must be an array"));
    };
    for value in values {
        *value = decode_query_value(value)?;
    }
    Ok(())
}

fn invalid_request(reason: impl Into<String>) -> FirestoreRunQueryRequestError {
    FirestoreRunQueryRequestError::InvalidRequest(reason.into())
}

fn unsupported_request(feature: impl Into<String>) -> FirestoreRunQueryRequestError {
    FirestoreRunQueryRequestError::Unsupported(feature.into())
}

fn parse_transaction(value: &str) -> Result<Vec<u8>, FirestoreRunQueryRequestError> {
    BASE64_STANDARD
        .decode(value)
        .map_err(|error| invalid_request(format!("invalid base64 transaction bytes: {error}")))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_run_query_request;

    #[test]
    fn parses_structured_query_and_decodes_filter_cursor_values_and_transaction() {
        let request = json!({
            "transaction": "AQID",
            "structuredQuery": {
                "from": [{ "collectionId": "cities" }],
                "where": {
                    "fieldFilter": {
                        "field": { "fieldPath": "state" },
                        "op": "EQUAL",
                        "value": { "stringValue": "CA" }
                    }
                },
                "startAt": {
                    "values": [{ "integerValue": "2" }],
                    "before": false
                },
                "limit": 5
            }
        });

        let parsed = parse_run_query_request(&request).expect("request should parse");

        assert_eq!(parsed.transaction, Some(vec![1, 2, 3]));
        assert_eq!(parsed.structured_query.from.len(), 1);
        assert_eq!(
            parsed
                .structured_query
                .where_filter
                .expect("filter should exist"),
            neovex_core::QueryFilter::FieldFilter(neovex_core::FieldFilter {
                field: neovex_core::FieldReference::new("state"),
                op: neovex_core::FieldFilterOperator::Equal,
                value: json!("CA"),
            })
        );
        assert_eq!(
            parsed
                .structured_query
                .start_at
                .expect("cursor should exist")
                .values,
            vec![json!(2)]
        );
    }

    #[test]
    fn parses_reference_values_for_document_id_filters_and_cursors() {
        let request = json!({
            "structuredQuery": {
                "from": [{ "collectionId": "cities" }],
                "where": {
                    "fieldFilter": {
                        "field": { "fieldPath": "__name__" },
                        "op": "GREATER_THAN_OR_EQUAL",
                        "value": {
                            "referenceValue": "projects/demo/databases/(default)/documents/cities/SF"
                        }
                    }
                },
                "orderBy": [{
                    "field": { "fieldPath": "__name__" },
                    "direction": "ASCENDING"
                }],
                "startAt": {
                    "values": [{
                        "referenceValue": "projects/demo/databases/(default)/documents/cities/SEA"
                    }],
                    "before": false
                }
            }
        });

        let parsed = parse_run_query_request(&request).expect("request should parse");
        match parsed
            .structured_query
            .where_filter
            .expect("filter should exist")
        {
            neovex_core::QueryFilter::FieldFilter(filter) => {
                assert_eq!(
                    filter.value,
                    json!("projects/demo/databases/(default)/documents/cities/SF")
                );
            }
            other => panic!("expected field filter, got {other:?}"),
        }
        assert_eq!(
            parsed
                .structured_query
                .start_at
                .expect("cursor should exist")
                .values,
            vec![json!(
                "projects/demo/databases/(default)/documents/cities/SEA"
            )]
        );
    }

    #[test]
    fn rejects_unsupported_consistency_selectors_bad_transaction_and_missing_structured_query() {
        let unsupported = json!({
            "structuredQuery": {
                "from": [{ "collectionId": "cities" }]
            },
            "readTime": "2026-04-25T00:00:00Z"
        });
        let bad_transaction = json!({
            "transaction": "!not-base64!",
            "structuredQuery": {
                "from": [{ "collectionId": "cities" }]
            }
        });
        let missing = json!({});

        let unsupported_error =
            parse_run_query_request(&unsupported).expect_err("readTime should be rejected");
        let bad_transaction_error = parse_run_query_request(&bad_transaction)
            .expect_err("bad transaction bytes should be rejected");
        let missing_error = parse_run_query_request(&missing)
            .expect_err("missing structuredQuery should be rejected");

        assert!(unsupported_error.to_string().contains("readTime"));
        assert!(bad_transaction_error.to_string().contains("base64"));
        assert!(missing_error.to_string().contains("structuredQuery"));
    }
}
