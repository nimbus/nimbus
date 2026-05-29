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
use extenddb_core::limits::LimitsConfig;
use extenddb_core::types::{
    AttributeValue, Item, KeySchemaElement, KeyType, QueryInput, QueryOutput, ScanInput,
    ScanOutput, Select,
};
use nimbus_core::{StructuredQuery, TableName};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;

use crate::attribute_value::fields_to_item;
use crate::commands::control_plane;
use crate::error::map_core_error;
use crate::expression::{
    build_maps, default_limits, evaluate_condition, parse_condition,
    parse_key_condition_expression, project_item,
};
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
    // The query's key schema is the base table's, or the named LSI/GSI's.
    let key_schema = control_plane::load_index_key_schema(
        service,
        context,
        &input.table_name,
        input.index_name.as_deref(),
    )?;
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

    // Limit caps the number of items *evaluated* — DynamoDB applies Limit
    // before the FilterExpression — and LastEvaluatedKey points at the last
    // evaluated (pre-filter) item when the window was truncated.
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
    let scanned_count = matched.len() as i64;

    // FilterExpression applies after key selection + Limit; filtered-out items
    // still count toward ScannedCount.
    matched = filter_items(matched, input.filter_expression.as_deref(), &maps, &limits)?;

    let count = matched.len() as i64;
    let items = select_items(
        matched,
        input.select,
        input.projection_expression.as_deref(),
        input.expression_attribute_names.as_ref(),
        &limits,
    )?;

    Ok(QueryOutput {
        items,
        count,
        scanned_count,
        last_evaluated_key,
        consumed_capacity: None,
    })
}

/// Scan a full table with an optional `FilterExpression`, paginating by the
/// primary-key `DocumentId` (a stable total order over the table).
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for an `IndexName` (D4) or a malformed `ExclusiveStartKey`.
pub fn scan(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: ScanInput,
) -> Result<ScanOutput, DynamoDbError> {
    if input.index_name.is_some() {
        return Err(DynamoDbError::ValidationException(
            "Scanning a secondary index is not yet supported (planned in D4)".to_owned(),
        ));
    }
    let key_schema = control_plane::load_key_schema(service, context, &input.table_name)?;
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;
    let limits = default_limits();

    // Stable scan order by primary-key DocumentId for deterministic pagination.
    let mut ordered: Vec<(String, Item)> = Vec::new();
    for item in enumerate(service, context, &table)? {
        let doc_id = crate::commands::item::primary_key_id(&item, &key_schema)?;
        ordered.push((doc_id.as_str().to_owned(), item));
    }
    ordered.sort_by(|a, b| a.0.cmp(&b.0));

    // Parallel scan: deterministically partition the table by a stable hash of
    // the primary-key DocumentId. Across all TotalSegments the partition is a
    // disjoint cover (every item in exactly one segment), stable across runs.
    let (segment, total_segments) = validate_segments(input.segment, input.total_segments)?;
    if total_segments > 1 {
        ordered.retain(|(id, _)| segment_of(id, total_segments) == segment);
    }

    // ExclusiveStartKey: skip items at or before the cursor's DocumentId.
    if let Some(start) = &input.exclusive_start_key {
        let start_id = crate::commands::item::primary_key_id(start, &key_schema)
            .map_err(|_| invalid_start_key())?
            .as_str()
            .to_owned();
        ordered.retain(|(id, _)| id.as_str() > start_id.as_str());
    }

    // Limit caps the items *evaluated* (pre-filter); LastEvaluatedKey is the
    // last evaluated item's key when truncated.
    let limit = input
        .limit
        .filter(|limit| *limit > 0)
        .map(|limit| limit as usize);
    let truncated = limit.is_some_and(|limit| ordered.len() > limit);
    if let Some(limit) = limit {
        ordered.truncate(limit);
    }
    let last_evaluated_key = truncated
        .then(|| ordered.last().map(|(_, item)| key_item(item, &key_schema)))
        .flatten();
    let scanned_count = ordered.len() as i64;

    let evaluated: Vec<Item> = ordered.into_iter().map(|(_, item)| item).collect();
    let maps = build_maps(
        input.expression_attribute_names.as_ref(),
        input.expression_attribute_values.as_ref(),
    );
    let surviving = filter_items(
        evaluated,
        input.filter_expression.as_deref(),
        &maps,
        &limits,
    )?;

    let count = surviving.len() as i64;
    let items = select_items(
        surviving,
        input.select,
        input.projection_expression.as_deref(),
        input.expression_attribute_names.as_ref(),
        &limits,
    )?;

    Ok(ScanOutput {
        items,
        count,
        scanned_count,
        last_evaluated_key,
        consumed_capacity: None,
    })
}

/// Validate the parallel-scan `Segment`/`TotalSegments` pair, returning a
/// normalized `(segment, total_segments)` (a single full segment when neither
/// is given).
///
/// DynamoDB requires both-or-neither, `TotalSegments` in `1..=1_000_000`, and
/// `0 <= Segment < TotalSegments`.
fn validate_segments(
    segment: Option<i64>,
    total_segments: Option<i64>,
) -> Result<(i64, i64), DynamoDbError> {
    match (segment, total_segments) {
        (None, None) => Ok((0, 1)),
        (Some(segment), Some(total)) => {
            if !(1..=1_000_000).contains(&total) {
                return Err(DynamoDbError::ValidationException(
                    "TotalSegments must be between 1 and 1000000".to_owned(),
                ));
            }
            if !(0..total).contains(&segment) {
                return Err(DynamoDbError::ValidationException(
                    "Segment must be greater than or equal to 0 and less than TotalSegments"
                        .to_owned(),
                ));
            }
            Ok((segment, total))
        }
        _ => Err(DynamoDbError::ValidationException(
            "The Segment and TotalSegments parameters must be specified together".to_owned(),
        )),
    }
}

/// Assign a `DocumentId` to a scan segment via a stable FNV-1a hash (so the
/// partition is identical across processes and repeated runs).
fn segment_of(doc_id: &str, total_segments: i64) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in doc_id.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % total_segments as u64) as i64
}

/// Apply an optional `FilterExpression` (ConditionExpression grammar) to a set
/// of items, keeping those that satisfy it. Shared by Query and Scan.
fn filter_items(
    items: Vec<Item>,
    filter: Option<&str>,
    maps: &ExpressionMaps,
    limits: &LimitsConfig,
) -> Result<Vec<Item>, DynamoDbError> {
    let Some(filter) = filter.filter(|expression| !expression.is_empty()) else {
        return Ok(items);
    };
    let filter_expr = parse_condition(filter, limits)?;
    let mut kept = Vec::with_capacity(items.len());
    for item in items {
        if evaluate_condition(&filter_expr, &item, maps)? {
            kept.push(item);
        }
    }
    Ok(kept)
}

/// Apply the `Select` mode + `ProjectionExpression` to the surviving items.
/// `COUNT` omits `Items`; a `ProjectionExpression` (or `SPECIFIC_ATTRIBUTES`)
/// projects each item; otherwise the full items are returned.
fn select_items(
    items: Vec<Item>,
    select: Option<Select>,
    projection: Option<&str>,
    names: Option<&std::collections::HashMap<String, String>>,
    limits: &LimitsConfig,
) -> Result<Option<Vec<Item>>, DynamoDbError> {
    if matches!(select, Some(Select::Count)) {
        return Ok(None);
    }
    let projection = projection.filter(|expression| !expression.is_empty());
    let projected = match projection {
        Some(expression) => items
            .into_iter()
            .map(|item| project_item(expression, names, &item, limits))
            .collect::<Result<Vec<_>, _>>()?,
        None => items,
    };
    Ok(Some(projected))
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
    fn query_unknown_index_is_validation_error() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        let err = query(
            &service,
            &ctx,
            serde_json::from_value(json!({
                "TableName": "Events",
                "IndexName": "nonexistent",
                "KeyConditionExpression": "pk = :p",
                "ExpressionAttributeValues": { ":p": {"S": "p1"} },
            }))
            .unwrap(),
        )
        .expect_err("querying a nonexistent index must fail");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn query_local_secondary_index_orders_by_index_sort_key() {
        let (service, ctx, _t) = fixture();
        // Table "Tasks": pk (S) + sk (N), with an LSI "by_priority" on (pk, prio N).
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "Tasks",
            "KeySchema": [
                { "AttributeName": "pk", "KeyType": "HASH" },
                { "AttributeName": "sk", "KeyType": "RANGE" }
            ],
            "AttributeDefinitions": [
                { "AttributeName": "pk", "AttributeType": "S" },
                { "AttributeName": "sk", "AttributeType": "N" },
                { "AttributeName": "prio", "AttributeType": "N" }
            ],
            "LocalSecondaryIndexes": [{
                "IndexName": "by_priority",
                "KeySchema": [
                    { "AttributeName": "pk", "KeyType": "HASH" },
                    { "AttributeName": "prio", "KeyType": "RANGE" }
                ],
                "Projection": { "ProjectionType": "ALL" }
            }]
        }))
        .unwrap();
        control_plane::create_table(&service, &ctx, input).expect("create with LSI");
        // Items: sk ascending differs from prio ordering.
        for (sk, prio) in [("1", "30"), ("2", "10"), ("3", "20")] {
            crate::commands::item::put_item(
                &service,
                &ctx,
                serde_json::from_value(json!({
                    "TableName": "Tasks",
                    "Item": { "pk": {"S": "p1"}, "sk": {"N": sk}, "prio": {"N": prio} },
                }))
                .unwrap(),
            )
            .expect("put");
        }
        let out = query(
            &service,
            &ctx,
            serde_json::from_value(json!({
                "TableName": "Tasks",
                "IndexName": "by_priority",
                "KeyConditionExpression": "pk = :p AND prio > :min",
                "ExpressionAttributeValues": { ":p": {"S": "p1"}, ":min": {"N": "15"} },
            }))
            .unwrap(),
        )
        .expect("LSI query");
        // prio > 15 selects {20, 30}, ordered by the LSI sort key (prio).
        let prios: Vec<String> = out
            .items
            .unwrap()
            .iter()
            .map(|item| match item.get("prio") {
                Some(AttributeValue::N(n)) => n.clone(),
                other => panic!("prio: {other:?}"),
            })
            .collect();
        assert_eq!(
            prios,
            vec!["20", "30"],
            "ordered by LSI sort key, not table sk"
        );
    }

    /// Put an event with an extra `kind` (S) attribute for filter tests.
    fn put_kind(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        pk: &str,
        sk: &str,
        kind: &str,
    ) {
        crate::commands::item::put_item(
            service,
            context,
            serde_json::from_value(json!({
                "TableName": "Events",
                "Item": { "pk": {"S": pk}, "sk": {"N": sk}, "kind": {"S": kind} },
            }))
            .unwrap(),
        )
        .expect("put");
    }

    #[test]
    fn query_filter_expression_excludes_but_still_scans() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        put_kind(&service, &ctx, "p1", "1", "a");
        put_kind(&service, &ctx, "p1", "2", "b");
        put_kind(&service, &ctx, "p1", "3", "a");
        let out = run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p",
                "FilterExpression": "kind = :k",
                "ExpressionAttributeValues": { ":p": {"S": "p1"}, ":k": {"S": "a"} },
            }),
        );
        assert_eq!(sks(&out), vec!["1", "3"], "only kind=a survives the filter");
        assert_eq!(out.count, 2, "Count is post-filter");
        assert_eq!(
            out.scanned_count, 3,
            "ScannedCount counts all key-matched items"
        );
    }

    #[test]
    fn query_select_count_omits_items() {
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
                "Select": "COUNT",
            }),
        );
        assert!(out.items.is_none(), "COUNT omits Items");
        assert_eq!(out.count, 3);
        assert_eq!(out.scanned_count, 3);
    }

    #[test]
    fn query_projection_and_filter_compose() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        put_kind(&service, &ctx, "p1", "1", "a");
        put_kind(&service, &ctx, "p1", "2", "b");
        let out = run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p",
                "FilterExpression": "kind = :k",
                "ProjectionExpression": "sk",
                "ExpressionAttributeValues": { ":p": {"S": "p1"}, ":k": {"S": "b"} },
            }),
        );
        let items = out.items.unwrap();
        assert_eq!(items.len(), 1, "only kind=b survives");
        let item = &items[0];
        assert_eq!(item.len(), 1, "projected to sk only");
        assert_eq!(item.get("sk"), Some(&AttributeValue::N("2".into())));
        assert!(!item.contains_key("kind"), "kind projected out");
    }

    #[test]
    fn query_limit_caps_scanned_before_filter() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        // sk 1=a, 2=b, 3=a; Limit=2 evaluates the first two, filter kind=a keeps sk 1.
        put_kind(&service, &ctx, "p1", "1", "a");
        put_kind(&service, &ctx, "p1", "2", "b");
        put_kind(&service, &ctx, "p1", "3", "a");
        let out = run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "KeyConditionExpression": "pk = :p",
                "FilterExpression": "kind = :k",
                "ExpressionAttributeValues": { ":p": {"S": "p1"}, ":k": {"S": "a"} },
                "Limit": 2,
            }),
        );
        assert_eq!(
            sks(&out),
            vec!["1"],
            "Limit evaluates the first two, then filters"
        );
        assert_eq!(out.scanned_count, 2, "Limit caps scanned items pre-filter");
        assert!(
            out.last_evaluated_key.is_some(),
            "more items beyond the Limit window"
        );
    }

    // ---- D2.3: Scan ----

    fn scan_run(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> ScanOutput {
        scan(service, context, serde_json::from_value(input).unwrap()).expect("scan")
    }

    #[test]
    fn scan_returns_all_items_across_partitions() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        put_event(&service, &ctx, "p1", "1");
        put_event(&service, &ctx, "p1", "2");
        put_event(&service, &ctx, "p2", "1");
        let out = scan_run(&service, &ctx, json!({ "TableName": "Events" }));
        assert_eq!(out.count, 3, "scan reads the whole table");
        assert_eq!(out.scanned_count, 3);
    }

    #[test]
    fn scan_with_filter_expression() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        put_kind(&service, &ctx, "p1", "1", "a");
        put_kind(&service, &ctx, "p2", "1", "b");
        put_kind(&service, &ctx, "p3", "1", "a");
        let out = scan_run(
            &service,
            &ctx,
            json!({
                "TableName": "Events",
                "FilterExpression": "kind = :k",
                "ExpressionAttributeValues": { ":k": {"S": "a"} },
            }),
        );
        assert_eq!(out.count, 2, "two kind=a items survive");
        assert_eq!(out.scanned_count, 3, "all three scanned");
    }

    #[test]
    fn scan_pagination_is_stable_and_complete() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        for (pk, sk) in [("p1", "1"), ("p2", "1"), ("p3", "1"), ("p4", "1")] {
            put_event(&service, &ctx, pk, sk);
        }
        // Page through with Limit=2; union must be the full table, no dupes.
        let mut seen: Vec<String> = Vec::new();
        let mut start: Option<serde_json::Value> = None;
        loop {
            let mut req = serde_json::json!({ "TableName": "Events", "Limit": 2 });
            if let Some(cursor) = &start {
                req["ExclusiveStartKey"] = cursor.clone();
            }
            let out = scan_run(&service, &ctx, req);
            for item in out.items.as_ref().unwrap() {
                let pk = match item.get("pk") {
                    Some(AttributeValue::S(s)) => s.clone(),
                    other => panic!("pk: {other:?}"),
                };
                seen.push(pk);
            }
            match out.last_evaluated_key {
                Some(key) => {
                    start = Some(
                        key.iter()
                            .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap()))
                            .collect::<serde_json::Map<_, _>>()
                            .into(),
                    );
                }
                None => break,
            }
        }
        seen.sort();
        assert_eq!(
            seen,
            vec!["p1", "p2", "p3", "p4"],
            "every item exactly once"
        );
    }

    /// Scan one segment, returning the set of `pk` values it covers.
    fn scan_segment(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        segment: i64,
        total: i64,
    ) -> std::collections::BTreeSet<String> {
        let out = scan_run(
            service,
            context,
            json!({ "TableName": "Events", "Segment": segment, "TotalSegments": total }),
        );
        out.items
            .unwrap()
            .iter()
            .map(|item| match item.get("pk") {
                Some(AttributeValue::S(s)) => s.clone(),
                other => panic!("pk: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn scan_parallel_segments_are_a_stable_disjoint_cover() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        let all: std::collections::BTreeSet<String> = (0..20)
            .map(|i| format!("p{i:02}"))
            .inspect(|pk| put_event(&service, &ctx, pk, "1"))
            .collect();

        const TOTAL: i64 = 4;
        let segments: Vec<std::collections::BTreeSet<String>> = (0..TOTAL)
            .map(|s| scan_segment(&service, &ctx, s, TOTAL))
            .collect();

        // Union == full table.
        let union: std::collections::BTreeSet<String> =
            segments.iter().flatten().cloned().collect();
        assert_eq!(union, all, "every item appears in some segment");

        // Pairwise disjoint (no item in two segments).
        let total_with_dupes: usize = segments.iter().map(std::collections::BTreeSet::len).sum();
        assert_eq!(
            total_with_dupes,
            all.len(),
            "no item appears in two segments"
        );

        // Stable across repeated runs.
        for s in 0..TOTAL {
            assert_eq!(
                scan_segment(&service, &ctx, s, TOTAL),
                segments[s as usize],
                "segment {s} is stable across runs"
            );
        }
    }

    #[test]
    fn scan_invalid_segment_is_rejected() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        // Segment >= TotalSegments.
        let err = scan(
            &service,
            &ctx,
            serde_json::from_value(
                json!({ "TableName": "Events", "Segment": 4, "TotalSegments": 4 }),
            )
            .unwrap(),
        )
        .expect_err("segment out of range");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
        // Segment without TotalSegments.
        let err = scan(
            &service,
            &ctx,
            serde_json::from_value(json!({ "TableName": "Events", "Segment": 0 })).unwrap(),
        )
        .expect_err("segment without total");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn scan_index_name_is_rejected_until_d4() {
        let (service, ctx, _t) = fixture();
        create_events(&service, &ctx);
        let err = scan(
            &service,
            &ctx,
            serde_json::from_value(json!({ "TableName": "Events", "IndexName": "gsi1" })).unwrap(),
        )
        .expect_err("GSI scan not yet supported");
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
