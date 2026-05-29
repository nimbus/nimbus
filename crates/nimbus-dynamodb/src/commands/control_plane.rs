//! DynamoDB control-plane handlers (table lifecycle) over the Nimbus `Service`.
//!
//! Each DynamoDB table's metadata (`TableDescription`) is persisted as one
//! document in a tenant-scoped catalog table (`_ddb_catalog`), keyed by the
//! table name. The data items themselves live in a Nimbus table named after the
//! DynamoDB table (created lazily on first write). Handlers are tenant-scoped via
//! the `TenantIsolationContext` resolved from the request's access key (D0.5).
//!
//! This module owns CreateTable / DescribeTable / DeleteTable; ListTables and
//! UpdateTable land next, and the dispatch/auth wiring that routes requests here
//! merges with D0.8.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use extenddb_core::error::DynamoDbError;
use extenddb_core::types::{
    AttributeDefinition, BillingMode, BillingModeSummary, CreateTableInput, CreateTableOutput,
    DeleteTableInput, DeleteTableOutput, DescribeTableInput, DescribeTableOutput, KeySchemaElement,
    KeyType, ProvisionedThroughputDescription, TableDescription, TableStatus,
};
use nimbus_core::{Document, DocumentId, TableName};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;
use serde_json::Value;

use crate::error::map_core_error;

/// Tenant-scoped table whose documents hold one `TableDescription` per DynamoDB
/// table. The `_ddb_` prefix is reserved (user table names with it are rejected).
const CATALOG_TABLE: &str = "_ddb_catalog";

/// CreateTable: validate, ensure the tenant, reject a duplicate, persist the
/// `TableDescription`, and return it.
pub fn create_table(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: CreateTableInput,
) -> Result<CreateTableOutput, DynamoDbError> {
    validate_table_name(&input.table_name)?;
    validate_key_schema(&input.key_schema)?;
    validate_key_attributes_defined(&input.key_schema, &input.attribute_definitions)?;

    crate::tenant::ensure_tenant(service, context)?;
    let id = catalog_id(&input.table_name)?;

    match service.get_document(context.tenant_id(), &catalog_table(), id.clone()) {
        Ok(_) => {
            return Err(DynamoDbError::ResourceInUseException(format!(
                "Table already exists: {}",
                input.table_name
            )));
        }
        Err(nimbus_core::Error::DocumentNotFound(_)) => {}
        Err(error) => return Err(map_core_error(error)),
    }

    let description = build_table_description(&input);
    let fields = description_to_fields(&description)?;
    service
        .insert_document_with_id(context.tenant_id(), catalog_table(), id, fields)
        .map_err(map_core_error)?;

    Ok(CreateTableOutput {
        table_description: description,
    })
}

/// DescribeTable: read the persisted `TableDescription`.
pub fn describe_table(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: DescribeTableInput,
) -> Result<DescribeTableOutput, DynamoDbError> {
    let description = load_description(service, context, &input.table_name)?;
    Ok(DescribeTableOutput { table: description })
}

/// DeleteTable: remove the catalog entry and report the deleted table with
/// `DELETING` status. (Bulk deletion of the table's data items via the
/// `deleting` lifecycle is refined in a later item — see the plan.)
pub fn delete_table(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: DeleteTableInput,
) -> Result<DeleteTableOutput, DynamoDbError> {
    let mut description = load_description(service, context, &input.table_name)?;
    let id = catalog_id(&input.table_name)?;
    service
        .delete_document(context.tenant_id(), catalog_table(), id)
        .map_err(map_core_error)?;
    description.table_status = TableStatus::Deleting;
    Ok(DeleteTableOutput {
        table_description: description,
    })
}

// -------- helpers --------

fn catalog_table() -> TableName {
    TableName::new(CATALOG_TABLE).expect("catalog table name is valid")
}

fn catalog_id(table_name: &str) -> Result<DocumentId, DynamoDbError> {
    DocumentId::from_key(table_name).map_err(map_core_error)
}

fn load_description(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<TableDescription, DynamoDbError> {
    let id = catalog_id(table_name)?;
    match service.get_document(context.tenant_id(), &catalog_table(), id) {
        Ok(document) => description_from_doc(&document),
        Err(nimbus_core::Error::DocumentNotFound(_)) => Err(resource_not_found(table_name)),
        Err(error) => Err(map_core_error(error)),
    }
}

fn resource_not_found(table_name: &str) -> DynamoDbError {
    DynamoDbError::ResourceNotFoundException(format!(
        "Requested resource not found: Table: {table_name} not found"
    ))
}

fn validate_table_name(name: &str) -> Result<(), DynamoDbError> {
    if name.starts_with("_ddb_") {
        return Err(DynamoDbError::ValidationException(format!(
            "Table name '{name}' uses the reserved '_ddb_' prefix"
        )));
    }
    if !(3..=255).contains(&name.len()) {
        return Err(DynamoDbError::ValidationException(
            "TableName must be between 3 and 255 characters long".to_owned(),
        ));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
    {
        return Err(DynamoDbError::ValidationException(
            "TableName may contain only a-z, A-Z, 0-9, '_', '-', and '.'".to_owned(),
        ));
    }
    Ok(())
}

fn validate_key_schema(key_schema: &[KeySchemaElement]) -> Result<(), DynamoDbError> {
    if key_schema.is_empty() || key_schema.len() > 2 {
        return Err(DynamoDbError::ValidationException(
            "KeySchema must contain exactly one HASH key and an optional RANGE key".to_owned(),
        ));
    }
    let hashes = key_schema
        .iter()
        .filter(|e| e.key_type == KeyType::Hash)
        .count();
    if hashes != 1 {
        return Err(DynamoDbError::ValidationException(
            "KeySchema must designate exactly one HASH (partition) key".to_owned(),
        ));
    }
    if key_schema.len() == 2
        && key_schema
            .iter()
            .filter(|e| e.key_type == KeyType::Range)
            .count()
            != 1
    {
        return Err(DynamoDbError::ValidationException(
            "A two-element KeySchema must have one HASH key and one RANGE key".to_owned(),
        ));
    }
    Ok(())
}

fn validate_key_attributes_defined(
    key_schema: &[KeySchemaElement],
    attribute_definitions: &[AttributeDefinition],
) -> Result<(), DynamoDbError> {
    for element in key_schema {
        if !attribute_definitions
            .iter()
            .any(|d| d.attribute_name == element.attribute_name)
        {
            return Err(DynamoDbError::ValidationException(format!(
                "KeySchema attribute '{}' has no matching AttributeDefinition",
                element.attribute_name
            )));
        }
    }
    Ok(())
}

fn build_table_description(input: &CreateTableInput) -> TableDescription {
    let billing_mode = input.billing_mode.unwrap_or(BillingMode::PayPerRequest);
    let (read_capacity_units, write_capacity_units) =
        match (&input.provisioned_throughput, billing_mode) {
            (Some(pt), BillingMode::Provisioned) => {
                (pt.read_capacity_units, pt.write_capacity_units)
            }
            _ => (0, 0),
        };

    TableDescription {
        table_name: input.table_name.clone(),
        key_schema: input.key_schema.clone(),
        attribute_definitions: input.attribute_definitions.clone(),
        table_status: TableStatus::Active,
        creation_date_time: now_epoch_seconds(),
        table_size_bytes: 0,
        item_count: 0,
        table_arn: format!(
            "arn:aws:dynamodb:ddblocal:000000000000:table/{}",
            input.table_name
        ),
        table_id: uuid::Uuid::new_v4().to_string(),
        provisioned_throughput: ProvisionedThroughputDescription {
            read_capacity_units,
            write_capacity_units,
            number_of_decreases_today: 0,
            last_increase_date_time: None,
            last_decrease_date_time: None,
        },
        billing_mode_summary: Some(BillingModeSummary {
            billing_mode,
            last_update_to_pay_per_request_date_time: None,
        }),
        global_secondary_indexes: None,
        local_secondary_indexes: None,
        stream_specification: input.stream_specification.clone(),
        latest_stream_arn: None,
        latest_stream_label: None,
        deletion_protection_enabled: input.deletion_protection_enabled.unwrap_or(false),
        sse_description: None,
        table_class_summary: None,
    }
}

fn now_epoch_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn description_to_fields(
    description: &TableDescription,
) -> Result<serde_json::Map<String, Value>, DynamoDbError> {
    match serde_json::to_value(description) {
        Ok(Value::Object(map)) => Ok(map),
        _ => Err(DynamoDbError::InternalServerError(
            "failed to serialize table description".to_owned(),
        )),
    }
}

fn description_from_doc(document: &Document) -> Result<TableDescription, DynamoDbError> {
    serde_json::from_value(Value::Object(document.fields.clone())).map_err(|error| {
        DynamoDbError::InternalServerError(format!("corrupt table metadata: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nimbus_core::TenantId;

    fn fixture() -> (Arc<Service>, TenantIsolationContext, tempfile::TempDir) {
        let temp = tempfile::tempdir().expect("tempdir");
        let service = Arc::new(Service::new(temp.path()).expect("service"));
        let context = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
        crate::tenant::ensure_tenant(&service, &context).expect("tenant");
        (service, context, temp)
    }

    fn input(name: &str, with_sort: bool) -> CreateTableInput {
        let mut key_schema = serde_json::json!([{ "AttributeName": "pk", "KeyType": "HASH" }]);
        let mut attrs = serde_json::json!([{ "AttributeName": "pk", "AttributeType": "S" }]);
        if with_sort {
            key_schema
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({ "AttributeName": "sk", "KeyType": "RANGE" }));
            attrs
                .as_array_mut()
                .unwrap()
                .push(serde_json::json!({ "AttributeName": "sk", "AttributeType": "N" }));
        }
        serde_json::from_value(serde_json::json!({
            "TableName": name,
            "KeySchema": key_schema,
            "AttributeDefinitions": attrs,
        }))
        .expect("valid CreateTableInput")
    }

    #[test]
    fn create_then_describe_roundtrips() {
        let (service, ctx, _t) = fixture();
        let created = create_table(&service, &ctx, input("orders", true)).expect("create");
        assert_eq!(created.table_description.table_name, "orders");
        assert_eq!(created.table_description.table_status, TableStatus::Active);
        assert_eq!(created.table_description.key_schema.len(), 2);

        let described = describe_table(
            &service,
            &ctx,
            DescribeTableInput {
                table_name: "orders".to_owned(),
            },
        )
        .expect("describe");
        assert_eq!(described.table.table_name, "orders");
        assert_eq!(described.table.table_status, TableStatus::Active);
        assert_eq!(
            described.table.table_id, created.table_description.table_id,
            "describe must return the same persisted metadata"
        );
    }

    #[test]
    fn duplicate_create_is_resource_in_use() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, input("orders", false)).expect("first create");
        let err = create_table(&service, &ctx, input("orders", false)).unwrap_err();
        assert!(matches!(err, DynamoDbError::ResourceInUseException(_)));
    }

    #[test]
    fn describe_missing_is_resource_not_found() {
        let (service, ctx, _t) = fixture();
        let err = describe_table(
            &service,
            &ctx,
            DescribeTableInput {
                table_name: "ghost".to_owned(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, DynamoDbError::ResourceNotFoundException(_)));
    }

    #[test]
    fn delete_removes_the_table() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, input("orders", false)).expect("create");
        let deleted = delete_table(
            &service,
            &ctx,
            DeleteTableInput {
                table_name: "orders".to_owned(),
            },
        )
        .expect("delete");
        assert_eq!(
            deleted.table_description.table_status,
            TableStatus::Deleting
        );
        // Subsequent describe is ResourceNotFoundException.
        assert!(matches!(
            describe_table(
                &service,
                &ctx,
                DescribeTableInput {
                    table_name: "orders".to_owned()
                }
            ),
            Err(DynamoDbError::ResourceNotFoundException(_))
        ));
    }

    #[test]
    fn reserved_prefix_and_bad_key_schema_rejected() {
        let (service, ctx, _t) = fixture();
        assert!(matches!(
            create_table(&service, &ctx, input("_ddb_secret", false)),
            Err(DynamoDbError::ValidationException(_))
        ));
        // Empty key schema.
        let bad: CreateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "nokey",
            "KeySchema": [],
            "AttributeDefinitions": [],
        }))
        .unwrap();
        assert!(matches!(
            create_table(&service, &ctx, bad),
            Err(DynamoDbError::ValidationException(_))
        ));
    }

    #[test]
    fn tenants_are_isolated() {
        let temp = tempfile::tempdir().expect("tempdir");
        let service = Arc::new(Service::new(temp.path()).expect("service"));
        let acme = crate::tenant::tenant_context(TenantId::new("acme").unwrap(), "test");
        let globex = crate::tenant::tenant_context(TenantId::new("globex").unwrap(), "test");
        crate::tenant::ensure_tenant(&service, &acme).unwrap();
        crate::tenant::ensure_tenant(&service, &globex).unwrap();

        create_table(&service, &acme, input("orders", false)).expect("acme create");
        // globex must not see acme's table.
        assert!(matches!(
            describe_table(
                &service,
                &globex,
                DescribeTableInput {
                    table_name: "orders".to_owned()
                }
            ),
            Err(DynamoDbError::ResourceNotFoundException(_))
        ));
    }
}
