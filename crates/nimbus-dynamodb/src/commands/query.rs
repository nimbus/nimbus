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
use nimbus_core::{DocumentId, StructuredQuery, TableName};
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;

use crate::attribute_value::fields_to_item;
use crate::commands::{control_plane, item};
use crate::error::map_core_error;
use crate::expression::{
    build_maps, default_limits, evaluate_condition, parse_condition,
    parse_key_condition_expression, project_item,
};
use crate::key::{encode_key, sortable_key};
use crate::tenant::caller_principal;

/// Query a single partition with an optional sort-key range.
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for a missing/invalid `KeyConditionExpression`, an `IndexName` (GSI/LSI is
/// D4), or a malformed `ExclusiveStartKey`.
pub fn query(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: QueryInput,
) -> Result<QueryOutput, DynamoDbError> {
    // The query keys on the base table's schema, or the named LSI/GSI's.
    let shape = control_plane::load_index_query_shape(
        engine,
        context,
        &input.table_name,
        input.index_name.as_deref(),
    )?;
    let key_schema = shape.key_schema.clone();
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
    // Encode the query's partition-key value once. A non-scalar *query* value is
    // a client `ValidationException` (propagated here); a non-scalar *item*
    // value is skipped in the loop below (sparse-index semantics), not an error.
    let pk_key = sortable_key(&pk_value)?;

    // Select the partition, then apply the optional sort-key condition.
    let mut matched: Vec<Item> = Vec::new();
    for item in enumerate_query_partition(
        engine,
        context,
        &table,
        &shape.table_key_schema,
        &hash_attr,
        &pk_value,
    )? {
        let Some(item_pk) = item.get(&hash_attr) else {
            continue;
        };
        // An item whose partition-key attribute is non-scalar (M / L / BOOL /
        // NULL) cannot match a scalar key condition — skip it rather than
        // aborting the whole Query on the un-encodable value (F7).
        let Ok(item_key) = sortable_key(item_pk) else {
            continue;
        };
        if item_key != pk_key {
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
    // Restrict each item to the index's projected attributes (KEYS_ONLY/INCLUDE).
    restrict_to_projection(&mut matched, shape.projected_attributes.as_ref());
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

/// Scan a table — or a secondary index when `IndexName` is set, returning only
/// the items that carry the index's key attributes (sparse) projected to the
/// index's attribute set — with an optional `FilterExpression`, paginating by
/// the base table's primary-key `DocumentId` (a stable total order).
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for an unknown `IndexName` or a malformed `ExclusiveStartKey`.
pub fn scan(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: ScanInput,
) -> Result<ScanOutput, DynamoDbError> {
    let shape = control_plane::load_index_query_shape(
        engine,
        context,
        &input.table_name,
        input.index_name.as_deref(),
    )?;
    // The physical storage key is always the base table's primary key.
    let key_schema = shape.table_key_schema.clone();
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;
    let limits = default_limits();

    // Index keys an item must carry to appear in an index scan (sparse index).
    let index_key_attrs: Vec<String> = input
        .index_name
        .as_ref()
        .map(|_| {
            shape
                .key_schema
                .iter()
                .map(|element| element.attribute_name.clone())
                .collect()
        })
        .unwrap_or_default();

    // Stable scan order by primary-key DocumentId for deterministic pagination.
    let mut ordered: Vec<(String, Item)> = Vec::new();
    for item in enumerate(engine, context, &table)? {
        // Sparse index: skip items missing any of the index's key attributes.
        if !index_key_attrs.iter().all(|attr| item.contains_key(attr)) {
            continue;
        }
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
    let mut surviving = filter_items(
        evaluated,
        input.filter_expression.as_deref(),
        &maps,
        &limits,
    )?;

    let count = surviving.len() as i64;
    // Restrict each item to the index's projected attributes (KEYS_ONLY/INCLUDE).
    restrict_to_projection(&mut surviving, shape.projected_attributes.as_ref());
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

/// Restrict each item to an index's projected attribute set (KEYS_ONLY/INCLUDE).
/// `None` (base table or `ALL` projection) leaves every attribute in place.
fn restrict_to_projection(
    items: &mut [Item],
    projected: Option<&std::collections::BTreeSet<String>>,
) {
    let Some(projected) = projected else {
        return;
    };
    for item in items {
        item.retain(|name, _| projected.contains(name));
    }
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table: &TableName,
) -> Result<Vec<Item>, DynamoDbError> {
    let documents = match engine.query_documents_structured_with_principal(
        context.tenant_id(),
        table,
        &StructuredQuery::default(),
        &caller_principal(context),
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

fn enumerate_query_partition(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table: &TableName,
    table_key_schema: &[KeySchemaElement],
    hash_attr: &str,
    pk_value: &AttributeValue,
) -> Result<Vec<Item>, DynamoDbError> {
    let physical_hash_attr = attr_of_kind(table_key_schema, KeyType::Hash);
    if physical_hash_attr.as_deref() != Some(hash_attr) {
        return enumerate(engine, context, table);
    }

    let encoded_pk = encode_key(pk_value, None)?;
    if attr_of_kind(table_key_schema, KeyType::Range).is_none() {
        let id = DocumentId::from_key(&encoded_pk).map_err(map_core_error)?;
        return item::read_item(engine, context, table, id).map(|item| item.into_iter().collect());
    }

    let id_prefix = format!("{encoded_pk}.");
    let documents = engine
        .scan_documents_by_id_prefix_cancellable(
            context.tenant_id(),
            table,
            &id_prefix,
            &caller_principal(context),
            &mut || Ok(()),
        )
        .map_err(map_core_error)?;
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

/// Type-correct comparison of an item's sort-key value (`left`) against a query
/// bound (`right`) under `op`, via the sortable encoding (lexicographic order of
/// the encoding == value order).
///
/// `left` is the stored item's attribute: if it is non-scalar (M / L / BOOL /
/// NULL) it cannot satisfy a scalar comparison, so the item is treated as a
/// no-match (skipped) rather than aborting the whole Query (F7). `right` is the
/// client-supplied bound, so a non-scalar bound there is still a propagated
/// `ValidationException`.
fn compare(
    left: &AttributeValue,
    op: &CompareOp,
    right: &AttributeValue,
) -> Result<bool, DynamoDbError> {
    let Ok(left_key) = sortable_key(left) else {
        return Ok(false);
    };
    let ordering = left_key.cmp(&sortable_key(right)?);
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
mod tests;
