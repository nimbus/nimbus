//! Batch operations (T3): BatchGetItem (D3.1) and BatchWriteItem (D3.2).
//!
//! Both fan out over the single-item handlers (`item::*`) per request item.
//! The Nimbus store is reliable and not throttled, so `UnprocessedKeys` /
//! `UnprocessedItems` are always empty — every requested key/op is processed.

use std::collections::HashMap;
use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    BatchGetItemInput, BatchGetItemOutput, BatchWriteItemInput, BatchWriteItemOutput, Item,
};
use nimbus_core::TableName;
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;

use crate::commands::control_plane;
use crate::commands::item::{primary_key_id, read_item, remove_item, store_item};
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
    service: &Arc<Service>,
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
        let key_schema = control_plane::load_key_schema(service, context, table_name)?;
        let table = TableName::new(table_name).map_err(map_core_error)?;
        let mut items = Vec::new();
        for key in &requested.keys {
            let id = primary_key_id(key, &key_schema)?;
            if let Some(item) = read_item(service, context, &table, id)? {
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

/// BatchWriteItem: apply up to 25 Put/Delete requests across tables. Each
/// `WriteRequest` must carry exactly one of `PutRequest`/`DeleteRequest`.
/// `UnprocessedItems` is always empty (the store applies every op; there is no
/// throttling). Note: unlike TransactWriteItems this is **not** atomic — a
/// later validation error leaves earlier writes applied (DynamoDB semantics).
///
/// # Errors
/// `ValidationException` for an empty request, more than 25 ops, or a
/// `WriteRequest` without exactly one of Put/Delete; `ResourceNotFoundException`
/// if a referenced table is absent.
pub fn batch_write_item(
    service: &Arc<Service>,
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

    for (table_name, requests) in &input.request_items {
        for request in requests {
            match (&request.put_request, &request.delete_request) {
                (Some(put), None) => store_item(service, context, table_name, &put.item)?,
                (None, Some(delete)) => remove_item(service, context, table_name, &delete.key)?,
                _ => {
                    return Err(DynamoDbError::ValidationException(
                        "Each WriteRequest must contain exactly one of PutRequest or DeleteRequest"
                            .to_owned(),
                    ));
                }
            }
        }
    }

    Ok(BatchWriteItemOutput {
        unprocessed_items: HashMap::new(),
        consumed_capacity: None,
        item_collection_metrics: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use extenddb_core::types::{AttributeValue, CreateTableInput};
    use nimbus_core::TenantId;
    use serde_json::json;

    fn fixture() -> (Arc<Service>, TenantIsolationContext, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let service = Arc::new(Service::new(temp.path()).expect("service"));
        let context = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
        crate::tenant::ensure_tenant(&service, &context).expect("tenant");
        (service, context, temp)
    }

    fn create_orders(service: &Arc<Service>, context: &TenantIsolationContext) {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "Orders",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap();
        control_plane::create_table(service, context, input).expect("create table");
    }

    fn put(service: &Arc<Service>, context: &TenantIsolationContext, pk: &str, v: &str) {
        crate::commands::item::put_item(
            service,
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
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<BatchGetItemOutput, DynamoDbError> {
        batch_get_item(service, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn batch_get_returns_present_items_and_skips_missing() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(&service, &ctx, "a", "1");
        put(&service, &ctx, "b", "2");
        let out = batch_get(
            &service,
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
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(&service, &ctx, "a", "1");
        let out = batch_get(
            &service,
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
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let err = batch_get(&service, &ctx, json!({ "RequestItems": {} }))
            .expect_err("empty request rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn batch_get_over_100_keys_is_validation_error() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let keys: Vec<serde_json::Value> = (0..101)
            .map(|i| json!({ "pk": {"S": format!("k{i}")} }))
            .collect();
        let err = batch_get(
            &service,
            &ctx,
            json!({ "RequestItems": { "Orders": { "Keys": keys } } }),
        )
        .expect_err("over-100 rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    // ---- D3.2: BatchWriteItem ----

    fn read(service: &Arc<Service>, context: &TenantIsolationContext, pk: &str) -> Option<Item> {
        let key: Item = [("pk".to_string(), AttributeValue::S(pk.into()))]
            .into_iter()
            .collect();
        let schema = control_plane::load_key_schema(service, context, "Orders").unwrap();
        let id = primary_key_id(&key, &schema).unwrap();
        read_item(service, context, &TableName::new("Orders").unwrap(), id).unwrap()
    }

    fn batch_write(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<extenddb_core::types::BatchWriteItemOutput, DynamoDbError> {
        batch_write_item(service, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn batch_write_puts_and_deletes() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(&service, &ctx, "old", "1"); // will be deleted
        let out = batch_write(
            &service,
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
        assert!(read(&service, &ctx, "a").is_some());
        assert!(read(&service, &ctx, "b").is_some());
        assert!(read(&service, &ctx, "old").is_none(), "deleted");
    }

    #[test]
    fn batch_write_request_with_both_put_and_delete_is_rejected() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let err = batch_write(
            &service,
            &ctx,
            json!({
                "RequestItems": {
                    "Orders": [ {
                        "PutRequest": { "Item": { "pk": {"S": "a"} } },
                        "DeleteRequest": { "Key": { "pk": {"S": "a"} } }
                    } ]
                }
            }),
        )
        .expect_err("both put and delete rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn batch_write_over_25_ops_is_validation_error() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let ops: Vec<serde_json::Value> = (0..26)
            .map(|i| json!({ "PutRequest": { "Item": { "pk": {"S": format!("k{i}")} } } }))
            .collect();
        let err = batch_write(&service, &ctx, json!({ "RequestItems": { "Orders": ops } }))
            .expect_err("over-25 rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn batch_write_empty_request_is_validation_error() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let err =
            batch_write(&service, &ctx, json!({ "RequestItems": {} })).expect_err("empty rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }
}
