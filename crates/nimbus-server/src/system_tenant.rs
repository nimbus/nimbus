use std::net::SocketAddr;
use std::sync::{Arc, Weak};
use std::time::{SystemTime, UNIX_EPOCH};

use nimbus_core::{
    CronJob, CronSchedule, DocumentId, Error, FieldSchema, FieldType, IndexDefinition, Mutation,
    Result, ScheduledJob, ScheduledJobOutcome, ScheduledJobResult, TableName, TableSchema,
    TenantId,
};
use nimbus_engine::{
    CommittedMutationEvent, CommittedMutationObserver, Service, TableSchemaChangeEvent,
    TableSchemaChangeObserver,
};
use nimbus_machine::{MachineConfigRecord, MachineLifecycle, MachineStateRecord};
use nimbus_sandbox::{PublishedEndpointProtocol, SandboxBackendKind, SandboxHandle, SandboxStatus};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tracing::warn;

pub(crate) const SYSTEM_TENANT_ID: &str = "_nimbus";
const TABLE_PROJECTION_OBSERVER: &str = "nimbus-system-table-projection";

struct TableProjectionObserver {
    service: Weak<Service>,
    projection_lock: Arc<tokio::sync::Mutex<()>>,
}

impl CommittedMutationObserver for TableProjectionObserver {
    fn committed_mutation_applied(&self, event: CommittedMutationEvent) {
        if is_reserved_tenant_id(&event.tenant_id) {
            return;
        }
        let tables = event
            .commit
            .affected_tables()
            .into_iter()
            .collect::<Vec<_>>();
        if tables.is_empty() {
            return;
        }
        self.project_tables(event.tenant_id, tables);
    }
}

impl TableSchemaChangeObserver for TableProjectionObserver {
    fn table_schema_changed(&self, event: TableSchemaChangeEvent) {
        if is_reserved_tenant_id(&event.tenant_id) {
            return;
        }
        self.project_tables(event.tenant_id, vec![event.table]);
    }
}

impl TableProjectionObserver {
    fn project_tables(&self, tenant_id: TenantId, mut tables: Vec<TableName>) {
        let Some(service) = self.service.upgrade() else {
            return;
        };
        let projection_lock = self.projection_lock.clone();
        tables.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        let Ok(handle) = tokio::runtime::Handle::try_current() else {
            warn!(
                tenant_id = %tenant_id,
                "skipping system table projection because no tokio runtime is active"
            );
            return;
        };
        handle.spawn(async move {
            let _projection_guard = projection_lock.lock().await;
            for table in tables {
                if let Err(error) = record_table_state_async(&service, &tenant_id, &table).await {
                    warn!(
                        tenant_id = %tenant_id,
                        table = %table,
                        error = %error,
                        "failed to project committed table state into _nimbus"
                    );
                }
            }
        });
    }
}

pub(crate) fn install_table_projection_observer(service: &Arc<Service>) {
    let observer = Arc::new(TableProjectionObserver {
        service: Arc::downgrade(service),
        projection_lock: Arc::new(tokio::sync::Mutex::new(())),
    });
    service.install_committed_mutation_observer(TABLE_PROJECTION_OBSERVER, observer.clone());
    service.install_table_schema_change_observer(TABLE_PROJECTION_OBSERVER, observer);
}

pub(crate) fn system_tenant_id() -> Result<TenantId> {
    TenantId::new(SYSTEM_TENANT_ID)
}

pub(crate) fn is_reserved_tenant_id(tenant_id: &TenantId) -> bool {
    tenant_id.as_str().starts_with('_')
}

pub(crate) fn is_system_tenant_id(tenant_id: &TenantId) -> bool {
    tenant_id.as_str() == SYSTEM_TENANT_ID
}

pub(crate) fn user_tenant_id(value: impl Into<String>) -> Result<TenantId> {
    let tenant_id = TenantId::new(value)?;
    if is_reserved_tenant_id(&tenant_id) {
        return Err(Error::InvalidInput(format!(
            "tenant ids starting with `_` are reserved for Nimbus system tenants: {tenant_id}"
        )));
    }
    Ok(tenant_id)
}

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

pub(crate) async fn ensure_system_tenant_async(service: &Arc<Service>) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    match service.create_tenant_async(tenant_id.clone()).await {
        Ok(()) | Err(Error::AlreadyExists(_)) => {}
        Err(error) => return Err(error),
    }

    for schema in system_table_schemas()? {
        service
            .set_table_schema_async(tenant_id.clone(), schema)
            .await?;
    }

    Ok(())
}

pub(crate) async fn prepare_system_tenant_async(
    service: &Arc<Service>,
    listen_addr: Option<SocketAddr>,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    record_system_status_async(service, listen_addr).await?;
    seed_system_documents_async(service, listen_addr).await?;
    sync_all_scheduler_state_async(service).await
}

pub(crate) async fn record_system_status_async(
    service: &Arc<Service>,
    listen_addr: Option<SocketAddr>,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    let started_at = existing_system_started_at_async(service).await?;
    let mut details = Map::new();
    if let Some(listen_addr) = listen_addr {
        details.insert("listenAddress".to_owned(), json!(listen_addr.to_string()));
    }
    upsert_system_document_async(
        service,
        "system_status",
        "system:server",
        object_fields(json!({
            "name": "server",
            "version": env!("CARGO_PKG_VERSION"),
            "health": "ok",
            "startedAt": started_at,
            "updatedAt": unix_time_millis()?,
            "details": details,
        })),
    )
    .await
}

pub(crate) async fn record_service_handle_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    handle: &SandboxHandle,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    let service_id = service_document_id(tenant_id, &handle.name);
    delete_service_port_documents_async(service, &service_id).await?;
    let endpoints = handle
        .published_endpoints
        .iter()
        .map(|endpoint| {
            json!({
                "name": endpoint.name.as_str(),
                "protocol": endpoint_protocol(endpoint.protocol),
                "host": endpoint.address.ip().to_string(),
                "port": endpoint.address.port(),
            })
        })
        .collect::<Vec<_>>();

    upsert_system_document_async(
        service,
        "services",
        &service_id,
        object_fields(json!({
            "name": handle.name.as_str(),
            "tenantId": tenant_id.as_str(),
            "kind": "sandbox",
            "state": sandbox_status(handle.status),
            "endpoints": endpoints,
            "health": {
                "sandboxId": handle.id.as_str(),
                "backend": sandbox_backend(handle.backend),
                "status": sandbox_status(handle.status),
            },
        })),
    )
    .await?;

    for endpoint in &handle.published_endpoints {
        let mut fields = object_fields(json!({
            "serviceId": service_id.as_str(),
            "hostPort": endpoint.address.port(),
            "protocol": endpoint_protocol(endpoint.protocol),
            "state": sandbox_status(handle.status),
        }));
        if let Some(guest_port) = endpoint.guest_port {
            fields.insert("guestPort".to_owned(), json!(guest_port));
        }
        upsert_system_document_async(
            service,
            "ports",
            &service_port_document_id(tenant_id, &handle.name, &endpoint.name),
            fields,
        )
        .await?;
    }

    Ok(())
}

pub(crate) async fn record_machine_state_async(
    service: &Arc<Service>,
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    let paths = config.roots.paths(&config.name);
    upsert_system_document_async(
        service,
        "machines",
        &machine_document_id(&config.name),
        object_fields(json!({
            "name": config.name.as_str(),
            "kind": "developer-machine",
            "state": state.lifecycle.as_str(),
            "provider": config.provider.as_str(),
            "resources": {
                "cpus": config.resources.cpus,
                "memoryMiB": config.resources.memory_mib,
                "diskGiB": config.resources.disk_gib,
            },
            "meta": {
                "manager": state.manager.as_str(),
                "provisioning": config.guest.provisioning,
                "image": describe_machine_image_source(&config.guest.image_source),
                "apiSocketPath": paths.api_socket_path.display().to_string(),
                "lastError": state.last_error.as_deref(),
            },
        })),
    )
    .await?;

    let listener_state = if matches!(state.lifecycle, MachineLifecycle::Running) {
        "listening"
    } else {
        state.lifecycle.as_str()
    };
    upsert_system_document_async(
        service,
        "listeners",
        &machine_listener_document_id(&config.name),
        object_fields(json!({
            "adapter": "machine",
            "protocol": "unix",
            "address": paths.api_socket_path.display().to_string(),
            "state": listener_state,
            "version": env!("CARGO_PKG_VERSION"),
        })),
    )
    .await?;

    if let Some(runtime) = state.runtime.as_ref() {
        upsert_system_document_async(
            service,
            "ports",
            &machine_port_document_id(&config.name, "ssh"),
            object_fields(json!({
                "machineId": config.name.as_str(),
                "hostPort": runtime.ssh_port,
                "guestPort": 22,
                "protocol": "tcp",
                "state": state.lifecycle.as_str(),
            })),
        )
        .await?;
    }

    Ok(())
}

pub(crate) async fn delete_machine_state_async(service: &Arc<Service>, name: &str) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    delete_system_document_if_exists_async(service, "machines", &machine_document_id(name)).await?;
    delete_system_document_if_exists_async(
        service,
        "listeners",
        &machine_listener_document_id(name),
    )
    .await?;
    delete_system_document_if_exists_async(
        service,
        "ports",
        &machine_port_document_id(name, "ssh"),
    )
    .await?;
    Ok(())
}

pub(crate) async fn record_system_event_async(
    service: &Arc<Service>,
    source: &str,
    level: &str,
    category: &str,
    message: &str,
    data: Value,
    correlation_id: Option<&str>,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    service
        .insert_document_async(
            system_tenant_id()?,
            TableName::new("events")?,
            object_fields(json!({
                "source": source,
                "level": level,
                "category": category,
                "message": message,
                "data": data,
                "correlationId": correlation_id,
                "createdAt": unix_time_millis()?,
            })),
        )
        .await?;
    Ok(())
}

pub(crate) async fn record_table_state_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    let schema = match service
        .get_table_schema_async(tenant_id.clone(), table.clone())
        .await
    {
        Ok(schema) => Some(schema),
        Err(Error::SchemaNotFound(_)) => None,
        Err(error) => return Err(error),
    };
    let row_count = service
        .count_table_documents_async(tenant_id.clone(), table.clone())
        .await?;
    let document_id = table_document_id(tenant_id, table);
    if schema.is_none() && row_count == 0 {
        delete_system_document_if_exists_async(service, "tables", &document_id).await?;
        return Ok(());
    }

    let mut fields = object_fields(json!({
        "tenantId": tenant_id.as_str(),
        "name": table.as_str(),
        "rowCount": row_count,
        "lastWriteAt": unix_time_millis()?,
    }));
    if let Some(schema) = schema {
        fields.insert(
            "schema".to_owned(),
            serde_json::to_value(schema)
                .map_err(|error| Error::Serialization(error.to_string()))?,
        );
    }
    upsert_system_document_async(service, "tables", &document_id, fields).await
}

pub(crate) async fn record_convex_deployment_state_async(
    service: &Arc<Service>,
    summary: &crate::adapters::convex::ConvexRegistryDeploySummary,
    source_ref: &str,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    let bundle_sha256 = deployment_bundle_sha256(summary);
    upsert_system_document_async(
        service,
        "bundles",
        &bundle_document_id(&bundle_sha256),
        object_fields(json!({
            "sha256": bundle_sha256.as_str(),
            "sourceRef": source_ref,
            "status": "active",
        })),
    )
    .await?;

    let active_function_ids = summary
        .functions
        .iter()
        .map(|function| function_document_id(&bundle_sha256, &function.name))
        .collect::<std::collections::BTreeSet<_>>();
    for function in &summary.functions {
        upsert_system_document_async(
            service,
            "functions",
            &function_document_id(&bundle_sha256, &function.name),
            object_fields(json!({
                "bundleId": bundle_sha256.as_str(),
                "path": function.name.as_str(),
                "kind": function.kind,
            })),
        )
        .await?;
    }
    delete_stale_deployment_documents_async(service, &bundle_sha256, &active_function_ids).await
}

pub(crate) struct RunRecord<'a> {
    pub(crate) tenant_id: &'a TenantId,
    pub(crate) function_path: &'a str,
    pub(crate) kind: &'a str,
    pub(crate) started_at: u64,
    pub(crate) duration_ms: f64,
    pub(crate) status: &'a str,
    pub(crate) error: Option<&'a str>,
}

pub(crate) async fn record_run_async(service: &Arc<Service>, record: RunRecord<'_>) -> Result<()> {
    if is_system_tenant_id(record.tenant_id) {
        return Ok(());
    }
    ensure_system_tenant_async(service).await?;
    let mut fields = object_fields(json!({
        "functionPath": record.function_path,
        "kind": record.kind,
        "durationMs": record.duration_ms,
        "status": record.status,
        "startedAt": record.started_at,
    }));
    if let Some(error) = record.error {
        fields.insert("error".to_owned(), json!({ "message": error }));
    }
    service
        .insert_document_async(system_tenant_id()?, TableName::new("runs")?, fields)
        .await?;
    Ok(())
}

pub(crate) async fn record_scheduled_job_state_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    job: &ScheduledJob,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    upsert_system_document_async(
        service,
        "scheduled_jobs",
        &scheduled_job_document_id(tenant_id, &job.id),
        scheduled_job_fields(tenant_id, &job.run_at, &job.mutation, "pending", None)?,
    )
    .await
}

pub(crate) async fn record_scheduled_job_result_state_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    result: &ScheduledJobResult,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    let status = match result.outcome {
        ScheduledJobOutcome::Completed => "completed",
        ScheduledJobOutcome::Failed => "failed",
    };
    upsert_system_document_async(
        service,
        "scheduled_jobs",
        &scheduled_job_document_id(tenant_id, &result.id),
        scheduled_job_fields(
            tenant_id,
            &result.run_at,
            &result.mutation,
            status,
            Some(result),
        )?,
    )
    .await
}

pub(crate) async fn delete_scheduled_job_state_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    job_id: &DocumentId,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    delete_system_document_if_exists_async(
        service,
        "scheduled_jobs",
        &scheduled_job_document_id(tenant_id, job_id),
    )
    .await
}

pub(crate) async fn record_cron_job_state_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    cron: &CronJob,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    upsert_system_document_async(
        service,
        "cron_jobs",
        &cron_job_document_id(tenant_id, &cron.name),
        cron_job_fields(tenant_id, cron)?,
    )
    .await
}

pub(crate) async fn delete_cron_job_state_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    name: &str,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    delete_system_document_if_exists_async(
        service,
        "cron_jobs",
        &cron_job_document_id(tenant_id, name),
    )
    .await
}

pub(crate) async fn sync_scheduler_state_for_tenant_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    let scheduled_jobs = service.list_scheduled_jobs_async(tenant_id.clone()).await?;
    let active_scheduled_ids = scheduled_jobs
        .iter()
        .map(|job| scheduled_job_document_id(tenant_id, &job.id))
        .collect::<std::collections::BTreeSet<_>>();
    for job in &scheduled_jobs {
        record_scheduled_job_state_async(service, tenant_id, job).await?;
    }
    delete_stale_scheduler_documents_async(
        service,
        "scheduled_jobs",
        tenant_id,
        "pending",
        &active_scheduled_ids,
    )
    .await?;

    let cron_jobs = service.list_cron_jobs_async(tenant_id.clone()).await?;
    let active_cron_ids = cron_jobs
        .iter()
        .map(|cron| cron_job_document_id(tenant_id, &cron.name))
        .collect::<std::collections::BTreeSet<_>>();
    for cron in &cron_jobs {
        record_cron_job_state_async(service, tenant_id, cron).await?;
    }
    delete_stale_scheduler_documents_async(
        service,
        "cron_jobs",
        tenant_id,
        "active",
        &active_cron_ids,
    )
    .await
}

pub(crate) async fn record_listener_state_async(
    service: &Arc<Service>,
    adapter: &str,
    protocol: &str,
    address: &str,
    state: &str,
    version: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    let mut fields = object_fields(json!({
        "adapter": adapter,
        "protocol": protocol,
        "address": address,
        "state": state,
    }));
    if let Some(version) = version {
        fields.insert("version".to_owned(), json!(version));
    }
    if let Some(error) = error {
        fields.insert("error".to_owned(), json!(error));
    }
    upsert_system_document_async(
        service,
        "listeners",
        &listener_document_id(adapter, protocol),
        fields,
    )
    .await
}

pub(crate) async fn record_subscription_state_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
    query_key: &str,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    upsert_system_document_async(
        service,
        "subscriptions",
        &subscription_document_id(adapter, tenant_id, subscription_id),
        object_fields(json!({
            "tenantId": tenant_id.as_str(),
            "adapter": adapter,
            "queryKey": query_key,
            "clientCount": 1,
            "lastDeliveryAt": unix_time_millis()?,
        })),
    )
    .await
}

pub(crate) async fn record_subscription_delivery_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
    query_key: &str,
) -> Result<()> {
    if is_system_tenant_id(tenant_id) {
        return Ok(());
    }
    record_subscription_state_async(service, tenant_id, adapter, subscription_id, query_key).await
}

pub(crate) async fn record_subscription_error_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
    query_key: &str,
    error: &str,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    upsert_system_document_async(
        service,
        "subscriptions",
        &subscription_document_id(adapter, tenant_id, subscription_id),
        object_fields(json!({
            "tenantId": tenant_id.as_str(),
            "adapter": adapter,
            "queryKey": query_key,
            "clientCount": 1,
            "lastDeliveryAt": unix_time_millis()?,
            "error": error,
        })),
    )
    .await
}

pub(crate) async fn delete_subscription_state_async(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
) -> Result<()> {
    ensure_system_tenant_async(service).await?;
    delete_system_document_if_exists_async(
        service,
        "subscriptions",
        &subscription_document_id(adapter, tenant_id, subscription_id),
    )
    .await
}

async fn delete_system_document_if_exists_async(
    service: &Arc<Service>,
    table: &str,
    document_id: &str,
) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    let table = TableName::new(table.to_owned())?;
    let document_id = DocumentId::from_key(document_id.to_owned())?;
    match service
        .delete_document_async(tenant_id, table, document_id)
        .await
    {
        Ok(()) | Err(Error::DocumentNotFound(_)) => Ok(()),
        Err(error) => Err(error),
    }
}

async fn delete_service_port_documents_async(
    service: &Arc<Service>,
    service_id: &str,
) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    let table = TableName::new("ports")?;
    let documents = service
        .list_documents_async(tenant_id.clone(), table.clone())
        .await?;
    for document in documents {
        if document.fields.get("serviceId") == Some(&json!(service_id)) {
            service
                .delete_document_async(tenant_id.clone(), table.clone(), document.id)
                .await?;
        }
    }
    Ok(())
}

async fn sync_all_scheduler_state_async(service: &Arc<Service>) -> Result<()> {
    let tenants = service.list_tenants_async().await?;
    for tenant_id in tenants {
        if is_reserved_tenant_id(&tenant_id) {
            continue;
        }
        sync_scheduler_state_for_tenant_async(service, &tenant_id).await?;
    }
    Ok(())
}

async fn existing_system_started_at_async(service: &Arc<Service>) -> Result<u64> {
    let system_tenant = system_tenant_id()?;
    let table = TableName::new("system_status")?;
    let document_id = DocumentId::from_key("system:server")?;
    match service
        .get_document_async(system_tenant, table, document_id)
        .await
    {
        Ok(document) => Ok(document
            .fields
            .get("startedAt")
            .and_then(Value::as_u64)
            .unwrap_or(unix_time_millis()?)),
        Err(Error::DocumentNotFound(_)) => unix_time_millis(),
        Err(error) => Err(error),
    }
}

async fn delete_stale_deployment_documents_async(
    service: &Arc<Service>,
    active_bundle_sha256: &str,
    active_function_ids: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let bundles_table = TableName::new("bundles")?;
    let bundles = service
        .list_documents_async(system_tenant.clone(), bundles_table.clone())
        .await?;
    for bundle in bundles {
        if bundle.fields.get("status") != Some(&json!("active"))
            || bundle.fields.get("sha256") == Some(&json!(active_bundle_sha256))
        {
            continue;
        }
        service
            .delete_document_async(system_tenant.clone(), bundles_table.clone(), bundle.id)
            .await?;
    }

    let functions_table = TableName::new("functions")?;
    let functions = service
        .list_documents_async(system_tenant.clone(), functions_table.clone())
        .await?;
    for function in functions {
        if function.fields.get("bundleId") == Some(&json!(active_bundle_sha256))
            && active_function_ids.contains(&function.id.to_string())
        {
            continue;
        }
        service
            .delete_document_async(system_tenant.clone(), functions_table.clone(), function.id)
            .await?;
    }

    Ok(())
}

async fn delete_stale_scheduler_documents_async(
    service: &Arc<Service>,
    table: &str,
    tenant_id: &TenantId,
    stale_status: &str,
    active_document_ids: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let table_name = TableName::new(table.to_owned())?;
    let documents = service
        .list_documents_async(system_tenant.clone(), table_name.clone())
        .await?;
    for document in documents {
        if document.fields.get("tenantId") != Some(&json!(tenant_id.as_str()))
            || document.fields.get("status") != Some(&json!(stale_status))
            || active_document_ids.contains(&document.id.to_string())
        {
            continue;
        }
        service
            .delete_document_async(system_tenant.clone(), table_name.clone(), document.id)
            .await?;
    }
    Ok(())
}

fn scheduled_job_fields(
    tenant_id: &TenantId,
    run_at: &nimbus_core::Timestamp,
    mutation: &Mutation,
    status: &str,
    result: Option<&ScheduledJobResult>,
) -> Result<Map<String, Value>> {
    let mut fields = object_fields(json!({
        "tenantId": tenant_id.as_str(),
        "functionPath": mutation_function_path(mutation),
        "scheduledTime": run_at.0,
        "status": status,
        "args": mutation_payload(mutation)?,
    }));
    if let Some(result) = result {
        fields.insert(
            "result".to_owned(),
            json!({
                "finishedAt": result.finished_at.0,
                "outcome": match result.outcome {
                    ScheduledJobOutcome::Completed => "completed",
                    ScheduledJobOutcome::Failed => "failed",
                },
                "error": result.error.as_deref(),
            }),
        );
    }
    Ok(fields)
}

fn cron_job_fields(tenant_id: &TenantId, cron: &CronJob) -> Result<Map<String, Value>> {
    let mut fields = object_fields(json!({
        "tenantId": tenant_id.as_str(),
        "name": cron.name.as_str(),
        "schedule": cron_schedule_label(&cron.schedule),
        "functionPath": mutation_function_path(&cron.mutation),
        "nextRunAt": cron.next_run.0,
        "status": if cron.enabled { "active" } else { "paused" },
    }));
    if let Some(last_run) = cron.last_run {
        fields.insert("lastRunAt".to_owned(), json!(last_run.0));
    }
    Ok(fields)
}

fn mutation_function_path(mutation: &Mutation) -> String {
    match mutation {
        Mutation::Insert { table, .. } => format!("documents.{}.insert", table.as_str()),
        Mutation::Update { table, .. } => format!("documents.{}.update", table.as_str()),
        Mutation::Delete { table, .. } => format!("documents.{}.delete", table.as_str()),
    }
}

fn mutation_payload(mutation: &Mutation) -> Result<Value> {
    serde_json::to_value(mutation).map_err(|error| Error::Serialization(error.to_string()))
}

fn deployment_bundle_sha256(
    summary: &crate::adapters::convex::ConvexRegistryDeploySummary,
) -> String {
    if let Some(fingerprint) = summary.runtime_bundle_fingerprint.as_deref() {
        return fingerprint.to_owned();
    }

    let mut hasher = Sha256::new();
    hasher.update(b"nimbus-convex-deploy-summary-v1");
    for function in &summary.functions {
        hasher.update(function.name.as_bytes());
        hasher.update([0]);
        hasher.update(function.kind.as_bytes());
        hasher.update([0]);
        hasher.update(function.fingerprint.as_bytes());
        hasher.update([0]);
    }
    for route in &summary.http_routes {
        hasher.update(route.key.as_bytes());
        hasher.update([0]);
        hasher.update(route.fingerprint.as_bytes());
        hasher.update([0]);
    }
    if let Some(fingerprint) = summary.schema_fingerprint.as_deref() {
        hasher.update(fingerprint.as_bytes());
    }
    hasher.update([0]);
    if let Some(fingerprint) = summary.index_fingerprint.as_deref() {
        hasher.update(fingerprint.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

fn cron_schedule_label(schedule: &CronSchedule) -> String {
    match schedule {
        CronSchedule::Interval { seconds } => format!("interval:{seconds}s"),
    }
}

async fn seed_system_documents_async(
    service: &Arc<Service>,
    listen_addr: Option<SocketAddr>,
) -> Result<()> {
    for route in route_inventory() {
        upsert_system_document_async(
            service,
            "routes",
            &route.document_id(),
            object_fields(json!({
                "method": route.method,
                "path": route.path,
                "adapter": route.adapter,
                "handler": route.handler,
                "authRequired": route.auth_required,
            })),
        )
        .await?;
    }

    for capability in adapter_capability_inventory() {
        upsert_system_document_async(
            service,
            "adapter_capabilities",
            &capability.document_id(),
            object_fields(json!({
                "adapter": capability.adapter,
                "feature": capability.feature,
                "status": capability.status,
                "caveat": capability.caveat,
                "evidence": capability.evidence,
            })),
        )
        .await?;
    }

    if let Some(listen_addr) = listen_addr {
        upsert_system_document_async(
            service,
            "listeners",
            "listener:http",
            object_fields(json!({
                "adapter": "native",
                "protocol": "http",
                "address": listen_addr.to_string(),
                "state": "listening",
                "version": env!("CARGO_PKG_VERSION"),
            })),
        )
        .await?;
    }

    Ok(())
}

fn unix_time_millis() -> Result<u64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Internal(format!("system clock is before Unix epoch: {error}")))?
        .as_millis() as u64)
}

async fn upsert_system_document_async(
    service: &Arc<Service>,
    table: &str,
    document_id: &str,
    fields: Map<String, Value>,
) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    let table = TableName::new(table.to_owned())?;
    let document_id = DocumentId::from_key(document_id.to_owned())?;

    match service
        .get_document_async(tenant_id.clone(), table.clone(), document_id.clone())
        .await
    {
        Ok(document) if document.fields == fields => return Ok(()),
        Ok(_) => {
            service
                .update_document_async(tenant_id, table, document_id, fields)
                .await?;
            return Ok(());
        }
        Err(Error::DocumentNotFound(_)) => {}
        Err(error) => return Err(error),
    }

    match service
        .insert_document_async_with_id(
            tenant_id.clone(),
            table.clone(),
            document_id.clone(),
            fields.clone(),
        )
        .await
    {
        Ok(_) => Ok(()),
        Err(Error::AlreadyExists(_)) => {
            service
                .update_document_async(tenant_id, table, document_id, fields)
                .await?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn object_fields(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(fields) => fields,
        _ => unreachable!("system document seed payload must be an object"),
    }
}

#[derive(Debug, Clone, Copy)]
struct RouteInventoryEntry {
    method: &'static str,
    path: &'static str,
    adapter: &'static str,
    handler: &'static str,
    auth_required: bool,
}

impl RouteInventoryEntry {
    fn document_id(self) -> String {
        format!(
            "route:{}:{}",
            self.method.to_ascii_lowercase(),
            stable_key_segment(self.path)
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct AdapterCapabilityEntry {
    adapter: &'static str,
    feature: &'static str,
    status: &'static str,
    caveat: &'static str,
    evidence: &'static str,
}

impl AdapterCapabilityEntry {
    fn document_id(self) -> String {
        format!(
            "capability:{}:{}",
            stable_key_segment(self.adapter),
            stable_key_segment(self.feature)
        )
    }
}

fn stable_key_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

fn service_document_id(tenant_id: &TenantId, service_name: &str) -> String {
    format!(
        "service:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(service_name)
    )
}

fn machine_document_id(machine_name: &str) -> String {
    format!("machine:{}", stable_key_segment(machine_name))
}

fn table_document_id(tenant_id: &TenantId, table: &TableName) -> String {
    format!(
        "table:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(table.as_str())
    )
}

fn bundle_document_id(sha256: &str) -> String {
    format!("bundle:{}", stable_key_segment(sha256))
}

fn function_document_id(bundle_sha256: &str, function_name: &str) -> String {
    format!(
        "function:{}:{}",
        stable_key_segment(bundle_sha256),
        stable_key_segment(function_name)
    )
}

fn scheduled_job_document_id(tenant_id: &TenantId, job_id: &DocumentId) -> String {
    format!(
        "scheduled-job:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(&job_id.to_string())
    )
}

fn cron_job_document_id(tenant_id: &TenantId, name: &str) -> String {
    format!(
        "cron-job:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(name)
    )
}

fn machine_listener_document_id(machine_name: &str) -> String {
    format!("listener:machine-api:{}", stable_key_segment(machine_name))
}

fn listener_document_id(adapter: &str, protocol: &str) -> String {
    format!(
        "listener:{}:{}",
        stable_key_segment(adapter),
        stable_key_segment(protocol)
    )
}

fn machine_port_document_id(machine_name: &str, port_name: &str) -> String {
    format!(
        "port:machine:{}:{}",
        stable_key_segment(machine_name),
        stable_key_segment(port_name)
    )
}

fn service_port_document_id(
    tenant_id: &TenantId,
    service_name: &str,
    endpoint_name: &str,
) -> String {
    format!(
        "port:service:{}:{}:{}",
        stable_key_segment(tenant_id.as_str()),
        stable_key_segment(service_name),
        stable_key_segment(endpoint_name)
    )
}

fn subscription_document_id(adapter: &str, tenant_id: &TenantId, subscription_id: u64) -> String {
    format!(
        "subscription:{}:{}:{}",
        stable_key_segment(adapter),
        stable_key_segment(tenant_id.as_str()),
        subscription_id
    )
}

fn describe_machine_image_source(source: &nimbus_machine::MachineImageSource) -> String {
    match source {
        nimbus_machine::MachineImageSource::OciReference { reference } => reference.clone(),
        nimbus_machine::MachineImageSource::HttpUrl { url } => url.clone(),
        nimbus_machine::MachineImageSource::LocalDisk { path } => path.display().to_string(),
    }
}

pub(crate) fn sandbox_backend(backend: SandboxBackendKind) -> &'static str {
    match backend {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    }
}

pub(crate) fn sandbox_status(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Starting => "starting",
        SandboxStatus::Ready => "ready",
        SandboxStatus::NotReady => "not_ready",
        SandboxStatus::Stopping => "stopping",
        SandboxStatus::Stopped => "stopped",
        SandboxStatus::Failed => "failed",
    }
}

pub(crate) fn endpoint_protocol(protocol: PublishedEndpointProtocol) -> &'static str {
    match protocol {
        PublishedEndpointProtocol::Tcp => "tcp",
        PublishedEndpointProtocol::Http => "http",
        PublishedEndpointProtocol::Https => "https",
    }
}

fn route_inventory() -> Vec<RouteInventoryEntry> {
    vec![
        route("GET", "/health", "native", "health", false),
        route("GET", "/ui", "ui", "ui_root", false),
        route("GET", "/ui/auth", "ui", "ui_auth", false),
        route("POST", "/ui/auth/session", "ui", "create_ui_session", false),
        route(
            "GET",
            "/debug/license/status",
            "native",
            "license_status",
            true,
        ),
        route(
            "GET",
            "/debug/encryption/status",
            "native",
            "encryption_status",
            true,
        ),
        route(
            "POST",
            "/api/system/token/rotate",
            "native",
            "rotate_local_admin_token",
            true,
        ),
        route(
            "POST",
            "/api/system/shutdown",
            "native",
            "shutdown_system",
            true,
        ),
        route(
            "GET",
            "/debug/runtime/metrics",
            "native",
            "runtime_diagnostics",
            true,
        ),
        route(
            "GET",
            "/debug/tenants/{tenant_id}/consistency",
            "native",
            "tenant_consistency_report",
            true,
        ),
        route(
            "GET",
            "/debug/tenants/{tenant_id}/engine/metrics",
            "native",
            "tenant_engine_diagnostics",
            true,
        ),
        route("GET", "/api/tenants", "native", "list_tenants", true),
        route("POST", "/api/tenants", "native", "create_tenant", true),
        route(
            "DELETE",
            "/api/tenants/{tenant_id}",
            "native",
            "delete_tenant",
            true,
        ),
        route(
            "POST",
            "/api/machines/{name}/create",
            "native",
            "create_machine",
            true,
        ),
        route(
            "PATCH",
            "/api/machines/{name}",
            "native",
            "update_machine",
            true,
        ),
        route(
            "DELETE",
            "/api/machines/{name}",
            "native",
            "delete_machine",
            true,
        ),
        route(
            "POST",
            "/api/machines/{name}/start",
            "native",
            "start_machine",
            true,
        ),
        route(
            "POST",
            "/api/machines/{name}/stop",
            "native",
            "stop_machine",
            true,
        ),
        route(
            "POST",
            "/api/machines/{name}/restart",
            "native",
            "restart_machine",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/services/{service_name}/start",
            "native",
            "start_service",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/services/{service_name}/stop",
            "native",
            "stop_service",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/services/{service_name}/restart",
            "native",
            "restart_service",
            true,
        ),
        route(
            "GET",
            "/api/tenants/{tenant_id}/documents/{table}",
            "native",
            "list_documents",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/documents",
            "native",
            "insert_document",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/query",
            "native",
            "query_documents",
            true,
        ),
        route(
            "POST",
            "/api/tenants/{tenant_id}/query/paginated",
            "native",
            "query_documents_paginated",
            true,
        ),
        route(
            "GET",
            "/api/tenants/{tenant_id}/journal",
            "native",
            "read_journal",
            true,
        ),
        route(
            "GET",
            "/api/tenants/{tenant_id}/journal/bootstrap",
            "native",
            "bootstrap_journal",
            true,
        ),
        route(
            "GET",
            "/api/tenants/{tenant_id}/schema",
            "native",
            "get_schema",
            true,
        ),
        route(
            "PUT",
            "/api/tenants/{tenant_id}/schema/{table}",
            "native",
            "set_table_schema",
            true,
        ),
        route(
            "POST",
            "/convex/{tenant_id}/query",
            "convex",
            "query",
            false,
        ),
        route(
            "POST",
            "/convex/{tenant_id}/query/paginated",
            "convex",
            "paginated_query",
            false,
        ),
        route(
            "POST",
            "/convex/{tenant_id}/mutation",
            "convex",
            "mutation",
            false,
        ),
        route(
            "POST",
            "/convex/{tenant_id}/action",
            "convex",
            "action",
            false,
        ),
        route("GET", "/convex/{tenant_id}/ws", "convex", "ws", false),
        route(
            "POST",
            "/v1/projects/{project_id}/databases/{database_id}/documents:commit",
            "firebase",
            "commit",
            false,
        ),
        route(
            "POST",
            "/v1/projects/{project_id}/databases/{database_id}/documents:runQuery",
            "firebase",
            "run_query",
            false,
        ),
        route(
            "GET",
            "/google.firestore.v1.Firestore/Listen",
            "firebase",
            "listen_websocket",
            false,
        ),
    ]
}

fn route(
    method: &'static str,
    path: &'static str,
    adapter: &'static str,
    handler: &'static str,
    auth_required: bool,
) -> RouteInventoryEntry {
    RouteInventoryEntry {
        method,
        path,
        adapter,
        handler,
        auth_required,
    }
}

fn adapter_capability_inventory() -> Vec<AdapterCapabilityEntry> {
    vec![
        capability(
            "convex",
            "reactive-functions",
            "supported",
            "",
            "Convex adapter executes query, mutation, action, scheduler, and WebSocket surfaces through nimbus-server.",
        ),
        capability(
            "convex",
            "system-tenant-ui-functions",
            "supported-with-caveats",
            "The system table contract exists; the packaged function bundle is still tracked by ST4.",
            "docs/plans/system-tenant-api-plan.md",
        ),
        capability(
            "mongodb",
            "wire-protocol-crud",
            "supported-with-caveats",
            "Nimbus implements the local compatibility surface, not Atlas administration.",
            "crates/nimbus-server/src/adapters/mongodb/",
        ),
        capability(
            "firebase",
            "firestore-rest-grpc",
            "supported-with-caveats",
            "Nimbus implements the Firestore-compatible local data path; Firebase project administration and hosted rules are not claimed.",
            "crates/nimbus-server/src/adapters/firebase/",
        ),
        capability(
            "native",
            "local-admin-rest",
            "supported",
            "",
            "crates/nimbus-server/src/router.rs",
        ),
        capability(
            "machine",
            "bootc-macos-machine",
            "supported-with-caveats",
            "Published bootc image is the current macOS default; live machine state persistence into _nimbus is still tracked by ST2.",
            "docs/architecture/sandbox/macos-machine-flow.md",
        ),
    ]
}

fn capability(
    adapter: &'static str,
    feature: &'static str,
    status: &'static str,
    caveat: &'static str,
    evidence: &'static str,
) -> AdapterCapabilityEntry {
    AdapterCapabilityEntry {
        adapter,
        feature,
        status,
        caveat,
        evidence,
    }
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
    IndexDefinition {
        name: name.to_string(),
        fields: fields.iter().map(|field| (*field).to_string()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_table_schemas_are_valid_and_cover_control_plane_contract() {
        let schemas = system_table_schemas().expect("system table schemas should build");
        let tables = schemas
            .iter()
            .map(|schema| schema.table.as_str())
            .collect::<std::collections::BTreeSet<_>>();

        assert_eq!(
            tables,
            std::collections::BTreeSet::from([
                "adapter_capabilities",
                "bundles",
                "cron_jobs",
                "events",
                "functions",
                "listeners",
                "machines",
                "ports",
                "routes",
                "runs",
                "scheduled_jobs",
                "services",
                "subscriptions",
                "system_status",
                "tables",
            ])
        );
        for schema in schemas {
            schema
                .validate_indexes()
                .expect("system table indexes should be valid");
            schema
                .validate_access_policy()
                .expect("system table access policy should be valid");
        }
    }

    #[tokio::test]
    async fn ensure_system_tenant_creates_reserved_tenant_and_schemas_idempotently() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));

        ensure_system_tenant_async(&service)
            .await
            .expect("system tenant should initialize");
        ensure_system_tenant_async(&service)
            .await
            .expect("system tenant initialization should be idempotent");

        let tenants = service
            .list_tenants_async()
            .await
            .expect("tenants should list");
        assert_eq!(
            tenants,
            vec![system_tenant_id().expect("system id should parse")]
        );

        let schema = service
            .get_schema_async(system_tenant_id().expect("system id should parse"))
            .await
            .expect("system tenant schema should load");
        assert_eq!(schema.tables.len(), system_table_schemas().unwrap().len());
        assert!(
            schema
                .tables
                .contains_key(&TableName::new("machines").unwrap())
        );
        assert!(
            schema
                .tables
                .contains_key(&TableName::new("adapter_capabilities").unwrap())
        );
        assert!(
            schema
                .tables
                .contains_key(&TableName::new("system_status").unwrap())
        );
    }

    #[tokio::test]
    async fn prepare_system_tenant_seeds_network_and_adapter_posture_documents() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));
        let listen_addr = "127.0.0.1:34567".parse().expect("listen addr should parse");

        prepare_system_tenant_async(&service, Some(listen_addr))
            .await
            .expect("system tenant should prepare");
        prepare_system_tenant_async(&service, Some(listen_addr))
            .await
            .expect("system tenant preparation should be idempotent");

        let tenant_id = system_tenant_id().expect("system id should parse");
        let routes = service
            .list_documents_async(
                tenant_id.clone(),
                TableName::new("routes").expect("table should parse"),
            )
            .await
            .expect("routes should list");
        assert_eq!(routes.len(), route_inventory().len());
        assert!(routes.iter().any(|document| {
            document.fields.get("path") == Some(&json!("/api/tenants"))
                && document.fields.get("method") == Some(&json!("GET"))
        }));

        let capabilities = service
            .list_documents_async(
                tenant_id.clone(),
                TableName::new("adapter_capabilities").expect("table should parse"),
            )
            .await
            .expect("capabilities should list");
        assert_eq!(capabilities.len(), adapter_capability_inventory().len());
        assert!(capabilities.iter().any(|document| {
            document.fields.get("adapter") == Some(&json!("machine"))
                && document.fields.get("feature") == Some(&json!("bootc-macos-machine"))
        }));

        let listeners = service
            .list_documents_async(
                tenant_id.clone(),
                TableName::new("listeners").expect("table should parse"),
            )
            .await
            .expect("listeners should list");
        assert_eq!(listeners.len(), 1);
        assert_eq!(
            listeners[0].fields.get("address"),
            Some(&json!(listen_addr.to_string()))
        );
        assert_eq!(listeners[0].fields.get("state"), Some(&json!("listening")));

        let status = service
            .get_document_async(
                tenant_id,
                TableName::new("system_status").expect("table should parse"),
                DocumentId::from_key("system:server").expect("id should parse"),
            )
            .await
            .expect("system status should exist");
        assert_eq!(status.fields.get("name"), Some(&json!("server")));
        assert_eq!(status.fields.get("health"), Some(&json!("ok")));
        assert_eq!(
            status.fields["details"]["listenAddress"],
            json!(listen_addr.to_string())
        );
        assert!(
            status.fields.get("startedAt").is_some_and(Value::is_number),
            "system status should record server start time: {status:?}"
        );
    }

    #[tokio::test]
    async fn record_machine_state_projects_machine_listener_and_port_documents() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));
        let roots = nimbus_machine::MachineRootLayout::new(
            temp.path().join("config"),
            temp.path().join("state"),
            temp.path().join("run"),
        );
        let config = nimbus_machine::MachineConfigRecord {
            version: nimbus_machine::CURRENT_MACHINE_CONFIG_VERSION,
            name: "default".to_string(),
            provider: nimbus_machine::MachineProvider::Krunkit,
            guest: nimbus_machine::MachineGuestConfig {
                image_source: nimbus_machine::MachineImageSource::OciReference {
                    reference: "docker://ghcr.io/nimbus/machine-os:v0.1.31".to_string(),
                },
                provisioning: nimbus_machine::MachineGuestProvisioning::BootcMachineConfig,
                ssh_user: "nimbus".to_string(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: nimbus_machine::MachineResources {
                cpus: 4,
                memory_mib: 4096,
                disk_gib: 50,
            },
            volumes: vec![],
            roots,
        };
        let mut state = nimbus_machine::MachineStateRecord::initialized();

        record_machine_state_async(&service, &config, &state)
            .await
            .expect("stopped machine state should project");

        let tenant_id = system_tenant_id().expect("system id should parse");
        let machine = service
            .get_document_async(
                tenant_id.clone(),
                TableName::new("machines").expect("table should parse"),
                DocumentId::from_key(machine_document_id("default")).expect("id should parse"),
            )
            .await
            .expect("machine document should exist");
        assert_eq!(machine.fields.get("state"), Some(&json!("stopped")));
        assert_eq!(machine.fields["resources"]["memoryMiB"], json!(4096));
        assert_eq!(
            machine.fields["meta"]["image"],
            json!("docker://ghcr.io/nimbus/machine-os:v0.1.31")
        );

        state.lifecycle = nimbus_machine::MachineLifecycle::Running;
        state.manager = nimbus_machine::MachineManagerState::Ready;
        state.runtime = Some(nimbus_machine::MachineRuntimeState {
            helper_binaries: nimbus_machine::MachineHelperBinaryPaths {
                krunkit: temp.path().join("krunkit"),
                gvproxy: temp.path().join("gvproxy"),
            },
            image_path: temp.path().join("default.raw"),
            efi_variable_store_path: temp.path().join("efi"),
            machine_image_source: "docker://ghcr.io/nimbus/machine-os:v0.1.31".to_string(),
            ssh_port: 2222,
            rest_uri: "unix:///tmp/nimbus/default-krunkit.sock".to_string(),
            ready_vsock_port: 1025,
        });

        record_machine_state_async(&service, &config, &state)
            .await
            .expect("running machine state should project");

        let listener = service
            .get_document_async(
                tenant_id.clone(),
                TableName::new("listeners").expect("table should parse"),
                DocumentId::from_key(machine_listener_document_id("default"))
                    .expect("id should parse"),
            )
            .await
            .expect("machine listener document should exist");
        assert_eq!(listener.fields.get("adapter"), Some(&json!("machine")));
        assert_eq!(listener.fields.get("protocol"), Some(&json!("unix")));
        assert_eq!(listener.fields.get("state"), Some(&json!("listening")));

        let ssh_port = service
            .get_document_async(
                tenant_id,
                TableName::new("ports").expect("table should parse"),
                DocumentId::from_key(machine_port_document_id("default", "ssh"))
                    .expect("id should parse"),
            )
            .await
            .expect("machine ssh port document should exist");
        assert_eq!(ssh_port.fields.get("machineId"), Some(&json!("default")));
        assert_eq!(ssh_port.fields.get("hostPort"), Some(&json!(2222)));
        assert_eq!(ssh_port.fields.get("guestPort"), Some(&json!(22)));
        assert_eq!(ssh_port.fields.get("state"), Some(&json!("running")));
    }

    #[tokio::test]
    async fn record_subscription_state_projects_live_subscription_document() {
        let temp = tempfile::tempdir().expect("tempdir should create");
        let service = Arc::new(Service::new(temp.path()).expect("service should create"));
        let tenant_id = TenantId::new("demo").expect("tenant should parse");

        record_subscription_state_async(
            &service,
            &tenant_id,
            "convex",
            42,
            "named:{\"name\":\"messages:list\"}",
        )
        .await
        .expect("subscription state should project");

        let document = service
            .get_document_async(
                system_tenant_id().expect("system id should parse"),
                TableName::new("subscriptions").expect("table should parse"),
                DocumentId::from_key(subscription_document_id("convex", &tenant_id, 42))
                    .expect("id should parse"),
            )
            .await
            .expect("subscription document should exist");
        assert_eq!(document.fields.get("tenantId"), Some(&json!("demo")));
        assert_eq!(document.fields.get("adapter"), Some(&json!("convex")));
        assert_eq!(document.fields.get("clientCount"), Some(&json!(1)));

        delete_subscription_state_async(&service, &tenant_id, "convex", 42)
            .await
            .expect("subscription state should delete");
        let deleted = service
            .get_document_async(
                system_tenant_id().expect("system id should parse"),
                TableName::new("subscriptions").expect("table should parse"),
                DocumentId::from_key(subscription_document_id("convex", &tenant_id, 42))
                    .expect("id should parse"),
            )
            .await;
        assert!(matches!(deleted, Err(Error::DocumentNotFound(_))));
    }

    #[test]
    fn user_tenant_id_rejects_reserved_prefix() {
        let error = user_tenant_id("_demo").expect_err("reserved user tenant should fail");

        assert!(matches!(error, Error::InvalidInput(message) if message.contains("reserved")));
    }
}
