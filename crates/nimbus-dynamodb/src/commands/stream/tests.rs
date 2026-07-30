//! The stream API surface: DescribeStream, GetShardIterator, ListStreams, and
//! the shaping and paging half of GetRecords.
//!
//! This module also owns the fixtures every stream test builds on — the engine
//! and tenant, the streamed-table constructors, the write helpers, and the
//! iterator decoding — so the child modules below can be about one concept each
//! rather than about their own setup.

mod authorization;
mod capture;
mod retention;

use super::*;
use extenddb_core::types::{AttributeValue, CreateTableInput};
use nimbus_core::TenantId;
use serde_json::json;

fn fixture() -> (Arc<Engine>, TenantIsolationContext, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("tempdir");
    let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
    let context = crate::tenant::test_context(TenantId::new("acme").unwrap(), "test");
    crate::tenant::ensure_tenant(&engine, &context).expect("tenant");
    (engine, context, temp)
}

#[test]
fn shape_record_rejects_corrupt_event_name() {
    let event = StoredEvent {
        seq: 7,
        created: 0,
        event_name: "UPSERT".to_owned(),
        keys: json!({ "pk": { "S": "k7" } }).as_object().unwrap().clone(),
        old_image: None,
        old_image_times: None,
        new_image: None,
        new_image_retained_update: None,
        user_identity: None,
        committed_at: 0,
    };

    let error = shape_record(&event, StreamViewType::KeysOnly)
        .expect_err("corrupt event name should not be coerced to INSERT");
    match error {
        DynamoDbError::InternalServerError(message) => assert!(
            message.contains("corrupt stream event name: UPSERT"),
            "unexpected error: {message}"
        ),
        other => panic!("expected corrupt event-name error, got {other:?}"),
    }
}

/// Create a stream-enabled table and return its stream ARN.
fn create_streamed(engine: &Arc<Engine>, context: &TenantIsolationContext) -> String {
    let input: CreateTableInput = serde_json::from_value(json!({
        "TableName": "events",
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_IMAGE" }
    }))
    .unwrap();
    control_plane::create_table(engine, context, input)
        .expect("create")
        .table_description
        .latest_stream_arn
        .expect("stream arn")
}

#[test]
fn describe_stream_returns_single_open_shard() {
    let (engine, ctx, _t) = fixture();
    let arn = create_streamed(&engine, &ctx);
    let out = describe_stream(
        &engine,
        &ctx,
        DescribeStreamInput {
            stream_arn: arn.clone(),
            limit: None,
            exclusive_start_shard_id: None,
        },
    )
    .expect("describe stream");
    let desc = out.stream_description;
    assert_eq!(desc.stream_arn, arn);
    assert_eq!(desc.stream_status, StreamStatus::Enabled);
    assert_eq!(desc.stream_view_type, StreamViewType::NewImage);
    assert_eq!(desc.table_name, "events");
    assert_eq!(desc.shards.len(), 1, "single shard");
    assert!(desc.shards[0].shard_id.starts_with("shardId-"));
    assert!(
        desc.shards[0]
            .sequence_number_range
            .ending_sequence_number
            .is_none(),
        "open shard"
    );
}

#[test]
fn describe_stream_unknown_arn_is_resource_not_found() {
    let (engine, ctx, _t) = fixture();
    create_streamed(&engine, &ctx);
    let err = describe_stream(
        &engine,
        &ctx,
        DescribeStreamInput {
            stream_arn: "arn:aws:dynamodb:ddblocal:000000000000:table/events/stream/wrong-label"
                .to_owned(),
            limit: None,
            exclusive_start_shard_id: None,
        },
    )
    .expect_err("unknown stream rejected");
    assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
}

fn shard_for(engine: &Arc<Engine>, context: &TenantIsolationContext, arn: &str) -> String {
    describe_stream(
        engine,
        context,
        DescribeStreamInput {
            stream_arn: arn.to_owned(),
            limit: None,
            exclusive_start_shard_id: None,
        },
    )
    .expect("describe")
    .stream_description
    .shards[0]
        .shard_id
        .clone()
}

/// The `next_sequence` encoded in an iterator (decode is private to D5.4, so
/// the test reverses the base64url encoding directly).
fn iterator_next_sequence(iterator: &str) -> i64 {
    let raw = String::from_utf8(URL_SAFE_NO_PAD.decode(iterator).unwrap()).unwrap();
    raw.rsplit('\u{1f}').next().unwrap().parse().unwrap()
}

fn get_iter(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    arn: &str,
    shard: &str,
    iter_type: &str,
    seq: Option<&str>,
) -> Result<GetShardIteratorOutput, DynamoDbError> {
    let mut input = json!({
        "StreamArn": arn,
        "ShardId": shard,
        "ShardIteratorType": iter_type,
    });
    if let Some(seq) = seq {
        input["SequenceNumber"] = json!(seq);
    }
    get_shard_iterator(engine, context, serde_json::from_value(input).unwrap())
}

#[test]
fn get_shard_iterator_each_type() {
    let (engine, ctx, _t) = fixture();
    let arn = create_streamed(&engine, &ctx);
    let shard = shard_for(&engine, &ctx, &arn);

    let trim = get_iter(&engine, &ctx, &arn, &shard, "TRIM_HORIZON", None)
        .expect("trim horizon")
        .shard_iterator
        .expect("iterator");
    assert_eq!(iterator_next_sequence(&trim), 0, "TRIM_HORIZON starts at 0");

    let latest = get_iter(&engine, &ctx, &arn, &shard, "LATEST", None)
        .expect("latest")
        .shard_iterator
        .expect("iterator");
    assert_eq!(
        iterator_next_sequence(&latest),
        0,
        "LATEST starts at the current end (0 with no records yet)"
    );

    let at = get_iter(&engine, &ctx, &arn, &shard, "AT_SEQUENCE_NUMBER", Some("5"))
        .expect("at")
        .shard_iterator
        .expect("iterator");
    assert_eq!(
        iterator_next_sequence(&at),
        5,
        "AT reads from the given sequence"
    );

    let after = get_iter(
        &engine,
        &ctx,
        &arn,
        &shard,
        "AFTER_SEQUENCE_NUMBER",
        Some("5"),
    )
    .expect("after")
    .shard_iterator
    .expect("iterator");
    assert_eq!(
        iterator_next_sequence(&after),
        6,
        "AFTER reads past the sequence"
    );
}

#[test]
fn get_shard_iterator_at_without_sequence_is_validation_error() {
    let (engine, ctx, _t) = fixture();
    let arn = create_streamed(&engine, &ctx);
    let shard = shard_for(&engine, &ctx, &arn);
    let err = get_iter(&engine, &ctx, &arn, &shard, "AT_SEQUENCE_NUMBER", None)
        .expect_err("missing sequence");
    assert!(matches!(err, DynamoDbError::ValidationException(_)));
}

#[test]
fn get_shard_iterator_unknown_shard_is_resource_not_found() {
    let (engine, ctx, _t) = fixture();
    let arn = create_streamed(&engine, &ctx);
    let err = get_iter(&engine, &ctx, &arn, "shardId-nope", "TRIM_HORIZON", None)
        .expect_err("unknown shard");
    assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
}

// ---- D5.4: GetRecords + event capture ----

/// Create a stream-enabled table with the given view type; returns the ARN.
fn streamed_table(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    view_type: &str,
) -> String {
    let input: CreateTableInput = serde_json::from_value(json!({
        "TableName": "events",
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        "StreamSpecification": { "StreamEnabled": true, "StreamViewType": view_type }
    }))
    .unwrap();
    control_plane::create_table(engine, context, input)
        .expect("create")
        .table_description
        .latest_stream_arn
        .expect("arn")
}

fn put(engine: &Arc<Engine>, context: &TenantIsolationContext, pk: &str, v: &str) {
    crate::commands::item::put_item(
        engine,
        context,
        serde_json::from_value(json!({
            "TableName": "events",
            "Item": { "pk": {"S": pk}, "v": {"N": v} },
        }))
        .unwrap(),
    )
    .expect("put");
}

fn delete(engine: &Arc<Engine>, context: &TenantIsolationContext, pk: &str) {
    crate::commands::item::delete_item(
        engine,
        context,
        serde_json::from_value(json!({ "TableName": "events", "Key": { "pk": {"S": pk} } }))
            .unwrap(),
    )
    .expect("delete");
}

fn seed_stream_event_collision(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
    seq: i64,
) {
    let item: Item = [
        (
            "pk".to_string(),
            AttributeValue::S(format!("collision-{seq}")),
        ),
        ("v".to_string(), AttributeValue::N(seq.to_string())),
    ]
    .into_iter()
    .collect();
    let change = ChangeEvent {
        event_name: StreamEventName::Insert,
        keys: &item,
        old_image: None,
        new_image: Some(&item),
        user_identity: None,
    };
    let batch =
        AtomicWriteBatch::new(vec![stream_event_write(table_name, seq, &change).unwrap()]).unwrap();
    engine
        .begin_mutation_execution_unit(context.tenant_id().clone(), adapter_principal())
        .unwrap()
        .execute_atomic_write_batch(batch)
        .unwrap();
}

fn seed_raw_stream_event(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
    seq: i64,
    event_name: &str,
) {
    let stored = StoredEvent {
        seq,
        created: epoch_seconds(),
        event_name: event_name.to_owned(),
        keys: json!({ "pk": { "S": format!("raw-{seq}") } })
            .as_object()
            .unwrap()
            .clone(),
        old_image: None,
        old_image_times: None,
        new_image: None,
        new_image_retained_update: None,
        user_identity: None,
        committed_at: 0,
    };
    let document = serde_json::to_value(stored)
        .unwrap()
        .as_object()
        .unwrap()
        .clone();
    let id = DocumentId::from_key(sequence_number(seq)).unwrap();
    let batch = AtomicWriteBatch::new(vec![AtomicWrite::Set {
        key: WriteKey::from(DocumentLocator::new(
            stream_events_table(table_name).unwrap(),
            id,
        )),
        document,
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    }])
    .unwrap();
    engine
        .begin_mutation_execution_unit(context.tenant_id().clone(), adapter_principal())
        .unwrap()
        .execute_atomic_write_batch(batch)
        .unwrap();
}

/// TRIM_HORIZON iterator + GetRecords from the start.
fn all_records(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    arn: &str,
) -> GetRecordsOutput {
    let shard = shard_for(engine, context, arn);
    let iter = get_shard_iterator(
        engine,
        context,
        GetShardIteratorInput {
            stream_arn: arn.to_owned(),
            shard_id: shard,
            shard_iterator_type: ShardIteratorType::TrimHorizon,
            sequence_number: None,
        },
    )
    .expect("iterator")
    .shard_iterator
    .expect("iterator");
    get_records(
        engine,
        context,
        GetRecordsInput {
            shard_iterator: iter,
            limit: None,
        },
    )
    .expect("get records")
}

#[test]
fn get_records_new_and_old_images_for_insert_modify_remove() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
    put(&engine, &ctx, "a", "1"); // INSERT
    put(&engine, &ctx, "a", "2"); // MODIFY
    delete(&engine, &ctx, "a"); // REMOVE
    let out = all_records(&engine, &ctx, &arn);
    assert_eq!(out.records.len(), 3);

    let insert = &out.records[0];
    assert_eq!(insert.event_name, StreamEventName::Insert);
    assert!(
        insert.dynamodb.old_image.is_none(),
        "INSERT has no old image"
    );
    assert_eq!(
        insert.dynamodb.new_image.as_ref().unwrap().get("v"),
        Some(&extenddb_core::types::AttributeValue::N("1".into()))
    );

    let modify = &out.records[1];
    assert_eq!(modify.event_name, StreamEventName::Modify);
    assert_eq!(
        modify.dynamodb.old_image.as_ref().unwrap().get("v"),
        Some(&extenddb_core::types::AttributeValue::N("1".into()))
    );
    assert_eq!(
        modify.dynamodb.new_image.as_ref().unwrap().get("v"),
        Some(&extenddb_core::types::AttributeValue::N("2".into()))
    );

    let remove = &out.records[2];
    assert_eq!(remove.event_name, StreamEventName::Remove);
    assert!(
        remove.dynamodb.new_image.is_none(),
        "REMOVE has no new image"
    );
    assert!(
        remove.dynamodb.old_image.is_some(),
        "REMOVE carries the old image"
    );

    assert!(
        out.next_shard_iterator.is_some(),
        "open shard always advances"
    );
}

#[test]
fn get_records_does_not_decode_events_before_iterator_start() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "NEW_IMAGE");
    seed_raw_stream_event(&engine, &ctx, "events", 0, "UPSERT");
    seed_stream_event_collision(&engine, &ctx, "events", 5);
    let shard = shard_for(&engine, &ctx, &arn);
    let iter = get_iter(
        &engine,
        &ctx,
        &arn,
        &shard,
        "AT_SEQUENCE_NUMBER",
        Some(&sequence_number(5)),
    )
    .expect("iterator")
    .shard_iterator
    .expect("iterator");

    let out = get_records(
        &engine,
        &ctx,
        GetRecordsInput {
            shard_iterator: iter,
            limit: Some(1),
        },
    )
    .expect("bounded read skips corrupt earlier event");

    assert_eq!(out.records.len(), 1);
    assert_eq!(out.records[0].event_id, sequence_number(5));
    assert_eq!(out.records[0].event_name, StreamEventName::Insert);
    assert_eq!(
        iterator_next_sequence(out.next_shard_iterator.as_ref().unwrap()),
        6
    );
}

#[test]
fn get_records_keys_only_view_omits_images() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "KEYS_ONLY");
    put(&engine, &ctx, "a", "1");
    let out = all_records(&engine, &ctx, &arn);
    let record = &out.records[0];
    assert!(record.dynamodb.new_image.is_none() && record.dynamodb.old_image.is_none());
    assert_eq!(
        record.dynamodb.keys.get("pk"),
        Some(&extenddb_core::types::AttributeValue::S("a".into()))
    );
}

#[test]
fn get_records_new_image_only() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "NEW_IMAGE");
    put(&engine, &ctx, "a", "1");
    put(&engine, &ctx, "a", "2");
    let out = all_records(&engine, &ctx, &arn);
    let modify = &out.records[1];
    assert!(
        modify.dynamodb.old_image.is_none(),
        "NEW_IMAGE omits the old image"
    );
    assert!(modify.dynamodb.new_image.is_some());
}

#[test]
fn get_records_iterator_advances_and_pages() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "NEW_IMAGE");
    for v in ["1", "2", "3"] {
        put(&engine, &ctx, "a", v);
    }
    let shard = shard_for(&engine, &ctx, &arn);
    // Page 1: limit 2.
    let iter1 = get_shard_iterator(
        &engine,
        &ctx,
        GetShardIteratorInput {
            stream_arn: arn.clone(),
            shard_id: shard,
            shard_iterator_type: ShardIteratorType::TrimHorizon,
            sequence_number: None,
        },
    )
    .expect("iter")
    .shard_iterator
    .expect("iter");
    let page1 = get_records(
        &engine,
        &ctx,
        GetRecordsInput {
            shard_iterator: iter1,
            limit: Some(2),
        },
    )
    .expect("page1");
    assert_eq!(page1.records.len(), 2);
    // Page 2: continue from NextShardIterator.
    let page2 = get_records(
        &engine,
        &ctx,
        GetRecordsInput {
            shard_iterator: page1.next_shard_iterator.expect("next"),
            limit: Some(2),
        },
    )
    .expect("page2");
    assert_eq!(page2.records.len(), 1, "the iterator advanced past page 1");
}

#[test]
fn describe_stream_for_missing_table_is_resource_not_found() {
    let (engine, ctx, _t) = fixture();
    let err = describe_stream(
        &engine,
        &ctx,
        DescribeStreamInput {
            stream_arn: "arn:aws:dynamodb:ddblocal:000000000000:table/ghost/stream/x".to_owned(),
            limit: None,
            exclusive_start_shard_id: None,
        },
    )
    .expect_err("missing table rejected");
    assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
}

// ---- D5.5: ListStreams + retention ----

/// Create a stream-enabled table with the given name; returns its ARN.
fn create_streamed_named(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    name: &str,
) -> String {
    let input: CreateTableInput = serde_json::from_value(json!({
        "TableName": name,
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_IMAGE" }
    }))
    .unwrap();
    control_plane::create_table(engine, context, input)
        .expect("create")
        .table_description
        .latest_stream_arn
        .expect("arn")
}

/// Create a table without a stream (it must be excluded from ListStreams).
fn create_plain_named(engine: &Arc<Engine>, context: &TenantIsolationContext, name: &str) {
    let input: CreateTableInput = serde_json::from_value(json!({
        "TableName": name,
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
    }))
    .unwrap();
    control_plane::create_table(engine, context, input).expect("create");
}

fn list(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: Option<&str>,
    limit: Option<i64>,
    start: Option<&str>,
) -> ListStreamsOutput {
    list_streams(
        engine,
        context,
        ListStreamsInput {
            table_name: table_name.map(str::to_owned),
            limit,
            exclusive_start_stream_arn: start.map(str::to_owned),
        },
    )
    .expect("list streams")
}

#[test]
fn list_streams_enumerates_only_stream_enabled_tables() {
    let (engine, ctx, _t) = fixture();
    let alpha = create_streamed_named(&engine, &ctx, "alpha");
    let beta = create_streamed_named(&engine, &ctx, "beta");
    create_plain_named(&engine, &ctx, "gamma");

    let out = list(&engine, &ctx, None, None, None);
    assert_eq!(out.streams.len(), 2, "only the two streamed tables");
    let arns: Vec<&str> = out.streams.iter().map(|s| s.stream_arn.as_str()).collect();
    assert!(arns.contains(&alpha.as_str()) && arns.contains(&beta.as_str()));
    let tables: Vec<&str> = out.streams.iter().map(|s| s.table_name.as_str()).collect();
    assert!(tables.contains(&"alpha") && tables.contains(&"beta"));
    assert!(!tables.contains(&"gamma"), "plain table excluded");
    assert!(
        out.streams.iter().all(|s| !s.stream_label.is_empty()),
        "every summary carries a label"
    );
    assert!(out.last_evaluated_stream_arn.is_none(), "fully enumerated");
}

#[test]
fn list_streams_filters_by_table_name() {
    let (engine, ctx, _t) = fixture();
    create_streamed_named(&engine, &ctx, "alpha");
    create_streamed_named(&engine, &ctx, "beta");

    let out = list(&engine, &ctx, Some("alpha"), None, None);
    assert_eq!(out.streams.len(), 1);
    assert_eq!(out.streams[0].table_name, "alpha");

    let none = list(&engine, &ctx, Some("nonexistent"), None, None);
    assert!(none.streams.is_empty(), "no match for an unknown table");
}

#[test]
fn list_streams_paginates_with_limit_and_exclusive_start() {
    let (engine, ctx, _t) = fixture();
    for name in ["alpha", "beta", "gamma"] {
        create_streamed_named(&engine, &ctx, name);
    }
    let page1 = list(&engine, &ctx, None, Some(2), None);
    assert_eq!(page1.streams.len(), 2);
    let cursor = page1
        .last_evaluated_stream_arn
        .clone()
        .expect("more pages remain");
    assert_eq!(
        cursor, page1.streams[1].stream_arn,
        "cursor is the last returned ARN"
    );

    let page2 = list(&engine, &ctx, None, Some(2), Some(&cursor));
    assert_eq!(page2.streams.len(), 1, "the final page");
    assert!(
        page2.last_evaluated_stream_arn.is_none(),
        "no more pages after the last"
    );
    // The two pages cover all three streams with no overlap.
    let mut seen: Vec<&str> = page1
        .streams
        .iter()
        .chain(page2.streams.iter())
        .map(|s| s.stream_arn.as_str())
        .collect();
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 3, "all three streams, no duplicates");
}
