//! DynamoDB resource tagging (T6): TagResource / UntagResource /
//! ListTagsOfResource (D6.3).
//!
//! Tags are adapter-local metadata — they do not affect data or control-plane
//! behavior. Each table's tag set is persisted as one doc in a reserved
//! `_ddb_tags` catalog, keyed by table name (parsed from the resource ARN).

use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    ListTagsOfResourceInput, ListTagsOfResourceOutput, Tag, TagResourceInput, UntagResourceInput,
};
use nimbus_core::{DocumentId, TableName};
use nimbus_engine::Engine;
use nimbus_tenant::TenantIsolationContext;
use serde_json::{Map, Value};

use crate::commands::control_plane;
use crate::error::map_core_error;

/// Reserved table holding one tag-set doc per table (doc id = table name).
const TAGS_TABLE: &str = "_ddb_tags";
/// DynamoDB resource tag limits.
const MAX_TAGS_PER_RESOURCE: usize = 50;
const MAX_TAG_KEY_LEN: usize = 128;
const MAX_TAG_VALUE_LEN: usize = 256;

fn tags_table() -> Result<TableName, DynamoDbError> {
    TableName::new(TAGS_TABLE).map_err(map_core_error)
}

fn tags_id(table_name: &str) -> Result<DocumentId, DynamoDbError> {
    DocumentId::from_key(table_name).map_err(map_core_error)
}

/// Drop `table_name`'s tag entries when the table is deleted, so a table
/// recreated under the same name does not inherit stale tags (F4).
pub(crate) fn reclaim_for_table(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<(), DynamoDbError> {
    match engine.delete_document(context.tenant_id(), tags_table()?, tags_id(table_name)?) {
        Ok(()) | Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            Ok(())
        }
        Err(error) => Err(map_core_error(error)),
    }
}

/// Extract the table name from a `…:table/<name>` resource ARN.
fn table_name_from_arn(arn: &str) -> Result<&str, DynamoDbError> {
    arn.split(":table/")
        .nth(1)
        .map(|tail| tail.split('/').next().unwrap_or(tail))
        .filter(|name| !name.is_empty())
        .ok_or_else(|| DynamoDbError::ValidationException(format!("Invalid TableArn: {arn}")))
}

/// Load the persisted tags for a table (empty when none were ever set).
fn load_tags(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<Vec<Tag>, DynamoDbError> {
    match engine.get_document(context.tenant_id(), &tags_table()?, tags_id(table_name)?) {
        Ok(document) => {
            let raw = document.fields.get("tags").cloned().unwrap_or(Value::Null);
            if raw.is_null() {
                return Ok(Vec::new());
            }
            serde_json::from_value(raw).map_err(|error| {
                DynamoDbError::InternalServerError(format!("corrupt tag record: {error}"))
            })
        }
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            Ok(Vec::new())
        }
        Err(error) => Err(map_core_error(error)),
    }
}

/// Persist a table's tag set (upsert).
fn store_tags(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
    tags: &[Tag],
) -> Result<(), DynamoDbError> {
    let table = tags_table()?;
    let id = tags_id(table_name)?;
    let mut fields = Map::new();
    fields.insert(
        "tags".to_owned(),
        serde_json::to_value(tags).map_err(|error| {
            DynamoDbError::InternalServerError(format!("failed to serialize tags: {error}"))
        })?,
    );
    match engine.get_document(context.tenant_id(), &table, id.clone()) {
        Ok(_) => {
            engine
                .update_document(context.tenant_id(), table, id, fields)
                .map_err(map_core_error)?;
        }
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            engine
                .insert_document_with_id(context.tenant_id(), table, id, fields)
                .map_err(map_core_error)?;
        }
        Err(error) => return Err(map_core_error(error)),
    }
    Ok(())
}

/// Resolve the table named by `resource_arn`, erroring if it does not exist.
fn resolve_resource<'a>(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    resource_arn: &'a str,
) -> Result<&'a str, DynamoDbError> {
    let table_name = table_name_from_arn(resource_arn)?;
    // A tag op on a non-existent table is a 404 (matches DynamoDB).
    control_plane::load_table_description(engine, context, table_name)?;
    Ok(table_name)
}

fn validate_tag(tag: &Tag) -> Result<(), DynamoDbError> {
    if tag.key.is_empty() || tag.key.chars().count() > MAX_TAG_KEY_LEN {
        return Err(DynamoDbError::ValidationException(
            "Tag key must be between 1 and 128 characters".to_owned(),
        ));
    }
    if tag.key.starts_with("aws:") {
        return Err(DynamoDbError::ValidationException(
            "Tag keys with the reserved prefix 'aws:' cannot be set".to_owned(),
        ));
    }
    if tag.value.chars().count() > MAX_TAG_VALUE_LEN {
        return Err(DynamoDbError::ValidationException(
            "Tag value must be at most 256 characters".to_owned(),
        ));
    }
    Ok(())
}

/// TagResource: merge `Tags` into the resource's tag set (an existing key's
/// value is overwritten). Returns an empty body.
///
/// # Errors
/// `ResourceNotFoundException` for an unknown table; `ValidationException` for a
/// malformed ARN, an invalid tag, or exceeding the 50-tag cap.
pub fn tag_resource(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: TagResourceInput,
) -> Result<Value, DynamoDbError> {
    let table_name = resolve_resource(engine, context, &input.resource_arn)?;
    for tag in &input.tags {
        validate_tag(tag)?;
    }
    let mut tags = load_tags(engine, context, table_name)?;
    for incoming in input.tags {
        match tags.iter_mut().find(|tag| tag.key == incoming.key) {
            Some(existing) => existing.value = incoming.value,
            None => tags.push(incoming),
        }
    }
    if tags.len() > MAX_TAGS_PER_RESOURCE {
        return Err(DynamoDbError::ValidationException(format!(
            "A resource can have at most {MAX_TAGS_PER_RESOURCE} tags"
        )));
    }
    store_tags(engine, context, table_name, &tags)?;
    Ok(Value::Object(Map::new()))
}

/// UntagResource: remove the named tag keys (absent keys are ignored). Returns
/// an empty body.
///
/// # Errors
/// `ResourceNotFoundException` for an unknown table; `ValidationException` for a
/// malformed ARN.
pub fn untag_resource(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: UntagResourceInput,
) -> Result<Value, DynamoDbError> {
    let table_name = resolve_resource(engine, context, &input.resource_arn)?;
    let mut tags = load_tags(engine, context, table_name)?;
    tags.retain(|tag| !input.tag_keys.contains(&tag.key));
    store_tags(engine, context, table_name, &tags)?;
    Ok(Value::Object(Map::new()))
}

/// ListTagsOfResource: return the resource's tags. Tag sets are small (≤50), so
/// they are returned in a single page (`NextToken` is always `None`).
///
/// # Errors
/// `ResourceNotFoundException` for an unknown table; `ValidationException` for a
/// malformed ARN.
pub fn list_tags_of_resource(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: ListTagsOfResourceInput,
) -> Result<ListTagsOfResourceOutput, DynamoDbError> {
    let table_name = resolve_resource(engine, context, &input.resource_arn)?;
    let tags = load_tags(engine, context, table_name)?;
    Ok(ListTagsOfResourceOutput {
        tags,
        next_token: None,
    })
}

#[cfg(test)]
mod tests {
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

    /// Create table `name` and return its ARN.
    fn create_table(engine: &Arc<Engine>, context: &TenantIsolationContext, name: &str) -> String {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": name,
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap();
        control_plane::create_table(engine, context, input)
            .expect("create")
            .table_description
            .table_arn
    }

    fn tag(key: &str, value: &str) -> Tag {
        Tag {
            key: key.to_owned(),
            value: value.to_owned(),
        }
    }

    fn list(engine: &Arc<Engine>, context: &TenantIsolationContext, arn: &str) -> Vec<Tag> {
        list_tags_of_resource(
            engine,
            context,
            ListTagsOfResourceInput {
                resource_arn: arn.to_owned(),
                next_token: None,
            },
        )
        .expect("list tags")
        .tags
    }

    #[test]
    fn tag_list_untag_roundtrip() {
        let (engine, ctx, _t) = fixture();
        let arn = create_table(&engine, &ctx, "Orders");
        assert!(list(&engine, &ctx, &arn).is_empty(), "no tags initially");

        tag_resource(
            &engine,
            &ctx,
            TagResourceInput {
                resource_arn: arn.clone(),
                tags: vec![tag("env", "prod"), tag("team", "payments")],
            },
        )
        .expect("tag");
        let mut tags = list(&engine, &ctx, &arn);
        tags.sort_by(|a, b| a.key.cmp(&b.key));
        assert_eq!(tags, vec![tag("env", "prod"), tag("team", "payments")]);

        // Untag one key; the other survives.
        untag_resource(
            &engine,
            &ctx,
            UntagResourceInput {
                resource_arn: arn.clone(),
                tag_keys: vec!["env".to_owned()],
            },
        )
        .expect("untag");
        assert_eq!(list(&engine, &ctx, &arn), vec![tag("team", "payments")]);
    }

    #[test]
    fn tag_resource_overwrites_existing_key() {
        let (engine, ctx, _t) = fixture();
        let arn = create_table(&engine, &ctx, "Orders");
        tag_resource(
            &engine,
            &ctx,
            TagResourceInput {
                resource_arn: arn.clone(),
                tags: vec![tag("env", "staging")],
            },
        )
        .expect("tag");
        tag_resource(
            &engine,
            &ctx,
            TagResourceInput {
                resource_arn: arn.clone(),
                tags: vec![tag("env", "prod")],
            },
        )
        .expect("retag");
        assert_eq!(
            list(&engine, &ctx, &arn),
            vec![tag("env", "prod")],
            "same key is updated, not duplicated"
        );
    }

    #[test]
    fn untag_unknown_key_is_a_noop() {
        let (engine, ctx, _t) = fixture();
        let arn = create_table(&engine, &ctx, "Orders");
        tag_resource(
            &engine,
            &ctx,
            TagResourceInput {
                resource_arn: arn.clone(),
                tags: vec![tag("env", "prod")],
            },
        )
        .expect("tag");
        untag_resource(
            &engine,
            &ctx,
            UntagResourceInput {
                resource_arn: arn.clone(),
                tag_keys: vec!["does-not-exist".to_owned()],
            },
        )
        .expect("untag noop");
        assert_eq!(list(&engine, &ctx, &arn), vec![tag("env", "prod")]);
    }

    #[test]
    fn reserved_aws_prefix_is_rejected() {
        let (engine, ctx, _t) = fixture();
        let arn = create_table(&engine, &ctx, "Orders");
        let err = tag_resource(
            &engine,
            &ctx,
            TagResourceInput {
                resource_arn: arn,
                tags: vec![tag("aws:billing", "x")],
            },
        )
        .expect_err("reserved prefix rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn tag_on_missing_table_is_resource_not_found() {
        let (engine, ctx, _t) = fixture();
        let err = tag_resource(
            &engine,
            &ctx,
            TagResourceInput {
                resource_arn: "arn:aws:dynamodb:ddblocal:000000000000:table/Ghost".to_owned(),
                tags: vec![tag("env", "prod")],
            },
        )
        .expect_err("missing table");
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }

    #[test]
    fn malformed_arn_is_validation_error() {
        let (engine, ctx, _t) = fixture();
        let err = list_tags_of_resource(
            &engine,
            &ctx,
            ListTagsOfResourceInput {
                resource_arn: "not-an-arn".to_owned(),
                next_token: None,
            },
        )
        .expect_err("malformed arn");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn tags_are_tenant_isolated() {
        let (engine, _ctx, _t) = fixture();
        let acme = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
        let globex = crate::tenant::tenant_context(TenantId::new("globex").unwrap(), "test");
        crate::tenant::ensure_tenant(&engine, &acme).expect("acme");
        crate::tenant::ensure_tenant(&engine, &globex).expect("globex");
        let acme_arn = create_table(&engine, &acme, "Orders");
        let globex_arn = create_table(&engine, &globex, "Orders");

        tag_resource(
            &engine,
            &acme,
            TagResourceInput {
                resource_arn: acme_arn.clone(),
                tags: vec![tag("env", "prod")],
            },
        )
        .expect("acme tag");
        assert_eq!(list(&engine, &acme, &acme_arn), vec![tag("env", "prod")]);
        assert!(
            list(&engine, &globex, &globex_arn).is_empty(),
            "another tenant's identically-named table has its own tags"
        );
    }
}
