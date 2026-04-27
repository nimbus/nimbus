use std::collections::HashSet;

use neovex_core::{
    AggregationOperator, CountAggregation, FieldReference, StructuredAggregation,
    StructuredAggregationQuery, StructuredQuery,
};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

use super::run_query_request::decode_structured_query_values;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ParsedRunAggregationQueryRequest {
    pub aggregation_query: StructuredAggregationQuery,
}

#[derive(Debug, Error)]
pub(crate) enum FirestoreRunAggregationQueryRequestError {
    #[error("invalid Firestore RunAggregationQuery request: {0}")]
    InvalidRequest(String),
    #[error("unsupported Firestore RunAggregationQuery feature: {0}")]
    Unsupported(String),
}

pub(crate) fn parse_run_aggregation_query_request(
    request: &Value,
) -> Result<ParsedRunAggregationQueryRequest, FirestoreRunAggregationQueryRequestError> {
    let request: RunAggregationQueryRequestJson = serde_json::from_value(request.clone())
        .map_err(|error| invalid_request(format!("malformed JSON body: {error}")))?;
    let consistency_selector_count = usize::from(request.transaction.is_some())
        + usize::from(request.new_transaction.is_some())
        + usize::from(request.read_time.is_some());
    if consistency_selector_count > 1 {
        return Err(invalid_request(
            "RunAggregationQuery request must set at most one of `transaction`, `newTransaction`, or `readTime`",
        ));
    }
    if request.transaction.is_some() {
        return Err(unsupported_request("`transaction`"));
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

    let mut aggregation_query = request.structured_aggregation_query.ok_or_else(|| {
        invalid_request("RunAggregationQuery request must include `structuredAggregationQuery`")
    })?;
    decode_structured_query_values(&mut aggregation_query.structured_query)
        .map_err(|error| invalid_request(error.to_string()))?;
    let structured_query: StructuredQuery =
        serde_json::from_value(aggregation_query.structured_query)
            .map_err(|error| invalid_request(format!("invalid `structuredQuery`: {error}")))?;
    let aggregations = lower_aggregations(aggregation_query.aggregations)?;

    Ok(ParsedRunAggregationQueryRequest {
        aggregation_query: StructuredAggregationQuery {
            structured_query,
            aggregations,
        },
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RunAggregationQueryRequestJson {
    structured_aggregation_query: Option<StructuredAggregationQueryJson>,
    transaction: Option<String>,
    #[serde(default)]
    new_transaction: Option<Value>,
    read_time: Option<String>,
    #[serde(default)]
    explain_options: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StructuredAggregationQueryJson {
    structured_query: Value,
    #[serde(default)]
    aggregations: Vec<AggregationJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AggregationJson {
    #[serde(default)]
    alias: Option<String>,
    #[serde(default)]
    count: Option<CountJson>,
    #[serde(default)]
    sum: Option<FieldAggregationJson>,
    #[serde(default)]
    avg: Option<FieldAggregationJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CountJson {
    #[serde(default)]
    up_to: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FieldAggregationJson {
    field: FieldReferenceJson,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FieldReferenceJson {
    field_path: String,
}

fn lower_aggregations(
    aggregations: Vec<AggregationJson>,
) -> Result<Vec<StructuredAggregation>, FirestoreRunAggregationQueryRequestError> {
    if aggregations.is_empty() {
        return Err(invalid_request(
            "`structuredAggregationQuery.aggregations` must include at least one aggregation",
        ));
    }

    let mut seen_aliases = HashSet::new();
    let mut generated_aliases = 0usize;
    aggregations
        .into_iter()
        .map(|aggregation| {
            let alias =
                normalize_alias(aggregation.alias, &mut generated_aliases, &mut seen_aliases)?;
            let operator_count = usize::from(aggregation.count.is_some())
                + usize::from(aggregation.sum.is_some())
                + usize::from(aggregation.avg.is_some());
            if operator_count != 1 {
                return Err(invalid_request(
                    "each aggregation must set exactly one of `count`, `sum`, or `avg`",
                ));
            }

            let operator = if let Some(count) = aggregation.count {
                AggregationOperator::Count(CountAggregation {
                    up_to: parse_optional_positive_int64(count.up_to, "`count.upTo`")?,
                })
            } else if let Some(sum) = aggregation.sum {
                AggregationOperator::Sum(FieldReference::new(sum.field.field_path))
            } else if let Some(avg) = aggregation.avg {
                AggregationOperator::Avg(FieldReference::new(avg.field.field_path))
            } else {
                return Err(invalid_request(
                    "each aggregation must set exactly one aggregation operator",
                ));
            };

            Ok(StructuredAggregation { alias, operator })
        })
        .collect()
}

fn normalize_alias(
    alias: Option<String>,
    generated_aliases: &mut usize,
    seen_aliases: &mut HashSet<String>,
) -> Result<String, FirestoreRunAggregationQueryRequestError> {
    let alias = match alias {
        Some(alias) if alias.trim().is_empty() => {
            return Err(invalid_request("aggregation aliases must not be empty"));
        }
        Some(alias) => alias,
        None => {
            *generated_aliases += 1;
            format!("field_{generated_aliases}")
        }
    };
    if !seen_aliases.insert(alias.clone()) {
        return Err(invalid_request(format!(
            "aggregation alias `{alias}` must be unique"
        )));
    }
    Ok(alias)
}

fn parse_optional_positive_int64(
    value: Option<Value>,
    field_name: &str,
) -> Result<Option<u64>, FirestoreRunAggregationQueryRequestError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let parsed = match value {
        Value::String(raw) => raw.parse::<u64>().map_err(|error| {
            invalid_request(format!(
                "{field_name} must be a positive int64 string: {error}"
            ))
        })?,
        Value::Number(raw) => raw.as_u64().ok_or_else(|| {
            invalid_request(format!("{field_name} must be a positive int64 number"))
        })?,
        _ => {
            return Err(invalid_request(format!(
                "{field_name} must be encoded as an int64 value"
            )));
        }
    };
    if parsed == 0 {
        return Err(invalid_request(format!(
            "{field_name} must be greater than zero"
        )));
    }
    if parsed > i64::MAX as u64 {
        return Err(invalid_request(format!(
            "{field_name} exceeds Firestore int64 range"
        )));
    }
    Ok(Some(parsed))
}

fn invalid_request(reason: impl Into<String>) -> FirestoreRunAggregationQueryRequestError {
    FirestoreRunAggregationQueryRequestError::InvalidRequest(reason.into())
}

fn unsupported_request(feature: impl Into<String>) -> FirestoreRunAggregationQueryRequestError {
    FirestoreRunAggregationQueryRequestError::Unsupported(feature.into())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::parse_run_aggregation_query_request;

    #[test]
    fn parses_count_aggregations_and_defaults_missing_aliases() {
        let request = json!({
            "structuredAggregationQuery": {
                "structuredQuery": {
                    "from": [{ "collectionId": "cities" }],
                    "where": {
                        "fieldFilter": {
                            "field": { "fieldPath": "state" },
                            "op": "EQUAL",
                            "value": { "stringValue": "CA" }
                        }
                    }
                },
                "aggregations": [
                    { "count": { "upTo": "5" }, "alias": "total" },
                    { "count": {} }
                ]
            }
        });

        let parsed = parse_run_aggregation_query_request(&request).expect("request should parse");

        assert_eq!(parsed.aggregation_query.aggregations[0].alias, "total");
        assert_eq!(parsed.aggregation_query.aggregations[1].alias, "field_1");
        assert_eq!(
            parsed.aggregation_query.structured_query.where_filter,
            Some(neovex_core::QueryFilter::FieldFilter(
                neovex_core::FieldFilter {
                    field: neovex_core::FieldReference::new("state"),
                    op: neovex_core::FieldFilterOperator::Equal,
                    value: json!("CA"),
                }
            ))
        );
    }

    #[test]
    fn rejects_transaction_and_duplicate_aliases() {
        let transaction_request = json!({
            "transaction": "abc",
            "structuredAggregationQuery": {
                "structuredQuery": { "from": [{ "collectionId": "cities" }] },
                "aggregations": [{ "count": {} }]
            }
        });
        let transaction_error = parse_run_aggregation_query_request(&transaction_request)
            .expect_err("transaction should be rejected");
        assert!(transaction_error.to_string().contains("`transaction`"));

        let duplicate_aliases = json!({
            "structuredAggregationQuery": {
                "structuredQuery": { "from": [{ "collectionId": "cities" }] },
                "aggregations": [
                    { "count": {}, "alias": "dup" },
                    { "count": {}, "alias": "dup" }
                ]
            }
        });
        let alias_error = parse_run_aggregation_query_request(&duplicate_aliases)
            .expect_err("duplicate aliases should be rejected");
        assert!(alias_error.to_string().contains("must be unique"));
    }
}
