//! Expression shim over `extenddb-core`'s DynamoDB expression engine.
//!
//! DynamoDB's expression language (Condition / Update / Projection /
//! KeyCondition) is fully implemented in `extenddb_core::expression`
//! (tokenizer, parser, evaluator, update evaluator, projection, key condition,
//! resolver). This module is the thin adapter the item handlers (D1.5+) call:
//! it composes tokenize → reserved-word-validate → parse with DynamoDB-parity
//! limits, builds the `ExpressionMaps` from a request's
//! `ExpressionAttributeNames`/`ExpressionAttributeValues`, and re-exports the
//! upstream evaluators.
//!
//! The evaluators operate on `extenddb_core::types::Item`
//! (`BTreeMap<String, AttributeValue>`). Stored Nimbus documents bridge to/from
//! that item shape through [`crate::attribute_value::stored_to_item`] /
//! [`crate::attribute_value::item_to_stored`] — so the typed AttributeValue
//! contract (`N`/`B`/`SS`/`NS`/`BS` precision) is preserved across evaluation.
//!
//! The composition mirrors ExtendDB's own `engine/expression_helpers.rs`
//! (Apache-2.0), which lives in its engine crate and so is reimplemented here
//! over the publicly re-exported `extenddb-core` primitives.

use std::collections::HashMap;

use extenddb_core::error::DynamoDbError;
use extenddb_core::expression::{
    Expr, ExpressionMaps, KeyCondition, PathElement, UpdateAction,
    parse_condition_with_depth_limit, parse_key_condition, parse_projection, parse_update,
    tokenize_for, validate_no_reserved_words,
};
use extenddb_core::limits::LimitsConfig;
use extenddb_core::types::AttributeValue;

// Re-export the upstream evaluators so handlers import everything expression
// from one place. These take the parsed AST + an `Item` + the `ExpressionMaps`.
pub use extenddb_core::expression::{apply_projection, apply_update, evaluate_condition};

/// DynamoDB-parity expression limits (token count, nesting depth, reserved-word
/// enforcement). Sourced from `extenddb-core`'s documented defaults.
#[must_use]
pub fn default_limits() -> LimitsConfig {
    LimitsConfig::default()
}

/// Build the resolver maps from a request's optional `ExpressionAttributeNames`
/// and `ExpressionAttributeValues`. The `#`/`:` placeholder prefixes are
/// stripped on insert (the AST/resolver key on the bare name), and numeric
/// values are pre-parsed once so repeated comparisons don't re-parse.
#[must_use]
pub fn build_maps(
    names: Option<&HashMap<String, String>>,
    values: Option<&HashMap<String, AttributeValue>>,
) -> ExpressionMaps {
    let names = names
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.strip_prefix('#').unwrap_or(k).to_owned(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    let values = values
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.strip_prefix(':').unwrap_or(k).to_owned(), v.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut maps = ExpressionMaps::new(names, values);
    maps.pre_parse_numerics();
    maps
}

/// Tokenize, reserved-word-validate, and parse a `ConditionExpression`.
///
/// # Errors
/// `ValidationException` for an empty string, oversized token stream, reserved
/// word used as a bare path, or a syntax/depth error.
pub fn parse_condition(expr: &str, limits: &LimitsConfig) -> Result<Expr, DynamoDbError> {
    let tokens = tokenize_for(expr, limits.max_expression_tokens, "ConditionExpression")?;
    if limits.enforce_reserved_keywords {
        validate_no_reserved_words(&tokens)?;
    }
    parse_condition_with_depth_limit(&tokens, limits.max_expression_depth)
}

/// Tokenize, reserved-word-validate, and parse an `UpdateExpression` into its
/// ordered action list (SET/REMOVE/ADD/DELETE).
///
/// # Errors
/// `ValidationException` for an empty string or any syntax error.
pub fn parse_update_expression(
    expr: &str,
    limits: &LimitsConfig,
) -> Result<Vec<UpdateAction>, DynamoDbError> {
    let tokens = tokenize_for(expr, limits.max_expression_tokens, "UpdateExpression")?;
    if limits.enforce_reserved_keywords {
        validate_no_reserved_words(&tokens)?;
    }
    parse_update(&tokens)
}

/// Tokenize, reserved-word-validate, and parse a `ProjectionExpression` into a
/// list of attribute paths.
///
/// # Errors
/// `ValidationException` for an empty string or any syntax error.
pub fn parse_projection_expression(
    expr: &str,
    limits: &LimitsConfig,
) -> Result<Vec<Vec<PathElement>>, DynamoDbError> {
    let tokens = tokenize_for(expr, limits.max_expression_tokens, "ProjectionExpression")?;
    if limits.enforce_reserved_keywords {
        validate_no_reserved_words(&tokens)?;
    }
    parse_projection(&tokens)
}

/// Tokenize, reserved-word-validate, and parse a `KeyConditionExpression`.
///
/// # Errors
/// `ValidationException` for an empty string or any syntax error.
pub fn parse_key_condition_expression(
    expr: &str,
    limits: &LimitsConfig,
) -> Result<KeyCondition, DynamoDbError> {
    let tokens = tokenize_for(expr, limits.max_expression_tokens, "KeyConditionExpression")?;
    if limits.enforce_reserved_keywords {
        validate_no_reserved_words(&tokens)?;
    }
    parse_key_condition(&tokens)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute_value::stored_to_item;
    use extenddb_core::types::Item;
    use nimbus_core::typed_scalar::StoredValue;

    /// A stored item `{ alpha: "hello", score: 7 (N), beta: "old" }` bridged to
    /// the AttributeValue item shape the evaluators consume. Uses the typed
    /// sidecar for `score` so numeric comparison exercises the bridge.
    fn item() -> Item {
        use nimbus_core::typed_scalar::TypedScalarValue;
        let mut stored = std::collections::BTreeMap::new();
        stored.insert(
            "alpha".to_string(),
            StoredValue::Json {
                value: serde_json::Value::String("hello".into()),
            },
        );
        stored.insert(
            "score".to_string(),
            StoredValue::TypedScalar {
                value: TypedScalarValue::Number { repr: "7".into() },
            },
        );
        stored.insert(
            "beta".to_string(),
            StoredValue::Json {
                value: serde_json::Value::String("old".into()),
            },
        );
        stored_to_item(&stored)
    }

    fn values(pairs: &[(&str, AttributeValue)]) -> HashMap<String, AttributeValue> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn condition_expression_parses_and_evaluates_against_an_item() {
        let limits = default_limits();
        let expr = parse_condition("attribute_exists(alpha) AND score > :min", &limits)
            .expect("parse condition");
        let maps = build_maps(
            None,
            Some(&values(&[(":min", AttributeValue::N("3".into()))])),
        );
        assert!(
            evaluate_condition(&expr, &item(), &maps).expect("evaluate"),
            "score 7 > 3 and alpha exists => true"
        );

        // Same condition with a higher threshold evaluates false.
        let maps = build_maps(
            None,
            Some(&values(&[(":min", AttributeValue::N("9".into()))])),
        );
        assert!(!evaluate_condition(&expr, &item(), &maps).expect("evaluate"));
    }

    #[test]
    fn update_expression_parses_and_applies_to_an_item() {
        let limits = default_limits();
        let actions =
            parse_update_expression("SET beta = :v REMOVE alpha", &limits).expect("parse update");
        let maps = build_maps(
            None,
            Some(&values(&[(":v", AttributeValue::S("new".into()))])),
        );
        let mut target = item();
        apply_update(&actions, &mut target, &maps).expect("apply update");
        assert_eq!(target.get("beta"), Some(&AttributeValue::S("new".into())));
        assert!(!target.contains_key("alpha"), "REMOVE drops alpha");
        assert_eq!(target.get("score"), Some(&AttributeValue::N("7".into())));
    }

    #[test]
    fn projection_expression_parses_and_selects_fields() {
        let limits = default_limits();
        let paths = parse_projection_expression("alpha, score", &limits).expect("parse projection");
        let projected =
            apply_projection(&item(), &paths, &build_maps(None, None)).expect("apply projection");
        assert_eq!(projected.len(), 2);
        assert!(projected.contains_key("alpha"));
        assert!(projected.contains_key("score"));
        assert!(!projected.contains_key("beta"), "beta is projected out");
    }

    #[test]
    fn key_condition_expression_parses_partition_and_sort() {
        let limits = default_limits();
        let kc = parse_key_condition_expression("alpha = :p AND score > :s", &limits)
            .expect("parse key condition");
        assert!(!kc.pk_path.is_empty(), "partition key path present");
        assert!(kc.sk_condition.is_some(), "sort key condition present");
    }

    #[test]
    fn empty_expression_is_rejected() {
        let limits = default_limits();
        assert!(matches!(
            parse_update_expression("", &limits),
            Err(DynamoDbError::ValidationException(_))
        ));
    }
}
