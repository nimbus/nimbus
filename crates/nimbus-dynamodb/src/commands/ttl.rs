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
use nimbus_core::{DocumentId, StructuredQuery, TableName, WritePrecondition};
use nimbus_engine::{Engine, MutationActor};
use nimbus_tenant::TenantIsolationContext;
use serde_json::{Map, Value};

use crate::attribute_value::fields_to_item;
use crate::commands::{control_plane, item, stream};
use crate::error::map_core_error;
use crate::tenant::{adapter_principal, caller_principal};

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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<(), DynamoDbError> {
    match engine.delete_document_with(
        context.tenant_id(),
        ttl_table()?,
        ttl_id(table_name)?,
        MutationActor::with_principal(&adapter_principal()),
    ) {
        Ok(()) | Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            Ok(())
        }
        Err(error) => Err(map_core_error(error)),
    }
}

/// The persisted TTL state for a table: `(enabled, attribute_name)`. Disabled
/// with no attribute when TTL was never configured.
fn load_ttl_state(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<(bool, Option<String>), DynamoDbError> {
    match engine.get_document_with_principal(
        context.tenant_id(),
        &ttl_table()?,
        ttl_id(table_name)?,
        &adapter_principal(),
    ) {
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: DescribeTimeToLiveInput,
) -> Result<DescribeTimeToLiveOutput, DynamoDbError> {
    // Existence check — an unknown table is a 404, not a silent DISABLED.
    control_plane::load_table_description(engine, context, &input.table_name)?;
    let (enabled, attribute_name) = load_ttl_state(engine, context, &input.table_name)?;
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    input: UpdateTimeToLiveInput,
) -> Result<UpdateTimeToLiveOutput, DynamoDbError> {
    control_plane::load_table_description(engine, context, &input.table_name)?;
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
    upsert_ttl_state(engine, context, &input.table_name, fields)?;
    Ok(UpdateTimeToLiveOutput {
        time_to_live_specification: TimeToLiveSpecificationOutput {
            attribute_name,
            enabled: spec.enabled,
        },
    })
}

fn upsert_ttl_state(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
    fields: Map<String, Value>,
) -> Result<(), DynamoDbError> {
    let table = ttl_table()?;
    let id = ttl_id(table_name)?;
    item::atomic_overwrite(
        engine,
        context,
        table,
        id,
        fields,
        WritePrecondition::default(),
        adapter_principal(),
    )
    .map_err(map_core_error)
}

/// The TTL attribute name a sweep should honor for `table_name`, or `None` when
/// TTL is disabled.
fn enabled_ttl_attribute(
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<Option<String>, DynamoDbError> {
    let (enabled, attribute_name) = load_ttl_state(engine, context, table_name)?;
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    table_name: &str,
    now: i64,
) -> Result<usize, DynamoDbError> {
    let Some(attribute) = enabled_ttl_attribute(engine, context, table_name)? else {
        return Ok(0);
    };
    let key_schema = control_plane::load_key_schema(engine, context, table_name)?;
    let table = TableName::new(table_name).map_err(map_core_error)?;
    // A sweep driven by a maintenance context runs as `system`: expiry is the
    // tenant's own configuration taking effect on a schedule, and must not be
    // narrowed by a table policy written for interactive callers.
    let documents = match engine.query_documents_structured_with_principal(
        context.tenant_id(),
        &table,
        &StructuredQuery::default(),
        &caller_principal(context),
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
        // A TTL deletion is a service-originated REMOVE. The delete and its
        // stream event commit in one AtomicWriteBatch so a crash can never
        // leave the row gone with no event emitted, nor an event emitted with
        // the row still present. The deleted item is the old image (DynamoDB
        // carries it for NEW_AND_OLD_IMAGES / OLD_IMAGE). The delete is
        // unconditional (last-writer-wins): TTL is best-effort maintenance over
        // a read snapshot, matching DynamoDB's own eventual TTL reclamation.
        let keys = extract_key(&item, &key_schema);
        let change = stream::StreamChange::new(
            table_name.to_string(),
            StreamEventName::Remove,
            keys,
            stream::OldImage::of(Some(&document))?,
            None,
            Some(stream::ttl_user_identity()),
        );
        stream::execute_atomic_write_batch_with_streams(
            engine,
            context,
            vec![item::delete_atomic_write(
                table.clone(),
                document.id.clone(),
                WritePrecondition::default(),
            )],
            &[change],
            map_core_error,
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
    engine: &Arc<Engine>,
    context: &TenantIsolationContext,
    now: i64,
) -> Result<usize, DynamoDbError> {
    let mut swept = 0;
    for description in control_plane::list_table_descriptions(engine, context)? {
        swept += sweep_table(engine, context, &description.table_name, now)?;
    }
    Ok(swept)
}

/// Run one TTL sweep pass across every tenant bound in `access_keys`, returning
/// the total items reclaimed plus any per-tenant errors. A failing tenant never
/// aborts the others — periodic TTL reclamation is best-effort maintenance, so
/// the driver logs the errors and keeps the schedule.
#[must_use]
pub fn sweep_all_tenants(
    engine: &Arc<Engine>,
    access_keys: &crate::AccessKeyRegistry,
    now: i64,
) -> (usize, Vec<(nimbus_core::TenantId, DynamoDbError)>) {
    let mut swept = 0;
    let mut errors = Vec::new();
    for tenant in access_keys.tenants() {
        let context = crate::tenant::maintenance_context(tenant.clone(), "ttl-sweeper");
        match sweep_tenant(engine, &context, now) {
            Ok(count) => swept += count,
            Err(error) => errors.push((tenant, error)),
        }
    }
    (swept, errors)
}

/// Run one TTL sweep pass through the provider-capable tenant lifecycle.
///
/// Each configured tenant is admitted or loaded before the synchronous sweep
/// core runs. Admission and sweep failures remain tenant-local: one failure is
/// reported with that tenant and does not prevent later tenants from running.
/// The pass is sequential so a periodic maintenance tick cannot create an
/// unbounded burst of tenant loads or storage work.
pub async fn sweep_all_tenants_async(
    engine: &Arc<Engine>,
    access_keys: &crate::AccessKeyRegistry,
    now: i64,
) -> (usize, Vec<(nimbus_core::TenantId, DynamoDbError)>) {
    let mut swept = 0;
    let mut errors = Vec::new();
    for tenant in access_keys.tenants() {
        let context = crate::tenant::maintenance_context(tenant.clone(), "ttl-sweeper");
        let result = match crate::tenant::ensure_tenant_async(engine, &context).await {
            Ok(()) => sweep_tenant(engine, &context, now),
            Err(error) => Err(error),
        };
        match result {
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

    fn fixture() -> (Arc<Engine>, TenantIsolationContext, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine"));
        let context = crate::tenant::test_context(TenantId::new("acme").unwrap(), "test");
        crate::tenant::ensure_tenant(&engine, &context).expect("tenant");
        (engine, context, temp)
    }

    fn create_table(engine: &Arc<Engine>, context: &TenantIsolationContext, name: &str) {
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": name,
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
        }))
        .unwrap();
        control_plane::create_table(engine, context, input).expect("create");
    }

    fn update(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        table: &str,
        enabled: bool,
        attribute_name: &str,
    ) -> Result<UpdateTimeToLiveOutput, DynamoDbError> {
        update_time_to_live(
            engine,
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
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        table: &str,
    ) -> TimeToLiveDescription {
        describe_time_to_live(
            engine,
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
        let (engine, ctx, _t) = fixture();
        create_table(&engine, &ctx, "Sessions");
        let desc = describe(&engine, &ctx, "Sessions");
        assert_eq!(desc.time_to_live_status, TimeToLiveStatus::Disabled);
        assert!(
            desc.attribute_name.is_none(),
            "DISABLED omits the attribute"
        );
    }

    #[test]
    fn update_enables_ttl_and_describe_reports_it() {
        let (engine, ctx, _t) = fixture();
        create_table(&engine, &ctx, "Sessions");
        let out = update(&engine, &ctx, "Sessions", true, "expiresAt").expect("enable");
        let spec = out.time_to_live_specification;
        assert!(spec.enabled);
        assert_eq!(spec.attribute_name, "expiresAt", "the request is echoed");

        let desc = describe(&engine, &ctx, "Sessions");
        assert_eq!(desc.time_to_live_status, TimeToLiveStatus::Enabled);
        assert_eq!(desc.attribute_name.as_deref(), Some("expiresAt"));
    }

    #[test]
    fn update_disable_then_describe_reports_disabled_without_attribute() {
        let (engine, ctx, _t) = fixture();
        create_table(&engine, &ctx, "Sessions");
        update(&engine, &ctx, "Sessions", true, "expiresAt").expect("enable");
        update(&engine, &ctx, "Sessions", false, "expiresAt").expect("disable");

        let desc = describe(&engine, &ctx, "Sessions");
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
        let (engine, ctx, _t) = fixture();
        create_table(&engine, &ctx, "Sessions");
        update(&engine, &ctx, "Sessions", true, "expiresAt").expect("enable");
        update(&engine, &ctx, "Sessions", true, "expiresAt").expect("re-enable, no cooldown");
        update(&engine, &ctx, "Sessions", false, "expiresAt").expect("disable, no cooldown");
        update(&engine, &ctx, "Sessions", true, "ttl").expect("re-enable new attr, no cooldown");
        let desc = describe(&engine, &ctx, "Sessions");
        assert_eq!(desc.time_to_live_status, TimeToLiveStatus::Enabled);
        assert_eq!(desc.attribute_name.as_deref(), Some("ttl"));
    }

    #[test]
    fn update_accepts_any_utf8_attribute_name() {
        // DDB-DIV-008: the TTL attribute-name charset is unrestricted (DynamoDB
        // allows any UTF-8; Nimbus has no SQL surface to defend).
        let (engine, ctx, _t) = fixture();
        create_table(&engine, &ctx, "Sessions");
        let exotic = "期限-✓.expires_at";
        update(&engine, &ctx, "Sessions", true, exotic).expect("exotic attr accepted");
        assert_eq!(
            describe(&engine, &ctx, "Sessions")
                .attribute_name
                .as_deref(),
            Some(exotic)
        );
    }

    #[test]
    fn update_rejects_empty_attribute_name() {
        let (engine, ctx, _t) = fixture();
        create_table(&engine, &ctx, "Sessions");
        let err = update(&engine, &ctx, "Sessions", true, "").expect_err("empty rejected");
        assert!(matches!(err, DynamoDbError::ValidationException(_)));
    }

    #[test]
    fn update_on_missing_table_is_resource_not_found() {
        let (engine, ctx, _t) = fixture();
        let err = update(&engine, &ctx, "Ghost", true, "expiresAt").expect_err("missing table");
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }

    #[test]
    fn describe_on_missing_table_is_resource_not_found() {
        let (engine, ctx, _t) = fixture();
        let err = describe_time_to_live(
            &engine,
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
        let (engine, _ctx, _t) = fixture();
        let acme = crate::tenant::test_context(TenantId::new("acme").unwrap(), "test");
        let globex = crate::tenant::test_context(TenantId::new("globex").unwrap(), "test");
        crate::tenant::ensure_tenant(&engine, &acme).expect("acme");
        crate::tenant::ensure_tenant(&engine, &globex).expect("globex");
        create_table(&engine, &acme, "Sessions");
        create_table(&engine, &globex, "Sessions");

        update(&engine, &acme, "Sessions", true, "expiresAt").expect("acme enables");
        assert_eq!(
            describe(&engine, &acme, "Sessions").time_to_live_status,
            TimeToLiveStatus::Enabled
        );
        // globex's identically-named table is untouched.
        assert_eq!(
            describe(&engine, &globex, "Sessions").time_to_live_status,
            TimeToLiveStatus::Disabled,
            "another tenant's TTL config is invisible"
        );
    }

    // ---- D6.2: TTL sweeper ----

    /// Put an item `pk` with an optional `expiresAt` TTL value (epoch seconds).
    fn put(
        engine: &Arc<Engine>,
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
            engine,
            context,
            serde_json::from_value(json!({ "TableName": table, "Item": item })).unwrap(),
        )
        .expect("put");
    }

    fn exists(
        engine: &Arc<Engine>,
        context: &TenantIsolationContext,
        table: &str,
        pk: &str,
    ) -> bool {
        crate::commands::item::get_item(
            engine,
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
        let (engine, ctx, _t) = fixture();
        create_table(&engine, &ctx, "Sessions");
        update(&engine, &ctx, "Sessions", true, "expiresAt").expect("enable");
        let now = 1_700_000_000;
        put(&engine, &ctx, "Sessions", "old", Some(now - 10)); // expired
        put(&engine, &ctx, "Sessions", "edge", Some(now)); // expires exactly now → expired
        put(&engine, &ctx, "Sessions", "fresh", Some(now + 10_000)); // future
        put(&engine, &ctx, "Sessions", "noattr", None); // no TTL attribute

        let swept = sweep_table(&engine, &ctx, "Sessions", now).expect("sweep");
        assert_eq!(swept, 2, "the past and exactly-now items are reclaimed");
        assert!(!exists(&engine, &ctx, "Sessions", "old"));
        assert!(!exists(&engine, &ctx, "Sessions", "edge"));
        assert!(
            exists(&engine, &ctx, "Sessions", "fresh"),
            "future item kept"
        );
        assert!(
            exists(&engine, &ctx, "Sessions", "noattr"),
            "an item without the TTL attribute is never expired"
        );
    }

    #[test]
    fn sweep_is_a_noop_when_ttl_is_disabled() {
        let (engine, ctx, _t) = fixture();
        create_table(&engine, &ctx, "Sessions");
        let now = 1_700_000_000;
        put(&engine, &ctx, "Sessions", "old", Some(now - 10));
        let swept = sweep_table(&engine, &ctx, "Sessions", now).expect("sweep");
        assert_eq!(swept, 0, "no TTL configured → nothing is reclaimed");
        assert!(exists(&engine, &ctx, "Sessions", "old"));
    }

    #[test]
    fn sweep_tenant_covers_every_table() {
        let (engine, ctx, _t) = fixture();
        let now = 1_700_000_000;
        for table in ["Sessions", "Carts"] {
            create_table(&engine, &ctx, table);
            update(&engine, &ctx, table, true, "expiresAt").expect("enable");
            put(&engine, &ctx, table, "old", Some(now - 10));
            put(&engine, &ctx, table, "fresh", Some(now + 10_000));
        }
        let swept = sweep_tenant(&engine, &ctx, now).expect("sweep tenant");
        assert_eq!(swept, 2, "one expired item per table");
        assert!(!exists(&engine, &ctx, "Sessions", "old"));
        assert!(!exists(&engine, &ctx, "Carts", "old"));
    }

    #[test]
    fn sweep_all_tenants_aggregates_and_isolates() {
        let (engine, _ctx, _t) = fixture();
        let acme = crate::tenant::test_context(TenantId::new("acme").unwrap(), "test");
        let globex = crate::tenant::test_context(TenantId::new("globex").unwrap(), "test");
        crate::tenant::ensure_tenant(&engine, &acme).expect("acme");
        crate::tenant::ensure_tenant(&engine, &globex).expect("globex");
        let now = 1_700_000_000;
        for ctx in [&acme, &globex] {
            create_table(&engine, ctx, "Sessions");
            update(&engine, ctx, "Sessions", true, "expiresAt").expect("enable");
            put(&engine, ctx, "Sessions", "old", Some(now - 10));
        }

        let registry = crate::AccessKeyRegistry::new()
            .bind("AKIAACME", TenantId::new("acme").unwrap())
            .bind("AKIAGLOBEX", TenantId::new("globex").unwrap());
        let (swept, errors) = sweep_all_tenants(&engine, &registry, now);
        assert_eq!(swept, 2, "one expired item reclaimed per tenant");
        assert!(errors.is_empty(), "no per-tenant errors: {errors:?}");
        assert!(!exists(&engine, &acme, "Sessions", "old"));
        assert!(!exists(&engine, &globex, "Sessions", "old"));
    }

    #[tokio::test]
    async fn provider_sweep_admits_tenant_before_sync_core() {
        let temp = tempfile::tempdir().expect("tempdir");
        let engine = Arc::new(
            Engine::new_with_memory_persistence(temp.path()).expect("memory provider engine"),
        );
        let tenant = TenantId::new("acme").expect("tenant");
        let registry = crate::AccessKeyRegistry::new().bind("AKIAACME", tenant);

        let (swept, errors) = sweep_all_tenants_async(&engine, &registry, 1_700_000_000).await;

        assert_eq!(swept, 0);
        assert!(
            errors.is_empty(),
            "provider-capable sweeps must admit each configured tenant before entering the synchronous command core: {errors:?}"
        );
        engine
            .ensure_tenant_exists(&TenantId::new("acme").expect("tenant"))
            .expect("the async sweep must leave the provider tenant registered");
    }

    #[test]
    fn ttl_removal_emits_service_user_identity_on_the_stream() {
        use extenddb_core::types::{
            DescribeStreamInput, GetRecordsInput, GetShardIteratorInput, ShardIteratorType,
            StreamEventName,
        };

        let (engine, ctx, _t) = fixture();
        // Stream-enabled (both images) + TTL-enabled table.
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "Sessions",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES" }
        }))
        .unwrap();
        let arn = control_plane::create_table(&engine, &ctx, input)
            .expect("create")
            .table_description
            .latest_stream_arn
            .expect("stream arn");
        update(&engine, &ctx, "Sessions", true, "expiresAt").expect("enable ttl");

        let now = 1_700_000_000;
        put(&engine, &ctx, "Sessions", "old", Some(now - 10));
        let swept = sweep_table(&engine, &ctx, "Sessions", now).expect("sweep");
        assert_eq!(swept, 1);

        // Read the stream back: the last record is the TTL REMOVE.
        let shard = stream::describe_stream(
            &engine,
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
            &engine,
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
            &engine,
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

    /// The TTL sweep deletes the item and emits its REMOVE on one atomic batch,
    /// so the two can never diverge: every deleted item yields exactly one
    /// REMOVE record (keyed to it), no surviving item yields one, and the record
    /// count tracks the delete count with no orphaned events or silent deletes.
    /// A regression to the former delete-then-capture path (two engine calls
    /// with a crash window) would break this joint invariant.
    #[test]
    fn sweep_commits_each_delete_and_its_remove_event_atomically() {
        use extenddb_core::types::{
            DescribeStreamInput, GetRecordsInput, GetShardIteratorInput, ShardIteratorType,
            StreamEventName,
        };

        let (engine, ctx, _t) = fixture();
        // Stream-enabled (both images) + TTL-enabled table.
        let input: CreateTableInput = serde_json::from_value(json!({
            "TableName": "Sessions",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES" }
        }))
        .unwrap();
        let arn = control_plane::create_table(&engine, &ctx, input)
            .expect("create")
            .table_description
            .latest_stream_arn
            .expect("stream arn");
        update(&engine, &ctx, "Sessions", true, "expiresAt").expect("enable ttl");

        let now = 1_700_000_000;
        let expired = ["a", "b", "c"];
        for pk in expired {
            put(&engine, &ctx, "Sessions", pk, Some(now - 10));
        }
        put(&engine, &ctx, "Sessions", "fresh1", Some(now + 10_000));
        put(&engine, &ctx, "Sessions", "fresh2", Some(now + 10_000));

        let swept = sweep_table(&engine, &ctx, "Sessions", now).expect("sweep");
        assert_eq!(swept, 3, "the three expired items are reclaimed");

        // Data side: every expired item is gone, every fresh item survives.
        for pk in expired {
            assert!(!exists(&engine, &ctx, "Sessions", pk), "{pk} was deleted");
        }
        assert!(exists(&engine, &ctx, "Sessions", "fresh1"), "fresh1 kept");
        assert!(exists(&engine, &ctx, "Sessions", "fresh2"), "fresh2 kept");

        // Stream side: read every record back from the shard.
        let shard = stream::describe_stream(
            &engine,
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
            &engine,
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
            &engine,
            &ctx,
            GetRecordsInput {
                shard_iterator: iterator,
                limit: None,
            },
        )
        .expect("get records")
        .records;

        let removes: Vec<_> = records
            .iter()
            .filter(|record| record.event_name == StreamEventName::Remove)
            .collect();
        assert_eq!(
            removes.len(),
            3,
            "exactly one REMOVE per deleted item — no orphaned events, no silent deletes"
        );

        // Each REMOVE is keyed to one of the deleted items (via its old image).
        let mut remove_keys: Vec<String> = removes
            .iter()
            .map(|record| {
                let old = record
                    .dynamodb
                    .old_image
                    .as_ref()
                    .expect("a REMOVE carries the deleted item as its old image");
                match old.get("pk").expect("old image has the pk attribute") {
                    AttributeValue::S(value) => value.clone(),
                    other => panic!("pk should be a string attribute, got {other:?}"),
                }
            })
            .collect();
        remove_keys.sort();
        assert_eq!(
            remove_keys,
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
            "the REMOVE events name exactly the deleted keys"
        );

        // Every REMOVE is TTL-attributed (service identity, not a tenant write).
        for record in &removes {
            let identity = record
                .user_identity
                .as_ref()
                .expect("a TTL REMOVE carries a userIdentity");
            assert_eq!(identity.identity_type, "Service");
            assert_eq!(identity.principal_id, "dynamodb.amazonaws.com");
        }
    }
}
