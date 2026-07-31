//! Batch operations (T3): BatchGetItem (D3.1) and BatchWriteItem (D3.2).
//!
//! Both fan out over the single-item handlers (`item::*`) per request item.
//! The Nimbus store is reliable and not throttled, so `UnprocessedKeys` /
//! `UnprocessedItems` are always empty — every requested key/op is processed.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    BatchGetItemInput, BatchGetItemOutput, BatchWriteItemInput, BatchWriteItemOutput, Item,
    StreamEventName, extract_key,
};
use nimbus_core::{DocumentId, TableName, WritePrecondition};
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;

use crate::attribute_value::{item_to_fields, validate_item, validate_item_size};
use crate::commands::item::{
    SingleItemTransactionPlan, delete_atomic_write, execute_single_item_transaction,
    overwrite_atomic_write, primary_key_id, read_item, read_old_image_in_transaction,
};
use crate::commands::{control_plane, stream};
use crate::error::map_core_error;
use crate::expression::{default_limits, project_item};

/// The DynamoDB per-call key limit for BatchGetItem.
const MAX_BATCH_GET_KEYS: usize = 100;
/// The DynamoDB per-call write limit for BatchWriteItem.
const MAX_BATCH_WRITE_OPS: usize = 25;

/// BatchGetItem: read up to 100 keys across tables, returning a per-table
/// `Responses` map. Missing items are simply absent. `UnprocessedKeys` is
/// always empty (the store processes every key).
///
/// # Errors
/// `ValidationException` for an empty request or more than 100 keys;
/// `ResourceNotFoundException` if a referenced table is absent.
pub fn batch_get_item(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: BatchGetItemInput,
) -> Result<BatchGetItemOutput, DynamoDbError> {
    let total_keys: usize = input
        .request_items
        .values()
        .map(|requested| requested.keys.len())
        .sum();
    if total_keys == 0 {
        return Err(DynamoDbError::ValidationException(
            "BatchGetItem requires at least one key in RequestItems".to_owned(),
        ));
    }
    if total_keys > MAX_BATCH_GET_KEYS {
        return Err(DynamoDbError::ValidationException(
            "Too many items requested for the BatchGetItem call".to_owned(),
        ));
    }

    let limits = default_limits();
    let mut responses: HashMap<String, Vec<Item>> = HashMap::new();
    for (table_name, requested) in &input.request_items {
        let key_schema = control_plane::load_key_schema(engine, context, table_name)?;
        let table = TableName::new(table_name).map_err(map_core_error)?;
        let mut items = Vec::new();
        for key in &requested.keys {
            let id = primary_key_id(key, &key_schema)?;
            if let Some(item) = read_item(engine, context, &table, id)? {
                let projected = match requested
                    .projection_expression
                    .as_deref()
                    .filter(|expression| !expression.is_empty())
                {
                    Some(expression) => project_item(
                        expression,
                        requested.expression_attribute_names.as_ref(),
                        &item,
                        &limits,
                    )?,
                    None => item,
                };
                items.push(projected);
            }
        }
        responses.insert(table_name.clone(), items);
    }

    Ok(BatchGetItemOutput {
        responses,
        unprocessed_keys: HashMap::new(),
        consumed_capacity: None,
    })
}

/// One BatchWriteItem op whose shape has already been checked: its table
/// resolved, its key schema applied, and its item converted. Building this is
/// the whole of an op's validation, so a prepared op can no longer fail the
/// batch for a validation reason.
struct PreparedWrite {
    /// The table name as the request spelled it, which is what the emitted
    /// stream record reports.
    table_name: String,
    table: TableName,
    id: DocumentId,
    keys: Item,
    op: PreparedOp,
}

enum PreparedOp {
    Put {
        item: Item,
        fields: serde_json::Map<String, serde_json::Value>,
    },
    Delete,
}

/// BatchWriteItem: apply up to 25 Put/Delete requests across tables. Each
/// `WriteRequest` must carry exactly one of `PutRequest`/`DeleteRequest`.
///
/// **Validation covers the whole request before any op runs.** DynamoDB
/// "rejects the entire batch write operation" when an op is malformed — a
/// request without exactly one of Put/Delete, key attributes that do not match
/// the table's key schema, a table that does not exist, an item over 400 KiB,
/// more than 25 ops — or when two ops name the same item. A
/// `ValidationException` from here leaves nothing applied, whatever the
/// position of the offending op. Execution is a second pass over ops already
/// known to be well formed.
///
/// The one documented rejection cause not checked here is the 16 MB
/// request ceiling, which bounds the *network payload* rather than the sum of
/// item sizes and so cannot be reached from 25 items of at most 400 KiB. It is
/// enforced where the wire bytes exist, by the DynamoDB listener's request-body
/// limit.
///
/// **Execution is still not atomic**, and that is the other half of the
/// contract: unlike TransactWriteItems, the ops are independent once they
/// start. A *runtime* failure part way through leaves the ops that already
/// committed applied. DynamoDB reports that case — exhausted throughput or an
/// internal processing failure — in `UnprocessedItems` for the caller to
/// retry; the Nimbus store neither throttles nor partially processes a healthy
/// request, so `UnprocessedItems` is always empty here and a genuine runtime
/// failure surfaces as an error instead.
///
/// Each op runs in its own single-item transaction, which is what keeps the
/// ops independent while still reading the prior image inside the transaction
/// that replaces it. Put and Delete are whole-item operations, so racing them
/// is last-writer-wins by contract; the transaction is here for the emitted
/// stream record, whose INSERT/MODIFY classification and `OldImage` must
/// describe the state this write actually replaced.
///
/// # Errors
/// `ValidationException` for an empty request, more than 25 ops, a
/// `WriteRequest` without exactly one of Put/Delete, an item whose key
/// attributes do not match the table's key schema, an item over 400 KiB, or two
/// ops naming the same item; `ResourceNotFoundException` if a referenced table
/// is absent.
pub fn batch_write_item(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: BatchWriteItemInput,
) -> Result<BatchWriteItemOutput, DynamoDbError> {
    let total_ops: usize = input.request_items.values().map(std::vec::Vec::len).sum();
    if total_ops == 0 {
        return Err(DynamoDbError::ValidationException(
            "BatchWriteItem requires at least one request in RequestItems".to_owned(),
        ));
    }
    if total_ops > MAX_BATCH_WRITE_OPS {
        return Err(DynamoDbError::ValidationException(
            "Too many items requested for the BatchWriteItem call".to_owned(),
        ));
    }

    let prepared = prepare_batch_writes(engine, context, &input)?;
    for write in &prepared {
        execute_prepared_write(engine, context, write)?;
    }

    Ok(BatchWriteItemOutput {
        unprocessed_items: HashMap::new(),
        consumed_capacity: None,
        item_collection_metrics: None,
    })
}

/// Validate every op in the request, resolving each table's key schema once.
///
/// # Errors
/// The first op that fails validation. No caller has applied anything by the
/// time this returns, which is the whole point — see [`batch_write_item`].
fn prepare_batch_writes(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: &BatchWriteItemInput,
) -> Result<Vec<PreparedWrite>, DynamoDbError> {
    let mut prepared = Vec::new();
    for (table_name, requests) in &input.request_items {
        let key_schema = control_plane::load_key_schema(engine, context, table_name)?;
        let table = TableName::new(table_name).map_err(map_core_error)?;
        for request in requests {
            // `key_source` is the item the primary key is read from: the whole
            // item for a Put, the supplied key for a Delete.
            let (op, key_source) = match (&request.put_request, &request.delete_request) {
                (Some(put), None) => {
                    validate_item(&put.item)?;
                    validate_item_size(&put.item)?;
                    (
                        PreparedOp::Put {
                            item: put.item.clone(),
                            fields: item_to_fields(&put.item)?,
                        },
                        &put.item,
                    )
                }
                (None, Some(delete)) => (PreparedOp::Delete, &delete.key),
                _ => {
                    return Err(DynamoDbError::ValidationException(
                        "Each WriteRequest must contain exactly one of PutRequest or DeleteRequest"
                            .to_owned(),
                    ));
                }
            };
            prepared.push(PreparedWrite {
                table_name: table_name.clone(),
                table: table.clone(),
                id: primary_key_id(key_source, &key_schema)?,
                keys: extract_key(key_source, &key_schema),
                op,
            });
        }
    }
    reject_duplicate_items(&prepared)?;
    Ok(prepared)
}

/// Reject a request that names the same item more than once.
///
/// DynamoDB rejects the entire batch write operation when "you try to perform
/// multiple operations on the same item in the same BatchWriteItem request",
/// and a put and a delete of one item are two operations on it. The alternative
/// — applying both and letting the later op win — would make the result depend
/// on an ordering the request never specified.
///
/// An item is identified by its table *and* its full primary key, so the same
/// key under two table names is two different items.
///
/// # Errors
/// `ValidationException` on the second op to name an already-named item.
fn reject_duplicate_items(prepared: &[PreparedWrite]) -> Result<(), DynamoDbError> {
    let mut seen: HashSet<(&str, &DocumentId)> = HashSet::with_capacity(prepared.len());
    for write in prepared {
        if !seen.insert((write.table_name.as_str(), &write.id)) {
            return Err(DynamoDbError::ValidationException(
                "Provided list of item keys contains duplicates".to_owned(),
            ));
        }
    }
    Ok(())
}

/// Apply one already-validated op in its own single-item transaction.
///
/// # Errors
/// A runtime failure from the engine or the store. Ops that committed before
/// this one stay applied — see [`batch_write_item`].
fn execute_prepared_write(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    write: &PreparedWrite,
) -> Result<(), DynamoDbError> {
    let PreparedWrite {
        table_name,
        table,
        id,
        keys,
        op,
    } = write;
    execute_single_item_transaction(engine, context, |token, principal| {
        let old =
            read_old_image_in_transaction(engine, context, token, principal, table, id.clone())?;
        let (writes, changes) = match op {
            PreparedOp::Put { item, fields } => {
                let change = stream::StreamChange::new(
                    table_name.clone(),
                    if old.is_some() {
                        StreamEventName::Modify
                    } else {
                        StreamEventName::Insert
                    },
                    keys.clone(),
                    old,
                    Some(item.clone()),
                    None,
                );
                (
                    vec![overwrite_atomic_write(
                        table.clone(),
                        id.clone(),
                        fields.clone(),
                        WritePrecondition::exists(change.old_image.is_some()),
                    )],
                    vec![change],
                )
            }
            // DynamoDB emits a REMOVE record only when an item was actually
            // deleted.
            PreparedOp::Delete => match old {
                Some(old_image) => (
                    vec![delete_atomic_write(
                        table.clone(),
                        id.clone(),
                        WritePrecondition::exists(true),
                    )],
                    vec![stream::StreamChange::new(
                        table_name.clone(),
                        StreamEventName::Remove,
                        keys.clone(),
                        Some(old_image),
                        None,
                        None,
                    )],
                ),
                None => (Vec::new(), Vec::new()),
            },
        };
        Ok(SingleItemTransactionPlan {
            output: (),
            writes,
            changes,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::attribute_value::MAX_ITEM_SIZE_BYTES;
    use extenddb_core::types::{
        AttributeValue, CreateTableInput, DescribeStreamInput, GetRecordsInput,
        GetShardIteratorInput, ShardIteratorType, StreamRecord,
    };
    use nimbus_core::TenantId;
    use nimbus_engine::commit_fault_labels;
    use serde_json::json;
    use std::time::Duration;

    fn fixture() -> (Arc<Engine>, TenantIsolationContext, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
        let context = crate::tenant::test_context(TenantId::new("acme").unwrap(), "test");
        crate::tenant::ensure_tenant(&engine, &context).expect("tenant");
        (engine, context, temp)
    }

    /// Create a `pk`-keyed table. Named tables let the cross-table cases show
    /// that item identity is per table, not per key.
    fn create_keyed_table(engine: &Arc<Engine>, context: &TenantIsolationContext, name: &str) {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": name,
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap();
        control_plane::create_table(engine, context, input).expect("create table");
    }

    fn create_orders(engine: &Arc<Engine>, context: &TenantIsolationContext) {
        create_keyed_table(engine, context, "Orders");
    }

    fn put(engine: &Arc<Engine>, context: &TenantIsolationContext, pk: &str, v: &str) {
        crate::commands::item::put_item(
            engine,
            context,
            serde_json::from_value(json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": pk}, "v": {"N": v} },
            }))
            .unwrap(),
        )
        .expect("put");
    }

    fn batch_get(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<BatchGetItemOutput, DynamoDbError> {
        batch_get_item(engine, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn batch_get_returns_present_items_and_skips_missing() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(&engine, &ctx, "a", "1");
        put(&engine, &ctx, "b", "2");
        let out = batch_get(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": { "Keys": [
                        { "pk": {"S": "a"} },
                        { "pk": {"S": "b"} },
                        { "pk": {"S": "absent"} }
                    ] }
                }
            }),
        )
        .expect("batch get");
        let items = &out.responses["Orders"];
        assert_eq!(items.len(), 2, "present keys only; missing is skipped");
        assert!(out.unprocessed_keys.is_empty());
        let vs: std::collections::BTreeSet<String> = items
            .iter()
            .map(|item| match item.get("v") {
                Some(AttributeValue::N(n)) => n.clone(),
                other => panic!("v: {other:?}"),
            })
            .collect();
        assert_eq!(vs, ["1", "2"].iter().map(|s| (*s).to_string()).collect());
    }

    #[test]
    fn batch_get_projection_applies_per_table() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(&engine, &ctx, "a", "1");
        let out = batch_get(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": {
                        "Keys": [ { "pk": {"S": "a"} } ],
                        "ProjectionExpression": "pk"
                    }
                }
            }),
        )
        .expect("batch get");
        let item = &out.responses["Orders"][0];
        assert_eq!(item.len(), 1, "projected to pk only");
        assert!(item.contains_key("pk") && !item.contains_key("v"));
    }

    #[test]
    fn batch_get_empty_request_is_validation_error() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let err = batch_get(&engine, &ctx, json!({ "RequestItems": {} }))
            .expect_err("empty request rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn batch_get_over_100_keys_is_validation_error() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let keys: Vec<serde_json::Value> = (0..101)
            .map(|i| json!({ "pk": {"S": format!("k{i}")} }))
            .collect();
        let err = batch_get(
            &engine,
            &ctx,
            json!({ "RequestItems": { "Orders": { "Keys": keys } } }),
        )
        .expect_err("over-100 rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    // ---- D3.2: BatchWriteItem ----

    fn read_from(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        table: &str,
        pk: &str,
    ) -> Option<Item> {
        let key: Item = [("pk".to_string(), AttributeValue::S(pk.into()))]
            .into_iter()
            .collect();
        let schema = control_plane::load_key_schema(engine, context, table).unwrap();
        let id = primary_key_id(&key, &schema).unwrap();
        read_item(engine, context, &TableName::new(table).unwrap(), id).unwrap()
    }

    fn read(engine: &Arc<Engine>, context: &TenantIsolationContext, pk: &str) -> Option<Item> {
        read_from(engine, context, "Orders", pk)
    }

    fn batch_write(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<extenddb_core::types::BatchWriteItemOutput, DynamoDbError> {
        batch_write_item(engine, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn batch_write_puts_and_deletes() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(&engine, &ctx, "old", "1"); // will be deleted
        let out = batch_write(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [
                        { "PutRequest": { "Item": { "pk": {"S": "a"}, "v": {"N": "1"} } } },
                        { "PutRequest": { "Item": { "pk": {"S": "b"}, "v": {"N": "2"} } } },
                        { "DeleteRequest": { "Key": { "pk": {"S": "old"} } } }
                    ]
                }
            }),
        )
        .expect("batch write");
        assert!(out.unprocessed_items.is_empty());
        assert!(read(&engine, &ctx, "a").is_some());
        assert!(read(&engine, &ctx, "b").is_some());
        assert!(read(&engine, &ctx, "old").is_none(), "deleted");
    }

    #[test]
    fn batch_write_request_with_both_put_and_delete_is_rejected() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let err = batch_write(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [
                        { "PutRequest": { "Item": { "pk": {"S": "first"} } } },
                        {
                            "PutRequest": { "Item": { "pk": {"S": "a"} } },
                            "DeleteRequest": { "Key": { "pk": {"S": "a"} } }
                        }
                    ]
                }
            }),
        )
        .expect_err("both put and delete rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
        assert!(
            read(&engine, &ctx, "first").is_none(),
            "the malformed request shape rejects the whole batch, including the \
             well-formed op ahead of it"
        );
    }

    #[test]
    fn batch_write_over_25_ops_is_validation_error() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let ops: Vec<serde_json::Value> = (0..26)
            .map(|i| json!({ "PutRequest": { "Item": { "pk": {"S": format!("k{i}")} } } }))
            .collect();
        let err = batch_write(&engine, &ctx, json!({ "RequestItems": { "Orders": ops } }))
            .expect_err("over-25 rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn batch_write_empty_request_is_validation_error() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let err =
            batch_write(&engine, &ctx, json!({ "RequestItems": {} })).expect_err("empty rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    // ---- FU3: stream-record fidelity under concurrency ----
    //
    // BatchWriteItem's Put and Delete are whole-item, so racing them is
    // last-writer-wins by contract and there is no lost update to prove. What a
    // stale prior image corrupts is the *stream record*: the INSERT vs MODIFY
    // classification and the `OldImage`. Both tests below pin the record
    // against a write that lands after the batch op's read would have run.

    /// Create a stream-enabled `Orders` table and return its stream ARN.
    fn create_streamed_orders(engine: &Arc<Engine>, context: &TenantIsolationContext) -> String {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "Orders",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": {
                "StreamEnabled": true,
                "StreamViewType": "NEW_AND_OLD_IMAGES"
            },
        }))
        .unwrap();
        control_plane::create_table(engine, context, input)
            .expect("create streamed table")
            .table_description
            .latest_stream_arn
            .expect("stream arn")
    }

    /// Every captured stream record, oldest first, read through the public
    /// GetRecords surface a client would use.
    fn stream_records(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        arn: &str,
    ) -> Vec<StreamRecord> {
        let shard = stream::describe_stream(
            engine,
            context,
            DescribeStreamInput {
                stream_arn: arn.to_owned(),
                limit: None,
                exclusive_start_shard_id: None,
            },
        )
        .expect("describe stream")
        .stream_description
        .shards
        .swap_remove(0)
        .shard_id;
        let iterator = stream::get_shard_iterator(
            engine,
            context,
            GetShardIteratorInput {
                stream_arn: arn.to_owned(),
                shard_id: shard,
                shard_iterator_type: ShardIteratorType::TrimHorizon,
                sequence_number: None,
            },
        )
        .expect("shard iterator")
        .shard_iterator
        .expect("shard iterator");
        stream::get_records(
            engine,
            context,
            GetRecordsInput {
                shard_iterator: iterator,
                limit: None,
            },
        )
        .expect("get records")
        .records
    }

    fn attribute<'a>(image: Option<&'a Item>, name: &str) -> Option<&'a AttributeValue> {
        image.and_then(|image| image.get(name))
    }

    /// A batch Put that reads its prior image outside the transaction that
    /// replaces it classifies the record from a snapshot the write never
    /// overwrote: it reports INSERT with no `OldImage` even though a concurrent
    /// writer created the item first, so the record claims the write created an
    /// item that already existed and hides the image it destroyed.
    #[test]
    fn batch_put_stream_record_reflects_the_image_it_actually_replaced() {
        let (engine, ctx, _t) = fixture();
        let arn = create_streamed_orders(&engine, &ctx);

        let faults = engine.commit_fault_handle_for_testing();
        faults.arm(commit_fault_labels::PREPARE_COMPLETE);
        let batched = std::thread::spawn({
            let engine = engine.clone();
            let ctx = ctx.clone();
            move || {
                batch_write(
                    &engine,
                    &ctx,
                    json!({
                        "RequestItems": {
                            "Orders": [
                                { "PutRequest": { "Item": { "pk": {"S": "x"}, "v": {"N": "2"} } } }
                            ]
                        }
                    }),
                )
            }
        });

        let entered = faults.wait_until_entered(
            commit_fault_labels::PREPARE_COMPLETE,
            Duration::from_secs(5),
        );
        if entered {
            // Lands after the batch op's prior-image read, before its commit.
            put(&engine, &ctx, "x", "1");
        }
        faults.release(commit_fault_labels::PREPARE_COMPLETE);
        assert!(
            entered,
            "the batch put should reach the deterministic commit pause"
        );
        batched
            .join()
            .expect("batch thread should join")
            .expect("batch put should retry and commit");

        let records = stream_records(&engine, &ctx, &arn);
        assert_eq!(
            records.len(),
            2,
            "the concurrent create and the batch put each emit one record: {records:?}"
        );
        // Both writers touch the same key, so the batch's record is whichever
        // one carries the batch's value.
        let batch_record = records
            .iter()
            .find(|record| {
                attribute(record.dynamodb.new_image.as_ref(), "v")
                    == Some(&AttributeValue::N("2".into()))
            })
            .expect("the batch put's record must be present");
        assert_eq!(
            batch_record.event_name,
            StreamEventName::Modify,
            "the batch put replaced an item the concurrent writer had already \
             created, so its record is a MODIFY, not an INSERT: {batch_record:?}"
        );
        assert_eq!(
            attribute(batch_record.dynamodb.old_image.as_ref(), "v"),
            Some(&AttributeValue::N("1".into())),
            "the OldImage must be the image the batch put actually replaced: {batch_record:?}"
        );
        // The item itself is last-writer-wins, which the batch op won.
        assert_eq!(
            read(&engine, &ctx, "x").and_then(|item| item.get("v").cloned()),
            Some(AttributeValue::N("2".into()))
        );
    }

    /// The same staleness on the Delete side: a REMOVE record must carry the
    /// image the delete actually removed, not an image a later writer replaced.
    #[test]
    fn batch_delete_stream_record_carries_the_image_it_actually_removed() {
        let (engine, ctx, _t) = fixture();
        let arn = create_streamed_orders(&engine, &ctx);
        put(&engine, &ctx, "y", "1");

        let faults = engine.commit_fault_handle_for_testing();
        faults.arm(commit_fault_labels::PREPARE_COMPLETE);
        let batched = std::thread::spawn({
            let engine = engine.clone();
            let ctx = ctx.clone();
            move || {
                batch_write(
                    &engine,
                    &ctx,
                    json!({
                        "RequestItems": {
                            "Orders": [ { "DeleteRequest": { "Key": { "pk": {"S": "y"} } } } ]
                        }
                    }),
                )
            }
        });

        let entered = faults.wait_until_entered(
            commit_fault_labels::PREPARE_COMPLETE,
            Duration::from_secs(5),
        );
        if entered {
            put(&engine, &ctx, "y", "2");
        }
        faults.release(commit_fault_labels::PREPARE_COMPLETE);
        assert!(
            entered,
            "the batch delete should reach the deterministic commit pause"
        );
        batched
            .join()
            .expect("batch thread should join")
            .expect("batch delete should retry and commit");

        let records = stream_records(&engine, &ctx, &arn);
        let remove = records
            .iter()
            .find(|record| record.event_name == StreamEventName::Remove)
            .expect("the batch delete's REMOVE record must be present");
        assert_eq!(
            attribute(remove.dynamodb.old_image.as_ref(), "v"),
            Some(&AttributeValue::N("2".into())),
            "the REMOVE must carry the image the delete removed — the concurrent \
             writer's value — not the stale one read before it landed: {remove:?}"
        );
        assert!(
            read(&engine, &ctx, "y").is_none(),
            "the delete still wins the race on the item itself"
        );
    }

    // ---- FU11: validation is whole-request, execution is per-item ----

    /// DynamoDB rejects the entire batch write operation when an op's key
    /// attributes do not match the table's key schema. The malformed op is last
    /// in the request, so the guarantee is only met if validation ran over the
    /// whole batch before any of it was applied.
    #[test]
    fn batch_write_validation_failure_applies_nothing() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let err = batch_write(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [
                        { "PutRequest": { "Item": { "pk": {"S": "first"}, "v": {"N": "1"} } } },
                        { "PutRequest": { "Item": { "v": {"N": "2"} } } }
                    ]
                }
            }),
        )
        .expect_err("the second op is missing its partition key");
        assert!(
            matches!(err, DynamoDbError::ValidationException(_)),
            "{err:?}"
        );
        assert!(
            read(&engine, &ctx, "first").is_none(),
            "a validation error rejects the whole request: the well-formed op \
             that preceded it must not have been applied"
        );
    }

    /// The other half of the contract. Once validation has passed, the batch is
    /// still not atomic: a runtime failure part way through — DynamoDB's
    /// throughput-exceeded or internal-error case, which it reports as
    /// `UnprocessedItems` rather than as a rejection — leaves the ops that
    /// already committed applied. That per-item independence is what makes
    /// retrying only the unprocessed ops correct.
    #[test]
    fn batch_write_runtime_failure_leaves_earlier_ops_applied() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);

        // Fail the second op's commit only. Each op commits in its own
        // single-item transaction, so the second hit of the commit fault is the
        // second op: the failure lands mid-batch, after the first op is durable.
        let faults = engine.commit_fault_handle_for_testing();
        faults.inject_error_on_nth_hit(
            commit_fault_labels::PREPARE_COMPLETE,
            2,
            nimbus_core::Error::Internal("injected storage failure".to_owned()),
        );

        let err = batch_write(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [
                        { "PutRequest": { "Item": { "pk": {"S": "first"}, "v": {"N": "1"} } } },
                        { "PutRequest": { "Item": { "pk": {"S": "second"}, "v": {"N": "2"} } } }
                    ]
                }
            }),
        )
        .expect_err("the second op's commit was failed by the injected fault");
        assert!(
            !matches!(err, DynamoDbError::ValidationException(_)),
            "a storage failure is a runtime error, not a validation error: {err:?}"
        );
        assert!(
            read(&engine, &ctx, "first").is_some(),
            "the op that committed before the runtime failure stays applied"
        );
        assert!(
            read(&engine, &ctx, "second").is_none(),
            "the op whose commit failed applied nothing"
        );
    }

    // ---- FU12: whole-request rejection rules ----
    //
    // Two more of DynamoDB's documented "rejects the entire batch write
    // operation" causes: "You try to perform multiple operations on the same
    // item in the same BatchWriteItem request" and "Any individual item in a
    // batch exceeds 400 KB". Both are validation errors, so they must land in
    // `prepare_batch_writes` and leave nothing applied.

    #[test]
    fn batch_write_duplicate_put_keys_is_validation_error() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let err = batch_write(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [
                        { "PutRequest": { "Item": { "pk": {"S": "dup"}, "v": {"N": "1"} } } },
                        { "PutRequest": { "Item": { "pk": {"S": "dup"}, "v": {"N": "2"} } } }
                    ]
                }
            }),
        )
        .expect_err("two puts of one item must be rejected, not resolved last-writer-wins");
        match &err {
            DynamoDbError::ValidationException(message) => assert!(
                message.contains("duplicates"),
                "expected DynamoDB's duplicate-key message, got {message:?}"
            ),
            other => panic!("expected ValidationException, got {other:?}"),
        }
        assert!(
            read(&engine, &ctx, "dup").is_none(),
            "the rejected request must apply nothing"
        );
    }

    #[test]
    fn batch_write_put_and_delete_of_same_key_is_validation_error() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(&engine, &ctx, "both", "0");
        let err = batch_write(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [
                        { "PutRequest": { "Item": { "pk": {"S": "both"}, "v": {"N": "1"} } } },
                        { "DeleteRequest": { "Key": { "pk": {"S": "both"} } } }
                    ]
                }
            }),
        )
        .expect_err("a put and a delete of one item are still two operations on that item");
        assert!(
            matches!(err, DynamoDbError::ValidationException(_)),
            "{err:?}"
        );
        assert_eq!(
            read(&engine, &ctx, "both")
                .and_then(|item| item.get("v").cloned())
                .expect("the pre-existing item survives a rejected request"),
            AttributeValue::N("0".into()),
            "neither the put nor the delete may be applied"
        );
    }

    /// The rule is one operation per *item*, and an item is identified by table
    /// plus key. The same key in two tables names two different items.
    #[test]
    fn batch_write_same_key_in_two_tables_is_not_a_duplicate() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        create_keyed_table(&engine, &ctx, "Invoices");
        batch_write(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [
                        { "PutRequest": { "Item": { "pk": {"S": "shared"}, "v": {"N": "1"} } } }
                    ],
                    "Invoices": [
                        { "PutRequest": { "Item": { "pk": {"S": "shared"}, "v": {"N": "2"} } } }
                    ]
                }
            }),
        )
        .expect("one op per item in each table is a well-formed request");
        assert!(read_from(&engine, &ctx, "Orders", "shared").is_some());
        assert!(read_from(&engine, &ctx, "Invoices", "shared").is_some());
    }

    #[test]
    fn batch_write_oversized_item_is_validation_error() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        // Just over the 400 KiB ceiling once the attribute names are counted:
        // DynamoDB sizes an item as the sum of the lengths of its attribute
        // names and values.
        let oversized = "a".repeat(MAX_ITEM_SIZE_BYTES);
        let err = batch_write(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [
                        { "PutRequest": { "Item": { "pk": {"S": "small"} } } },
                        { "PutRequest": { "Item": { "pk": {"S": "big"}, "blob": {"S": oversized} } } }
                    ]
                }
            }),
        )
        .expect_err("an item over 400 KiB must be rejected");
        assert!(
            matches!(err, DynamoDbError::ValidationException(_)),
            "{err:?}"
        );
        assert!(
            read(&engine, &ctx, "small").is_none(),
            "the oversized item rejects the whole request, including the op ahead of it"
        );
    }

    /// The ceiling is a ceiling, not a rounding: an item just under 400 KiB is
    /// accepted. Without this the size rule could pass by rejecting everything.
    #[test]
    fn batch_write_item_just_under_the_size_limit_is_accepted() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        // Names "pk" (2) and "blob" (4) plus the key's value "big" (3) are 9
        // bytes, so a blob of MAX - 9 puts the item exactly at the limit.
        let value = "a".repeat(MAX_ITEM_SIZE_BYTES - 9);
        batch_write(
            &engine,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [
                        { "PutRequest": { "Item": { "pk": {"S": "big"}, "blob": {"S": value} } } }
                    ]
                }
            }),
        )
        .expect("an item at exactly the limit is within it");
        assert!(read(&engine, &ctx, "big").is_some());
    }
}
