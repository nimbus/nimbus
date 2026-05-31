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
    DeleteTableInput, DeleteTableOutput, DescribeTableInput, DescribeTableOutput,
    GlobalSecondaryIndexUpdate, GsiDescription, KeySchemaElement, KeyType, ListTablesInput,
    ListTablesOutput, LsiDescription, ProvisionedThroughputDescription, TableDescription,
    TableStatus, UpdateTableInput, UpdateTableOutput,
};
use nimbus_core::{Document, DocumentId, StructuredQuery, TableName, WritePrecondition};
use nimbus_engine::Service;
use nimbus_tenant::TenantIsolationContext;
use serde_json::Value;

use crate::commands::{item, stream, tag, ttl};
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
    validate_secondary_indexes(&input)?;

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
    // Reclaim the table's data items first (bulk delete over the shared
    // `documents` table — the `deleting` lifecycle, not a physical DROP TABLE),
    // so a failure here leaves the table describable and the delete retryable.
    reclaim_table_items(service, context, &input.table_name)?;
    // Reclaim the table's sidecar state so a table later recreated under the
    // same name starts clean — stream events + the `_ddb_streamseq_` high-water
    // counter, the `_ddb_ttl` config doc, and the `_ddb_tags` entries (F4).
    // Otherwise a recreated table would inherit a stale stream sequence and
    // orphaned TTL/tag metadata.
    stream::reclaim_for_table(service, context, &input.table_name)?;
    ttl::reclaim_for_table(service, context, &input.table_name)?;
    tag::reclaim_for_table(service, context, &input.table_name)?;
    let id = catalog_id(&input.table_name)?;
    service
        .delete_document(context.tenant_id(), catalog_table(), id)
        .map_err(map_core_error)?;
    description.table_status = TableStatus::Deleting;
    Ok(DeleteTableOutput {
        table_description: description,
    })
}

/// ListTables: enumerate the tenant's tables (catalog doc ids), sorted, with
/// `ExclusiveStartTableName`/`Limit` pagination (Limit clamped to 1..=100).
pub fn list_tables(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: ListTablesInput,
) -> Result<ListTablesOutput, DynamoDbError> {
    let documents = match service.query_documents_structured(
        context.tenant_id(),
        &catalog_table(),
        &StructuredQuery::default(),
    ) {
        Ok(documents) => documents,
        // No tables created yet → the catalog table does not exist.
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            Vec::new()
        }
        Err(error) => return Err(map_core_error(error)),
    };

    let mut names: Vec<String> = documents
        .iter()
        .map(|document| document.id.as_str().to_owned())
        .collect();
    names.sort();

    if let Some(start) = input.exclusive_start_table_name.as_deref() {
        names.retain(|name| name.as_str() > start);
    }

    let limit = input.limit.unwrap_or(100).clamp(1, 100) as usize;
    let truncated = names.len() > limit;
    names.truncate(limit);
    let last_evaluated_table_name = truncated.then(|| names.last().cloned()).flatten();

    Ok(ListTablesOutput {
        table_names: names,
        last_evaluated_table_name,
    })
}

/// UpdateTable: apply the supported in-place changes (billing mode, deletion
/// protection, stream specification) and re-persist. GSI updates are deferred to
/// D4 and rejected for now.
pub fn update_table(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    input: UpdateTableInput,
) -> Result<UpdateTableOutput, DynamoDbError> {
    let mut description = load_description(service, context, &input.table_name)?;

    // Merge any new attribute definitions (a new GSI may key on new attributes).
    if let Some(new_defs) = &input.attribute_definitions {
        for def in new_defs {
            if !description
                .attribute_definitions
                .iter()
                .any(|existing| existing.attribute_name == def.attribute_name)
            {
                description.attribute_definitions.push(def.clone());
            }
        }
    }

    // Apply GSI Create/Update/Delete actions.
    if let Some(updates) = &input.global_secondary_index_updates {
        apply_gsi_updates(&mut description, updates)?;
    }

    if let Some(billing_mode) = input.billing_mode {
        description.billing_mode_summary = Some(BillingModeSummary {
            billing_mode,
            last_update_to_pay_per_request_date_time: None,
        });
        if billing_mode == BillingMode::PayPerRequest {
            description.provisioned_throughput.read_capacity_units = 0;
            description.provisioned_throughput.write_capacity_units = 0;
        } else if let Some(throughput) = &input.provisioned_throughput {
            description.provisioned_throughput.read_capacity_units = throughput.read_capacity_units;
            description.provisioned_throughput.write_capacity_units =
                throughput.write_capacity_units;
        }
    }
    if let Some(enabled) = input.deletion_protection_enabled {
        description.deletion_protection_enabled = enabled;
    }
    if let Some(stream) = input.stream_specification.clone() {
        if stream.stream_enabled {
            let label = description.table_id.clone();
            description.latest_stream_arn =
                Some(format!("{}/stream/{label}", description.table_arn));
            description.latest_stream_label = Some(label);
        } else {
            description.latest_stream_arn = None;
            description.latest_stream_label = None;
        }
        description.stream_specification = Some(stream);
    }

    // Re-persist the catalog doc as a single atomic full replace. A field-merge
    // (`update_document`) cannot *remove* fields, so clearing the last GSI
    // (`GlobalSecondaryIndexes` → absent) needs a wholesale rewrite; routing it
    // through the atomic Overwrite primitive makes that rewrite a single storage
    // transaction instead of a delete-then-insert with a crash window (F2).
    let fields = description_to_fields(&description)?;
    let id = catalog_id(&input.table_name)?;
    item::atomic_overwrite(
        service,
        context,
        catalog_table(),
        id,
        fields,
        WritePrecondition::default(),
    )
    .map_err(map_core_error)?;

    Ok(UpdateTableOutput {
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

/// Load a table's key schema (HASH + optional RANGE elements) from the catalog,
/// for the item handlers' primary-key extraction.
///
/// # Errors
/// `ResourceNotFoundException` if the table does not exist.
pub fn load_key_schema(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<Vec<KeySchemaElement>, DynamoDbError> {
    Ok(load_description(service, context, table_name)?.key_schema)
}

/// The key schema + projected-attribute set a Query/Scan reads through. For the
/// base table, every attribute is available (`projected_attributes` is `None`);
/// for an index, the attributes projected into it (KEYS_ONLY/INCLUDE/ALL).
pub struct IndexQueryShape {
    /// The key schema the read keys/orders on — the base table's, or the index's.
    pub key_schema: Vec<KeySchemaElement>,
    /// The base table's primary-key schema (the physical storage key, used to
    /// derive each item's `DocumentId` for Scan ordering/pagination).
    pub table_key_schema: Vec<KeySchemaElement>,
    /// `None` = all attributes available (base table or `ALL` projection);
    /// `Some(set)` = only these attribute names are available from the index.
    pub projected_attributes: Option<std::collections::BTreeSet<String>>,
}

/// Resolve the Query/Scan shape for the base table (`index_name` = `None`) or a
/// named LSI/GSI.
///
/// # Errors
/// `ResourceNotFoundException` if the table is absent; `ValidationException` if
/// the named index does not exist on the table.
pub fn load_index_query_shape(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
    index_name: Option<&str>,
) -> Result<IndexQueryShape, DynamoDbError> {
    let description = load_description(service, context, table_name)?;
    let table_key_schema = description.key_schema.clone();
    let Some(name) = index_name else {
        return Ok(IndexQueryShape {
            key_schema: description.key_schema,
            table_key_schema,
            projected_attributes: None,
        });
    };
    // Table key attributes are always projected into every index.
    let table_keys: std::collections::BTreeSet<String> = table_key_schema
        .iter()
        .map(|element| element.attribute_name.clone())
        .collect();
    if let Some(lsi) = description
        .local_secondary_indexes
        .as_ref()
        .and_then(|indexes| indexes.iter().find(|index| index.index_name == name))
    {
        return Ok(IndexQueryShape {
            projected_attributes: projected_set(&lsi.projection, &table_keys, &lsi.key_schema),
            key_schema: lsi.key_schema.clone(),
            table_key_schema,
        });
    }
    if let Some(gsi) = description
        .global_secondary_indexes
        .as_ref()
        .and_then(|indexes| indexes.iter().find(|index| index.index_name == name))
    {
        return Ok(IndexQueryShape {
            projected_attributes: projected_set(&gsi.projection, &table_keys, &gsi.key_schema),
            key_schema: gsi.key_schema.clone(),
            table_key_schema,
        });
    }
    Err(DynamoDbError::ValidationException(format!(
        "The table does not have the specified index: {name}"
    )))
}

/// The attribute names available from an index given its projection. `ALL` →
/// `None` (no restriction); KEYS_ONLY → table keys ∪ index keys; INCLUDE → those
/// ∪ the declared non-key attributes.
fn projected_set(
    projection: &extenddb_core::types::Projection,
    table_keys: &std::collections::BTreeSet<String>,
    index_key_schema: &[KeySchemaElement],
) -> Option<std::collections::BTreeSet<String>> {
    use extenddb_core::types::ProjectionType;
    match projection.projection_type {
        ProjectionType::All => None,
        ProjectionType::KeysOnly | ProjectionType::Include => {
            let mut set = table_keys.clone();
            set.extend(
                index_key_schema
                    .iter()
                    .map(|element| element.attribute_name.clone()),
            );
            if projection.projection_type == ProjectionType::Include
                && let Some(non_key) = &projection.non_key_attributes
            {
                set.extend(non_key.iter().cloned());
            }
            Some(set)
        }
    }
}

/// Load a table's full `TableDescription` from the catalog (tenant-scoped).
/// Public so Streams (D5) can resolve a table from its stream ARN.
///
/// # Errors
/// `ResourceNotFoundException` if the table does not exist.
pub fn load_table_description(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<TableDescription, DynamoDbError> {
    load_description(service, context, table_name)
}

/// Enumerate every table's `TableDescription` from the catalog (tenant-scoped).
/// Public so Streams (D5.5 ListStreams) can find stream-enabled tables.
///
/// # Errors
/// A mapped engine error if the catalog cannot be read or a record is corrupt.
pub fn list_table_descriptions(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
) -> Result<Vec<TableDescription>, DynamoDbError> {
    let documents = match service.query_documents_structured(
        context.tenant_id(),
        &catalog_table(),
        &StructuredQuery::default(),
    ) {
        Ok(documents) => documents,
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            return Ok(Vec::new());
        }
        Err(error) => return Err(map_core_error(error)),
    };
    documents.iter().map(description_from_doc).collect()
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

/// Reclaim every data item stored under `table_name` (a Nimbus table named after
/// the DynamoDB table) by deleting each document. This is the shared-`documents`
/// bulk delete the storage-layout decision mandates instead of a physical DROP
/// TABLE; the same `TableName` D1's item writes target. A data table that was
/// never materialized (no writes) reclaims nothing. Returns the count reclaimed.
fn reclaim_table_items(
    service: &Arc<Service>,
    context: &TenantIsolationContext,
    table_name: &str,
) -> Result<usize, DynamoDbError> {
    let table = TableName::new(table_name).map_err(map_core_error)?;
    let documents = match service.query_documents_structured(
        context.tenant_id(),
        &table,
        &StructuredQuery::default(),
    ) {
        Ok(documents) => documents,
        // The data table may never have been materialized (no items written).
        Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => {
            return Ok(0);
        }
        Err(error) => return Err(map_core_error(error)),
    };
    let mut reclaimed = 0;
    for document in documents {
        service
            .delete_document(context.tenant_id(), table.clone(), document.id)
            .map_err(map_core_error)?;
        reclaimed += 1;
    }
    Ok(reclaimed)
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

/// Validate secondary indexes declared at CreateTable. Each index's key schema
/// must be well-formed with its key attributes defined; an LSI must share the
/// table's partition key and declare a sort key (DynamoDB's LSI rules).
fn validate_secondary_indexes(input: &CreateTableInput) -> Result<(), DynamoDbError> {
    let table_hash = input
        .key_schema
        .iter()
        .find(|element| element.key_type == KeyType::Hash)
        .map(|element| element.attribute_name.as_str());

    for lsi in input.local_secondary_indexes.iter().flatten() {
        validate_key_schema(&lsi.key_schema)?;
        validate_key_attributes_defined(&lsi.key_schema, &input.attribute_definitions)?;
        let lsi_hash = lsi
            .key_schema
            .iter()
            .find(|element| element.key_type == KeyType::Hash)
            .map(|element| element.attribute_name.as_str());
        if lsi_hash != table_hash {
            return Err(DynamoDbError::ValidationException(format!(
                "Local Secondary Index '{}' partition key must match the table partition key",
                lsi.index_name
            )));
        }
        if !lsi
            .key_schema
            .iter()
            .any(|element| element.key_type == KeyType::Range)
        {
            return Err(DynamoDbError::ValidationException(format!(
                "Local Secondary Index '{}' must specify a sort (RANGE) key",
                lsi.index_name
            )));
        }
    }

    for gsi in input.global_secondary_indexes.iter().flatten() {
        validate_key_schema(&gsi.key_schema)?;
        validate_key_attributes_defined(&gsi.key_schema, &input.attribute_definitions)?;
    }
    Ok(())
}

/// Apply `GlobalSecondaryIndexUpdates` (Create/Update/Delete) to the table
/// description. GSIs become `ACTIVE` immediately (DDB-DIV-004).
///
/// # Errors
/// `ValidationException` for a malformed action, a Create whose key schema is
/// invalid or keys an undefined attribute, a Create of an existing index, or an
/// Update/Delete of a missing index.
fn apply_gsi_updates(
    description: &mut TableDescription,
    updates: &[GlobalSecondaryIndexUpdate],
) -> Result<(), DynamoDbError> {
    for update in updates {
        match (&update.create, &update.update, &update.delete) {
            (Some(create), None, None) => {
                validate_key_schema(&create.key_schema)?;
                validate_key_attributes_defined(
                    &create.key_schema,
                    &description.attribute_definitions,
                )?;
                let arn = format!("{}/index/{}", description.table_arn, create.index_name);
                let gsis = description
                    .global_secondary_indexes
                    .get_or_insert_with(Vec::new);
                if gsis.iter().any(|gsi| gsi.index_name == create.index_name) {
                    return Err(DynamoDbError::ValidationException(format!(
                        "Attempting to create an index which already exists: {}",
                        create.index_name
                    )));
                }
                gsis.push(GsiDescription {
                    index_name: create.index_name.clone(),
                    key_schema: create.key_schema.clone(),
                    projection: create.projection.clone(),
                    index_status: "ACTIVE".to_owned(),
                    provisioned_throughput: None,
                    index_size_bytes: 0,
                    item_count: 0,
                    index_arn: arn,
                });
            }
            (None, Some(action), None) => {
                let exists = description
                    .global_secondary_indexes
                    .as_ref()
                    .is_some_and(|gsis| gsis.iter().any(|gsi| gsi.index_name == action.index_name));
                if !exists {
                    return Err(index_not_found(&action.index_name));
                }
                // UpdateGsiAction only carries the index name (throughput is the
                // sole settable field and is not metered here) — no-op.
            }
            (None, None, Some(action)) => {
                let gsis = description
                    .global_secondary_indexes
                    .get_or_insert_with(Vec::new);
                let before = gsis.len();
                gsis.retain(|gsi| gsi.index_name != action.index_name);
                if gsis.len() == before {
                    return Err(index_not_found(&action.index_name));
                }
                if gsis.is_empty() {
                    description.global_secondary_indexes = None;
                }
            }
            _ => {
                return Err(DynamoDbError::ValidationException(
                    "Each GlobalSecondaryIndexUpdate must contain exactly one of Create, Update, \
                     or Delete"
                        .to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn index_not_found(index_name: &str) -> DynamoDbError {
    DynamoDbError::ValidationException(format!(
        "The table does not have the specified index: {index_name}"
    ))
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

    let table_arn = format!(
        "arn:aws:dynamodb:ddblocal:000000000000:table/{}",
        input.table_name
    );
    let index_arn = |index_name: &str| format!("{table_arn}/index/{index_name}");
    let table_id = uuid::Uuid::new_v4().to_string();

    // When a stream is enabled, the table reports its LatestStreamArn/Label
    // (the label is the stable stream id; here, the table id).
    let stream_enabled = input
        .stream_specification
        .as_ref()
        .is_some_and(|spec| spec.stream_enabled);
    let latest_stream_label = stream_enabled.then(|| table_id.clone());
    let latest_stream_arn = latest_stream_label
        .as_ref()
        .map(|label| format!("{table_arn}/stream/{label}"));

    // Secondary indexes declared at CreateTable. LSIs are immutable; GSIs become
    // ACTIVE immediately (see DDB-DIV-004 — no async CREATING phase).
    let local_secondary_indexes = input.local_secondary_indexes.as_ref().map(|indexes| {
        indexes
            .iter()
            .map(|lsi| LsiDescription {
                index_name: lsi.index_name.clone(),
                key_schema: lsi.key_schema.clone(),
                projection: lsi.projection.clone(),
                index_size_bytes: 0,
                item_count: 0,
                index_arn: index_arn(&lsi.index_name),
            })
            .collect()
    });
    let global_secondary_indexes = input.global_secondary_indexes.as_ref().map(|indexes| {
        indexes
            .iter()
            .map(|gsi| GsiDescription {
                index_name: gsi.index_name.clone(),
                key_schema: gsi.key_schema.clone(),
                projection: gsi.projection.clone(),
                index_status: "ACTIVE".to_owned(),
                provisioned_throughput: None,
                index_size_bytes: 0,
                item_count: 0,
                index_arn: index_arn(&gsi.index_name),
            })
            .collect()
    });

    TableDescription {
        table_name: input.table_name.clone(),
        key_schema: input.key_schema.clone(),
        attribute_definitions: input.attribute_definitions.clone(),
        table_status: TableStatus::Active,
        creation_date_time: now_epoch_seconds(),
        table_size_bytes: 0,
        item_count: 0,
        table_arn,
        table_id,
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
        global_secondary_indexes,
        local_secondary_indexes,
        stream_specification: input.stream_specification.clone(),
        latest_stream_arn,
        latest_stream_label,
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
    fn create_with_stream_records_spec_and_arn() {
        let (service, ctx, _t) = fixture();
        let input: CreateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "events",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_AND_OLD_IMAGES" }
        }))
        .unwrap();
        let created = create_table(&service, &ctx, input).expect("create with stream");
        let desc = &created.table_description;
        let spec = desc.stream_specification.as_ref().expect("stream spec");
        assert!(spec.stream_enabled);
        assert_eq!(
            spec.stream_view_type,
            Some(extenddb_core::types::StreamViewType::NewAndOldImages)
        );
        let arn = desc.latest_stream_arn.as_ref().expect("stream arn");
        assert!(arn.contains("/stream/"), "stream ARN: {arn}");
        assert!(desc.latest_stream_label.is_some());
    }

    #[test]
    fn update_table_enables_then_disables_stream() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, input("orders", false)).expect("create");
        // Enable a stream.
        let enable: UpdateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "orders",
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "KEYS_ONLY" }
        }))
        .unwrap();
        let enabled = update_table(&service, &ctx, enable).expect("enable stream");
        assert!(enabled.table_description.latest_stream_arn.is_some());
        // Disable it.
        let disable: UpdateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "orders",
            "StreamSpecification": { "StreamEnabled": false }
        }))
        .unwrap();
        let disabled = update_table(&service, &ctx, disable).expect("disable stream");
        assert!(
            disabled.table_description.latest_stream_arn.is_none(),
            "disabling clears the stream ARN"
        );
    }

    #[test]
    fn create_with_lsi_persists_and_describes() {
        let (service, ctx, _t) = fixture();
        let input: CreateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "tasks",
            "KeySchema": [
                { "AttributeName": "pk", "KeyType": "HASH" },
                { "AttributeName": "sk", "KeyType": "RANGE" }
            ],
            "AttributeDefinitions": [
                { "AttributeName": "pk", "AttributeType": "S" },
                { "AttributeName": "sk", "AttributeType": "N" },
                { "AttributeName": "prio", "AttributeType": "N" }
            ],
            "LocalSecondaryIndexes": [{
                "IndexName": "by_priority",
                "KeySchema": [
                    { "AttributeName": "pk", "KeyType": "HASH" },
                    { "AttributeName": "prio", "KeyType": "RANGE" }
                ],
                "Projection": { "ProjectionType": "KEYS_ONLY" }
            }]
        }))
        .unwrap();
        let created = create_table(&service, &ctx, input).expect("create with LSI");
        let lsis = created
            .table_description
            .local_secondary_indexes
            .expect("LSIs present");
        assert_eq!(lsis.len(), 1);
        assert_eq!(lsis[0].index_name, "by_priority");
        assert!(lsis[0].index_arn.ends_with("/index/by_priority"));

        // DescribeTable returns the LSI too.
        let described = describe_table(
            &service,
            &ctx,
            DescribeTableInput {
                table_name: "tasks".to_owned(),
            },
        )
        .expect("describe");
        assert_eq!(
            described
                .table
                .local_secondary_indexes
                .expect("LSIs in describe")
                .len(),
            1
        );
    }

    #[test]
    fn create_lsi_with_mismatched_partition_key_is_rejected() {
        let (service, ctx, _t) = fixture();
        let input: CreateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "tasks",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [
                { "AttributeName": "pk", "AttributeType": "S" },
                { "AttributeName": "other", "AttributeType": "S" },
                { "AttributeName": "prio", "AttributeType": "N" }
            ],
            "LocalSecondaryIndexes": [{
                "IndexName": "bad",
                "KeySchema": [
                    { "AttributeName": "other", "KeyType": "HASH" },
                    { "AttributeName": "prio", "KeyType": "RANGE" }
                ],
                "Projection": { "ProjectionType": "ALL" }
            }]
        }))
        .unwrap();
        assert!(matches!(
            create_table(&service, &ctx, input),
            Err(DynamoDbError::ValidationException(_))
        ));
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
    fn delete_table_reclaims_data_items() {
        // DeleteTable must reclaim the table's data rows (shared-`documents`
        // bulk delete), not just drop the catalog entry. Seed items directly via
        // the engine (the same `TableName` D1's PutItem will write to), delete,
        // and assert the rows are gone and the table is no longer describable.
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, input("orders", false)).expect("create");
        let table = TableName::new("orders").unwrap();
        for key in ["item-1", "item-2", "item-3"] {
            let mut fields = serde_json::Map::new();
            fields.insert("v".to_owned(), Value::String(key.to_owned()));
            service
                .insert_document_with_id(
                    ctx.tenant_id(),
                    table.clone(),
                    DocumentId::from_key(key).unwrap(),
                    fields,
                )
                .expect("seed data item");
        }
        assert_eq!(count_items(&service, &ctx, "orders"), 3);

        delete_table(
            &service,
            &ctx,
            DeleteTableInput {
                table_name: "orders".to_owned(),
            },
        )
        .expect("delete");

        assert_eq!(
            count_items(&service, &ctx, "orders"),
            0,
            "DeleteTable must reclaim every data item"
        );
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
    fn delete_table_reclaims_sidecars_so_recreate_starts_fresh() {
        // F4: DeleteTable must drop the table's stream events, the
        // `_ddb_streamseq_` high-water counter, the `_ddb_ttl` config doc, and
        // the `_ddb_tags` entry — so a table recreated under the same name does
        // not inherit a stale stream sequence or orphaned TTL/tag metadata.
        let (service, ctx, _t) = fixture();
        let streamed: CreateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "events",
            "KeySchema": [{ "AttributeName": "pk", "KeyType": "HASH" }],
            "AttributeDefinitions": [{ "AttributeName": "pk", "AttributeType": "S" }],
            "StreamSpecification": { "StreamEnabled": true, "StreamViewType": "NEW_IMAGE" }
        }))
        .unwrap();
        create_table(&service, &ctx, streamed.clone()).expect("create");

        // Writes capture stream events and advance the sequence counter.
        for pk in ["a", "b", "c"] {
            crate::commands::item::put_item(
                &service,
                &ctx,
                serde_json::from_value(
                    serde_json::json!({ "TableName": "events", "Item": { "pk": {"S": pk} } }),
                )
                .unwrap(),
            )
            .expect("put");
        }
        // Seed TTL + tag sidecar docs (keyed by table name, as their stores are).
        for sidecar in ["_ddb_ttl", "_ddb_tags"] {
            let mut fields = serde_json::Map::new();
            fields.insert("seeded".to_owned(), Value::Bool(true));
            service
                .insert_document_with_id(
                    ctx.tenant_id(),
                    TableName::new(sidecar).unwrap(),
                    DocumentId::from_key("events").unwrap(),
                    fields,
                )
                .expect("seed sidecar");
        }
        assert_eq!(
            stream::next_sequence_value(&service, &ctx, "events").unwrap(),
            3,
            "counter advanced by the three writes"
        );
        assert_eq!(count_items(&service, &ctx, "_ddb_stream_events"), 3);

        delete_table(
            &service,
            &ctx,
            DeleteTableInput {
                table_name: "events".to_owned(),
            },
        )
        .expect("delete");

        // Every sidecar is reclaimed.
        assert_eq!(
            count_items(&service, &ctx, "_ddb_stream_events"),
            0,
            "stream events reclaimed"
        );
        assert_eq!(
            stream::next_sequence_value(&service, &ctx, "events").unwrap(),
            0,
            "sequence high-water counter reclaimed"
        );
        assert_eq!(
            count_items(&service, &ctx, "_ddb_ttl"),
            0,
            "TTL config reclaimed"
        );
        assert_eq!(
            count_items(&service, &ctx, "_ddb_tags"),
            0,
            "tags reclaimed"
        );

        // Recreate under the same name → a fresh stream starting at sequence 0.
        create_table(&service, &ctx, streamed).expect("recreate");
        crate::commands::item::put_item(
            &service,
            &ctx,
            serde_json::from_value(
                serde_json::json!({ "TableName": "events", "Item": { "pk": {"S": "fresh"} } }),
            )
            .unwrap(),
        )
        .expect("put after recreate");
        assert_eq!(
            stream::next_sequence_value(&service, &ctx, "events").unwrap(),
            1,
            "the recreated stream restarted at 0 (now 1 after one write), not the stale mark"
        );
    }

    fn count_items(service: &Arc<Service>, ctx: &TenantIsolationContext, table: &str) -> usize {
        match service.query_documents_structured(
            ctx.tenant_id(),
            &TableName::new(table).unwrap(),
            &StructuredQuery::default(),
        ) {
            Ok(docs) => docs.len(),
            Err(nimbus_core::Error::NotFound(_) | nimbus_core::Error::DocumentNotFound(_)) => 0,
            Err(error) => panic!("query failed: {error:?}"),
        }
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
        // ListTables is likewise tenant-scoped.
        let globex_list = list_tables(
            &service,
            &globex,
            ListTablesInput {
                limit: None,
                exclusive_start_table_name: None,
            },
        )
        .expect("globex list");
        assert!(globex_list.table_names.is_empty());
    }

    fn list_input(limit: Option<i32>, start: Option<&str>) -> ListTablesInput {
        ListTablesInput {
            limit,
            exclusive_start_table_name: start.map(str::to_owned),
        }
    }

    #[test]
    fn list_tables_is_sorted_and_paginates() {
        let (service, ctx, _t) = fixture();
        for name in ["orders", "users", "events"] {
            create_table(&service, &ctx, input(name, false)).expect("create");
        }

        // Empty start, no limit → all three, sorted.
        let all = list_tables(&service, &ctx, list_input(None, None)).expect("list");
        assert_eq!(all.table_names, vec!["events", "orders", "users"]);
        assert!(all.last_evaluated_table_name.is_none());

        // Limit 2 → first page + LastEvaluatedTableName.
        let page1 = list_tables(&service, &ctx, list_input(Some(2), None)).expect("page1");
        assert_eq!(page1.table_names, vec!["events", "orders"]);
        assert_eq!(page1.last_evaluated_table_name.as_deref(), Some("orders"));

        // Continue from the cursor → remaining page, no further cursor.
        let page2 =
            list_tables(&service, &ctx, list_input(Some(2), Some("orders"))).expect("page2");
        assert_eq!(page2.table_names, vec!["users"]);
        assert!(page2.last_evaluated_table_name.is_none());
    }

    #[test]
    fn list_tables_empty_when_none_created() {
        let (service, ctx, _t) = fixture();
        let listed = list_tables(&service, &ctx, list_input(None, None)).expect("list");
        assert!(listed.table_names.is_empty());
    }

    #[test]
    fn update_table_sets_deletion_protection_and_persists() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, input("orders", false)).expect("create");

        let updated = update_table(
            &service,
            &ctx,
            UpdateTableInput {
                table_name: "orders".to_owned(),
                billing_mode: None,
                provisioned_throughput: None,
                deletion_protection_enabled: Some(true),
                global_secondary_index_updates: None,
                attribute_definitions: None,
                stream_specification: None,
            },
        )
        .expect("update");
        assert!(updated.table_description.deletion_protection_enabled);

        // The change is persisted (visible on a fresh describe).
        let described = describe_table(
            &service,
            &ctx,
            DescribeTableInput {
                table_name: "orders".to_owned(),
            },
        )
        .expect("describe");
        assert!(described.table.deletion_protection_enabled);
    }

    #[test]
    fn update_table_gsi_create_update_delete() {
        let (service, ctx, _t) = fixture();
        create_table(&service, &ctx, input("orders", false)).expect("create");

        // Create a GSI (its key attribute `gsk` is supplied via AttributeDefinitions).
        let create: UpdateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "orders",
            "AttributeDefinitions": [{ "AttributeName": "gsk", "AttributeType": "S" }],
            "GlobalSecondaryIndexUpdates": [{
                "Create": {
                    "IndexName": "by_gsk",
                    "KeySchema": [{ "AttributeName": "gsk", "KeyType": "HASH" }],
                    "Projection": { "ProjectionType": "ALL" }
                }
            }],
        }))
        .unwrap();
        let after_create = update_table(&service, &ctx, create).expect("create gsi");
        let gsis = after_create
            .table_description
            .global_secondary_indexes
            .expect("gsi present");
        assert_eq!(gsis.len(), 1);
        assert_eq!(gsis[0].index_name, "by_gsk");
        assert_eq!(gsis[0].index_status, "ACTIVE");

        // Creating the same index again is rejected.
        let dup: UpdateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "orders",
            "AttributeDefinitions": [{ "AttributeName": "gsk", "AttributeType": "S" }],
            "GlobalSecondaryIndexUpdates": [{
                "Create": {
                    "IndexName": "by_gsk",
                    "KeySchema": [{ "AttributeName": "gsk", "KeyType": "HASH" }],
                    "Projection": { "ProjectionType": "ALL" }
                }
            }],
        }))
        .unwrap();
        assert!(matches!(
            update_table(&service, &ctx, dup),
            Err(DynamoDbError::ValidationException(_))
        ));

        // Delete the GSI.
        let delete: UpdateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "orders",
            "GlobalSecondaryIndexUpdates": [{ "Delete": { "IndexName": "by_gsk" } }],
        }))
        .unwrap();
        let after_delete = update_table(&service, &ctx, delete).expect("delete gsi");
        assert!(
            after_delete
                .table_description
                .global_secondary_indexes
                .is_none(),
            "last GSI removed"
        );

        // Deleting a missing index is rejected.
        let missing: UpdateTableInput = serde_json::from_value(serde_json::json!({
            "TableName": "orders",
            "GlobalSecondaryIndexUpdates": [{ "Delete": { "IndexName": "ghost" } }],
        }))
        .unwrap();
        assert!(matches!(
            update_table(&service, &ctx, missing),
            Err(DynamoDbError::ValidationException(_))
        ));
    }
}
