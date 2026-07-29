use nimbus_core::StructuredQuery;
use serde::Deserialize;
use serde_json::Value;

use super::request_error::{FirestoreRequestError, FirestoreRpc};
use super::serializer;
use super::transaction_token;

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedRunQueryRequest {
    pub structured_query: StructuredQuery,
    pub transaction: Option<Vec<u8>>,
}

pub fn parse_run_query_request(
    request: &Value,
) -> Result<ParsedRunQueryRequest, FirestoreRequestError> {
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
        .map(transaction_token::decode)
        .transpose()
        .map_err(|error| invalid_request(error.to_string()))?;

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

pub fn decode_structured_query_values(
    structured_query: &mut Value,
) -> Result<(), FirestoreRequestError> {
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

/// Wire types whose stored projection cannot be compared correctly.
///
/// Query filters and cursors are evaluated against `Document.fields`, the plain
/// JSON projection kept beside a field's typed metadata, so a comparison cannot
/// tell a typed value from a plain one that projects to the same JSON: a
/// `bytesValue` projects to its base64 string, a `timestampValue` to its RFC3339
/// string, a `geoPointValue` to an ordinary two-key map. Accepting these
/// operands would match documents of a different type. Ordering is worse than
/// equality: RFC3339 strings do not sort chronologically (`…05.5Z` sorts before
/// `…05Z`, because `.` is 0x2E and `Z` is 0x5A), base64 does not sort in byte
/// order, and a map has no ordering at all — so range filters and cursors would
/// silently omit or misplace documents.
///
/// They stay rejected until filter evaluation carries type metadata rather than
/// comparing projections. `referenceValue` is deliberately absent: `__name__`
/// document-ID filters are built on it, it is compared against the document
/// name rather than a stored field, and it has been accepted here all along.
const PROJECTION_UNSAFE_QUERY_VALUE_TYPES: [&str; 3] =
    ["timestampValue", "bytesValue", "geoPointValue"];

fn decode_query_value(value: &Value) -> Result<Value, FirestoreRequestError> {
    if let Some(value) = value.as_object() {
        if let Some(reference) = value.get("referenceValue").and_then(Value::as_str) {
            return Ok(Value::String(reference.to_string()));
        }
        if let Some(unsafe_type) = PROJECTION_UNSAFE_QUERY_VALUE_TYPES
            .iter()
            .find(|wire_type| value.contains_key(**wire_type))
        {
            return Err(unsupported_request(format!(
                "`{unsafe_type}` in a query filter or cursor: comparisons run against the stored \
                 JSON projection, which cannot distinguish it from a plain value that projects to \
                 the same JSON, and does not order it correctly"
            )));
        }
        // Containers recurse here rather than through the stored-value lowering
        // because `in`/`not-in` legitimately carry an array of array candidates,
        // which the document write path rejects.
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

fn decode_query_filter_values(filter: &mut Value) -> Result<(), FirestoreRequestError> {
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

fn decode_cursor_values(cursor: &mut Value) -> Result<(), FirestoreRequestError> {
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

fn invalid_request(reason: impl Into<String>) -> FirestoreRequestError {
    FirestoreRequestError::invalid_request(FirestoreRpc::RunQuery, reason)
}

fn unsupported_request(feature: impl Into<String>) -> FirestoreRequestError {
    FirestoreRequestError::unsupported(FirestoreRpc::RunQuery, feature)
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
            nimbus_core::QueryFilter::FieldFilter(nimbus_core::FieldFilter {
                field: nimbus_core::FieldReference::new("state"),
                op: nimbus_core::FieldFilterOperator::Equal,
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
    fn rejects_projection_unsafe_typed_values_in_filters_and_cursors() {
        // Every projection-unsafe wire type is refused in both places a query
        // compares a value, and the error names the type so a caller can tell
        // this apart from a malformed request.
        for value in [
            json!({ "timestampValue": "2024-01-02T03:04:05.123456789Z" }),
            json!({ "bytesValue": "AQIDBA==" }),
            json!({ "geoPointValue": { "latitude": 37.7749, "longitude": -122.4194 } }),
        ] {
            let wire_type = value
                .as_object()
                .and_then(|value| value.keys().next().cloned())
                .expect("wire type should be present");

            let filter_request = json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "events" }],
                    "where": {
                        "fieldFilter": {
                            "field": { "fieldPath": "createdAt" },
                            "op": "EQUAL",
                            "value": value.clone()
                        }
                    }
                }
            });
            let error = parse_run_query_request(&filter_request)
                .expect_err("projection-unsafe filter operand should be rejected")
                .to_string();
            assert!(
                error.contains(&wire_type) && error.contains("query filter or cursor"),
                "filter rejection should name `{wire_type}`: {error}"
            );

            let cursor_request = json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "events" }],
                    "startAt": { "values": [value.clone()], "before": true }
                }
            });
            let error = parse_run_query_request(&cursor_request)
                .expect_err("projection-unsafe cursor operand should be rejected")
                .to_string();
            assert!(
                error.contains(&wire_type) && error.contains("query filter or cursor"),
                "cursor rejection should name `{wire_type}`: {error}"
            );

            // Nesting does not launder the operand: an `in` filter carrying the
            // same value inside an arrayValue is refused on the same grounds.
            let nested_request = json!({
                "structuredQuery": {
                    "from": [{ "collectionId": "events" }],
                    "where": {
                        "fieldFilter": {
                            "field": { "fieldPath": "createdAt" },
                            "op": "IN",
                            "value": { "arrayValue": { "values": [value] } }
                        }
                    }
                }
            });
            let error = parse_run_query_request(&nested_request)
                .expect_err("nested projection-unsafe operand should be rejected")
                .to_string();
            assert!(
                error.contains(&wire_type),
                "nested rejection should name `{wire_type}`: {error}"
            );
        }
    }

    #[test]
    fn projection_collision_shows_why_typed_query_operands_stay_rejected() {
        // The rejection above is not conservatism. These typed values project to
        // exactly the JSON a plain value of another type projects to, and query
        // evaluation compares those projections, so accepting them would report
        // documents of the wrong type as matches.
        use nimbus_core::{StoredValue, TypedScalarValue};

        let bytes = StoredValue::TypedScalar {
            value: TypedScalarValue::Bytes {
                data: vec![1, 2, 3, 4],
            },
        };
        assert_eq!(bytes.projected_json(), json!("AQIDBA=="));

        let timestamp = StoredValue::TypedScalar {
            value: TypedScalarValue::FirestoreTimestamp {
                rfc3339: "2024-01-02T03:04:05Z".to_string(),
            },
        };
        assert_eq!(timestamp.projected_json(), json!("2024-01-02T03:04:05Z"));

        let geo_point = StoredValue::TypedScalar {
            value: TypedScalarValue::GeoPoint {
                latitude: 37.7749,
                longitude: -122.4194,
            },
        };
        assert_eq!(
            geo_point.projected_json(),
            json!({ "latitude": 37.7749, "longitude": -122.4194 })
        );

        // And the ordering the projection implies is not the ordering the values
        // have: a later timestamp sorts before an earlier one as a string. Both
        // spellings below are canonical stored forms, so this is the ordering a
        // range filter would actually get, not an artifact of a odd spelling.
        assert!("2024-01-02T03:04:05.5Z" < "2024-01-02T03:04:05Z");
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
            nimbus_core::QueryFilter::FieldFilter(filter) => {
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
