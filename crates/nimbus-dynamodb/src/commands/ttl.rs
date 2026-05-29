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
    DescribeTimeToLiveInput, DescribeTimeToLiveOutput, TimeToLiveDescription,
    TimeToLiveSpecificationOutput, TimeToLiveStatus, UpdateTimeToLiveInput, UpdateTimeToLiveOutput,
};
use nimbus_core::{DocumentId, TableName};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;
use serde_json::{Map, Value};

use crate::commands::control_plane;
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
}
