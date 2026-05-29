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
use extenddb_core::types::{AttributeValue, Item};

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

/// The AWS message for a failed conditional write.
const CONDITION_FAILED_MESSAGE: &str = "The conditional request failed";

/// Evaluate an optional `ConditionExpression` against `item` as a write gate.
///
/// `Ok(())` when there is no condition (or it passes); a
/// `ConditionalCheckFailedException` (item omitted) when a well-formed
/// condition evaluates to false. PutItem/DeleteItem/UpdateItem call this before
/// mutating; the handler attaches the existing item to the error when
/// `ReturnValuesOnConditionCheckFailure` requests it. The condition is evaluated
/// against the *current* item — for a create-if-absent (`attribute_not_exists`)
/// the caller passes an empty item when no row exists.
///
/// # Errors
/// `ValidationException` for a malformed/empty expression;
/// `ConditionalCheckFailedException` when the condition is not satisfied.
pub fn check_condition(
    condition: Option<&str>,
    names: Option<&HashMap<String, String>>,
    values: Option<&HashMap<String, AttributeValue>>,
    item: &Item,
    limits: &LimitsConfig,
) -> Result<(), DynamoDbError> {
    let Some(expr_str) = condition.filter(|s| !s.is_empty()) else {
        return Ok(());
    };
    let expr = parse_condition(expr_str, limits)?;
    let maps = build_maps(names, values);
    if evaluate_condition(&expr, item, &maps)? {
        Ok(())
    } else {
        Err(DynamoDbError::ConditionalCheckFailedException(
            CONDITION_FAILED_MESSAGE.to_owned(),
            None,
        ))
    }
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

    // ---- D1.2: ConditionExpression integration coverage ----

    /// `{ alpha: S "alice", score: N 30, tags: SS ["x","y"], nested: M {city:
    /// "nyc"}, nums: L [N1,N2,N3] }` — covers scalar, set, map, and list shapes.
    fn rich_item() -> Item {
        use std::collections::BTreeMap;
        let mut m: Item = BTreeMap::new();
        m.insert("alpha".into(), AttributeValue::S("alice".into()));
        m.insert("score".into(), AttributeValue::N("30".into()));
        m.insert(
            "tags".into(),
            AttributeValue::SS(["x", "y"].iter().map(|s| (*s).to_string()).collect()),
        );
        let mut nested = BTreeMap::new();
        nested.insert("city".into(), AttributeValue::S("nyc".into()));
        m.insert("nested".into(), AttributeValue::M(nested));
        m.insert(
            "nums".into(),
            AttributeValue::L(vec![
                AttributeValue::N("1".into()),
                AttributeValue::N("2".into()),
                AttributeValue::N("3".into()),
            ]),
        );
        m
    }

    /// True if the condition passes against `rich_item`, false if it fails the
    /// conditional check. Panics on any non-condition error.
    fn passes(cond: &str, vals: &[(&str, AttributeValue)]) -> bool {
        passes_with_names(cond, None, vals)
    }

    fn passes_with_names(
        cond: &str,
        names: Option<&HashMap<String, String>>,
        vals: &[(&str, AttributeValue)],
    ) -> bool {
        let limits = default_limits();
        let vmap = values(vals);
        match check_condition(Some(cond), names, Some(&vmap), &rich_item(), &limits) {
            Ok(()) => true,
            Err(DynamoDbError::ConditionalCheckFailedException(..)) => false,
            Err(error) => panic!("unexpected error for `{cond}`: {error:?}"),
        }
    }

    #[test]
    fn no_condition_always_passes() {
        let limits = default_limits();
        assert!(check_condition(None, None, None, &rich_item(), &limits).is_ok());
        assert!(check_condition(Some(""), None, None, &rich_item(), &limits).is_ok());
    }

    #[test]
    fn failed_condition_maps_to_conditional_check_failed() {
        let limits = default_limits();
        let err = check_condition(
            Some("score = :v"),
            None,
            Some(&values(&[(":v", AttributeValue::N("99".into()))])),
            &rich_item(),
            &limits,
        )
        .expect_err("condition should fail");
        match err {
            DynamoDbError::ConditionalCheckFailedException(message, item) => {
                assert_eq!(message, "The conditional request failed");
                assert!(item.is_none(), "gate omits the item; handler attaches it");
            }
            other => panic!("expected ConditionalCheckFailedException, got {other:?}"),
        }
    }

    #[test]
    fn comparison_operators() {
        let n = |s: &str| AttributeValue::N(s.into());
        assert!(passes("score = :v", &[(":v", n("30"))]));
        assert!(!passes("score = :v", &[(":v", n("31"))]));
        assert!(passes("score <> :v", &[(":v", n("31"))]));
        assert!(passes("score < :v", &[(":v", n("31"))]));
        assert!(passes("score <= :v", &[(":v", n("30"))]));
        assert!(passes("score > :v", &[(":v", n("29"))]));
        assert!(passes("score >= :v", &[(":v", n("30"))]));
        assert!(!passes("score > :v", &[(":v", n("30"))]));
    }

    #[test]
    fn logical_operators_and_between_and_in() {
        let n = |s: &str| AttributeValue::N(s.into());
        let s = |v: &str| AttributeValue::S(v.into());
        assert!(passes(
            "score > :lo AND score < :hi",
            &[(":lo", n("10")), (":hi", n("40"))]
        ));
        assert!(!passes(
            "score > :lo AND score < :hi",
            &[(":lo", n("10")), (":hi", n("20"))]
        ));
        assert!(passes(
            "score < :lo OR score > :hi",
            &[(":lo", n("10")), (":hi", n("20"))]
        ));
        assert!(passes("NOT attribute_exists(absentattr)", &[]));
        assert!(passes(
            "score BETWEEN :lo AND :hi",
            &[(":lo", n("20")), (":hi", n("40"))]
        ));
        assert!(!passes(
            "score BETWEEN :lo AND :hi",
            &[(":lo", n("31")), (":hi", n("40"))]
        ));
        assert!(passes(
            "alpha IN (:a, :b)",
            &[(":a", s("bob")), (":b", s("alice"))]
        ));
        assert!(!passes(
            "alpha IN (:a, :b)",
            &[(":a", s("bob")), (":b", s("carol"))]
        ));
    }

    #[test]
    fn functions_exist_type_begins_with_contains_size() {
        let s = |v: &str| AttributeValue::S(v.into());
        let n = |v: &str| AttributeValue::N(v.into());
        // attribute_exists / attribute_not_exists
        assert!(passes("attribute_exists(alpha)", &[]));
        assert!(!passes("attribute_exists(absentattr)", &[]));
        assert!(passes("attribute_not_exists(absentattr)", &[]));
        assert!(!passes("attribute_not_exists(alpha)", &[]));
        // attribute_type
        assert!(passes("attribute_type(score, :t)", &[(":t", s("N"))]));
        assert!(!passes("attribute_type(score, :t)", &[(":t", s("S"))]));
        assert!(passes("attribute_type(tags, :t)", &[(":t", s("SS"))]));
        // begins_with (string prefix)
        assert!(passes("begins_with(alpha, :p)", &[(":p", s("al"))]));
        assert!(!passes("begins_with(alpha, :p)", &[(":p", s("zz"))]));
        // contains (substring of a string, and set membership)
        assert!(passes("contains(alpha, :sub)", &[(":sub", s("lic"))]));
        assert!(passes("contains(tags, :member)", &[(":member", s("x"))]));
        assert!(!passes("contains(tags, :member)", &[(":member", s("z"))]));
        // size (string length, set size, list length)
        assert!(passes("size(alpha) = :len", &[(":len", n("5"))]));
        assert!(passes("size(tags) = :len", &[(":len", n("2"))]));
        assert!(passes("size(nums) = :len", &[(":len", n("3"))]));
    }

    #[test]
    fn nested_path_and_expression_attribute_names_resolve() {
        // Nested map path through the resolver.
        assert!(passes(
            "nested.city = :c",
            &[(":c", AttributeValue::S("nyc".into()))]
        ));
        // A reserved-word-safe alias still flows through ExpressionAttributeNames.
        let names: HashMap<String, String> = [("#a".to_string(), "alpha".to_string())]
            .into_iter()
            .collect();
        assert!(passes_with_names(
            "#a = :v",
            Some(&names),
            &[(":v", AttributeValue::S("alice".into()))]
        ));
    }
}
