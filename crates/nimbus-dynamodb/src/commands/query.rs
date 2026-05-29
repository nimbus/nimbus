//! Query (T2, D2.1) and Scan (D2.3) — multi-item reads.
//!
//! Items live as AttributeValue wire-JSON in `Document.fields` keyed by the
//! composite-key `DocumentId` (DDB-DIV-005). A Query selects one partition
//! (`pk = :p`), evaluates an optional sort-key range against the real `sk`
//! attribute using the order-preserving D0.3 sortable encoding (type-correct
//! `N`/`S`/`B` ordering — closes DDB-DIV-002, no separate `_sk` projection
//! needed), orders by `sk` (honoring `ScanIndexForward`), and paginates by
//! sort key (`ExclusiveStartKey`/`LastEvaluatedKey`). Filtering, projection,
//! and Scan land in D2.2/D2.3.

use std::cmp::Ordering;
use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use extenddb_core::expression::{CompareOp, Expr, ExpressionMaps, SortKeyCondition};
use extenddb_core::types::{
    AttributeValue, Item, KeySchemaElement, KeyType, QueryInput, QueryOutput,
};
use nimbus_core::{StructuredQuery, TableName};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;

use crate::attribute_value::fields_to_item;
use crate::commands::control_plane;
use crate::error::map_core_error;
use crate::expression::{build_maps, default_limits, parse_key_condition_expression};
use crate::key::sortable_key;

/// Query a single partition with an optional sort-key range.
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for a missing/invalid `KeyConditionExpression`, an `IndexName` (GSI/LSI is
/// D4), or a malformed `ExclusiveStartKey`.
pub fn query(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: QueryInput,
) -> Result<QueryOutput, DynamoDbError> {
    if input.index_name.is_some() {
        return Err(DynamoDbError::ValidationException(
            "Querying a secondary index is not yet supported (planned in D4)".to_owned(),
        ));
    }
    let key_schema = control_plane::load_key_schema(service, context, &input.table_name)?;
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;
    let limits = default_limits();

    let kce = input
        .key_condition_expression
        .as_deref()
        .filter(|expression| !expression.is_empty())
        .ok_or_else(|| {
            DynamoDbError::ValidationException(
                "Either the KeyCondition or KeyConditionExpression parameter must be specified \
                 in the request."
                    .to_owned(),
            )
        })?;
    let mut key_condition = parse_key_condition_expression(kce, &limits)?;
    let maps = build_maps(
        input.expression_attribute_names.as_ref(),
        input.expression_attribute_values.as_ref(),
    );

    let hash_attr = attr_of_kind(&key_schema, KeyType::Hash).ok_or_else(|| {
        DynamoDbError::ValidationException("table key schema has no partition key".to_owned())
    })?;
    let range_attr = attr_of_kind(&key_schema, KeyType::Range);

    // Disambiguate which clause is PK vs SK against the real partition key.
    key_condition.resolve_pk_sk(&hash_attr, &maps.names)?;
    let pk_value = resolve_value(&key_condition.pk_value, &maps)?;

    // Select the partition, then apply the optional sort-key condition.
    let mut matched: Vec<Item> = Vec::new();
    for item in enumerate(service, context, &table)? {
        let Some(item_pk) = item.get(&hash_attr) else {
            continue;
        };
        if !sortable_eq(item_pk, &pk_value)? {
            continue;
        }
        if let Some(sk_condition) = &key_condition.sk_condition {
            let sk_name = range_attr.as_deref().ok_or_else(|| {
                DynamoDbError::ValidationException(
                    "Query key condition uses a sort key, but the table has none".to_owned(),
                )
            })?;
            if !eval_sort_condition(sk_condition, item.get(sk_name), &maps)? {
                continue;
            }
        }
        matched.push(item);
    }

    // Order by sort key (type-correct via the sortable encoding), then direction.
    if let Some(sk_name) = &range_attr {
        matched.sort_by(|a, b| sort_cmp(a, b, sk_name));
        if !input.scan_index_forward {
            matched.reverse();
        }
    }

    // ExclusiveStartKey: drop everything up to and including the cursor's sort
    // position (within a partition `sk` is unique, so it is a total order).
    if let Some(start) = &input.exclusive_start_key {
        matched = apply_start_key(
            matched,
            start,
            range_attr.as_deref(),
            input.scan_index_forward,
        )?;
    }

    let scanned_count = matched.len() as i64;

    // Limit + LastEvaluatedKey.
    let limit = input
        .limit
        .filter(|limit| *limit > 0)
        .map(|limit| limit as usize);
    let truncated = limit.is_some_and(|limit| matched.len() > limit);
    if let Some(limit) = limit {
        matched.truncate(limit);
    }
    let last_evaluated_key = truncated
        .then(|| matched.last().map(|item| key_item(item, &key_schema)))
        .flatten();

    let count = matched.len() as i64;
    Ok(QueryOutput {
        items: Some(matched),
        count,
        scanned_count,
        last_evaluated_key,
        consumed_capacity: None,
    })
}

/// Read every item stored in `table` as a decoded `Item`.
pub(crate) fn enumerate(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table: &TableName,
) -> Result<Vec<Item>, DynamoDbError> {
    let documents = match service.query_documents_structured(
        context.tenant_id(),
        table,
        &StructuredQuery::default(),
    ) {
        Ok(documents) => documents,
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(map_core_error(error)),
    };
    documents
        .iter()
        .map(|document| fields_to_item(&document.fields))
        .collect()
}

/// The resolved top-level attribute name of `key_schema`'s HASH/RANGE element.
fn attr_of_kind(key_schema: &[KeySchemaElement], kind: KeyType) -> Option<String> {
    key_schema
        .iter()
        .find(|element| element.key_type == kind)
        .map(|element| element.attribute_name.clone())
}

/// Resolve a key-condition value expression (always a `:value` placeholder).
fn resolve_value(expr: &Expr, maps: &ExpressionMaps) -> Result<AttributeValue, DynamoDbError> {
    match expr {
        Expr::Placeholder(name) => maps.resolve_value(name).cloned(),
        _ => Err(DynamoDbError::ValidationException(
            "key condition values must be expression attribute value placeholders".to_owned(),
        )),
    }
}

/// Type-correct equality via the order-preserving sortable encoding (so `5`
/// and `5.0` compare equal for numeric keys).
fn sortable_eq(a: &AttributeValue, b: &AttributeValue) -> Result<bool, DynamoDbError> {
    Ok(sortable_key(a)? == sortable_key(b)?)
}

/// Compare two items by their sort-key attribute (ascending, type-correct).
fn sort_cmp(a: &Item, b: &Item, sk_name: &str) -> Ordering {
    let key = |item: &Item| {
        item.get(sk_name)
            .and_then(|value| sortable_key(value).ok())
            .unwrap_or_default()
    };
    key(a).cmp(&key(b))
}

/// Evaluate a sort-key condition against an item's sort-key attribute.
fn eval_sort_condition(
    condition: &SortKeyCondition,
    sk: Option<&AttributeValue>,
    maps: &ExpressionMaps,
) -> Result<bool, DynamoDbError> {
    let Some(sk) = sk else {
        // No sort-key attribute on the item → no range can match.
        return Ok(false);
    };
    match condition {
        SortKeyCondition::Compare { op, value, .. } => {
            let bound = resolve_value(value, maps)?;
            compare(sk, op, &bound)
        }
        SortKeyCondition::Between { low, high, .. } => {
            let low = resolve_value(low, maps)?;
            let high = resolve_value(high, maps)?;
            Ok(compare(sk, &CompareOp::Ge, &low)? && compare(sk, &CompareOp::Le, &high)?)
        }
        SortKeyCondition::BeginsWith { prefix, .. } => {
            let prefix = resolve_value(prefix, maps)?;
            match (sk, &prefix) {
                (AttributeValue::S(value), AttributeValue::S(prefix)) => {
                    Ok(value.starts_with(prefix.as_str()))
                }
                _ => Ok(false),
            }
        }
    }
}

/// Type-correct comparison of two attribute values under `op`, via the sortable
/// encoding (lexicographic order of the encoding == value order).
fn compare(
    left: &AttributeValue,
    op: &CompareOp,
    right: &AttributeValue,
) -> Result<bool, DynamoDbError> {
    let ordering = sortable_key(left)?.cmp(&sortable_key(right)?);
    Ok(match op {
        CompareOp::Eq => ordering == Ordering::Equal,
        CompareOp::Ne => ordering != Ordering::Equal,
        CompareOp::Lt => ordering == Ordering::Less,
        CompareOp::Le => ordering != Ordering::Greater,
        CompareOp::Gt => ordering == Ordering::Greater,
        CompareOp::Ge => ordering != Ordering::Less,
    })
}

/// Drop items at or before the `ExclusiveStartKey`'s sort position.
fn apply_start_key(
    matched: Vec<Item>,
    start: &Item,
    range_attr: Option<&str>,
    forward: bool,
) -> Result<Vec<Item>, DynamoDbError> {
    let Some(sk_name) = range_attr else {
        // No sort key: a partition holds at most one item, so any start key has
        // already consumed it.
        return Ok(Vec::new());
    };
    let start_sk = start.get(sk_name).ok_or_else(invalid_start_key)?;
    let start_key = sortable_key(start_sk)?;
    Ok(matched
        .into_iter()
        .filter(|item| {
            let item_key = item
                .get(sk_name)
                .and_then(|value| sortable_key(value).ok())
                .unwrap_or_default();
            if forward {
                item_key > start_key
            } else {
                item_key < start_key
            }
        })
        .collect())
}

fn invalid_start_key() -> DynamoDbError {
    DynamoDbError::ValidationException("The provided starting key is invalid".to_owned())
}

/// Project an item down to just its key attributes (for `LastEvaluatedKey`).
fn key_item(item: &Item, key_schema: &[KeySchemaElement]) -> Item {
    key_schema
        .iter()
        .filter_map(|element| {
            item.get(&element.attribute_name)
                .map(|value| (element.attribute_name.clone(), value.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use extenddb_core::types::CreateTableInput;
    use nimbus_core::TenantId;
    use serde_json::json;

    fn fixture() -> (Arc<Service>, TenantIsolationContext, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let service = Arc::new(Service::new(temp.path()).expect("service"));
        let context = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
        crate::tenant::ensure_tenant(&service, &context).expect("tenant");
        (service, context, temp)
    }

    /// Table "Events" with pk (S) + sk (N) composite key.
    fn create_events(service: &Arc<Service>, context: &TenantIsolationContext) {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "Events",
            "KeySchema": [
                { "AttributeName": "pk", "KeyType": "HASH" },
                { "AttributeName": "sk", "KeyType": "RANGE" }
            ],
            "AttributeDefinitions": [
                { "AttributeName": "pk", "AttributeType": "S" },
                { "AttributeName": "sk", "AttributeType": "N" }
            ],
        }))
        .unwrap();
        control_plane::create_table(service, context, input).expect("create table");
    }

    fn put_event(service: &Arc<Service>, context: &TenantIsolationContext, pk: &str, sk: &str) {
        crate::commands::item::put_item(
            service,
            context,
            serde_json::from_value(json!({
                "TableName": "Events",
                "Item": { "pk": {"S": pk}, "sk": {"N": sk} },
            }))
            .unwrap(),
        )
        .expect("put");
    }

    fn run(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> QueryOutput {
        query(service, context, serde_json::from_value(input).unwrap()).expect("query")
    }

    fn sks(out: &QueryOutput) -> Vec<String> {
        out.items
            .as_ref()
            .unwrap()
            .iter()
            .map(|item| match item.get("sk") {
                Some(AttributeValue::N(n)) => n.clone(),
                other => panic!("unexpected sk: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn query_partition_returns_sorted_items() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        for sk in ["3", "1", "2"] {
            put_event(&service, &ctx, "p1", sk);
        }
        put_event(&service, &ctx, "other", "9"); // different partition
        let out = run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": {"S": "p1"} },
            }),
        );
        assert_eq!(sks(&out), vec!["1", "2", "3"], "ascending by sort key");
        assert_eq!(out.count, 3);
        assert_eq!(out.scanned_count, 3);
    }

    #[test]
    fn query_descending_with_scan_index_forward_false() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        for sk in ["1", "2", "3"] {
            put_event(&service, &ctx, "p1", sk);
        }
        let out = run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": {"S": "p1"} },
                "ScanIndexForward": false,
            }),
        );
        assert_eq!(sks(&out), vec!["3", "2", "1"]);
    }

    #[test]
    fn query_sort_key_range_is_type_correct() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        // Numeric ordering, not lexicographic: 2 < 10 < 100.
        for sk in ["2", "10", "100"] {
            put_event(&service, &ctx, "p1", sk);
        }
        let out = run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p AND sk > :min",
                "ExpressionAttributeValues": { ":p": {"S": "p1"}, ":min": {"N": "9"} },
            }),
        );
        assert_eq!(
            sks(&out),
            vec!["10", "100"],
            "sk > 9 is numeric, not string"
        );
    }

    #[test]
    fn query_between_and_begins_with() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        for sk in ["1", "5", "10", "20"] {
            put_event(&service, &ctx, "p1", sk);
        }
        let between = run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p AND sk BETWEEN :lo AND :hi",
                "ExpressionAttributeValues": { ":p": {"S": "p1"}, ":lo": {"N": "5"}, ":hi": {"N": "10"} },
            }),
        );
        assert_eq!(sks(&between), vec!["5", "10"]);
    }

    #[test]
    fn query_pagination_with_limit_and_exclusive_start_key() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        for sk in ["1", "2", "3", "4"] {
            put_event(&service, &ctx, "p1", sk);
        }
        let page1 = run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": {"S": "p1"} },
                "Limit": 2,
            }),
        );
        assert_eq!(sks(&page1), vec!["1", "2"]);
        let cursor = page1.last_evaluated_key.expect("page truncated");
        let page2 = query(
            &service,
            &ctx,
            serde_json::from_value(json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": {"S": "p1"} },
                "Limit": 2,
                "ExclusiveStartKey": cursor.iter().map(|(k, v)| {
                    (k.clone(), serde_json::to_value(v).unwrap())
                }).collect::<serde_json::Map<_, _>>(),
            }))
            .unwrap(),
        )
        .expect("page2");
        assert_eq!(sks(&page2), vec!["3", "4"]);
        assert!(page2.last_evaluated_key.is_none(), "last page");
    }

    #[test]
    fn query_index_name_is_rejected_until_d4() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        let err = query(
            &service,
            &ctx,
            serde_json::from_value(json!({
                "TableName": "Events",
                "IndexName": "gsi1",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": {"S": "p1"} },
            }))
            .unwrap(),
        )
        .expect_err("GSI query not yet supported");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn query_isolates_partitions() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        put_event(&service, &ctx, "p1", "1");
        put_event(&service, &ctx, "p2", "1");
        let out = run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": {"S": "p2"} },
            }),
        );
        assert_eq!(out.count, 1);
        assert_eq!(
            out.items.unwrap()[0].get("pk"),
            Some(&AttributeValue::S("p2".into()))
        );
    }
}
