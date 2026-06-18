use super::*;
use extenddb_core::types::CreateTableInput;
use nimbus_core::TenantId;
use serde_json::json;

fn fixture() -> (Arc<Engine>, TenantIsolationContext, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
    let context = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
    crate::tenant::ensure_tenant(&engine, &context).expect("tenant");
    (engine, context, temp)
}

/// Table "Events" with pk (S) + sk (N) composite key.
fn create_events(engine: &Arc<Engine>, context: &TenantIsolationContext) {
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
    control_plane::create_table(engine, context, input).expect("create table");
}

fn put_event(engine: &Arc<Engine>, context: &TenantIsolationContext, pk: &str, sk: &str) {
    crate::commands::item::put_item(
        engine,
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: serde_json::Value,
) -> QueryOutput {
    query(engine, context, serde_json::from_value(input).unwrap()).expect("query")
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    for sk in ["3", "1", "2"] {
        put_event(&engine, &ctx, "p1", sk);
    }
    put_event(&engine, &ctx, "other", "9"); // different partition
    let out = run(
        &engine,
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
fn query_partition_does_not_decode_unrelated_partition_rows() {
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    put_event(&engine, &ctx, "target", "1");

    let corrupt_id = DocumentId::from_key(
        encode_key(
            &AttributeValue::S("other".to_owned()),
            Some(&AttributeValue::N("1".to_owned())),
        )
        .expect("corrupt-row id should encode"),
    )
    .expect("corrupt-row id should be valid");
    let mut corrupt_fields = serde_json::Map::new();
    corrupt_fields.insert("pk".to_owned(), json!("not AttributeValue wire JSON"));
    engine
        .insert_document_with_id(
            ctx.tenant_id(),
            TableName::new("Events").expect("table name should parse"),
            corrupt_id,
            corrupt_fields,
        )
        .expect("seed corrupt off-partition row");

    let out = run(
        &engine,
        &ctx,
        json!({
            "TableName": "Events",
            "KeyConditionExpression": "pk = :p",
            "ExpressionAttributeValues": { ":p": {"S": "target"} },
        }),
    );

    assert_eq!(out.count, 1);
    assert_eq!(
        out.items.unwrap()[0].get("pk"),
        Some(&AttributeValue::S("target".to_owned()))
    );
}

#[test]
fn query_descending_with_scan_index_forward_false() {
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    for sk in ["1", "2", "3"] {
        put_event(&engine, &ctx, "p1", sk);
    }
    let out = run(
        &engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    // Numeric ordering, not lexicographic: 2 < 10 < 100.
    for sk in ["2", "10", "100"] {
        put_event(&engine, &ctx, "p1", sk);
    }
    let out = run(
        &engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    for sk in ["1", "5", "10", "20"] {
        put_event(&engine, &ctx, "p1", sk);
    }
    let between = run(
        &engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    for sk in ["1", "2", "3", "4"] {
        put_event(&engine, &ctx, "p1", sk);
    }
    let page1 = run(
        &engine,
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
        &engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    let err = query(
        &engine,
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

/// Create a table with one GSI; `extra_attrs` are added to AttributeDefinitions.
fn create_with_gsi(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table: &str,
    gsi_key: serde_json::Value,
    projection: serde_json::Value,
    extra_attrs: serde_json::Value,
) {
    let mut attrs = serde_json::json!([{ "AttributeName": "pk", "AttributeType": "S" }]);
    attrs
        .as_array_mut()
        .unwrap()
        .extend(extra_attrs.as_array().unwrap().iter().cloned());
    let input: CreateTableInput = serde_json::from_value(json!({
        "TableName": table,
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": attrs,
        "GlobalSecondaryIndexes": [{
            "IndexName": "gsi",
            "KeySchema": gsi_key,
            "Projection": projection
        }]
    }))
    .unwrap();
    control_plane::create_table(engine, context, input).expect("create with GSI");
}

fn put_json(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table: &str,
    item: serde_json::Value,
) {
    crate::commands::item::put_item(
        engine,
        context,
        serde_json::from_value(json!({ "TableName": table, "Item": item })).unwrap(),
    )
    .expect("put");
}

#[test]
fn query_gsi_projection_keys_only_and_include() {
    let (engine, ctx, _t) = fixture();
    // GSI keyed on `g` (S); KEYS_ONLY projects only {pk, g}.
    create_with_gsi(
        &engine,
        &ctx,
        "KeysOnly",
        json!([{ "AttributeName": "g", "KeyType": "HASH" }]),
        json!({ "ProjectionType": "KEYS_ONLY" }),
        json!([{ "AttributeName": "g", "AttributeType": "S" }]),
    );
    put_json(
        &engine,
        &ctx,
        "KeysOnly",
        json!({ "pk": {"S": "a"}, "g": {"S": "x"}, "extra": {"N": "9"} }),
    );
    let out = query(
        &engine,
        &ctx,
        serde_json::from_value(json!({
            "TableName": "KeysOnly",
            "IndexName": "gsi",
            "KeyConditionExpression": "g = :g",
            "ExpressionAttributeValues": { ":g": {"S": "x"} },
        }))
        .unwrap(),
    )
    .expect("keys-only query");
    let item = &out.items.unwrap()[0];
    assert!(item.contains_key("pk") && item.contains_key("g"));
    assert!(
        !item.contains_key("extra"),
        "KEYS_ONLY drops non-projected attrs"
    );

    // INCLUDE projects {pk, g, extra}.
    create_with_gsi(
        &engine,
        &ctx,
        "Included",
        json!([{ "AttributeName": "g", "KeyType": "HASH" }]),
        json!({ "ProjectionType": "INCLUDE", "NonKeyAttributes": ["extra"] }),
        json!([{ "AttributeName": "g", "AttributeType": "S" }]),
    );
    put_json(
        &engine,
        &ctx,
        "Included",
        json!({ "pk": {"S": "a"}, "g": {"S": "x"}, "extra": {"N": "9"}, "other": {"N": "1"} }),
    );
    let out = query(
        &engine,
        &ctx,
        serde_json::from_value(json!({
            "TableName": "Included",
            "IndexName": "gsi",
            "KeyConditionExpression": "g = :g",
            "ExpressionAttributeValues": { ":g": {"S": "x"} },
        }))
        .unwrap(),
    )
    .expect("include query");
    let item = &out.items.unwrap()[0];
    assert!(
        item.contains_key("extra"),
        "INCLUDE keeps the named non-key attr"
    );
    assert!(!item.contains_key("other"), "INCLUDE drops unnamed attrs");
}

#[test]
fn query_gsi_numeric_range_preserves_precision_beyond_f64() {
    let (engine, ctx, _t) = fixture();
    // GSI keyed on (g S, n N) — a numeric sort key.
    create_with_gsi(
        &engine,
        &ctx,
        "Big",
        json!([
            { "AttributeName": "g", "KeyType": "HASH" },
            { "AttributeName": "n", "KeyType": "RANGE" }
        ]),
        json!({ "ProjectionType": "ALL" }),
        json!([
            { "AttributeName": "g", "AttributeType": "S" },
            { "AttributeName": "n", "AttributeType": "N" }
        ]),
    );
    // These 18-digit values are indistinguishable as f64 but must order.
    for (sk, n) in [("a", "100000000000000002"), ("b", "100000000000000001")] {
        put_json(
            &engine,
            &ctx,
            "Big",
            json!({ "pk": {"S": sk}, "g": {"S": "p"}, "n": {"N": n} }),
        );
    }
    let out = query(
        &engine,
        &ctx,
        serde_json::from_value(json!({
            "TableName": "Big",
            "IndexName": "gsi",
            "KeyConditionExpression": "g = :g AND n > :min",
            "ExpressionAttributeValues": { ":g": {"S": "p"}, ":min": {"N": "100000000000000000"} },
        }))
        .unwrap(),
    )
    .expect("numeric range query");
    let ns: Vec<String> = out
        .items
        .unwrap()
        .iter()
        .map(|item| match item.get("n") {
            Some(AttributeValue::N(n)) => n.clone(),
            other => panic!("n: {other:?}"),
        })
        .collect();
    assert_eq!(
        ns,
        vec!["100000000000000001", "100000000000000002"],
        "full-precision numeric ordering (f64 would collapse these)"
    );
}

#[test]
fn query_gsi_binary_range_is_byte_wise() {
    let (engine, ctx, _t) = fixture();
    // GSI keyed on (g S, b B) — a binary sort key.
    create_with_gsi(
        &engine,
        &ctx,
        "Bins",
        json!([
            { "AttributeName": "g", "KeyType": "HASH" },
            { "AttributeName": "b", "KeyType": "RANGE" }
        ]),
        json!({ "ProjectionType": "ALL" }),
        json!([
            { "AttributeName": "g", "AttributeType": "S" },
            { "AttributeName": "b", "AttributeType": "B" }
        ]),
    );
    // Base64 of bytes [0x01], [0x02], [0xff]; byte-wise order is 01 < 02 < ff.
    for (sk, b64) in [("a", "/w=="), ("b", "AQ=="), ("c", "Ag==")] {
        put_json(
            &engine,
            &ctx,
            "Bins",
            json!({ "pk": {"S": sk}, "g": {"S": "p"}, "b": {"B": b64} }),
        );
    }
    let out = query(
        &engine,
        &ctx,
        serde_json::from_value(json!({
            "TableName": "Bins",
            "IndexName": "gsi",
            "KeyConditionExpression": "g = :g",
            "ExpressionAttributeValues": { ":g": {"S": "p"} },
        }))
        .unwrap(),
    )
    .expect("binary range query");
    let bs: Vec<Vec<u8>> = out
        .items
        .unwrap()
        .iter()
        .map(|item| match item.get("b") {
            Some(AttributeValue::B(bytes)) => bytes.clone(),
            other => panic!("b: {other:?}"),
        })
        .collect();
    assert_eq!(
        bs,
        vec![vec![0x01u8], vec![0x02u8], vec![0xffu8]],
        "binary sort key orders byte-wise (0x01 < 0x02 < 0xff)"
    );
}

#[test]
fn scan_index_is_sparse_and_projected() {
    let (engine, ctx, _t) = fixture();
    create_with_gsi(
        &engine,
        &ctx,
        "Sparse",
        json!([{ "AttributeName": "g", "KeyType": "HASH" }]),
        json!({ "ProjectionType": "KEYS_ONLY" }),
        json!([{ "AttributeName": "g", "AttributeType": "S" }]),
    );
    // Two items have `g` (in the index), one does not (sparse).
    put_json(
        &engine,
        &ctx,
        "Sparse",
        json!({ "pk": {"S": "a"}, "g": {"S": "x"}, "extra": {"N": "1"} }),
    );
    put_json(
        &engine,
        &ctx,
        "Sparse",
        json!({ "pk": {"S": "b"}, "g": {"S": "y"}, "extra": {"N": "2"} }),
    );
    put_json(&engine, &ctx, "Sparse", json!({ "pk": {"S": "c"} }));
    let out = scan(
        &engine,
        &ctx,
        serde_json::from_value(json!({ "TableName": "Sparse", "IndexName": "gsi" })).unwrap(),
    )
    .expect("index scan");
    assert_eq!(out.count, 2, "only items present in the index are scanned");
    for item in out.items.unwrap() {
        assert!(item.contains_key("g"), "indexed item");
        assert!(
            !item.contains_key("extra"),
            "KEYS_ONLY drops non-projected attrs"
        );
    }
}

/// F7: a GSI Query over a table containing heterogeneous items — some with
/// the indexed key attribute non-scalar (M / L / BOOL / NULL) or absent —
/// must return only the matching scalar items, *skipping* the others, rather
/// than aborting the whole request with a `ValidationException`.
#[test]
fn query_gsi_skips_non_scalar_and_absent_index_keys() {
    let (engine, ctx, _t) = fixture();
    create_with_gsi(
        &engine,
        &ctx,
        "Hetero",
        json!([{ "AttributeName": "g", "KeyType": "HASH" }]),
        json!({ "ProjectionType": "ALL" }),
        json!([{ "AttributeName": "g", "AttributeType": "S" }]),
    );
    // A matching scalar, a different scalar, and three items the index can't
    // key: a Map, a List, and one with no `g` at all.
    put_json(
        &engine,
        &ctx,
        "Hetero",
        json!({ "pk": {"S": "a"}, "g": {"S": "match"} }),
    );
    put_json(
        &engine,
        &ctx,
        "Hetero",
        json!({ "pk": {"S": "b"}, "g": {"S": "other"} }),
    );
    put_json(
        &engine,
        &ctx,
        "Hetero",
        json!({ "pk": {"S": "c"}, "g": {"M": { "nested": {"S": "x"} }} }),
    );
    put_json(
        &engine,
        &ctx,
        "Hetero",
        json!({ "pk": {"S": "d"}, "g": {"L": [{"S": "y"}]} }),
    );
    put_json(&engine, &ctx, "Hetero", json!({ "pk": {"S": "e"} }));

    let out = query(
        &engine,
        &ctx,
        serde_json::from_value(json!({
            "TableName": "Hetero",
            "IndexName": "gsi",
            "KeyConditionExpression": "g = :g",
            "ExpressionAttributeValues": { ":g": {"S": "match"} }
        }))
        .unwrap(),
    )
    .expect("non-scalar index keys must be skipped, not error the Query");
    assert_eq!(out.count, 1, "only the matching scalar item is returned");
    let items = out.items.unwrap();
    assert_eq!(items[0].get("pk"), Some(&AttributeValue::S("a".into())));
}

#[test]
fn query_local_secondary_index_orders_by_index_sort_key() {
    let (engine, ctx, _t) = fixture();
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
    control_plane::create_table(&engine, &ctx, input).expect("create with LSI");
    // Items: sk ascending differs from prio ordering.
    for (sk, prio) in [("1", "30"), ("2", "10"), ("3", "20")] {
        crate::commands::item::put_item(
            &engine,
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
        &engine,
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    pk: &str,
    sk: &str,
    kind: &str,
) {
    crate::commands::item::put_item(
        engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    put_kind(&engine, &ctx, "p1", "1", "a");
    put_kind(&engine, &ctx, "p1", "2", "b");
    put_kind(&engine, &ctx, "p1", "3", "a");
    let out = run(
        &engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    for sk in ["1", "2", "3"] {
        put_event(&engine, &ctx, "p1", sk);
    }
    let out = run(
        &engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    put_kind(&engine, &ctx, "p1", "1", "a");
    put_kind(&engine, &ctx, "p1", "2", "b");
    let out = run(
        &engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    // sk 1=a, 2=b, 3=a; Limit=2 evaluates the first two, filter kind=a keeps sk 1.
    put_kind(&engine, &ctx, "p1", "1", "a");
    put_kind(&engine, &ctx, "p1", "2", "b");
    put_kind(&engine, &ctx, "p1", "3", "a");
    let out = run(
        &engine,
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: serde_json::Value,
) -> ScanOutput {
    scan(engine, context, serde_json::from_value(input).unwrap()).expect("scan")
}

#[test]
fn scan_returns_all_items_across_partitions() {
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    put_event(&engine, &ctx, "p1", "1");
    put_event(&engine, &ctx, "p1", "2");
    put_event(&engine, &ctx, "p2", "1");
    let out = scan_run(&engine, &ctx, json!({ "TableName": "Events" }));
    assert_eq!(out.count, 3, "scan reads the whole table");
    assert_eq!(out.scanned_count, 3);
}

#[test]
fn scan_with_filter_expression() {
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    put_kind(&engine, &ctx, "p1", "1", "a");
    put_kind(&engine, &ctx, "p2", "1", "b");
    put_kind(&engine, &ctx, "p3", "1", "a");
    let out = scan_run(
        &engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    for (pk, sk) in [("p1", "1"), ("p2", "1"), ("p3", "1"), ("p4", "1")] {
        put_event(&engine, &ctx, pk, sk);
    }
    // Page through with Limit=2; union must be the full table, no dupes.
    let mut seen: Vec<String> = Vec::new();
    let mut start: Option<serde_json::Value> = None;
    loop {
        let mut req = serde_json::json!({ "TableName": "Events", "Limit": 2 });
        if let Some(cursor) = &start {
            req["ExclusiveStartKey"] = cursor.clone();
        }
        let out = scan_run(&engine, &ctx, req);
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    segment: i64,
    total: i64,
) -> std::collections::BTreeSet<String> {
    let out = scan_run(
        engine,
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
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    let all: std::collections::BTreeSet<String> = (0..20)
        .map(|i| format!("p{i:02}"))
        .inspect(|pk| put_event(&engine, &ctx, pk, "1"))
        .collect();

    const TOTAL: i64 = 4;
    let segments: Vec<std::collections::BTreeSet<String>> = (0..TOTAL)
        .map(|s| scan_segment(&engine, &ctx, s, TOTAL))
        .collect();

    // Union == full table.
    let union: std::collections::BTreeSet<String> = segments.iter().flatten().cloned().collect();
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
            scan_segment(&engine, &ctx, s, TOTAL),
            segments[s as usize],
            "segment {s} is stable across runs"
        );
    }
}

#[test]
fn scan_invalid_segment_is_rejected() {
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    // Segment >= TotalSegments.
    let err = scan(
        &engine,
        &ctx,
        serde_json::from_value(json!({ "TableName": "Events", "Segment": 4, "TotalSegments": 4 }))
            .unwrap(),
    )
    .expect_err("segment out of range");
    assert!(matches!(err, DynamoDbError::ValidationException(_)));
    // Segment without TotalSegments.
    let err = scan(
        &engine,
        &ctx,
        serde_json::from_value(json!({ "TableName": "Events", "Segment": 0 })).unwrap(),
    )
    .expect_err("segment without total");
    assert!(matches!(err, DynamoDbError::ValidationException(_)));
}

#[test]
fn scan_unknown_index_name_is_rejected() {
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    // "Events" has no index named "gsi1"; scanning a known index is covered by
    // `scan_index_is_sparse_and_projected`.
    let err = scan(
        &engine,
        &ctx,
        serde_json::from_value(json!({ "TableName": "Events", "IndexName": "gsi1" })).unwrap(),
    )
    .expect_err("scanning a nonexistent index must fail");
    match err {
        DynamoDbError::ValidationException(message) => {
            assert!(
                message.contains("does not have the specified index") && message.contains("gsi1"),
                "validation error should name the unknown index: {message}"
            );
        }
        other => panic!("expected ValidationException for an unknown index, got {other:?}"),
    }
}

#[test]
fn query_isolates_partitions() {
    let (engine, ctx, _t) = fixture();
    create_events(&engine, &ctx);
    put_event(&engine, &ctx, "p1", "1");
    put_event(&engine, &ctx, "p2", "1");
    let out = run(
        &engine,
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

#[test]
fn query_gsi_with_consistent_read_is_served_consistently() {
    // D4.4 decision (DDB-DIV-010): real DynamoDB rejects ConsistentRead=true
    // on a GSI Query with ValidationException (GSIs are eventually
    // consistent). Nimbus's single store is strongly consistent, so it
    // accepts the flag and serves a consistent result — a strict upgrade.
    let (engine, ctx, _t) = fixture();
    create_with_gsi(
        &engine,
        &ctx,
        "Strong",
        json!([{ "AttributeName": "g", "KeyType": "HASH" }]),
        json!({ "ProjectionType": "ALL" }),
        json!([{ "AttributeName": "g", "AttributeType": "S" }]),
    );
    put_json(
        &engine,
        &ctx,
        "Strong",
        json!({ "pk": { "S": "a" }, "g": { "S": "grp" }, "v": { "N": "1" } }),
    );
    let out = query(
        &engine,
        &ctx,
        serde_json::from_value(json!({
            "TableName": "Strong",
            "IndexName": "gsi",
            "KeyConditionExpression": "g = :g",
            "ExpressionAttributeValues": { ":g": { "S": "grp" } },
            "ConsistentRead": true,
        }))
        .unwrap(),
    )
    .expect("ConsistentRead on a GSI is accepted, not a ValidationException");
    assert_eq!(out.count, 1, "the GSI query returns the item");
    assert_eq!(
        out.items.unwrap()[0].get("v"),
        Some(&AttributeValue::N("1".into())),
        "and the result reflects the latest write (strongly consistent)"
    );
}
