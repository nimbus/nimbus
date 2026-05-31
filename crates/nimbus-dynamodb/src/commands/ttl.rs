//! DynamoDB Time To Live (T6): UpdateTimeToLive + DescribeTimeToLive (D6.1).
//!
//! TTL configuration (enabled flag + attribute name) is persisted as one doc
//! per table in a reserved `_ddb_ttl` catalog, keyed by the table name. Unlike
//! DynamoDB, the change takes effect immediately (no async ENABLING/DISABLING
//! state) and there is no modification cooldown — see DDB-DIV-008/009. The
//! actual expiry sweep is D6.2.

use std::sync::Arc;

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    AttributeValue, DescribeTimeToLiveInput, DescribeTimeToLiveOutput, StreamEventName,
    TimeToLiveDescription, TimeToLiveSpecificationOutput, TimeToLiveStatus, UpdateTimeToLiveInput,
    UpdateTimeToLiveOutput, extract_key,
};
use nimbus_core::{DocumentId, StructuredQuery, TableName};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;
use serde_json::{Map, Value};

use crate::attribute_value::fields_to_item;
use crate::commands::{control_plane, stream};
use crate::error::map_core_error;

/// Reserved table holding one TTL-config doc per table (doc id = table name).
const TTL_TABLE: &str = "_ddb_ttl";
/// Max length of a TTL attribute name (DynamoDB's attribute-name cap).
const MAX_TTL_ATTRIBUTE_LEN: usize = 255;

fn ttl_table() -> Result<TableName, DynamoDbError> {
    TableName::new(TTL_TABLE).map_err(map_core_error)
}

fn ttl_id(table_name: &str) -> Result<DocumentId, DynamoDbError> {
    DocumentId::from_key(table_name).map_err(map_core_error)
}

/// Drop `table_name`'s TTL configuration document when the table is deleted, so
/// a table recreated under the same name does not inherit stale TTL state (F4).
pub(crate) fn reclaim_for_table(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<(), DynamoDbError> {
    match service.delete_document(context.tenant_id(), ttl_table()?, ttl_id(table_name)?) {
        Ok(()) | Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            Ok(())
        }
        Err(error) => Err(map_core_error(error)),
    }
}

/// The persisted TTL state for a table: `(enabled, attribute_name)`. Disabled
/// with no attribute when TTL was never configured.
fn load_ttl_state(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<(bool, Option<String>), DynamoDbError> {
    match service.get_document(context.tenant_id(), &ttl_table()?, ttl_id(table_name)?) {
        Ok(document) => {
            let enabled = document
                .fields
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let attribute_name = document
                .fields
                .get("attribute_name")
                .and_then(Value::as_str)
                .map(str::to_owned);
            Ok((enabled, attribute_name))
        }
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            Ok((false, None))
        }
        Err(error) => Err(map_core_error(error)),
    }
}

/// DescribeTimeToLive: report the table's TTL status. ENABLED carries the
/// attribute name; DISABLED omits it (matches DynamoDB).
///
/// # Errors
/// `ResourceNotFoundException` if the table does not exist.
pub fn describe_time_to_live(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: DescribeTimeToLiveInput,
) -> Result<DescribeTimeToLiveOutput, DynamoDbError> {
    // Existence check — an unknown table is a 404, not a silent DISABLED.
    control_plane::load_table_description(service, context, &input.table_name)?;
    let (enabled, attribute_name) = load_ttl_state(service, context, &input.table_name)?;
    let description = if enabled {
        TimeToLiveDescription {
            time_to_live_status: TimeToLiveStatus::Enabled,
            attribute_name,
        }
    } else {
        TimeToLiveDescription {
            time_to_live_status: TimeToLiveStatus::Disabled,
            attribute_name: None,
        }
    };
    Ok(DescribeTimeToLiveOutput {
        time_to_live_description: description,
    })
}

/// UpdateTimeToLive: enable or disable TTL on `AttributeName`. Takes effect
/// immediately (no async ENABLING/DISABLING state) with no modification
/// cooldown (DDB-DIV-009); the attribute-name charset is unrestricted beyond a
/// 1–255 length bound (DDB-DIV-008). Echoes the requested spec.
///
/// # Errors
/// `ResourceNotFoundException` if the table does not exist; `ValidationException`
/// if the attribute name is empty or longer than 255 characters.
pub fn update_time_to_live(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: UpdateTimeToLiveInput,
) -> Result<UpdateTimeToLiveOutput, DynamoDbError> {
    control_plane::load_table_description(service, context, &input.table_name)?;
    let spec = input.time_to_live_specification;
    let attribute_name = spec.attribute_name;
    if attribute_name.is_empty() || attribute_name.chars().count() > MAX_TTL_ATTRIBUTE_LEN {
        return Err(DynamoDbError::ValidationException(
            "TimeToLiveSpecification.AttributeName must be between 1 and 255 characters".to_owned(),
        ));
    }
    let mut fields = Map::new();
    fields.insert("enabled".to_owned(), Value::Bool(spec.enabled));
    fields.insert(
        "attribute_name".to_owned(),
        Value::String(attribute_name.clone()),
    );
    upsert_ttl_state(service, context, &input.table_name, fields)?;
    Ok(UpdateTimeToLiveOutput {
        time_to_live_specification: TimeToLiveSpecificationOutput {
            attribute_name,
            enabled: spec.enabled,
        },
    })
}

fn upsert_ttl_state(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
    fields: Map<String, Value>,
) -> Result<(), DynamoDbError> {
    let table = ttl_table()?;
    let id = ttl_id(table_name)?;
    match service.get_document(context.tenant_id(), &table, id.clone()) {
        Ok(_) => {
            service
                .update_document(context.tenant_id(), table, id, fields)
                .map_err(map_core_error)?;
        }
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            service
                .insert_document_with_id(context.tenant_id(), table, id, fields)
                .map_err(map_core_error)?;
        }
        Err(error) => return Err(map_core_error(error)),
    }
    Ok(())
}

/// The TTL attribute name a sweep should honor for `table_name`, or `None` when
/// TTL is disabled.
fn enabled_ttl_attribute(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<Option<String>, DynamoDbError> {
    let (enabled, attribute_name) = load_ttl_state(service, context, table_name)?;
    Ok(if enabled { attribute_name } else { None })
}

/// True when `item`'s TTL `attribute` is a Number epoch-seconds value at or
/// before `now`. DynamoDB only expires items whose TTL attribute is a Number;
/// a missing or non-Number attribute (or an unparseable one) is never expired.
fn is_expired(item: &extenddb_core::types::Item, attribute: &str, now: i64) -> bool {
    match item.get(attribute) {
        Some(AttributeValue::N(value)) => {
            value.parse::<f64>().is_ok_and(|epoch| epoch <= now as f64)
        }
        _ => false,
    }
}

/// Sweep one table: delete every item whose TTL attribute is past `now`,
/// emitting a TTL-attributed REMOVE stream event for each (no-op unless a stream
/// is enabled). Returns the number of items reclaimed. A no-op when TTL is
/// disabled for the table.
///
/// # Errors
/// A mapped engine error if the table cannot be scanned or an item deleted.
pub fn sweep_table(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
    now: i64,
) -> Result<usize, DynamoDbError> {
    let Some(attribute) = enabled_ttl_attribute(service, context, table_name)? else {
        return Ok(0);
    };
    let key_schema = control_plane::load_key_schema(service, context, table_name)?;
    let table = TableName::new(table_name).map_err(map_core_error)?;
    let documents = match service.query_documents_structured(
        context.tenant_id(),
        &table,
        &StructuredQuery::default(),
    ) {
        Ok(documents) => documents,
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            return Ok(0);
        }
        Err(error) => return Err(map_core_error(error)),
    };

    let mut swept = 0;
    for document in documents {
        let item = fields_to_item(&document.fields)?;
        if !is_expired(&item, &attribute, now) {
            continue;
        }
        service
            .delete_document(context.tenant_id(), table.clone(), document.id.clone())
            .map_err(map_core_error)?;
        // A TTL deletion is a service-originated REMOVE; the deleted item is the
        // old image (DynamoDB carries it for NEW_AND_OLD_IMAGES / OLD_IMAGE).
        let keys = extract_key(&item, &key_schema);
        stream::capture_event(
            service,
            context,
            table_name,
            stream::ChangeEvent {
                event_name: StreamEventName::Remove,
                keys: &keys,
                old_image: Some(&item),
                new_image: None,
                user_identity: Some(stream::ttl_user_identity()),
            },
        )?;
        swept += 1;
    }
    Ok(swept)
}

/// Sweep every table the tenant owns, returning the total items reclaimed.
///
/// # Errors
/// A mapped engine error if the catalog cannot be enumerated or a sweep fails.
pub fn sweep_tenant(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    now: i64,
) -> Result<usize, DynamoDbError> {
    let mut swept = 0;
    for description in control_plane::list_table_descriptions(service, context)? {
        swept += sweep_table(service, context, &description.table_name, now)?;
    }
    Ok(swept)
}

/// Run one TTL sweep pass across every tenant bound in `access_keys`, returning
/// the total items reclaimed plus any per-tenant errors. A failing tenant never
/// aborts the others — periodic TTL reclamation is best-effort maintenance, so
/// the driver logs the errors and keeps the schedule.
#[must_use]
pub fn sweep_all_tenants(
    service: &Arc<Service>,
    access_keys: &crate::AccessKeyRegistry,
    now: i64,
) -> (usize, Vec<(nimbus_core::TenantId, DynamoDbError)>) {
    let mut swept = 0;
    let mut errors = Vec::new();
    for tenant in access_keys.tenants() {
        let context = crate::tenant::tenant_context(tenant.clone(), "ttl-sweeper");
        match sweep_tenant(service, &context, now) {
            Ok(count) => swept += count,
            Err(error) => errors.push((tenant, error)),
        }
    }
    (swept, errors)
}

#[cfg(test)]
mod tests {
    use super::*;
    use extenddb_core::types::{CreateTableInput, TimeToLiveSpecification};
    use nimbus_core::TenantId;
    use serde_json::json;

    fn fixture() -> (Arc<Service>, TenantIsolationContext, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let service = Arc::new(Service::new(temp.path()).expect("service"));
        let context = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
        crate::tenant::ensure_tenant(&service, &context).expect("tenant");
        (service, context, temp)
    }

    fn create_table(service: &Arc<Service>, context: &TenantIsolationContext, name: &str) {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": name,
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap();
        control_plane::create_table(service, context, input).expect("create");
    }

    fn update(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        table: &str,
        enabled: bool,
        attribute_name: &str,
    ) -> Result<UpdateTimeToLiveOutput, DynamoDbError> {
        update_time_to_live(
            service,
            context,
            UpdateTimeToLiveInput {
                table_name: table.to_owned(),
                time_to_live_specification: TimeToLiveSpecification {
                    enabled,
                    attribute_name: attribute_name.to_owned(),
                },
            },
        )
    }

    fn describe(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        table: &str,
    ) -> TimeToLiveDescription {
        describe_time_to_live(
            service,
            context,
            DescribeTimeToLiveInput {
                table_name: table.to_owned(),
            },
        )
        .expect("describe")
        .time_to_live_description
    }

    #[test]
    fn describe_defaults_to_disabled_when_never_configured() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, "Sessions");
        let desc = describe(&service, &ctx, "Sessions");
        assert_eq!(desc.time_to_live_status, TimeToLiveStatus::Disabled);
        assert!(
            desc.attribute_name.is_none(),
            "DISABLED omits the attribute"
        );
    }

    #[test]
    fn update_enables_ttl_and_describe_reports_it() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, "Sessions");
        let out = update(&service, &ctx, "Sessions", true, "expiresAt").expect("enable");
        let spec = out.time_to_live_specification;
        assert!(spec.enabled);
        assert_eq!(spec.attribute_name, "expiresAt", "the request is echoed");

        let desc = describe(&service, &ctx, "Sessions");
        assert_eq!(desc.time_to_live_status, TimeToLiveStatus::Enabled);
        assert_eq!(desc.attribute_name.as_deref(), Some("expiresAt"));
    }

    #[test]
    fn update_disable_then_describe_reports_disabled_without_attribute() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, "Sessions");
        update(&service, &ctx, "Sessions", true, "expiresAt").expect("enable");
        update(&service, &ctx, "Sessions", false, "expiresAt").expect("disable");

        let desc = describe(&service, &ctx, "Sessions");
        assert_eq!(desc.time_to_live_status, TimeToLiveStatus::Disabled);
        assert!(
            desc.attribute_name.is_none(),
            "DISABLED omits the attribute even though it was once set"
        );
    }

    #[test]
    fn update_is_idempotent_with_no_cooldown() {
        // DDB-DIV-009: DynamoDB rejects rapid re-toggling with a cooldown
        // ValidationException; Nimbus applies every change immediately.
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, "Sessions");
        update(&service, &ctx, "Sessions", true, "expiresAt").expect("enable");
        update(&service, &ctx, "Sessions", true, "expiresAt").expect("re-enable, no cooldown");
        update(&service, &ctx, "Sessions", false, "expiresAt").expect("disable, no cooldown");
        update(&service, &ctx, "Sessions", true, "ttl").expect("re-enable new attr, no cooldown");
        let desc = describe(&service, &ctx, "Sessions");
        assert_eq!(desc.time_to_live_status, TimeToLiveStatus::Enabled);
        assert_eq!(desc.attribute_name.as_deref(), Some("ttl"));
    }

    #[test]
    fn update_accepts_any_utf8_attribute_name() {
        // DDB-DIV-008: the TTL attribute-name charset is unrestricted (DynamoDB
        // allows any UTF-8; Nimbus has no SQL surface to defend).
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, "Sessions");
        let exotic = "期限-✓.expires_at";
        update(&service, &ctx, "Sessions", true, exotic).expect("exotic attr accepted");
        assert_eq!(
            describe(&service, &ctx, "Sessions")
                .attribute_name
                .as_deref(),
            Some(exotic)
        );
    }

    #[test]
    fn update_rejects_empty_attribute_name() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, "Sessions");
        let err = update(&service, &ctx, "Sessions", true, "").expect_err("empty rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn update_on_missing_table_is_resource_not_found() {
        let (service, ctx, _t) = fixture();
        let err = update(&service, &ctx, "Ghost", true, "expiresAt").expect_err("missing table");
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }

    #[test]
    fn describe_on_missing_table_is_resource_not_found() {
        let (service, ctx, _t) = fixture();
        let err = describe_time_to_live(
            &service,
            &ctx,
            DescribeTimeToLiveInput {
                table_name: "Ghost".to_owned(),
            },
        )
        .expect_err("missing table");
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }

    #[test]
    fn ttl_state_is_tenant_isolated() {
        let (service, _ctx, _t) = fixture();
        let acme = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
        let globex = crate::tenant::tenant_context(TenantId::new("globex").unwrap(), "test");
        crate::tenant::ensure_tenant(&service, &acme).expect("acme");
        crate::tenant::ensure_tenant(&service, &globex).expect("globex");
        create_table(&service, &acme, "Sessions");
        create_table(&service, &globex, "Sessions");

        update(&service, &acme, "Sessions", true, "expiresAt").expect("acme enables");
        assert_eq!(
            describe(&service, &acme, "Sessions").time_to_live_status,
            TimeToLiveStatus::Enabled
        );
        // globex's identically-named table is untouched.
        assert_eq!(
            describe(&service, &globex, "Sessions").time_to_live_status,
            TimeToLiveStatus::Disabled,
            "another tenant's TTL config is invisible"
        );
    }

    // ---- D6.2: TTL sweeper ----

    /// Put an item `pk` with an optional `expiresAt` TTL value (epoch seconds).
    fn put(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        table: &str,
        pk: &str,
        expires_at: Option<i64>,
    ) {
        let mut item = json!({ "pk": { "S": pk } });
        if let Some(epoch) = expires_at {
            item["expiresAt"] = json!({ "N": epoch.to_string() });
        }
        crate::commands::item::put_item(
            service,
            context,
            serde_json::from_value(json!({ "TableName": table, "Item": item })).unwrap(),
        )
        .expect("put");
    }

    fn exists(
        service: &Arc<Service>,
        context: &TenantIsolationContext,
        table: &str,
        pk: &str,
    ) -> bool {
        crate::commands::item::get_item(
            service,
            context,
            serde_json::from_value(json!({ "TableName": table, "Key": { "pk": { "S": pk } } }))
                .unwrap(),
        )
        .expect("get")
        .item
        .is_some()
    }

    #[test]
    fn sweep_deletes_expired_items_and_leaves_the_rest() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, "Sessions");
        update(&service, &ctx, "Sessions", true, "expiresAt").expect("enable");
        let now = 1_700_000_000;
        put(&service, &ctx, "Sessions", "old", Some(now - 10)); // expired
        put(&service, &ctx, "Sessions", "edge", Some(now)); // expires exactly now → expired
        put(&service, &ctx, "Sessions", "fresh", Some(now + 10_000)); // future
        put(&service, &ctx, "Sessions", "noattr", None); // no TTL attribute

        let swept = sweep_table(&service, &ctx, "Sessions", now).expect("sweep");
        assert_eq!(swept, 2, "the past and exactly-now items are reclaimed");
        assert!(!exists(&service, &ctx, "Sessions", "old"));
        assert!(!exists(&service, &ctx, "Sessions", "edge"));
        assert!(
            exists(&service, &ctx, "Sessions", "fresh"),
            "future item kept"
        );
        assert!(
            exists(&service, &ctx, "Sessions", "noattr"),
            "an item without the TTL attribute is never expired"
        );
    }

    #[test]
    fn sweep_is_a_noop_when_ttl_is_disabled() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, "Sessions");
        let now = 1_700_000_000;
        put(&service, &ctx, "Sessions", "old", Some(now - 10));
        let swept = sweep_table(&service, &ctx, "Sessions", now).expect("sweep");
        assert_eq!(swept, 0, "no TTL configured → nothing is reclaimed");
        assert!(exists(&service, &ctx, "Sessions", "old"));
    }

    #[test]
    fn sweep_tenant_covers_every_table() {
        let (service, ctx, _t) = fixture();
        let now = 1_700_000_000;
        for table in ["Sessions", "Carts"] {
            create_table(&service, &ctx, table);
            update(&service, &ctx, table, true, "expiresAt").expect("enable");
            put(&service, &ctx, table, "old", Some(now - 10));
            put(&service, &ctx, table, "fresh", Some(now + 10_000));
        }
        let swept = sweep_tenant(&service, &ctx, now).expect("sweep tenant");
        assert_eq!(swept, 2, "one expired item per table");
        assert!(!exists(&service, &ctx, "Sessions", "old"));
        assert!(!exists(&service, &ctx, "Carts", "old"));
    }

    #[test]
    fn sweep_all_tenants_aggregates_and_isolates() {
        let (service, _ctx, _t) = fixture();
        let acme = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
        let globex = crate::tenant::tenant_context(TenantId::new("globex").unwrap(), "test");
        crate::tenant::ensure_tenant(&service, &acme).expect("acme");
        crate::tenant::ensure_tenant(&service, &globex).expect("globex");
        let now = 1_700_000_000;
        for ctx in [&acme, &globex] {
            create_table(&service, ctx, "Sessions");
            update(&service, ctx, "Sessions", true, "expiresAt").expect("enable");
            put(&service, ctx, "Sessions", "old", Some(now - 10));
        }

        let registry = crate::AccessKeyRegistry::new()
            .bind("AKIAACME", TenantId::new("acme").unwrap())
            .bind("AKIAGLOBEX", TenantId::new("globex").unwrap());
        let (swept, errors) = sweep_all_tenants(&service, &registry, now);
        assert_eq!(swept, 2, "one expired item reclaimed per tenant");
        assert!(errors.is_empty(), "no per-tenant errors: {errors:?}");
        assert!(!exists(&service, &acme, "Sessions", "old"));
        assert!(!exists(&service, &globex, "Sessions", "old"));
    }

    #[test]
    fn ttl_removal_emits_service_user_identity_on_the_stream() {
        use extenddb_core::types::{
            DescribeStreamInput, GetRecordsInput, GetShardIteratorInput, ShardIteratorType,
            StreamEventName,
        };

        let (service, ctx, _t) = fixture();
        // Stream-enabled (both images) + TTL-enabled table.
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "Sessions",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES" }
        }))
        .unwrap();
        let arn = control_plane::create_table(&service, &ctx, input)
            .expect("create")
            .table_description
            .latest_stream_arn
            .expect("stream arn");
        update(&service, &ctx, "Sessions", true, "expiresAt").expect("enable ttl");

        let now = 1_700_000_000;
        put(&service, &ctx, "Sessions", "old", Some(now - 10));
        let swept = sweep_table(&service, &ctx, "Sessions", now).expect("sweep");
        assert_eq!(swept, 1);

        // Read the stream back: the last record is the TTL REMOVE.
        let shard = stream::describe_stream(
            &service,
            &ctx,
            DescribeStreamInput {
                stream_arn: arn.clone(),
                limit: None,
                exclusive_start_shard_id: None,
            },
        )
        .expect("describe stream")
        .stream_description
        .shards[0]
            .shard_id
            .clone();
        let iterator = stream::get_shard_iterator(
            &service,
            &ctx,
            GetShardIteratorInput {
                stream_arn: arn.clone(),
                shard_id: shard,
                shard_iterator_type: ShardIteratorType::TrimHorizon,
                sequence_number: None,
            },
        )
        .expect("iterator")
        .shard_iterator
        .expect("iterator");
        let records = stream::get_records(
            &service,
            &ctx,
            GetRecordsInput {
                shard_iterator: iterator,
                limit: None,
            },
        )
        .expect("get records")
        .records;

        let remove = records
            .iter()
            .find(|record| record.event_name == StreamEventName::Remove)
            .expect("a REMOVE record from the TTL sweep");
        let identity = remove
            .user_identity
            .as_ref()
            .expect("TTL REMOVE carries a userIdentity");
        assert_eq!(identity.identity_type, "Service");
        assert_eq!(identity.principal_id, "dynamodb.amazonaws.com");
        assert!(
            remove.dynamodb.old_image.is_some(),
            "the deleted item is the old image"
        );
    }
}
