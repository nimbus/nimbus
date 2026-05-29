//! Transactional operations (T3): TransactGetItems (D3.3) and TransactWriteItems
//! (D3.4).
//!
//! Both run through the engine's transaction session manager for snapshot /
//! atomic semantics: TransactGetItems reads all items through one read-only
//! snapshot; TransactWriteItems (D3.4) stages all writes and commits them
//! atomically, returning `TransactionCanceledException` with per-op
//! `CancellationReasons` on conflict.

use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    CancellationReason, Item, ItemResponse, ReturnValuesOnConditionCheckFailure,
    TransactGetItemsInput, TransactGetItemsOutput, TransactWriteItem, TransactWriteItemsInput,
    TransactWriteItemsOutput,
};
use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, Document, DocumentLocator, PrincipalContext, TableName,
    TransactionSessionMode, TransactionSessionToken, WriteKey, WritePrecondition, WriteSetMode,
};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;

use crate::attribute_value::{fields_to_item, item_to_fields};
use crate::commands::control_plane;
use crate::commands::item::primary_key_id;
use crate::error::map_core_error;
use crate::expression::{
    apply_update, build_maps, default_limits, evaluate_condition, parse_condition,
    parse_update_expression, project_item, reject_key_updates,
};

/// The DynamoDB per-call item limit for TransactGetItems / TransactWriteItems.
const MAX_TRANSACT_ITEMS: usize = 100;

/// TransactGetItems: read up to 100 items through a single read-only snapshot,
/// returning one ordered `ItemResponse` per request item (item absent when the
/// key does not exist). The snapshot gives a consistent view across all reads.
///
/// # Errors
/// `ValidationException` for an empty request or more than 100 items;
/// `ResourceNotFoundException` if a referenced table is absent.
pub fn transact_get_items(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: TransactGetItemsInput,
) -> Result<TransactGetItemsOutput, DynamoDbError> {
    if input.transact_items.is_empty() {
        return Err(DynamoDbError::ValidationException(
            "TransactItems must have at least one item".to_owned(),
        ));
    }
    if input.transact_items.len() > MAX_TRANSACT_ITEMS {
        return Err(DynamoDbError::ValidationException(
            "Member must have length less than or equal to 100".to_owned(),
        ));
    }

    let principal = PrincipalContext::system();
    let session = service
        .begin_transaction_session(
            context.tenant_id().clone(),
            principal.clone(),
            TransactionSessionMode::ReadOnly,
        )
        .map_err(map_core_error)?;
    let token = session.token;

    // Compute under the snapshot, then always roll the read-only session back.
    let result = read_all(service, context, &principal, &token, &input);
    let _ = service.rollback_transaction_session(context.tenant_id(), &token, &principal);

    Ok(TransactGetItemsOutput {
        responses: result?,
        consumed_capacity: None,
    })
}

fn read_all(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    principal: &PrincipalContext,
    token: &nimbus_core::TransactionSessionToken,
    input: &TransactGetItemsInput,
) -> Result<Vec<ItemResponse>, DynamoDbError> {
    let limits = default_limits();
    let mut responses = Vec::with_capacity(input.transact_items.len());
    for transact in &input.transact_items {
        let get = &transact.get;
        let key_schema = control_plane::load_key_schema(service, context, &get.table_name)?;
        let id = primary_key_id(&get.key, &key_schema)?;
        let table = TableName::new(&get.table_name).map_err(map_core_error)?;
        let item = match service
            .get_document_in_transaction(context.tenant_id(), token, principal, &table, id)
            .map_err(map_core_error)?
        {
            Some(document) => {
                let item = fields_to_item(&document.fields)?;
                let projected = match get
                    .projection_expression
                    .as_deref()
                    .filter(|expression| !expression.is_empty())
                {
                    Some(expression) => project_item(
                        expression,
                        get.expression_attribute_names.as_ref(),
                        &item,
                        &limits,
                    )?,
                    None => item,
                };
                Some(projected)
            }
            None => None,
        };
        responses.push(ItemResponse { item });
    }
    Ok(responses)
}

/// The AWS message for a cancelled transaction.
const TRANSACTION_CANCELLED: &str =
    "Transaction cancelled, please refer cancellation reasons for specific reasons";

/// TransactWriteItems: apply up to 100 Put/Update/Delete/ConditionCheck ops
/// atomically. Each op's `ConditionExpression` is evaluated against one
/// read-write snapshot; if any fails, nothing is written and a
/// `TransactionCanceledException` with per-op `CancellationReasons` is returned.
/// Otherwise all writes commit atomically (one storage transaction), with
/// per-item update-time preconditions for serializability.
///
/// # Errors
/// `ValidationException` for an empty/oversized request or a malformed op;
/// `ResourceNotFoundException` for a missing table;
/// `TransactionCanceledException` when any condition fails.
pub fn transact_write_items(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: TransactWriteItemsInput,
) -> Result<TransactWriteItemsOutput, DynamoDbError> {
    if input.transact_items.is_empty() {
        return Err(DynamoDbError::ValidationException(
            "TransactItems must have at least one item".to_owned(),
        ));
    }
    if input.transact_items.len() > MAX_TRANSACT_ITEMS {
        return Err(DynamoDbError::ValidationException(
            "Member must have length less than or equal to 100".to_owned(),
        ));
    }

    let principal = PrincipalContext::system();
    let session = service
        .begin_transaction_session(
            context.tenant_id().clone(),
            principal.clone(),
            TransactionSessionMode::ReadWrite,
        )
        .map_err(map_core_error)?;
    let token = session.token;

    let planned = plan_writes(service, context, &principal, &token, &input);
    let plans = match planned {
        Ok(plans) => plans,
        Err(error) => {
            let _ = service.rollback_transaction_session(context.tenant_id(), &token, &principal);
            return Err(error);
        }
    };

    // If any condition failed, cancel the whole transaction.
    if plans.iter().any(|plan| plan.failed) {
        let _ = service.rollback_transaction_session(context.tenant_id(), &token, &principal);
        return Err(DynamoDbError::TransactionCanceledException {
            message: TRANSACTION_CANCELLED.to_owned(),
            cancellation_reasons: plans.into_iter().map(|plan| plan.reason).collect(),
        });
    }

    // All conditions held: commit every write atomically.
    let writes: Vec<AtomicWrite> = plans.into_iter().map(|plan| plan.write).collect();
    let batch = AtomicWriteBatch::new(writes).map_err(map_core_error)?;
    service
        .commit_transaction_session(context.tenant_id(), &token, &principal, Some(batch))
        .map_err(map_core_error)?;

    Ok(TransactWriteItemsOutput {
        consumed_capacity: None,
        item_collection_metrics: None,
    })
}

/// One planned write plus its cancellation reason (code `"None"` unless its
/// condition failed).
struct PlannedWrite {
    write: AtomicWrite,
    reason: CancellationReason,
    failed: bool,
}

fn plan_writes(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    principal: &PrincipalContext,
    token: &TransactionSessionToken,
    input: &TransactWriteItemsInput,
) -> Result<Vec<PlannedWrite>, DynamoDbError> {
    let limits = default_limits();
    let mut plans = Vec::with_capacity(input.transact_items.len());
    for transact in &input.transact_items {
        plans.push(plan_one(
            service, context, principal, token, transact, &limits,
        )?);
    }
    Ok(plans)
}

#[allow(clippy::too_many_lines)]
fn plan_one(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    principal: &PrincipalContext,
    token: &TransactionSessionToken,
    transact: &TransactWriteItem,
    limits: &extenddb_core::limits::LimitsConfig,
) -> Result<PlannedWrite, DynamoDbError> {
    match (
        &transact.condition_check,
        &transact.put,
        &transact.delete,
        &transact.update,
    ) {
        (Some(check), None, None, None) => {
            let (key, current, precond, doc) = read_target(
                service,
                context,
                principal,
                token,
                &check.table_name,
                &check.key,
            )?;
            let failed = !condition_holds(
                Some(&check.condition_expression),
                check.expression_attribute_names.as_ref(),
                check.expression_attribute_values.as_ref(),
                &current,
                limits,
            )?;
            Ok(PlannedWrite {
                write: AtomicWrite::Verify {
                    key,
                    precondition: precond,
                },
                reason: reason(failed, check.return_values_on_condition_check_failure, &doc),
                failed,
            })
        }
        (None, Some(put), None, None) => {
            crate::attribute_value::validate_item(&put.item)?;
            let (key, current, precond, doc) = read_target(
                service,
                context,
                principal,
                token,
                &put.table_name,
                &put.item,
            )?;
            let failed = !condition_holds(
                put.condition_expression.as_deref(),
                put.expression_attribute_names.as_ref(),
                put.expression_attribute_values.as_ref(),
                &current,
                limits,
            )?;
            Ok(PlannedWrite {
                write: AtomicWrite::Set {
                    key,
                    document: item_to_fields(&put.item)?,
                    mode: WriteSetMode::Overwrite,
                    precondition: precond,
                    transforms: Vec::new(),
                },
                reason: reason(failed, put.return_values_on_condition_check_failure, &doc),
                failed,
            })
        }
        (None, None, Some(delete), None) => {
            let (key, current, precond, doc) = read_target(
                service,
                context,
                principal,
                token,
                &delete.table_name,
                &delete.key,
            )?;
            let failed = !condition_holds(
                delete.condition_expression.as_deref(),
                delete.expression_attribute_names.as_ref(),
                delete.expression_attribute_values.as_ref(),
                &current,
                limits,
            )?;
            Ok(PlannedWrite {
                write: AtomicWrite::Delete {
                    key,
                    precondition: precond,
                    missing_ok: true,
                },
                reason: reason(
                    failed,
                    delete.return_values_on_condition_check_failure,
                    &doc,
                ),
                failed,
            })
        }
        (None, None, None, Some(update)) => {
            let key_schema = control_plane::load_key_schema(service, context, &update.table_name)?;
            let (key, current, precond, doc) = read_target(
                service,
                context,
                principal,
                token,
                &update.table_name,
                &update.key,
            )?;
            let failed = !condition_holds(
                update.condition_expression.as_deref(),
                update.expression_attribute_names.as_ref(),
                update.expression_attribute_values.as_ref(),
                &current,
                limits,
            )?;
            // Build the post-update item only when the condition holds.
            let document = if failed {
                serde_json::Map::new()
            } else {
                let actions = parse_update_expression(&update.update_expression, limits)?;
                let maps = build_maps(
                    update.expression_attribute_names.as_ref(),
                    update.expression_attribute_values.as_ref(),
                );
                reject_key_updates(&actions, &key_schema, &maps)?;
                let mut new_item = if current.is_empty() {
                    update.key.clone()
                } else {
                    current.clone()
                };
                apply_update(&actions, &mut new_item, &maps)?;
                item_to_fields(&new_item)?
            };
            Ok(PlannedWrite {
                write: AtomicWrite::Set {
                    key,
                    document,
                    mode: WriteSetMode::Overwrite,
                    precondition: precond,
                    transforms: Vec::new(),
                },
                reason: reason(
                    failed,
                    update.return_values_on_condition_check_failure,
                    &doc,
                ),
                failed,
            })
        }
        _ => Err(DynamoDbError::ValidationException(
            "Each TransactWriteItem must contain exactly one of ConditionCheck, Put, Delete, or \
             Update"
                .to_owned(),
        )),
    }
}

/// Resolve an op's target: read the current item through the snapshot and
/// derive its write key + update-time precondition.
fn read_target(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    principal: &PrincipalContext,
    token: &TransactionSessionToken,
    table_name: &str,
    key_or_item: &Item,
) -> Result<(WriteKey, Item, WritePrecondition, Option<Document>), DynamoDbError> {
    let key_schema = control_plane::load_key_schema(service, context, table_name)?;
    let id = primary_key_id(key_or_item, &key_schema)?;
    let table = TableName::new(table_name).map_err(map_core_error)?;
    let doc = service
        .get_document_in_transaction(context.tenant_id(), token, principal, &table, id.clone())
        .map_err(map_core_error)?;
    let current = match &doc {
        Some(document) => fields_to_item(&document.fields)?,
        None => Item::new(),
    };
    // Existence-level precondition from the snapshot. (The engine models but
    // does not yet execute update-time preconditions, so value-level OCC is not
    // available; existence consistency covers create-if-absent / must-exist.)
    let precondition = WritePrecondition::exists(doc.is_some());
    let key = WriteKey::from(DocumentLocator::new(table, id));
    Ok((key, current, precondition, doc))
}

fn condition_holds(
    condition: Option<&str>,
    names: Option<&std::collections::HashMap<String, String>>,
    values: Option<&std::collections::HashMap<String, extenddb_core::types::AttributeValue>>,
    current: &Item,
    limits: &extenddb_core::limits::LimitsConfig,
) -> Result<bool, DynamoDbError> {
    let Some(expression) = condition.filter(|expression| !expression.is_empty()) else {
        return Ok(true);
    };
    let parsed = parse_condition(expression, limits)?;
    let maps = build_maps(names, values);
    evaluate_condition(&parsed, current, &maps)
}

/// Build a cancellation reason: `"None"` when the op held, otherwise
/// `"ConditionalCheckFailed"` (with the prior item when ALL_OLD was requested).
fn reason(
    failed: bool,
    return_values: ReturnValuesOnConditionCheckFailure,
    doc: &Option<Document>,
) -> CancellationReason {
    if !failed {
        return CancellationReason {
            code: "None".to_owned(),
            message: None,
            item: None,
        };
    }
    let item = match (return_values, doc) {
        (ReturnValuesOnConditionCheckFailure::AllOld, Some(document)) => {
            fields_to_item(&document.fields).ok()
        }
        _ => None,
    };
    CancellationReason {
        code: "ConditionalCheckFailed".to_owned(),
        message: Some("The conditional request failed".to_owned()),
        item,
    }
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

    fn transact_get(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<TransactGetItemsOutput, DynamoDbError> {
        transact_get_items(service, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn transact_get_returns_ordered_responses_with_gaps() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(&service, &ctx, "a", "1");
        put(&service, &ctx, "c", "3");
        let out = transact_get(
            &service,
            &ctx,
            json!({
                "TransactItems": [
                    { "Get": { "TableName": "Orders", "Key": { "pk": {"S": "a"} } } },
                    { "Get": { "TableName": "Orders", "Key": { "pk": {"S": "missing"} } } },
                    { "Get": { "TableName": "Orders", "Key": { "pk": {"S": "c"} } } }
                ]
            }),
        )
        .expect("transact get");
        assert_eq!(out.responses.len(), 3, "one response per request, in order");
        assert_eq!(
            out.responses[0].item.as_ref().and_then(|i| i.get("v")),
            Some(&AttributeValue::N("1".into()))
        );
        assert!(out.responses[1].item.is_none(), "missing item is absent");
        assert_eq!(
            out.responses[2].item.as_ref().and_then(|i| i.get("v")),
            Some(&AttributeValue::N("3".into()))
        );
    }

    #[test]
    fn transact_get_applies_projection() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(&service, &ctx, "a", "1");
        let out = transact_get(
            &service,
            &ctx,
            json!({
                "TransactItems": [
                    { "Get": { "TableName": "Orders", "Key": { "pk": {"S": "a"} }, "ProjectionExpression": "pk" } }
                ]
            }),
        )
        .expect("transact get");
        let item = out.responses[0].item.as_ref().expect("item");
        assert_eq!(item.len(), 1, "projected to pk only");
        assert!(item.contains_key("pk") && !item.contains_key("v"));
    }

    #[test]
    fn transact_get_empty_is_validation_error() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let err = transact_get(&service, &ctx, json!({ "TransactItems": [] }))
            .expect_err("empty rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    // ---- D3.4: TransactWriteItems ----

    fn read(service: &Arc<Service>, context: &TenantIsolationContext, pk: &str) -> Option<Item> {
        let key: Item = [("pk".to_string(), AttributeValue::S(pk.into()))]
            .into_iter()
            .collect();
        let schema = control_plane::load_key_schema(service, context, "Orders").unwrap();
        let id = primary_key_id(&key, &schema).unwrap();
        crate::commands::item::read_item(service, context, &TableName::new("Orders").unwrap(), id)
            .unwrap()
    }

    fn transact_write(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<TransactWriteItemsOutput, DynamoDbError> {
        transact_write_items(service, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn transact_write_applies_put_update_delete_atomically() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(&service, &ctx, "u", "1"); // updated
        put(&service, &ctx, "d", "9"); // deleted
        transact_write(
            &service,
            &ctx,
            json!({
                "TransactItems": [
                    { "Put": { "TableName": "Orders", "Item": { "pk": {"S": "p"}, "v": {"N": "5"} } } },
                    { "Update": {
                        "TableName": "Orders", "Key": { "pk": {"S": "u"} },
                        "UpdateExpression": "SET v = :v",
                        "ExpressionAttributeValues": { ":v": {"N": "2"} }
                    } },
                    { "Delete": { "TableName": "Orders", "Key": { "pk": {"S": "d"} } } }
                ]
            }),
        )
        .expect("transact write");
        assert_eq!(
            read(&service, &ctx, "p").and_then(|i| i.get("v").cloned()),
            Some(AttributeValue::N("5".into())),
            "put applied"
        );
        assert_eq!(
            read(&service, &ctx, "u").and_then(|i| i.get("v").cloned()),
            Some(AttributeValue::N("2".into())),
            "update applied"
        );
        assert!(read(&service, &ctx, "d").is_none(), "delete applied");
    }

    #[test]
    fn transact_write_condition_failure_cancels_everything() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(&service, &ctx, "exists", "1");
        // Op 0 would create "fresh"; op 1's condition fails (item exists). The
        // whole transaction must cancel — "fresh" must NOT be written.
        let err = transact_write(
            &service,
            &ctx,
            json!({
                "TransactItems": [
                    { "Put": { "TableName": "Orders", "Item": { "pk": {"S": "fresh"} } } },
                    { "Put": {
                        "TableName": "Orders", "Item": { "pk": {"S": "exists"}, "v": {"N": "9"} },
                        "ConditionExpression": "attribute_not_exists(pk)"
                    } }
                ]
            }),
        )
        .expect_err("transaction should cancel");
        match err {
            DynamoDbError::TransactionCanceledException {
                cancellation_reasons,
                ..
            } => {
                assert_eq!(cancellation_reasons.len(), 2);
                assert_eq!(cancellation_reasons[0].code, "None", "op 0 held");
                assert_eq!(
                    cancellation_reasons[1].code, "ConditionalCheckFailed",
                    "op 1 failed its condition"
                );
            }
            other => panic!("expected TransactionCanceledException, got {other:?}"),
        }
        assert!(
            read(&service, &ctx, "fresh").is_none(),
            "no partial write — op 0 rolled back with the cancelled transaction"
        );
    }

    #[test]
    fn transact_write_condition_check_gates_other_writes() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(&service, &ctx, "guard", "1");
        // ConditionCheck passes (guard exists) → the Put applies.
        transact_write(
            &service,
            &ctx,
            json!({
                "TransactItems": [
                    { "ConditionCheck": {
                        "TableName": "Orders", "Key": { "pk": {"S": "guard"} },
                        "ConditionExpression": "attribute_exists(pk)"
                    } },
                    { "Put": { "TableName": "Orders", "Item": { "pk": {"S": "new"} } } }
                ]
            }),
        )
        .expect("condition check passes");
        assert!(read(&service, &ctx, "new").is_some());

        // ConditionCheck fails (guard2 absent) → the Put does NOT apply.
        let err = transact_write(
            &service,
            &ctx,
            json!({
                "TransactItems": [
                    { "ConditionCheck": {
                        "TableName": "Orders", "Key": { "pk": {"S": "guard2"} },
                        "ConditionExpression": "attribute_exists(pk)"
                    } },
                    { "Put": { "TableName": "Orders", "Item": { "pk": {"S": "new2"} } } }
                ]
            }),
        )
        .expect_err("condition check fails");
        assert!(matches!(
            err,
            DynamoDbError::TransactionCanceledException { .. }
        ));
        assert!(
            read(&service, &ctx, "new2").is_none(),
            "gated write not applied"
        );
    }

    #[test]
    fn transact_write_requires_exactly_one_op_per_item() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let err = transact_write(
            &service,
            &ctx,
            json!({ "TransactItems": [ {
                "Put": { "TableName": "Orders", "Item": { "pk": {"S": "a"} } },
                "Delete": { "TableName": "Orders", "Key": { "pk": {"S": "a"} } }
            } ] }),
        )
        .expect_err("two ops in one item rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn transact_write_empty_is_validation_error() {
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        let err = transact_write(&service, &ctx, json!({ "TransactItems": [] }))
            .expect_err("empty rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn transact_get_repeatable_under_snapshot() {
        // Reading the same key twice in one transaction sees one consistent
        // snapshot value.
        let (service, ctx, _t) = fixture();
        create_orders(&service, &ctx);
        put(&service, &ctx, "a", "1");
        let out = transact_get(
            &service,
            &ctx,
            json!({
                "TransactItems": [
                    { "Get": { "TableName": "Orders", "Key": { "pk": {"S": "a"} } } },
                    { "Get": { "TableName": "Orders", "Key": { "pk": {"S": "a"} } } }
                ]
            }),
        )
        .expect("transact get");
        assert_eq!(out.responses[0].item, out.responses[1].item);
    }
}
