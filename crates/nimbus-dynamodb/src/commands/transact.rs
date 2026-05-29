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
use extenddb_core::types::{ItemResponse, TransactGetItemsInput, TransactGetItemsOutput};
use nimbus_core::{PrincipalContext, TableName, TransactionSessionMode};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;

use crate::attribute_value::fields_to_item;
use crate::commands::control_plane;
use crate::commands::item::primary_key_id;
use crate::error::map_core_error;
use crate::expression::{default_limits, project_item};

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
