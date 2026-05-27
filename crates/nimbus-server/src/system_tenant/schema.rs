use nimbus_core::{FieldSchema, FieldType, IndexDefinition, Result, TableName, TableSchema};

pub(crate) fn system_table_schemas() -> Result<Vec<TableSchema>> {
    Ok(vec![
        table(
            "machines",
            &[
                string("name", true),
                string("kind", true),
                string("state", true),
                string("provider", true),
                object("resources", false),
                object("meta", false),
            ],
            &[
                index("by_name", &["name"]),
                index("by_state", &["state"]),
                index("by_provider", &["provider"]),
            ],
        )?,
        table(
            "services",
            &[
                string("tenantId", true),
                string("name", true),
                string("machineId", false),
                string("bundleId", false),
                string("kind", true),
                string("state", true),
                array("endpoints", false),
                object("health", false),
            ],
            &[
                index("by_tenantId", &["tenantId"]),
                index("by_name", &["name"]),
                index("by_machineId", &["machineId"]),
                index("by_state", &["state"]),
            ],
        )?,
        table(
            "bundles",
            &[
                string("sha256", true),
                number("sizeBytes", false),
                string("sourceRef", false),
                string("status", true),
            ],
            &[
                index("by_sha256", &["sha256"]),
                index("by_status", &["status"]),
            ],
        )?,
        table(
            "functions",
            &[
                string("bundleId", true),
                string("path", true),
                string("kind", true),
                object("argsSchema", false),
                object("returnsSchema", false),
            ],
            &[
                index("by_bundleId", &["bundleId"]),
                index("by_kind", &["kind"]),
            ],
        )?,
        table(
            "tables",
            &[
                string("tenantId", true),
                string("name", true),
                object("schema", false),
                number("rowCount", false),
                number("lastWriteAt", false),
            ],
            &[
                index("by_tenantId", &["tenantId"]),
                index("by_name", &["name"]),
                index("by_tenantId_and_name", &["tenantId", "name"]),
            ],
        )?,
        table(
            "events",
            &[
                string("source", true),
                string("level", true),
                string("category", true),
                string("message", true),
                object("data", false),
                string("correlationId", false),
                number("createdAt", true),
            ],
            &[
                index("by_source", &["source"]),
                index("by_level", &["level"]),
                index("by_category", &["category"]),
                index("by_correlationId", &["correlationId"]),
                index("by_createdAt", &["createdAt"]),
            ],
        )?,
        table(
            "runs",
            &[
                string("bundleId", false),
                string("functionPath", true),
                string("kind", true),
                number("durationMs", false),
                string("status", true),
                object("error", false),
                number("startedAt", true),
            ],
            &[
                index("by_bundleId", &["bundleId"]),
                index("by_functionPath", &["functionPath"]),
                index("by_status", &["status"]),
                index("by_startedAt", &["startedAt"]),
            ],
        )?,
        table(
            "scheduled_jobs",
            &[
                string("tenantId", true),
                string("functionPath", true),
                number("scheduledTime", true),
                string("status", true),
                any("args", false),
                any("result", false),
            ],
            &[
                index("by_tenantId", &["tenantId"]),
                index("by_status", &["status"]),
                index("by_scheduledTime", &["scheduledTime"]),
            ],
        )?,
        table(
            "cron_jobs",
            &[
                string("tenantId", true),
                string("name", true),
                string("schedule", true),
                string("functionPath", true),
                number("lastRunAt", false),
                number("nextRunAt", false),
                string("status", true),
            ],
            &[
                index("by_tenantId", &["tenantId"]),
                index("by_status", &["status"]),
                index("by_nextRunAt", &["nextRunAt"]),
            ],
        )?,
        table(
            "routes",
            &[
                string("method", true),
                string("path", true),
                string("adapter", true),
                string("handler", false),
                boolean("authRequired", true),
                number("lastRequestAt", false),
            ],
            &[
                index("by_adapter", &["adapter"]),
                index("by_path", &["path"]),
            ],
        )?,
        table(
            "listeners",
            &[
                string("adapter", true),
                string("protocol", true),
                string("address", true),
                string("state", true),
                string("version", false),
                string("error", false),
            ],
            &[
                index("by_adapter", &["adapter"]),
                index("by_state", &["state"]),
            ],
        )?,
        table(
            "subscriptions",
            &[
                string("tenantId", false),
                string("adapter", true),
                string("queryKey", true),
                number("clientCount", true),
                number("lastDeliveryAt", false),
                string("error", false),
            ],
            &[
                index("by_tenantId", &["tenantId"]),
                index("by_adapter", &["adapter"]),
            ],
        )?,
        table(
            "ports",
            &[
                string("machineId", false),
                string("serviceId", false),
                number("hostPort", true),
                number("guestPort", false),
                string("protocol", true),
                string("state", true),
            ],
            &[
                index("by_machineId", &["machineId"]),
                index("by_serviceId", &["serviceId"]),
                index("by_state", &["state"]),
            ],
        )?,
        table(
            "adapter_capabilities",
            &[
                string("adapter", true),
                string("feature", true),
                string("status", true),
                string("caveat", false),
                string("evidence", false),
            ],
            &[
                index("by_adapter", &["adapter"]),
                index("by_status", &["status"]),
            ],
        )?,
        table(
            "system_status",
            &[
                string("name", true),
                string("version", true),
                string("health", true),
                number("startedAt", true),
                number("updatedAt", true),
                object("details", false),
            ],
            &[index("by_name", &["name"]), index("by_health", &["health"])],
        )?,
    ])
}

fn table(name: &str, fields: &[FieldSchema], indexes: &[IndexDefinition]) -> Result<TableSchema> {
    Ok(TableSchema {
        table: TableName::new(name.to_string())?,
        fields: fields.to_vec(),
        indexes: indexes.to_vec(),
        access_policy: None,
    })
}

fn field(name: &str, field_type: FieldType, required: bool) -> FieldSchema {
    FieldSchema {
        name: name.to_string(),
        field_type,
        required,
    }
}

fn string(name: &str, required: bool) -> FieldSchema {
    field(name, FieldType::String, required)
}

fn number(name: &str, required: bool) -> FieldSchema {
    field(name, FieldType::Number, required)
}

fn boolean(name: &str, required: bool) -> FieldSchema {
    field(name, FieldType::Boolean, required)
}

fn array(name: &str, required: bool) -> FieldSchema {
    field(name, FieldType::Array, required)
}

fn object(name: &str, required: bool) -> FieldSchema {
    field(name, FieldType::Object, required)
}

fn any(name: &str, required: bool) -> FieldSchema {
    field(name, FieldType::Any, required)
}

fn index(name: &str, fields: &[&str]) -> IndexDefinition {
    IndexDefinition::new(name, fields.iter().copied())
}
