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
use nimbus_core::{
    AtomicWrite, AtomicWriteBatch, DocumentId, DocumentLocator, PrincipalContext, TableName,
    TransactionSessionMode, TransactionSessionToken, WriteKey, WritePrecondition, WriteSetMode,
};
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;

use crate::attribute_value::{fields_to_item, item_to_fields, validate_item};
use crate::commands::{control_plane, stream};
use crate::error::map_core_error;
use crate::expression::{
    apply_update, build_maps, check_condition, default_limits, parse_update_expression,
    project_item, reject_key_updates, updated_attributes,
};
use crate::key::encode_key;
use crate::tenant::caller_principal;

/// Bound retries for a single-item optimistic transaction. A fresh transaction
/// re-reads the item and re-evaluates its condition/update after every conflict.
const MAX_SINGLE_ITEM_TRANSACTION_ATTEMPTS: usize = 32;

pub(crate) struct SingleItemTransactionPlan<T> {
    pub(crate) output: T,
    pub(crate) writes: Vec<AtomicWrite>,
    pub(crate) changes: Vec<stream::StreamChange>,
}

/// Execute one DynamoDB single-item operation in an engine transaction. The
/// item snapshot, condition evaluation, update expression, returned image,
/// data write, and stream effects share one conflict boundary.
///
/// `batch.rs` drives each BatchWriteItem op through this too: a batch op is
/// still independent of its siblings (one transaction each, no cross-item
/// atomicity), but its prior image has to be read inside the transaction that
/// replaces it or the emitted stream record describes a state that was never
/// overwritten.
pub(crate) fn execute_single_item_transaction<T, F>(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    mut plan: F,
) -> Result<T, DynamoDbError>
where
    F: FnMut(
        &TransactionSessionToken,
        &PrincipalContext,
    ) -> Result<SingleItemTransactionPlan<T>, DynamoDbError>,
{
    let principal = caller_principal(context);
    for attempt in 1..=MAX_SINGLE_ITEM_TRANSACTION_ATTEMPTS {
        let session = engine
            .begin_transaction_session(
                context.tenant_id().clone(),
                principal.clone(),
                TransactionSessionMode::ReadWrite,
            )
            .map_err(map_core_error)?;
        let token = session.token;

        let SingleItemTransactionPlan {
            output,
            mut writes,
            changes,
        } = match plan(&token, &principal) {
            Ok(plan) => plan,
            Err(error) => {
                let _ =
                    engine.rollback_transaction_session(context.tenant_id(), &token, &principal);
                return Err(error);
            }
        };

        if let Err(error) = stream::append_stream_writes(engine, context, &mut writes, &changes) {
            let _ = engine.rollback_transaction_session(context.tenant_id(), &token, &principal);
            return Err(error);
        }
        let batch = if writes.is_empty() {
            None
        } else {
            match AtomicWriteBatch::new(writes) {
                Ok(batch) => Some(batch),
                Err(error) => {
                    let _ = engine.rollback_transaction_session(
                        context.tenant_id(),
                        &token,
                        &principal,
                    );
                    return Err(map_core_error(error));
                }
            }
        };

        match engine.commit_transaction_session(context.tenant_id(), &token, &principal, batch) {
            Ok(_) => return Ok(output),
            Err(error) if single_item_transaction_should_retry(&error) => {
                if attempt == MAX_SINGLE_ITEM_TRANSACTION_ATTEMPTS {
                    return Err(DynamoDbError::TransactionConflictException(format!(
                        "single-item transaction exhausted {MAX_SINGLE_ITEM_TRANSACTION_ATTEMPTS} attempts: {error}"
                    )));
                }
                std::thread::yield_now();
            }
            Err(error) => return Err(map_core_error(error)),
        }
    }

    unreachable!("positive transaction-attempt bound must return from the retry loop")
}

fn single_item_transaction_should_retry(error: &nimbus_core::Error) -> bool {
    matches!(
        error,
        nimbus_core::Error::Conflict {
            retryable: true,
            ..
        } | nimbus_core::Error::OutOfRetention { .. }
            | nimbus_core::Error::AlreadyExists(_)
    )
}

/// Atomically create-or-replace `fields` under `id` in a **single** storage
/// transaction (`WriteSetMode::Overwrite`), replacing the former non-atomic
/// `delete` + `insert` whose crash window could leave the row deleted with the
/// replacement never written (F2). `precondition` re-validates the document's
/// live existence state at commit, closing the check-then-write TOCTOU on
/// conditional writes (F9). Returns the raw core error so the caller can map a
/// lost existence-precondition race to `ConditionalCheckFailedException`.
///
/// `principal` is explicit because this helper serves both user tables (where
/// the write must be authorized as the calling access key) and the adapter's own
/// reserved stores (where it must not be).
pub(crate) fn atomic_overwrite(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table: TableName,
    id: DocumentId,
    fields: serde_json::Map<String, serde_json::Value>,
    precondition: WritePrecondition,
    principal: PrincipalContext,
) -> Result<(), nimbus_core::Error> {
    let batch = AtomicWriteBatch::new(vec![overwrite_atomic_write(
        table,
        id,
        fields,
        precondition,
    )])?;
    engine
        .begin_mutation_execution_unit(context.tenant_id().clone(), principal)?
        .execute_atomic_write_batch(batch)?;
    Ok(())
}

pub(crate) fn overwrite_atomic_write(
    table: TableName,
    id: DocumentId,
    fields: serde_json::Map<String, serde_json::Value>,
    precondition: WritePrecondition,
) -> AtomicWrite {
    AtomicWrite::Set {
        key: WriteKey::from(DocumentLocator::new(table, id)),
        document: fields,
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition,
        transforms: Vec::new(),
    }
}

pub(crate) fn delete_atomic_write(
    table: TableName,
    id: DocumentId,
    precondition: WritePrecondition,
) -> AtomicWrite {
    AtomicWrite::Delete {
        key: WriteKey::from(DocumentLocator::new(table, id)),
        precondition,
        missing_ok: true,
    }
}

/// Map a single-item **conditional** write failure. A lost existence
/// precondition — a concurrent writer flipped the document's existence between
/// the condition check and the commit — surfaces from the Overwrite/Delete
/// precondition as `AlreadyExists`/`DocumentNotFound`; for a conditional write
/// that is a `ConditionalCheckFailedException`. All other errors map normally.
#[cfg(test)]
fn map_conditional_write_error(error: nimbus_core::Error) -> DynamoDbError {
    match error {
        nimbus_core::Error::AlreadyExists(_) | nimbus_core::Error::DocumentNotFound(_) => {
            DynamoDbError::ConditionalCheckFailedException(
                crate::expression::CONDITION_FAILED_MESSAGE.to_owned(),
                None,
            )
        }
        other => map_core_error(other),
    }
}

/// PutItem: validate the item, gate on any `ConditionExpression`, replace-or-
/// insert it, and honor `ReturnValues` (NONE / ALL_OLD).
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException`
/// for an invalid item or missing key; `ConditionalCheckFailedException` if the
/// condition fails; a mapped engine error otherwise.
pub fn put_item(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: PutItemInput,
) -> Result<PutItemOutput, DynamoDbError> {
    validate_item(&input.item)?;
    let key_schema = control_plane::load_key_schema(engine, context, &input.table_name)?;
    let id = primary_key_id(&input.item, &key_schema)?;
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;

    let limits = default_limits();
    let fields = item_to_fields(&input.item)?;
    let keys = extract_key(&input.item, &key_schema);
    execute_single_item_transaction(engine, context, |token, principal| {
        let existing =
            read_item_in_transaction(engine, context, token, principal, &table, id.clone())?;
        check_condition(
            input.condition_expression.as_deref(),
            input.expression_attribute_names.as_ref(),
            input.expression_attribute_values.as_ref(),
            &existing.clone().unwrap_or_default(),
            &limits,
        )?;

        let event_name = if existing.is_some() {
            StreamEventName::Modify
        } else {
            StreamEventName::Insert
        };
        let change = stream::StreamChange::new(
            input.table_name.clone(),
            event_name,
            keys.clone(),
            existing.clone(),
            Some(input.item.clone()),
            None,
        );
        let attributes = match input.return_values {
            ReturnValues::AllOld => existing,
            _ => None,
        };
        Ok(SingleItemTransactionPlan {
            output: PutItemOutput {
                attributes,
                consumed_capacity: None,
                item_collection_metrics: None,
            },
            writes: vec![overwrite_atomic_write(
                table.clone(),
                id.clone(),
                fields.clone(),
                WritePrecondition::exists(change.old_image.is_some()),
            )],
            changes: vec![change],
        })
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: GetItemInput,
) -> Result<GetItemOutput, DynamoDbError> {
    let key_schema = control_plane::load_key_schema(engine, context, &input.table_name)?;
    let id = primary_key_id(&input.key, &key_schema)?;
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;

    let item = match read_item(engine, context, &table, id)? {
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: DeleteItemInput,
) -> Result<DeleteItemOutput, DynamoDbError> {
    let key_schema = control_plane::load_key_schema(engine, context, &input.table_name)?;
    let id = primary_key_id(&input.key, &key_schema)?;
    let table = TableName::new(&input.table_name).map_err(map_core_error)?;

    let limits = default_limits();
    let keys = extract_key(&input.key, &key_schema);
    execute_single_item_transaction(engine, context, |token, principal| {
        let existing =
            read_item_in_transaction(engine, context, token, principal, &table, id.clone())?;
        check_condition(
            input.condition_expression.as_deref(),
            input.expression_attribute_names.as_ref(),
            input.expression_attribute_values.as_ref(),
            &existing.clone().unwrap_or_default(),
            &limits,
        )?;

        let (writes, changes) = match &existing {
            Some(_) => (
                vec![delete_atomic_write(
                    table.clone(),
                    id.clone(),
                    WritePrecondition::exists(true),
                )],
                vec![stream::StreamChange::new(
                    input.table_name.clone(),
                    StreamEventName::Remove,
                    keys.clone(),
                    existing.clone(),
                    None,
                    None,
                )],
            ),
            None => (Vec::new(), Vec::new()),
        };
        let attributes = match input.return_values {
            ReturnValues::AllOld => existing,
            _ => None,
        };
        Ok(SingleItemTransactionPlan {
            output: DeleteItemOutput {
                attributes,
                consumed_capacity: None,
                item_collection_metrics: None,
            },
            writes,
            changes,
        })
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: UpdateItemInput,
) -> Result<UpdateItemOutput, DynamoDbError> {
    let key_schema = control_plane::load_key_schema(engine, context, &input.table_name)?;
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

    execute_single_item_transaction(engine, context, |token, principal| {
        let old_item =
            read_item_in_transaction(engine, context, token, principal, &table, id.clone())?;
        check_condition(
            input.condition_expression.as_deref(),
            input.expression_attribute_names.as_ref(),
            input.expression_attribute_values.as_ref(),
            &old_item.clone().unwrap_or_default(),
            &limits,
        )?;

        let mut new_item = old_item.clone().unwrap_or_else(|| input.key.clone());
        apply_update(&actions, &mut new_item, &maps)?;
        let fields = item_to_fields(&new_item)?;
        let event_name = if old_item.is_some() {
            StreamEventName::Modify
        } else {
            StreamEventName::Insert
        };
        let change = stream::StreamChange::new(
            input.table_name.clone(),
            event_name,
            extract_key(&new_item, &key_schema),
            old_item.clone(),
            Some(new_item.clone()),
            None,
        );
        let attributes = match input.return_values {
            ReturnValues::None => None,
            ReturnValues::AllOld => old_item,
            ReturnValues::AllNew => Some(new_item.clone()),
            ReturnValues::UpdatedOld => old_item
                .map(|item| updated_attributes(&item, &actions, &maps))
                .filter(|item| !item.is_empty()),
            ReturnValues::UpdatedNew => {
                Some(updated_attributes(&new_item, &actions, &maps)).filter(|item| !item.is_empty())
            }
        };
        Ok(SingleItemTransactionPlan {
            output: UpdateItemOutput {
                attributes,
                consumed_capacity: None,
                item_collection_metrics: None,
            },
            writes: vec![overwrite_atomic_write(
                table.clone(),
                id.clone(),
                fields,
                WritePrecondition::exists(change.old_image.is_some()),
            )],
            changes: vec![change],
        })
    })
}

pub(crate) fn read_item_in_transaction(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    token: &TransactionSessionToken,
    principal: &PrincipalContext,
    table: &TableName,
    id: DocumentId,
) -> Result<Option<Item>, DynamoDbError> {
    engine
        .get_document_in_transaction(context.tenant_id(), token, principal, table, id)
        .map_err(map_core_error)?
        .map(|document| fields_to_item(&document.fields))
        .transpose()
}

/// Read a stored item by id, mapping a missing document to `None`.
pub(crate) fn read_item(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table: &TableName,
    id: DocumentId,
) -> Result<Option<Item>, DynamoDbError> {
    match engine.get_document_with_principal(
        context.tenant_id(),
        table,
        id,
        &caller_principal(context),
    ) {
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

    /// Create table "Orders" with a single `pk` (String) partition key.
    fn create_orders(engine: &Arc<Engine>, context: &TenantIsolationContext) {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "Orders",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap();
        control_plane::create_table(engine, context, input).expect("create table");
    }

    fn put(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<PutItemOutput, DynamoDbError> {
        put_item(engine, context, serde_json::from_value(input).unwrap())
    }

    /// Read the stored item for a given `pk` value.
    fn stored(engine: &Arc<Engine>, context: &TenantIsolationContext, pk: &str) -> Option<Item> {
        let key: Item = [("pk".to_string(), AttributeValue::S(pk.into()))]
            .into_iter()
            .collect();
        let schema = control_plane::load_key_schema(engine, context, "Orders").unwrap();
        let id = primary_key_id(&key, &schema).unwrap();
        read_item(engine, context, &TableName::new("Orders").unwrap(), id).unwrap()
    }

    #[test]
    fn put_then_read_stores_the_item_losslessly() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "o1"}, "qty": {"N": "42"}, "tags": {"SS": ["a", "b"]} },
            }),
        )
        .expect("put");
        let item = stored(&engine, &ctx, "o1").expect("item present");
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "x": {"N": "1"} } }),
        )
        .expect("first put");
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "y": {"N": "2"} } }),
        )
        .expect("second put");
        let item = stored(&engine, &ctx, "o1").expect("item present");
        assert!(!item.contains_key("x"), "PutItem replaces, so x is gone");
        assert_eq!(item.get("y"), Some(&AttributeValue::N("2".into())));
    }

    #[test]
    fn put_all_old_returns_the_previous_item() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        // No previous item → ALL_OLD returns nothing.
        let first = put(
            &engine,
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
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("first put");
        // attribute_not_exists(pk) must fail now that the item exists.
        let err = put(
            &engine,
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
        let item = stored(&engine, &ctx, "o1").expect("item present");
        assert!(!item.contains_key("v"));
    }

    #[test]
    fn put_create_if_absent_succeeds_when_absent() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "fresh"}, "v": {"N": "1"} },
                "ConditionExpression": "attribute_not_exists(pk)",
            }),
        )
        .expect("create-if-absent should succeed for a new key");
        assert!(stored(&engine, &ctx, "fresh").is_some());
    }

    /// Build the `Orders` document id for a `pk` value (mirrors `stored`).
    fn orders_id(engine: &Arc<Engine>, context: &TenantIsolationContext, pk: &str) -> DocumentId {
        let key: Item = [("pk".to_string(), AttributeValue::S(pk.into()))]
            .into_iter()
            .collect();
        let schema = control_plane::load_key_schema(engine, context, "Orders").unwrap();
        primary_key_id(&key, &schema).unwrap()
    }

    fn pk_fields(pk: &str) -> serde_json::Map<String, serde_json::Value> {
        let item: Item = [("pk".to_string(), AttributeValue::S(pk.into()))]
            .into_iter()
            .collect();
        item_to_fields(&item).unwrap()
    }

    /// F9: the existence precondition makes the atomic overwrite reject a write
    /// whose snapshot existence assumption no longer holds — the engine-level
    /// closure of the check-then-write TOCTOU. A stale create (snapshot said
    /// absent, item now exists) and a stale must-exist (snapshot said present,
    /// item now gone) both fail, and the conditional mapper surfaces them as
    /// `ConditionalCheckFailedException` rather than leaking ResourceInUse /
    /// ResourceNotFound.
    #[test]
    fn atomic_overwrite_enforces_existence_precondition() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "live"} } }),
        )
        .expect("seed");

        let table = TableName::new("Orders").unwrap();
        // Snapshot claimed absence, but "live" exists → exists(false) rejected.
        let stale_create = atomic_overwrite(
            &engine,
            &ctx,
            table.clone(),
            orders_id(&engine, &ctx, "live"),
            pk_fields("live"),
            WritePrecondition::exists(false),
            caller_principal(&ctx),
        )
        .map_err(map_conditional_write_error)
        .expect_err("stale create must be rejected");
        assert!(matches!(
            stale_create,
            DynamoDbError::ConditionalCheckFailedException(_, _)
        ));

        // Snapshot claimed presence, but "ghost" is absent → exists(true) rejected.
        let stale_update = atomic_overwrite(
            &engine,
            &ctx,
            table,
            orders_id(&engine, &ctx, "ghost"),
            pk_fields("ghost"),
            WritePrecondition::exists(true),
            caller_principal(&ctx),
        )
        .map_err(map_conditional_write_error)
        .expect_err("stale must-exist update must be rejected");
        assert!(matches!(
            stale_update,
            DynamoDbError::ConditionalCheckFailedException(_, _)
        ));

        // Neither rejected write mutated the store.
        assert!(stored(&engine, &ctx, "live").is_some());
        assert!(stored(&engine, &ctx, "ghost").is_none());
    }

    #[test]
    fn put_missing_partition_key_is_validation_error() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let err = put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "other": {"S": "x"} } }),
        )
        .expect_err("missing pk should fail");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn put_to_missing_table_is_resource_not_found() {
        let (engine, ctx, _t) = fixture();
        let err = put(
            &engine,
            &ctx,
            json!({ "TableName": "Ghost", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect_err("missing table should fail");
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }

    // ---- D1.6: GetItem ----

    fn get(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> GetItemOutput {
        get_item(engine, context, serde_json::from_value(input).unwrap()).expect("get")
    }

    #[test]
    fn get_returns_the_stored_item() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "qty": {"N": "5"} } }),
        )
        .expect("put");
        let out = get(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "o1"} } }),
        );
        let item = out.item.expect("item present");
        assert_eq!(item.get("pk"), Some(&AttributeValue::S("o1".into())));
        assert_eq!(item.get("qty"), Some(&AttributeValue::N("5".into())));
    }

    #[test]
    fn get_missing_item_returns_none() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let out = get(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "absent"} } }),
        );
        assert!(out.item.is_none(), "missing item yields no Item field");
    }

    #[test]
    fn get_with_projection_selects_subset() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "o1"}, "a": {"N": "1"}, "b": {"N": "2"}, "c": {"N": "3"} },
            }),
        )
        .expect("put");
        let out = get(
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("put");
        // ConsistentRead=true must not change the result (single store is
        // already strongly consistent).
        let out = get(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "o1"} }, "ConsistentRead": true }),
        );
        assert!(out.item.is_some());
    }

    // ---- D1.7: DeleteItem ----

    fn delete(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<DeleteItemOutput, DynamoDbError> {
        delete_item(engine, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn delete_removes_the_item() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("put");
        delete(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "o1"} } }),
        )
        .expect("delete");
        assert!(stored(&engine, &ctx, "o1").is_none(), "item is gone");
    }

    #[test]
    fn delete_all_old_returns_the_deleted_item() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "v": {"N": "7"} } }),
        )
        .expect("put");
        let out = delete(
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let out = delete(
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("put");
        // attribute_not_exists(pk) must fail since the item exists.
        let err = delete(
            &engine,
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
        assert!(stored(&engine, &ctx, "o1").is_some(), "item not deleted");
    }

    // ---- D1.8: UpdateItem ----

    fn update(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        input: serde_json::Value,
    ) -> Result<UpdateItemOutput, DynamoDbError> {
        update_item(engine, context, serde_json::from_value(input).unwrap())
    }

    #[test]
    fn update_set_modifies_and_all_new_returns_full_item() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "v": {"N": "1"} } }),
        )
        .expect("put");
        let out = update(
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        update(
            &engine,
            &ctx,
            json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "new"} },
                "UpdateExpression": "SET v = :v",
                "ExpressionAttributeValues": { ":v": {"N": "5"} },
            }),
        )
        .expect("upsert update");
        let item = stored(&engine, &ctx, "new").expect("item created");
        assert_eq!(item.get("pk"), Some(&AttributeValue::S("new".into())));
        assert_eq!(item.get("v"), Some(&AttributeValue::N("5".into())));
    }

    #[test]
    fn update_no_op_upsert_creates_key_only_item() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        // No UpdateExpression on an absent key creates a key-only item.
        update(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Key": { "pk": {"S": "bare"} } }),
        )
        .expect("no-op upsert");
        let item = stored(&engine, &ctx, "bare").expect("key-only item created");
        assert_eq!(item.len(), 1);
        assert_eq!(item.get("pk"), Some(&AttributeValue::S("bare".into())));
    }

    #[test]
    fn update_updated_new_returns_only_changed_attributes() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "a": {"N": "1"}, "b": {"N": "2"} } }),
        )
        .expect("put");
        let out = update(
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"} } }),
        )
        .expect("put");
        // SET a brand-new attribute: UPDATED_OLD has no prior value → Attributes omitted.
        let out = update(
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "o1"}, "m": {"M": { "a": {"N": "1"}, "b": {"N": "2"} }} },
            }),
        )
        .expect("put");
        let out = update(
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let err = update(
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        let err = update(
            &engine,
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
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "o1"}, "v": {"N": "1"} } }),
        )
        .expect("put");
        let err = update(
            &engine,
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
            stored(&engine, &ctx, "o1").unwrap().get("v"),
            Some(&AttributeValue::N("1".into()))
        );
    }

    #[test]
    fn concurrent_add_updates_retry_from_a_fresh_snapshot_without_lost_writes() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({
                "TableName": "Orders",
                "Item": { "pk": {"S": "counter"}, "value": {"N": "0"} },
            }),
        )
        .expect("seed counter");

        let input = || -> UpdateItemInput {
            serde_json::from_value(json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "counter"} },
                "UpdateExpression": "ADD #value :one",
                "ExpressionAttributeNames": { "#value": "value" },
                "ExpressionAttributeValues": { ":one": {"N": "1"} },
            }))
            .unwrap()
        };

        let faults = engine.commit_fault_handle_for_testing();
        faults.arm(commit_fault_labels::PREPARE_COMPLETE);
        let first = std::thread::spawn({
            let engine = engine.clone();
            let ctx = ctx.clone();
            let input = input();
            move || update_item(&engine, &ctx, input)
        });

        let entered = faults.wait_until_entered(
            commit_fault_labels::PREPARE_COMPLETE,
            Duration::from_secs(5),
        );
        if entered {
            update_item(&engine, &ctx, input()).expect("concurrent update should commit");
        }
        faults.release(commit_fault_labels::PREPARE_COMPLETE);
        assert!(
            entered,
            "first update should reach the deterministic commit pause"
        );
        first
            .join()
            .expect("first update thread should join")
            .expect("first update should retry and commit");

        assert!(
            faults.hit_count(commit_fault_labels::PREPARE_COMPLETE) >= 3,
            "first attempt, concurrent commit, and retried attempt should all reach commit"
        );
        assert_eq!(
            stored(&engine, &ctx, "counter").unwrap().get("value"),
            Some(&AttributeValue::N("2".into())),
            "both ADD operations must be preserved"
        );
    }

    /// Two UpdateItem calls that SET **different** attributes of one item must
    /// both survive. `ADD` merges numerically, so a lost write there shows up
    /// only as a wrong total; a distinct-attribute `SET` is the sharper probe —
    /// a read-modify-write that writes its own snapshot back wholesale drops
    /// the attribute the interleaved writer added, leaving no trace at all.
    #[test]
    fn concurrent_set_updates_on_distinct_attributes_both_survive() {
        let (engine, ctx, _t) = fixture();
        create_orders(&engine, &ctx);
        put(
            &engine,
            &ctx,
            json!({ "TableName": "Orders", "Item": { "pk": {"S": "item"} } }),
        )
        .expect("seed item");

        let set = |attribute: &str| -> UpdateItemInput {
            serde_json::from_value(json!({
                "TableName": "Orders",
                "Key": { "pk": {"S": "item"} },
                "UpdateExpression": format!("SET {attribute} = :v"),
                "ExpressionAttributeValues": { ":v": {"S": attribute} },
            }))
            .unwrap()
        };

        let faults = engine.commit_fault_handle_for_testing();
        faults.arm(commit_fault_labels::PREPARE_COMPLETE);
        let first = std::thread::spawn({
            let engine = engine.clone();
            let ctx = ctx.clone();
            let input = set("alpha");
            move || update_item(&engine, &ctx, input)
        });

        let entered = faults.wait_until_entered(
            commit_fault_labels::PREPARE_COMPLETE,
            Duration::from_secs(5),
        );
        if entered {
            update_item(&engine, &ctx, set("beta")).expect("concurrent update should commit");
        }
        faults.release(commit_fault_labels::PREPARE_COMPLETE);
        assert!(
            entered,
            "first update should reach the deterministic commit pause"
        );
        first
            .join()
            .expect("first update thread should join")
            .expect("first update should retry and commit");

        assert!(
            faults.hit_count(commit_fault_labels::PREPARE_COMPLETE) >= 3,
            "first attempt, concurrent commit, and retried attempt should all reach commit"
        );
        let item = stored(&engine, &ctx, "item").expect("item present");
        assert_eq!(
            item.get("alpha"),
            Some(&AttributeValue::S("alpha".into())),
            "the retried update's own attribute must be present"
        );
        assert_eq!(
            item.get("beta"),
            Some(&AttributeValue::S("beta".into())),
            "the interleaved concurrent update must not be lost"
        );
    }
}
