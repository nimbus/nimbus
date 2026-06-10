use nimbus_core::{FieldSchema, FieldType, IndexDefinition, Result, TableName, TableSchema};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum SystemTable {
    AdapterCapabilities,
    Bundles,
    CronJobs,
    Events,
    Functions,
    Listeners,
    Machines,
    Ports,
    Routes,
    Runs,
    ScheduledJobs,
    Services,
    Subscriptions,
    SystemStatus,
    Tables,
    WorkloadStatus,
}

impl SystemTable {
    #[cfg(test)]
    pub(crate) const ALL: [Self; 16] = [
        Self::AdapterCapabilities,
        Self::Bundles,
        Self::CronJobs,
        Self::Events,
        Self::Functions,
        Self::Listeners,
        Self::Machines,
        Self::Ports,
        Self::Routes,
        Self::Runs,
        Self::ScheduledJobs,
        Self::Services,
        Self::Subscriptions,
        Self::SystemStatus,
        Self::Tables,
        Self::WorkloadStatus,
    ];

    pub(crate) const fn name(self) -> &'static str {
        match self {
            Self::AdapterCapabilities => "adapter_capabilities",
            Self::Bundles => "bundles",
            Self::CronJobs => "cron_jobs",
            Self::Events => "events",
            Self::Functions => "functions",
            Self::Listeners => "listeners",
            Self::Machines => "machines",
            Self::Ports => "ports",
            Self::Routes => "routes",
            Self::Runs => "runs",
            Self::ScheduledJobs => "scheduled_jobs",
            Self::Services => "services",
            Self::Subscriptions => "subscriptions",
            Self::SystemStatus => "system_status",
            Self::Tables => "tables",
            Self::WorkloadStatus => "workload_status",
        }
    }

    pub(crate) fn table_name(self) -> Result<TableName> {
        TableName::new(self.name().to_owned())
    }
}

pub(crate) fn system_table_schemas() -> Result<Vec<TableSchema>> {
    Ok(vec![
        table(
            SystemTable::Machines,
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
            SystemTable::Services,
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
            SystemTable::Bundles,
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
            SystemTable::Functions,
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
            SystemTable::Tables,
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
            SystemTable::Events,
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
            SystemTable::Runs,
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
            SystemTable::ScheduledJobs,
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
                index("by_tenantId_and_status", &["tenantId", "status"]),
                index("by_scheduledTime", &["scheduledTime"]),
            ],
        )?,
        table(
            SystemTable::CronJobs,
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
                index("by_tenantId_and_status", &["tenantId", "status"]),
                index("by_nextRunAt", &["nextRunAt"]),
            ],
        )?,
        table(
            SystemTable::Routes,
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
            SystemTable::Listeners,
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
            SystemTable::Subscriptions,
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
            SystemTable::Ports,
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
            SystemTable::AdapterCapabilities,
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
            SystemTable::SystemStatus,
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
        table(
            SystemTable::WorkloadStatus,
            &[
                string("tenantId", true),
                string("workloadUid", true),
                string("decisionId", true),
                number("observedGeneration", true),
                string("nodeId", true),
                string("phase", true),
                string("target", true),
                object("evidence", false),
                object("diagnostics", false),
                number("updatedAt", true),
            ],
            &[
                index("by_tenantId", &["tenantId"]),
                index("by_decisionId", &["decisionId"]),
                index("by_phase", &["phase"]),
            ],
        )?,
    ])
}

fn table(
    table: SystemTable,
    fields: &[FieldSchema],
    indexes: &[IndexDefinition],
) -> Result<TableSchema> {
    Ok(TableSchema {
        table: table.table_name()?,
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
