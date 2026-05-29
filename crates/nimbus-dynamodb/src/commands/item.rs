//! Single-item operations (T1): PutItem (D1.5), GetItem (D1.6), DeleteItem
//! (D1.7), UpdateItem (D1.8).
//!
//! Items are stored as AttributeValue wire-JSON in `Document.fields` (lossless;
//! see DDB-DIV-005) under the order-preserving composite-key `DocumentId` (D0.3),
//! in a Nimbus table named after the DynamoDB table. Each handler is
//! tenant-scoped via the `TenantIsolationContext`.

use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    GetItemInput, GetItemOutput, Item, KeySchemaElement, KeyType, PutItemInput, PutItemOutput,
    ReturnValues,
};
use nimbus_core::{DocumentId, TableName};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;

use crate::attribute_value::{fields_to_item, item_to_fields, validate_item};
use crate::commands::control_plane;
use crate::error::map_core_error;
use crate::expression::{check_condition, default_limits, project_item};
use crate::key::encode_key;

/// PutItem: validate the item, gate on any `ConditionExpression`, replace-or-
/// insert it, and honor `ReturnValues` (NONE / ALL_OLD).
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for an invalid item or missing key; `ConditionalCheckFailedException` if the
/// condition fails; a mapped engine error otherwise.
pub fn put_item(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: PutItemInput,
) -> Result<PutItemOutput, DynamoDbError> {
    validate_item(&input.item)?;
    let key_schema = control_plane::load_key_schema(service, context, &input.table_name)?;
    let id = primary_key_id(&input.item, &key_schema)?;
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;

    // The existing item backs both the condition gate and ReturnValues=ALL_OLD.
    let existing = read_item(service, context, &table, id.clone())?;

    let limits = default_limits();
    let gate_item = existing.clone().unwrap_or_default();
    check_condition(
        input.condition_expression.as_deref(),
        input.expression_attribute_names.as_ref(),
        input.expression_attribute_values.as_ref(),
        &gate_item,
        &limits,
    )?;

    // PutItem fully *replaces* the item. The engine has no atomic upsert and a
    // bare insert errors on an existing key, so an overwrite is delete + insert.
    // (An atomic store-level upsert is the proper follow-up — see DDB-DIV-005.)
    let fields = item_to_fields(&input.item)?;
    if existing.is_some() {
        service
            .delete_document(context.tenant_id(), table.clone(), id.clone())
            .map_err(map_core_error)?;
    }
    service
        .insert_document_with_id(context.tenant_id(), table, id, fields)
        .map_err(map_core_error)?;

    let attributes = match input.return_values {
        ReturnValues::AllOld => existing,
        _ => None,
    };
    Ok(PutItemOutput {
        attributes,
        consumed_capacity: None,
        item_collection_metrics: None,
    })
}

/// GetItem: read an item by key, applying any `ProjectionExpression` (or legacy
/// `AttributesToGet`). `ConsistentRead` is accepted and ignored — the single
/// store is strictly consistent, so every read is already strongly consistent.
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for a missing key or malformed projection.
pub fn get_item(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: GetItemInput,
) -> Result<GetItemOutput, DynamoDbError> {
    let key_schema = control_plane::load_key_schema(service, context, &input.table_name)?;
    let id = primary_key_id(&input.key, &key_schema)?;
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;

    let item = match read_item(service, context, &table, id)? {
        Some(item) => Some(project_get(&input, item)?),
        None => None,
    };
    Ok(GetItemOutput {
        item,
        consumed_capacity: None,
    })
}

/// Apply a GetItem's `ProjectionExpression` (or legacy top-level
/// `AttributesToGet`) to the read item; an unprojected read returns it whole.
fn project_get(input: &GetItemInput, item: Item) -> Result<Item, DynamoDbError> {
    if let Some(projection) = input
        .projection_expression
        .as_deref()
        .filter(|expression| !expression.is_empty())
    {
        return project_item(
            projection,
            input.expression_attribute_names.as_ref(),
            &item,
            &default_limits(),
        );
    }
    if let Some(names) = &input.attributes_to_get {
        // Legacy AttributesToGet selects top-level attributes by name.
        return Ok(names
            .iter()
            .filter_map(|name| item.get(name).map(|value| (name.clone(), value.clone())))
            .collect());
    }
    Ok(item)
}

/// Read a stored item by id, mapping a missing document to `None`.
pub(crate) fn read_item(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table: &TableName,
    id: DocumentId,
) -> Result<Option<Item>, DynamoDbError> {
    match service.get_document(context.tenant_id(), table, id) {
        Ok(document) => Ok(Some(fields_to_item(&document.fields)?)),
        Err(nimbus_core::Error::DocumentNotFound(_)) => Ok(None),
        Err(error) => Err(map_core_error(error)),
    }
}

/// Extract the primary-key `DocumentId` for `item` under `key_schema`: the HASH
/// attribute (and RANGE, when the table defines one) must be present, and are
/// encoded with the order-preserving composite-key codec (D0.3).
///
/// # Errors
/// `ValidationException` if the schema lacks a partition key or a required key
/// attribute is missing from the item; a key-codec error for a non-scalar or
/// oversize key.
pub(crate) fn primary_key_id(
    item: &Item,
    key_schema: &[KeySchemaElement],
) -> Result<DocumentId, DynamoDbError> {
    let pk_name = key_schema
        .iter()
        .find(|element| element.key_type == KeyType::Hash)
        .ok_or_else(|| {
            DynamoDbError::ValidationException("table key schema has no partition key".to_owned())
        })?;
    let pk = item
        .get(&pk_name.attribute_name)
        .ok_or_else(|| missing_key(&pk_name.attribute_name))?;
    let sk = match key_schema
        .iter()
        .find(|element| element.key_type == KeyType::Range)
    {
        Some(sk_name) => Some(
            item.get(&sk_name.attribute_name)
                .ok_or_else(|| missing_key(&sk_name.attribute_name))?,
        ),
        None => None,
    };
    let encoded = encode_key(pk, sk)?;
    DocumentId::from_key(&encoded).map_err(map_core_error)
}

fn missing_key(attribute: &str) -> DynamoDbError {
    DynamoDbError::ValidationException(format!(
        "One of the required keys was not given a value: {attribute}"
    ))
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

    /// Create table "Orders" with a single `pk` (String) partition key.
    fn create_orders(service: &Arc<Service>, context: &TenantIsolationContext) {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "Orders",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap();
        control_plane::create_table(service, context, input).expect("create table");
    }

    fn put(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<PutItemOutput, DynamoDbError> {
        put_item(service, context, serde_json::from_value(input).unwrap())
    }

    /// Read the stored item for a given `pk` value.
    fn stored(service: &Arc<Service>, context: &TenantIsolationContext, pk: &str) -> Option<Item> {
        let key: Item = [("pk".to_string(), AttributeValue::S(pk.into()))]
            .into_iter()
            .collect();
        let schema = control_plane::load_key_schema(service, context, "Orders").unwrap();
        let id = primary_key_id(&key, &schema).unwrap();
        read_item(service, context, &TableName::new("Orders").unwrap(), id).unwrap()
    }

    #[test]
    fn put_then_read_stores_the_item_losslessly() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "o1"}, "qty": {"N": "42"}, "tags": {"SS": ["a", "b"]} },
            }),
        )
        .expect("put");
        let item = stored(&service, &ctx, "o1").expect("item present");
        assert_eq!(item.get("qty"), Some(&AttributeValue::N("42".into())));
        assert_eq!(
            item.get("tags"),
            Some(&AttributeValue::SS(
                ["a", "b"].iter().map(|s| (*s).to_string()).collect()
            ))
        );
    }

    #[test]
    fn put_overwrite_fully_replaces_not_merges() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "x": {"N": "1"} } }),
        )
        .expect("first put");
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "y": {"N": "2"} } }),
        )
        .expect("second put");
        let item = stored(&service, &ctx, "o1").expect("item present");
        assert!(!item.contains_key("x"), "PutItem replaces, so x is gone");
        assert_eq!(item.get("y"), Some(&AttributeValue::N("2".into())));
    }

    #[test]
    fn put_all_old_returns_the_previous_item() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        // No previous item → ALL_OLD returns nothing.
        let first = put(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "o1"}, "v": {"N": "1"} },
                "ReturnValues": "ALL_OLD",
            }),
        )
        .expect("first put");
        assert!(first.attributes.is_none(), "no previous item to return");
        // Overwrite → ALL_OLD returns the previous item.
        let second = put(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "o1"}, "v": {"N": "2"} },
                "ReturnValues": "ALL_OLD",
            }),
        )
        .expect("second put");
        let old = second.attributes.expect("previous item returned");
        assert_eq!(old.get("v"), Some(&AttributeValue::N("1".into())));
    }

    #[test]
    fn put_with_failing_condition_is_conditional_check_failed() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("first put");
        // attribute_not_exists(pk) must fail now that the item exists.
        let err = put(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "o1"}, "v": {"N": "9"} },
                "ConditionExpression": "attribute_not_exists(pk)",
            }),
        )
        .expect_err("condition should fail");
        assert!(matches!(
            err,
            DynamoDbError::ConditionalCheckFailedException(_, _)
        ));
        // The original item is unchanged (no v attribute written).
        let item = stored(&service, &ctx, "o1").expect("item present");
        assert!(!item.contains_key("v"));
    }

    #[test]
    fn put_create_if_absent_succeeds_when_absent() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "fresh"}, "v": {"N": "1"} },
                "ConditionExpression": "attribute_not_exists(pk)",
            }),
        )
        .expect("create-if-absent should succeed for a new key");
        assert!(stored(&service, &ctx, "fresh").is_some());
    }

    #[test]
    fn put_missing_partition_key_is_validation_error() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let err = put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "other": {"S": "x"} } }),
        )
        .expect_err("missing pk should fail");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn put_to_missing_table_is_resource_not_found() {
        let (service, ctx, _t) = fixture();
        let err = put(
            &service,
            &ctx,
            json!({ "TableName": "Ghost", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect_err("missing table should fail");
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }

    // ---- D1.6: GetItem ----

    fn get(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> GetItemOutput {
        get_item(service, context, serde_json::from_value(input).unwrap()).expect("get")
    }

    #[test]
    fn get_returns_the_stored_item() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "qty": {"N": "5"} } }),
        )
        .expect("put");
        let out = get(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "o1"} } }),
        );
        let item = out.item.expect("item present");
        assert_eq!(item.get("pk"), Some(&AttributeValue::S("o1".into())));
        assert_eq!(item.get("qty"), Some(&AttributeValue::N("5".into())));
    }

    #[test]
    fn get_missing_item_returns_none() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let out = get(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "absent"} } }),
        );
        assert!(out.item.is_none(), "missing item yields no Item field");
    }

    #[test]
    fn get_with_projection_selects_subset() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "o1"}, "a": {"N": "1"}, "b": {"N": "2"}, "c": {"N": "3"} },
            }),
        )
        .expect("put");
        let out = get(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "ProjectionExpression": "a, c",
            }),
        );
        let item = out.item.expect("item present");
        assert_eq!(item.len(), 2);
        assert!(item.contains_key("a") && item.contains_key("c"));
        assert!(!item.contains_key("b"), "b is projected out");
    }

    #[test]
    fn get_with_consistent_read_is_accepted_and_ignored() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("put");
        // ConsistentRead=true must not change the result (single store is
        // already strongly consistent).
        let out = get(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "o1"} }, "ConsistentRead": true }),
        );
        assert!(out.item.is_some());
    }
}
