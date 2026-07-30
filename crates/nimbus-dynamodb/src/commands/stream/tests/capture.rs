//! What a write records.
//!
//! Capture rides in the same transaction as the data write, so these cover the
//! two halves of that claim: every write path emits the record it should, and a
//! write whose event cannot commit takes the data write down with it.

use super::*;

fn stored_item(engine: &Arc<Engine>, context: &TenantIsolationContext, pk: &str) -> Option<Item> {
    let key: Item = [("pk".to_string(), AttributeValue::S(pk.to_owned()))]
        .into_iter()
        .collect();
    let schema = control_plane::load_key_schema(engine, context, "events").unwrap();
    let id = crate::commands::item::primary_key_id(&key, &schema).unwrap();
    crate::commands::item::read_item(engine, context, &TableName::new("events").unwrap(), id)
        .unwrap()
}

fn assert_stream_collision(error: DynamoDbError) {
    match error {
        DynamoDbError::InternalServerError(message) => assert!(
            message.contains("stream sequence allocation exhausted retries"),
            "unexpected internal error: {message}"
        ),
        DynamoDbError::TransactionConflictException(message) => assert!(
            message.contains("single-item transaction exhausted"),
            "unexpected transaction conflict: {message}"
        ),
        other => panic!("expected stream sequence collision exhaustion, got {other:?}"),
    }
}

/// H3/F3: BatchWriteItem emits stream records (it previously emitted none).
/// A put of a fresh key is INSERT, a put over an existing key is MODIFY, and
/// a delete of an existing key is REMOVE — all delivered via GetRecords with
/// strictly increasing sequence numbers.
#[test]
fn batch_write_emits_stream_records() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
    put(&engine, &ctx, "old", "1"); // seed: will be MODIFY then nothing
    let input = serde_json::from_value(json!({
        "RequestItems": { "events": [
            { "PutRequest": { "Item": { "pk": {"S": "fresh"}, "v": {"N": "1"} } } },
            { "PutRequest": { "Item": { "pk": {"S": "old"}, "v": {"N": "2"} } } },
            { "DeleteRequest": { "Key": { "pk": {"S": "fresh"} } } }
        ] }
    }))
    .unwrap();
    crate::commands::batch::batch_write_item(&engine, &ctx, input).expect("batch write");

    let out = all_records(&engine, &ctx, &arn);
    // 1 (seed INSERT) + INSERT(fresh) + MODIFY(old) + REMOVE(fresh).
    let names: Vec<StreamEventName> = out.records.iter().map(|r| r.event_name).collect();
    assert_eq!(
        names,
        vec![
            StreamEventName::Insert, // seed put
            StreamEventName::Insert, // batch put fresh
            StreamEventName::Modify, // batch put over old
            StreamEventName::Remove, // batch delete fresh
        ],
        "BatchWriteItem must emit one stream record per write"
    );
    let seqs: Vec<i64> = out
        .records
        .iter()
        .map(|r| r.dynamodb.sequence_number.parse::<i64>().unwrap())
        .collect();
    assert!(
        seqs.windows(2).all(|w| w[1] > w[0]),
        "sequence numbers strictly increase: {seqs:?}"
    );
}

#[test]
fn single_item_writes_roll_back_when_stream_event_cannot_commit() {
    {
        let (engine, ctx, _t) = fixture();
        streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
        seed_stream_event_collision(&engine, &ctx, "events", 0);
        let err = crate::commands::item::put_item(
            &engine,
            &ctx,
            serde_json::from_value(json!({
                "TableName": "events",
                "Item": { "pk": {"S": "put"}, "v": {"N": "1"} }
            }))
            .unwrap(),
        )
        .expect_err("stream collision rejects put");
        assert_stream_collision(err);
        assert!(
            stored_item(&engine, &ctx, "put").is_none(),
            "PutItem must not commit without its stream record"
        );
    }

    {
        let (engine, ctx, _t) = fixture();
        streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
        put(&engine, &ctx, "delete", "1");
        seed_stream_event_collision(&engine, &ctx, "events", 1);
        let err = crate::commands::item::delete_item(
            &engine,
            &ctx,
            serde_json::from_value(json!({
                "TableName": "events",
                "Key": { "pk": {"S": "delete"} }
            }))
            .unwrap(),
        )
        .expect_err("stream collision rejects delete");
        assert_stream_collision(err);
        assert!(
            stored_item(&engine, &ctx, "delete").is_some(),
            "DeleteItem must not remove the item without its stream record"
        );
    }

    {
        let (engine, ctx, _t) = fixture();
        streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
        put(&engine, &ctx, "update", "1");
        seed_stream_event_collision(&engine, &ctx, "events", 1);
        let err = crate::commands::item::update_item(
            &engine,
            &ctx,
            serde_json::from_value(json!({
                "TableName": "events",
                "Key": { "pk": {"S": "update"} },
                "UpdateExpression": "SET v = :v",
                "ExpressionAttributeValues": { ":v": {"N": "2"} }
            }))
            .unwrap(),
        )
        .expect_err("stream collision rejects update");
        assert_stream_collision(err);
        assert_eq!(
            stored_item(&engine, &ctx, "update").and_then(|item| item.get("v").cloned()),
            Some(AttributeValue::N("1".to_owned())),
            "UpdateItem must preserve the old item without its stream record"
        );
    }
}

#[test]
fn batch_write_requests_roll_back_when_stream_event_cannot_commit() {
    {
        let (engine, ctx, _t) = fixture();
        streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
        seed_stream_event_collision(&engine, &ctx, "events", 0);
        let err = crate::commands::batch::batch_write_item(
            &engine,
            &ctx,
            serde_json::from_value(json!({
                "RequestItems": {
                    "events": [
                        { "PutRequest": { "Item": { "pk": {"S": "batch-put"}, "v": {"N": "1"} } } }
                    ]
                }
            }))
            .unwrap(),
        )
        .expect_err("stream collision rejects batch put");
        assert_stream_collision(err);
        assert!(
            stored_item(&engine, &ctx, "batch-put").is_none(),
            "BatchWriteItem put must not commit without its stream record"
        );
    }

    {
        let (engine, ctx, _t) = fixture();
        streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
        put(&engine, &ctx, "batch-delete", "1");
        seed_stream_event_collision(&engine, &ctx, "events", 1);
        let err = crate::commands::batch::batch_write_item(
            &engine,
            &ctx,
            serde_json::from_value(json!({
                "RequestItems": {
                    "events": [
                        { "DeleteRequest": { "Key": { "pk": {"S": "batch-delete"} } } }
                    ]
                }
            }))
            .unwrap(),
        )
        .expect_err("stream collision rejects batch delete");
        assert_stream_collision(err);
        assert!(
            stored_item(&engine, &ctx, "batch-delete").is_some(),
            "BatchWriteItem delete must not remove the item without its stream record"
        );
    }
}

/// H3/F3: TransactWriteItems emits stream records, folded into the same
/// atomic commit as the data writes. A put and an update in one transaction
/// deliver INSERT + INSERT (the update upserts a fresh key) via GetRecords.
#[test]
fn transact_write_emits_stream_records() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "NEW_AND_OLD_IMAGES");
    let input = serde_json::from_value(json!({
        "TransactItems": [
            { "Put": { "TableName": "events", "Item": { "pk": {"S": "p1"}, "v": {"N": "1"} } } },
            { "Update": {
                "TableName": "events",
                "Key": { "pk": {"S": "u1"} },
                "UpdateExpression": "SET v = :v",
                "ExpressionAttributeValues": { ":v": {"N": "9"} }
            } }
        ]
    }))
    .unwrap();
    crate::commands::transact::transact_write_items(&engine, &ctx, input).expect("transact");

    let out = all_records(&engine, &ctx, &arn);
    let names: Vec<StreamEventName> = out.records.iter().map(|r| r.event_name).collect();
    assert_eq!(
        names,
        vec![StreamEventName::Insert, StreamEventName::Insert],
        "TransactWriteItems must emit a stream record per write"
    );
    let seqs: Vec<i64> = out
        .records
        .iter()
        .map(|r| r.dynamodb.sequence_number.parse::<i64>().unwrap())
        .collect();
    assert_eq!(
        seqs,
        vec![0, 1],
        "transacted events get consecutive sequences"
    );
}

/// H3/F8: sequence allocation is monotonic and gap-free across writes — the
/// high-water counter advances atomically in the same batch as each event,
/// so no two records share a sequence number and none is skipped.
#[test]
fn writes_allocate_monotonic_gap_free_sequences() {
    let (engine, ctx, _t) = fixture();
    let arn = streamed_table(&engine, &ctx, "NEW_IMAGE");
    for i in 0..5 {
        put(&engine, &ctx, &format!("k{i}"), &i.to_string());
    }
    let out = all_records(&engine, &ctx, &arn);
    let seqs: Vec<i64> = out
        .records
        .iter()
        .map(|r| r.dynamodb.sequence_number.parse::<i64>().unwrap())
        .collect();
    assert_eq!(seqs, vec![0, 1, 2, 3, 4], "monotonic, gap-free sequences");
}

#[test]
fn capture_is_skipped_for_non_stream_tables() {
    let (engine, ctx, _t) = fixture();
    // Table without a stream — writes produce no events, and there is no
    // stream store to read.
    let input: CreateTableInput = serde_json::from_value(json!({
        "TableName": "plain",
        "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
        "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
    }))
    .unwrap();
    control_plane::create_table(&engine, &ctx, input).expect("create");
    crate::commands::item::put_item(
        &engine,
        &ctx,
        serde_json::from_value(json!({ "TableName": "plain", "Item": { "pk": {"S": "a"} } }))
            .unwrap(),
    )
    .expect("put");
    assert_eq!(
        next_sequence_value(&engine, &ctx, "plain").unwrap(),
        0,
        "no events captured for a non-stream table"
    );
}
