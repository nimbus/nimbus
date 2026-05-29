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
    DeleteItemInput, DeleteItemOutput, GetItemInput, GetItemOutput, Item, KeySchemaElement,
    KeyType, PutItemInput, PutItemOutput, ReturnValues, StreamEventName, UpdateItemInput,
    UpdateItemOutput, extract_key,
};
use nimbus_core::{DocumentId, TableName};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;

use crate::attribute_value::{fields_to_item, item_to_fields, validate_item};
use crate::commands::{control_plane, stream};
use crate::error::map_core_error;
use crate::expression::{
    apply_update, build_maps, check_condition, default_limits, parse_update_expression,
    project_item, reject_key_updates, updated_attributes,
};
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

    // Capture a stream event (no-op unless the table has a stream enabled).
    let event_name = if existing.is_some() {
        StreamEventName::Modify
    } else {
        StreamEventName::Insert
    };
    let keys = extract_key(&input.item, &key_schema);
    stream::capture_event(
        service,
        context,
        &input.table_name,
        event_name,
        &keys,
        existing.as_ref(),
        Some(&input.item),
    )?;

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

/// DeleteItem: gate on any `ConditionExpression`, delete the item if present,
/// and honor `ReturnValues` (NONE / ALL_OLD). Deleting an absent key with no
/// condition is a successful no-op (DynamoDB semantics).
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for a missing key; `ConditionalCheckFailedException` if the condition fails.
pub fn delete_item(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: DeleteItemInput,
) -> Result<DeleteItemOutput, DynamoDbError> {
    let key_schema = control_plane::load_key_schema(service, context, &input.table_name)?;
    let id = primary_key_id(&input.key, &key_schema)?;
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;

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

    if existing.is_some() {
        service
            .delete_document(context.tenant_id(), table, id)
            .map_err(map_core_error)?;
        // Capture a REMOVE stream event (no-op unless a stream is enabled).
        let keys = extract_key(&input.key, &key_schema);
        stream::capture_event(
            service,
            context,
            &input.table_name,
            StreamEventName::Remove,
            &keys,
            existing.as_ref(),
            None,
        )?;
    }

    let attributes = match input.return_values {
        ReturnValues::AllOld => existing,
        _ => None,
    };
    Ok(DeleteItemOutput {
        attributes,
        consumed_capacity: None,
        item_collection_metrics: None,
    })
}

/// UpdateItem: parse the `UpdateExpression`, gate on any `ConditionExpression`,
/// upsert-and-mutate the item, and return the requested `ReturnValues` view
/// (NONE / ALL_OLD / ALL_NEW / UPDATED_OLD / UPDATED_NEW).
///
/// No `UpdateExpression` is a no-op upsert (the item is created with just its
/// key when absent); `Some("")` errors via the tokenizer. Updates to a key
/// attribute are rejected. UPDATED_OLD/UPDATED_NEW return only the touched
/// attributes (leaf-wrapped for nested paths) and omit `Attributes` when empty.
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for a missing key, empty/malformed expression, or a key-attribute update;
/// `ConditionalCheckFailedException` if the condition fails.
pub fn update_item(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: UpdateItemInput,
) -> Result<UpdateItemOutput, DynamoDbError> {
    let key_schema = control_plane::load_key_schema(service, context, &input.table_name)?;
    let id = primary_key_id(&input.key, &key_schema)?;
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;
    let limits = default_limits();

    let actions = match input.update_expression.as_deref() {
        Some(expression) => parse_update_expression(expression, &limits)?,
        None => Vec::new(),
    };
    let maps = build_maps(
        input.expression_attribute_names.as_ref(),
        input.expression_attribute_values.as_ref(),
    );
    reject_key_updates(&actions, &key_schema, &maps)?;

    let old_item = read_item(service, context, &table, id.clone())?;

    let gate_item = old_item.clone().unwrap_or_default();
    check_condition(
        input.condition_expression.as_deref(),
        input.expression_attribute_names.as_ref(),
        input.expression_attribute_values.as_ref(),
        &gate_item,
        &limits,
    )?;

    // Upsert: base on the existing item, else the Key item (so a created item
    // carries its key attributes), then apply the actions.
    let mut new_item = old_item.clone().unwrap_or_else(|| input.key.clone());
    apply_update(&actions, &mut new_item, &maps)?;

    // Store the result (replace; overwrite atomicity per DDB-DIV-005).
    let fields = item_to_fields(&new_item)?;
    if old_item.is_some() {
        service
            .delete_document(context.tenant_id(), table.clone(), id.clone())
            .map_err(map_core_error)?;
    }
    service
        .insert_document_with_id(context.tenant_id(), table, id, fields)
        .map_err(map_core_error)?;

    // Capture a stream event (no-op unless the table has a stream enabled).
    let event_name = if old_item.is_some() {
        StreamEventName::Modify
    } else {
        StreamEventName::Insert
    };
    let keys = extract_key(&new_item, &key_schema);
    stream::capture_event(
        service,
        context,
        &input.table_name,
        event_name,
        &keys,
        old_item.as_ref(),
        Some(&new_item),
    )?;

    let attributes = match input.return_values {
        ReturnValues::None => None,
        ReturnValues::AllOld => old_item,
        ReturnValues::AllNew => Some(new_item),
        ReturnValues::UpdatedOld => old_item
            .map(|item| updated_attributes(&item, &actions, &maps))
            .filter(|item| !item.is_empty()),
        ReturnValues::UpdatedNew => {
            Some(updated_attributes(&new_item, &actions, &maps)).filter(|item| !item.is_empty())
        }
    };
    Ok(UpdateItemOutput {
        attributes,
        consumed_capacity: None,
        item_collection_metrics: None,
    })
}

/// Store `item` under its primary key (full replace, no condition/ReturnValues).
/// Shared by BatchWriteItem (D3.2) and TransactWriteItems (D3.4).
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for an invalid item or missing key.
pub(crate) fn store_item(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
    item: &Item,
) -> Result<(), DynamoDbError> {
    validate_item(item)?;
    let key_schema = control_plane::load_key_schema(service, context, table_name)?;
    let id = primary_key_id(item, &key_schema)?;
    let table = TableName::new(table_name).map_err(map_core_error)?;
    if read_item(service, context, &table, id.clone())?.is_some() {
        service
            .delete_document(context.tenant_id(), table.clone(), id.clone())
            .map_err(map_core_error)?;
    }
    service
        .insert_document_with_id(context.tenant_id(), table, id, item_to_fields(item)?)
        .map_err(map_core_error)?;
    Ok(())
}

/// Delete the item at `key` (no condition). A no-op if the key is absent.
/// Shared by BatchWriteItem (D3.2) and TransactWriteItems (D3.4).
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for a missing key.
pub(crate) fn remove_item(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
    key: &Item,
) -> Result<(), DynamoDbError> {
    let key_schema = control_plane::load_key_schema(service, context, table_name)?;
    let id = primary_key_id(key, &key_schema)?;
    let table = TableName::new(table_name).map_err(map_core_error)?;
    if read_item(service, context, &table, id.clone())?.is_some() {
        service
            .delete_document(context.tenant_id(), table, id)
            .map_err(map_core_error)?;
    }
    Ok(())
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

    // ---- D1.7: DeleteItem ----

    fn delete(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<DeleteItemOutput, DynamoDbError> {
        delete_item(service, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn delete_removes_the_item() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("put");
        delete(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "o1"} } }),
        )
        .expect("delete");
        assert!(stored(&service, &ctx, "o1").is_none(), "item is gone");
    }

    #[test]
    fn delete_all_old_returns_the_deleted_item() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "v": {"N": "7"} } }),
        )
        .expect("put");
        let out = delete(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "ReturnValues": "ALL_OLD",
            }),
        )
        .expect("delete");
        let old = out.attributes.expect("deleted item returned");
        assert_eq!(old.get("v"), Some(&AttributeValue::N("7".into())));
    }

    #[test]
    fn delete_absent_key_is_a_noop_success() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let out = delete(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "absent"} },
                "ReturnValues": "ALL_OLD",
            }),
        )
        .expect("delete of absent key succeeds");
        assert!(out.attributes.is_none(), "nothing to return");
    }

    #[test]
    fn delete_with_failing_condition_is_conditional_check_failed() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("put");
        // attribute_not_exists(pk) must fail since the item exists.
        let err = delete(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "ConditionExpression": "attribute_not_exists(pk)",
            }),
        )
        .expect_err("condition should fail");
        assert!(matches!(
            err,
            DynamoDbError::ConditionalCheckFailedException(_, _)
        ));
        assert!(stored(&service, &ctx, "o1").is_some(), "item not deleted");
    }

    // ---- D1.8: UpdateItem ----

    fn update(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<UpdateItemOutput, DynamoDbError> {
        update_item(service, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn update_set_modifies_and_all_new_returns_full_item() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "v": {"N": "1"} } }),
        )
        .expect("put");
        let out = update(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "UpdateExpression": "SET v = :v, label = :l",
                "ExpressionAttributeValues": { ":v": {"N": "9"}, ":l": {"S": "hi"} },
                "ReturnValues": "ALL_NEW",
            }),
        )
        .expect("update");
        let item = out.attributes.expect("ALL_NEW item");
        assert_eq!(item.get("v"), Some(&AttributeValue::N("9".into())));
        assert_eq!(item.get("label"), Some(&AttributeValue::S("hi".into())));
        assert_eq!(item.get("pk"), Some(&AttributeValue::S("o1".into())));
    }

    #[test]
    fn update_upsert_creates_when_absent() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        update(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "new"} },
                "UpdateExpression": "SET v = :v",
                "ExpressionAttributeValues": { ":v": {"N": "5"} },
            }),
        )
        .expect("upsert update");
        let item = stored(&service, &ctx, "new").expect("item created");
        assert_eq!(item.get("pk"), Some(&AttributeValue::S("new".into())));
        assert_eq!(item.get("v"), Some(&AttributeValue::N("5".into())));
    }

    #[test]
    fn update_no_op_upsert_creates_key_only_item() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        // No UpdateExpression on an absent key creates a key-only item.
        update(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "bare"} } }),
        )
        .expect("no-op upsert");
        let item = stored(&service, &ctx, "bare").expect("key-only item created");
        assert_eq!(item.len(), 1);
        assert_eq!(item.get("pk"), Some(&AttributeValue::S("bare".into())));
    }

    #[test]
    fn update_updated_new_returns_only_changed_attributes() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "a": {"N": "1"}, "b": {"N": "2"} } }),
        )
        .expect("put");
        let out = update(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "UpdateExpression": "SET a = :a",
                "ExpressionAttributeValues": { ":a": {"N": "10"} },
                "ReturnValues": "UPDATED_NEW",
            }),
        )
        .expect("update");
        let item = out.attributes.expect("UPDATED_NEW");
        assert_eq!(item.len(), 1, "only the changed attribute is returned");
        assert_eq!(item.get("a"), Some(&AttributeValue::N("10".into())));
    }

    #[test]
    fn update_updated_old_omits_attributes_when_empty() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("put");
        // SET a brand-new attribute: UPDATED_OLD has no prior value → Attributes omitted.
        let out = update(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "UpdateExpression": "SET fresh = :v",
                "ExpressionAttributeValues": { ":v": {"N": "1"} },
                "ReturnValues": "UPDATED_OLD",
            }),
        )
        .expect("update");
        assert!(
            out.attributes.is_none(),
            "UPDATED_OLD omits Attributes when nothing had a prior value"
        );
    }

    #[test]
    fn update_nested_path_updated_new_leaf_wraps() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "o1"}, "m": {"M": { "a": {"N": "1"}, "b": {"N": "2"} }} },
            }),
        )
        .expect("put");
        let out = update(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "UpdateExpression": "SET m.a = :v",
                "ExpressionAttributeValues": { ":v": {"N": "9"} },
                "ReturnValues": "UPDATED_NEW",
            }),
        )
        .expect("update");
        // UPDATED_NEW leaf-wraps the nested path: { m: { a: 9 } } (only a).
        let item = out.attributes.expect("UPDATED_NEW");
        let mut inner = std::collections::BTreeMap::new();
        inner.insert("a".to_string(), AttributeValue::N("9".into()));
        assert_eq!(item.get("m"), Some(&AttributeValue::M(inner)));
    }

    #[test]
    fn update_rejects_key_attribute_mutation() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let err = update(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "UpdateExpression": "SET pk = :v",
                "ExpressionAttributeValues": { ":v": {"S": "other"} },
            }),
        )
        .expect_err("updating the key must be rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn update_empty_expression_string_errors() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let err = update(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "UpdateExpression": "",
            }),
        )
        .expect_err("empty UpdateExpression must error");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn update_condition_gate_blocks_mutation() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(
            &service,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "v": {"N": "1"} } }),
        )
        .expect("put");
        let err = update(
            &service,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "o1"} },
                "UpdateExpression": "SET v = :v",
                "ExpressionAttributeValues": { ":v": {"N": "9"} },
                "ConditionExpression": "attribute_not_exists(pk)",
            }),
        )
        .expect_err("condition should block");
        assert!(matches!(
            err,
            DynamoDbError::ConditionalCheckFailedException(_, _)
        ));
        // The item is unchanged.
        assert_eq!(
            stored(&service, &ctx, "o1").unwrap().get("v"),
            Some(&AttributeValue::N("1".into()))
        );
    }
}
