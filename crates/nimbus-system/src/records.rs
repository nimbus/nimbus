use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nimbus_core::{
    CronJob, CronSchedule, Document, DocumentId, Error, Filter, FilterOp, Mutation, Query, Result,
    ScheduledJob, ScheduledJobOutcome, ScheduledJobResult, TableName, TenantId,
};
use nimbus_engine::Engine;
use nimbus_machine::{MachineConfigRecord, MachineLifecycle, MachineStateRecord};
use nimbus_sandbox::{PublishedEndpointProtocol, SandboxBackendKind, SandboxHandle, SandboxStatus};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use nimbus_node::{
    HostLifecycleFuture, StatusEvidenceWrite, StatusEvidenceWriter, TenantWorkloadStatus,
    ensure_status_matches_projection,
};
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::TenantSystemEvidenceProjection;

use super::identity::{is_reserved_tenant_id, is_system_tenant_id, system_tenant_id};
use super::inventory::{adapter_capability_inventory, route_inventory};
use super::keys::{
    bundle_document_id, cron_job_document_id, function_document_id, listener_document_id,
    machine_document_id, machine_listener_document_id, machine_port_document_id,
    module_document_id, scheduled_job_document_id, service_document_id, service_port_document_id,
    source_package_document_id, subscription_document_id, table_document_id,
    workload_status_document_id,
};
use super::schema::{SystemTable, system_table_schemas};
use super::source_package::parse_source_package;
use super::source_store::SourcePackageStore;

pub async fn ensure_system_tenant_async(engine: &Arc<Engine>) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    match engine.create_tenant_async(tenant_id.clone()).await {
        Ok(()) | Err(Error::AlreadyExists(_)) => {}
        Err(error) => return Err(error),
    }

    for schema in system_table_schemas()? {
        engine
            .set_table_schema_async(tenant_id.clone(), schema)
            .await?;
    }

    Ok(())
}

pub async fn prepare_system_tenant_async(
    engine: &Arc<Engine>,
    listen_addr: Option<SocketAddr>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    record_system_status_async(engine, listen_addr).await?;
    seed_system_documents_async(engine, listen_addr).await?;
    sync_all_scheduler_state_async(engine).await
}

pub(crate) async fn record_system_status_async(
    engine: &Arc<Engine>,
    listen_addr: Option<SocketAddr>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let started_at = existing_system_started_at_async(engine).await?;
    let mut details = Map::new();
    if let Some(listen_addr) = listen_addr {
        details.insert("listenAddress".to_owned(), json!(listen_addr.to_string()));
    }
    upsert_system_document_async(
        engine,
        SystemTable::SystemStatus,
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

pub async fn record_service_handle_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    handle: &SandboxHandle,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let service_id = service_document_id(tenant_id, &handle.name);
    delete_service_port_documents_async(engine, &service_id).await?;
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
        engine,
        SystemTable::Services,
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
            "tenantId": tenant_id.as_str(),
            "serviceName": handle.name.as_str(),
            "endpointName": endpoint.name.as_str(),
            "hostPort": endpoint.address.port(),
            "protocol": endpoint_protocol(endpoint.protocol),
            "state": sandbox_status(handle.status),
        }));
        if let Some(guest_port) = endpoint.guest_port {
            fields.insert("guestPort".to_owned(), json!(guest_port));
        }
        upsert_system_document_async(
            engine,
            SystemTable::Ports,
            &service_port_document_id(tenant_id, &handle.name, &endpoint.name),
            fields,
        )
        .await?;
    }

    Ok(())
}

pub async fn record_machine_state_async(
    engine: &Arc<Engine>,
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let paths = config.roots.paths(&config.name);
    upsert_system_document_async(
        engine,
        SystemTable::Machines,
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
        engine,
        SystemTable::Listeners,
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
            engine,
            SystemTable::Ports,
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

pub async fn delete_machine_state_async(engine: &Arc<Engine>, name: &str) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::Machines,
        &machine_document_id(name),
    )
    .await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::Listeners,
        &machine_listener_document_id(name),
    )
    .await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::Ports,
        &machine_port_document_id(name, "ssh"),
    )
    .await?;
    Ok(())
}

pub async fn record_system_event_async(
    engine: &Arc<Engine>,
    source: &str,
    level: &str,
    category: &str,
    message: &str,
    data: Value,
    correlation_id: Option<&str>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    engine
        .insert_document_async(
            system_tenant_id()?,
            SystemTable::Events.table_name()?,
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

#[derive(Clone)]
pub struct SystemTenantStatusEvidenceWriter {
    engine: Arc<Engine>,
    authority: TenantIsolationContext,
}

impl SystemTenantStatusEvidenceWriter {
    pub fn new(engine: Arc<Engine>, authority: TenantIsolationContext) -> Self {
        Self { engine, authority }
    }

    pub fn operator(engine: Arc<Engine>, surface: &'static str) -> Result<Self> {
        Ok(Self::new(
            engine,
            TenantIsolationContext::operator(system_tenant_id()?, surface),
        ))
    }
}

impl StatusEvidenceWriter for SystemTenantStatusEvidenceWriter {
    fn write_status<'a>(&'a self, write: StatusEvidenceWrite<'a>) -> HostLifecycleFuture<'a, ()> {
        Box::pin(async move {
            record_tenant_workload_status_async(
                &self.engine,
                &self.authority,
                write.projection(),
                write.status(),
            )
            .await
        })
    }
}

pub(crate) async fn record_tenant_workload_status_async(
    engine: &Arc<Engine>,
    authority: &TenantIsolationContext,
    projection: &TenantSystemEvidenceProjection,
    status: &TenantWorkloadStatus,
) -> Result<()> {
    authority.ensure_system_or_operator_authority("_nimbus workload status projection")?;
    ensure_status_matches_projection(projection, status)?;
    ensure_system_tenant_async(engine).await?;

    let evidence = json!({
        "lifecycle": status.lifecycle_evidence(),
        "nodeObservation": status.node_observation_ids(),
        "cleanupProgress": status.cleanup_progress(),
        "correlationIds": status.evidence_correlation_ids(),
        "redactedFields": projection.redacted_fields(),
        "workloadSubject": projection.workload_subject(),
    });
    let diagnostics = serde_json::to_value(status.diagnostics())
        .map_err(|error| Error::Serialization(error.to_string()))?;
    upsert_system_document_async(
        engine,
        SystemTable::WorkloadStatus,
        &workload_status_document_id(projection.tenant_id(), projection.workload_uid().as_str()),
        object_fields(json!({
            "tenantId": projection.tenant_id().as_str(),
            "workloadUid": projection.workload_uid().as_str(),
            "decisionId": projection.decision_id().as_str(),
            "observedGeneration": status.observed_generation().as_u64(),
            "nodeId": status.writer_node_id().as_str(),
            "phase": status.phase().label(),
            "target": status.target().label(),
            "evidence": evidence,
            "diagnostics": diagnostics,
            "updatedAt": unix_time_millis()?,
        })),
    )
    .await
}

pub async fn record_table_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let schema = match engine
        .get_table_schema_async(tenant_id.clone(), table.clone())
        .await
    {
        Ok(schema) => Some(schema),
        Err(Error::SchemaNotFound(_)) => None,
        Err(error) => return Err(error),
    };
    let row_count = engine
        .count_table_documents_async(tenant_id.clone(), table.clone())
        .await?;
    let document_id = table_document_id(tenant_id, table);
    if schema.is_none() && row_count == 0 {
        delete_system_document_if_exists_async(engine, SystemTable::Tables, &document_id).await?;
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
    upsert_system_document_async(engine, SystemTable::Tables, &document_id, fields).await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDeploymentRecordInput<'a> {
    pub source_ref: &'a str,
    pub functions: Vec<SystemDeploymentFunctionRecordInput<'a>>,
    pub http_routes: Vec<SystemDeploymentHttpRouteRecordInput<'a>>,
    pub schema_fingerprint: Option<&'a str>,
    pub index_fingerprint: Option<&'a str>,
    pub runtime_bundle_fingerprint: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDeploymentFunctionRecordInput<'a> {
    pub name: &'a str,
    pub kind: &'a str,
    pub fingerprint: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemDeploymentHttpRouteRecordInput<'a> {
    pub key: &'a str,
    pub fingerprint: &'a str,
}

pub async fn record_deployment_state_async(
    engine: &Arc<Engine>,
    input: &SystemDeploymentRecordInput<'_>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let bundle_sha256 = deployment_bundle_sha256(input);
    upsert_system_document_async(
        engine,
        SystemTable::Bundles,
        &bundle_document_id(&bundle_sha256),
        object_fields(json!({
            "sha256": bundle_sha256.as_str(),
            "sourceRef": input.source_ref,
            "status": "active",
        })),
    )
    .await?;

    let active_function_ids = input
        .functions
        .iter()
        .map(|function| function_document_id(&bundle_sha256, function.name))
        .collect::<std::collections::BTreeSet<_>>();
    for function in &input.functions {
        upsert_system_document_async(
            engine,
            SystemTable::Functions,
            &function_document_id(&bundle_sha256, function.name),
            object_fields(json!({
                "bundleId": bundle_sha256.as_str(),
                "path": function.name,
                "kind": function.kind,
            })),
        )
        .await?;
    }
    delete_stale_deployment_documents_async(engine, &bundle_sha256, &active_function_ids).await
}

/// The deploy-captured source package (the read-artifact behind the console
/// Source view) and its modules. The bytes themselves are persisted separately
/// in the content-addressed source-package store; this projects the metadata
/// rows that let the console resolve `module:function` -> module -> package.
/// See the Function Source Visibility plan (FSV3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemSourcePackageRecordInput<'a> {
    pub digest: &'a str,
    pub storage_key: &'a str,
    pub size_bytes: u64,
    pub unpacked_bytes: u64,
    pub modules: Vec<SystemModuleRecordInput<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemModuleRecordInput<'a> {
    pub path: &'a str,
    pub sha256: &'a str,
}

/// Project a deployed source package and its modules into the system tenant,
/// then GC any prior package (and its modules) so the console always reflects
/// the active deployment. Re-recording the same digest is idempotent.
pub async fn record_source_package_state_async(
    engine: &Arc<Engine>,
    input: &SystemSourcePackageRecordInput<'_>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;

    upsert_system_document_async(
        engine,
        SystemTable::SourcePackages,
        &source_package_document_id(input.digest),
        object_fields(json!({
            "digest": input.digest,
            "storageKey": input.storage_key,
            "sizeBytes": input.size_bytes,
            "unpackedBytes": input.unpacked_bytes,
            "status": "active",
        })),
    )
    .await?;

    let active_module_ids = input
        .modules
        .iter()
        .map(|module| module_document_id(input.digest, module.path))
        .collect::<std::collections::BTreeSet<_>>();
    for module in &input.modules {
        upsert_system_document_async(
            engine,
            SystemTable::Modules,
            &module_document_id(input.digest, module.path),
            object_fields(json!({
                "path": module.path,
                "sourcePackageId": input.digest,
                "sha256": module.sha256,
            })),
        )
        .await?;
    }

    delete_stale_source_package_documents_async(engine, input.digest, &active_module_ids).await
}

/// A module's source resolved from the active source package, for the console
/// Source view. Read path (FSV4): module path -> `sourcePackageId` -> CAS bytes
/// (hash-verified by the store) -> the module's source text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModuleSource {
    pub path: String,
    pub source: String,
    pub source_map: Option<String>,
    pub type_info: Option<Value>,
    pub digest: String,
}

/// Resolve a module's source from the content-addressed store, or `None` when
/// the module is unknown. The store verifies the bytes against the digest, so a
/// tampered package fails closed rather than serving wrong source.
pub async fn read_module_source_async(
    engine: &Arc<Engine>,
    store: &dyn SourcePackageStore,
    module_path: &str,
) -> Result<Option<ModuleSource>> {
    let modules = query_system_documents_by_eq_async(
        engine,
        SystemTable::Modules,
        [("path", json!(module_path))],
    )
    .await?;
    let Some(module) = modules.into_iter().next() else {
        return Ok(None);
    };
    let Some(digest) = module.fields.get("sourcePackageId").and_then(Value::as_str) else {
        return Ok(None);
    };
    let bytes = store.get(digest)?;
    let parsed = parse_source_package(&bytes)?;
    let Some(found) = parsed
        .modules
        .into_iter()
        .find(|candidate| candidate.path == module_path)
    else {
        return Ok(None);
    };
    Ok(Some(ModuleSource {
        path: found.path,
        source: found.source,
        source_map: found.source_map,
        type_info: found.type_info,
        digest: digest.to_owned(),
    }))
}

/// All modules (path + source) in the source package that contains
/// `module_path`. Backs the cross-module call graph ("called by"); empty when
/// the module is unknown. See the Function Source Visibility plan (FSV7).
pub async fn read_source_package_modules_async(
    engine: &Arc<Engine>,
    store: &dyn SourcePackageStore,
    module_path: &str,
) -> Result<Vec<(String, String)>> {
    let modules = query_system_documents_by_eq_async(
        engine,
        SystemTable::Modules,
        [("path", json!(module_path))],
    )
    .await?;
    let Some(module) = modules.into_iter().next() else {
        return Ok(Vec::new());
    };
    let Some(digest) = module.fields.get("sourcePackageId").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let bytes = store.get(digest)?;
    let parsed = parse_source_package(&bytes)?;
    Ok(parsed
        .modules
        .into_iter()
        .map(|module| (module.path, module.source))
        .collect())
}

/// All modules (path + source) in the active deployment's source package.
/// Backs the deployment-wide call graph; empty when nothing is deployed. FSV7.
pub async fn read_active_source_package_modules_async(
    engine: &Arc<Engine>,
    store: &dyn SourcePackageStore,
) -> Result<Vec<(String, String)>> {
    let packages = query_system_documents_by_eq_async(
        engine,
        SystemTable::SourcePackages,
        [("status", json!("active"))],
    )
    .await?;
    let Some(package) = packages.into_iter().next() else {
        return Ok(Vec::new());
    };
    let Some(digest) = package.fields.get("digest").and_then(Value::as_str) else {
        return Ok(Vec::new());
    };
    let bytes = store.get(digest)?;
    let parsed = parse_source_package(&bytes)?;
    Ok(parsed
        .modules
        .into_iter()
        .map(|module| (module.path, module.source))
        .collect())
}

pub struct RunRecord<'a> {
    pub tenant_id: &'a TenantId,
    pub function_path: &'a str,
    pub kind: &'a str,
    pub started_at: u64,
    pub duration_ms: f64,
    pub status: &'a str,
    pub error: Option<&'a str>,
}

pub async fn record_run_async(engine: &Arc<Engine>, record: RunRecord<'_>) -> Result<()> {
    if is_system_tenant_id(record.tenant_id) {
        return Ok(());
    }
    ensure_system_tenant_async(engine).await?;
    let mut fields = object_fields(json!({
        "functionPath": record.function_path,
        "kind": record.kind,
        "durationMs": record.duration_ms,
        "status": record.status,
        "startedAt": record.started_at,
    }));
    if let Some(error) = record.error {
        let mut error_value = json!({ "message": error });
        if let (Some(location), Some(map)) =
            (extract_error_location(error), error_value.as_object_mut())
        {
            map.insert("location".to_owned(), json!(location));
        }
        fields.insert("error".to_owned(), error_value);
    }
    engine
        .insert_document_async(system_tenant_id()?, SystemTable::Runs.table_name()?, fields)
        .await?;
    Ok(())
}

/// Lift a `module:line` source location out of a remapped runtime-handler error.
///
/// The runtime remap (codegen `emit/runtime_remap.mjs`) appends ` (at module:line)`
/// to the thrown message so a failed run names the developer's own source line.
/// Storing that location as a structured field lets the console link the failure
/// straight to its source line instead of forcing the reader to parse the
/// message string. Returns `None` for messages without a well-formed location.
fn extract_error_location(message: &str) -> Option<&str> {
    let after = message.find("(at ")? + "(at ".len();
    let rest = &message[after..];
    let close = rest.find(')')?;
    let location = &rest[..close];
    let (module, line) = location.rsplit_once(':')?;
    if module.is_empty() || line.is_empty() || !line.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(location)
}

pub(crate) async fn record_scheduled_job_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    job: &ScheduledJob,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    upsert_system_document_async(
        engine,
        SystemTable::ScheduledJobs,
        &scheduled_job_document_id(tenant_id, &job.id),
        scheduled_job_fields(tenant_id, &job.run_at, &job.mutation, "pending", None)?,
    )
    .await
}

pub async fn record_scheduled_job_result_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    result: &ScheduledJobResult,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let status = match result.outcome {
        ScheduledJobOutcome::Completed => "completed",
        ScheduledJobOutcome::Failed => "failed",
    };
    upsert_system_document_async(
        engine,
        SystemTable::ScheduledJobs,
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

pub async fn delete_scheduled_job_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    job_id: &DocumentId,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::ScheduledJobs,
        &scheduled_job_document_id(tenant_id, job_id),
    )
    .await
}

pub(crate) async fn record_cron_job_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    cron: &CronJob,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    upsert_system_document_async(
        engine,
        SystemTable::CronJobs,
        &cron_job_document_id(tenant_id, &cron.name),
        cron_job_fields(tenant_id, cron)?,
    )
    .await
}

pub async fn delete_cron_job_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    name: &str,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::CronJobs,
        &cron_job_document_id(tenant_id, name),
    )
    .await
}

pub async fn sync_scheduler_state_for_tenant_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let scheduled_jobs = engine.list_scheduled_jobs_async(tenant_id.clone()).await?;
    let active_scheduled_ids = scheduled_jobs
        .iter()
        .map(|job| scheduled_job_document_id(tenant_id, &job.id))
        .collect::<std::collections::BTreeSet<_>>();
    for job in &scheduled_jobs {
        record_scheduled_job_state_async(engine, tenant_id, job).await?;
    }
    delete_stale_scheduler_documents_async(
        engine,
        SystemTable::ScheduledJobs,
        tenant_id,
        "pending",
        &active_scheduled_ids,
    )
    .await?;

    let cron_jobs = engine.list_cron_jobs_async(tenant_id.clone()).await?;
    let active_cron_ids = cron_jobs
        .iter()
        .map(|cron| cron_job_document_id(tenant_id, &cron.name))
        .collect::<std::collections::BTreeSet<_>>();
    for cron in &cron_jobs {
        record_cron_job_state_async(engine, tenant_id, cron).await?;
    }
    delete_stale_scheduler_documents_async(
        engine,
        SystemTable::CronJobs,
        tenant_id,
        "active",
        &active_cron_ids,
    )
    .await
}

pub async fn record_listener_state_async(
    engine: &Arc<Engine>,
    adapter: &str,
    protocol: &str,
    address: &str,
    state: &str,
    version: Option<&str>,
    error: Option<&str>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
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
        engine,
        SystemTable::Listeners,
        &listener_document_id(adapter, protocol),
        fields,
    )
    .await
}

pub async fn record_subscription_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
    query_key: &str,
) -> Result<()> {
    if should_skip_subscription_projection(tenant_id) {
        return Ok(());
    }
    ensure_system_tenant_async(engine).await?;
    upsert_system_document_async(
        engine,
        SystemTable::Subscriptions,
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

pub async fn record_subscription_delivery_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
    query_key: &str,
) -> Result<()> {
    record_subscription_state_async(engine, tenant_id, adapter, subscription_id, query_key).await
}

pub async fn record_subscription_error_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
    query_key: &str,
    error: &str,
) -> Result<()> {
    if should_skip_subscription_projection(tenant_id) {
        return Ok(());
    }
    ensure_system_tenant_async(engine).await?;
    upsert_system_document_async(
        engine,
        SystemTable::Subscriptions,
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

fn should_skip_subscription_projection(tenant_id: &TenantId) -> bool {
    is_system_tenant_id(tenant_id)
}

pub async fn delete_subscription_state_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    adapter: &str,
    subscription_id: u64,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::Subscriptions,
        &subscription_document_id(adapter, tenant_id, subscription_id),
    )
    .await
}

async fn delete_system_document_if_exists_async(
    engine: &Arc<Engine>,
    table: SystemTable,
    document_id: &str,
) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    let table = table.table_name()?;
    let document_id = DocumentId::from_key(document_id.to_owned())?;
    match engine
        .delete_document_async(tenant_id, table, document_id)
        .await
    {
        Ok(()) | Err(Error::DocumentNotFound(_)) => Ok(()),
        Err(error) => Err(error),
    }
}

async fn delete_service_port_documents_async(engine: &Arc<Engine>, service_id: &str) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    let table = SystemTable::Ports.table_name()?;
    let documents = query_system_documents_by_eq_async(
        engine,
        SystemTable::Ports,
        [("serviceId", json!(service_id))],
    )
    .await?;
    for document in documents {
        engine
            .delete_document_async(tenant_id.clone(), table.clone(), document.id)
            .await?;
    }
    Ok(())
}

async fn sync_all_scheduler_state_async(engine: &Arc<Engine>) -> Result<()> {
    let tenants = engine.list_tenants_async().await?;
    for tenant_id in tenants {
        if is_reserved_tenant_id(&tenant_id) {
            continue;
        }
        sync_scheduler_state_for_tenant_async(engine, &tenant_id).await?;
    }
    Ok(())
}

async fn existing_system_started_at_async(engine: &Arc<Engine>) -> Result<u64> {
    let system_tenant = system_tenant_id()?;
    let table = SystemTable::SystemStatus.table_name()?;
    let document_id = DocumentId::from_key("system:server")?;
    match engine
        .get_document_async(system_tenant, table, document_id)
        .await
    {
        Ok(document) => started_at_or_else(&document.fields, unix_time_millis),
        Err(Error::DocumentNotFound(_)) => unix_time_millis(),
        Err(error) => Err(error),
    }
}

fn started_at_or_else<F>(fields: &Map<String, Value>, fallback: F) -> Result<u64>
where
    F: FnOnce() -> Result<u64>,
{
    match fields.get("startedAt").and_then(Value::as_u64) {
        Some(started_at) => Ok(started_at),
        None => fallback(),
    }
}

async fn delete_stale_deployment_documents_async(
    engine: &Arc<Engine>,
    active_bundle_sha256: &str,
    active_function_ids: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let bundles_table = SystemTable::Bundles.table_name()?;
    let bundles = query_system_documents_by_eq_async(
        engine,
        SystemTable::Bundles,
        [("status", json!("active"))],
    )
    .await?;
    for bundle in bundles {
        let Some(bundle_sha256) = bundle.fields.get("sha256").and_then(Value::as_str) else {
            engine
                .delete_document_async(system_tenant.clone(), bundles_table.clone(), bundle.id)
                .await?;
            continue;
        };
        if bundle_sha256 == active_bundle_sha256 {
            continue;
        }
        delete_functions_for_bundle_async(engine, bundle_sha256, |_| true).await?;
        engine
            .delete_document_async(system_tenant.clone(), bundles_table.clone(), bundle.id)
            .await?;
    }

    delete_functions_for_bundle_async(engine, active_bundle_sha256, |function| {
        !active_function_ids.contains(&function.id.to_string())
    })
    .await?;

    Ok(())
}

async fn delete_functions_for_bundle_async(
    engine: &Arc<Engine>,
    bundle_sha256: &str,
    should_delete: impl Fn(&Document) -> bool,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let functions_table = SystemTable::Functions.table_name()?;
    let functions = query_system_documents_by_eq_async(
        engine,
        SystemTable::Functions,
        [("bundleId", json!(bundle_sha256))],
    )
    .await?;
    for function in functions {
        if should_delete(&function) {
            engine
                .delete_document_async(system_tenant.clone(), functions_table.clone(), function.id)
                .await?;
        }
    }
    Ok(())
}

async fn delete_stale_source_package_documents_async(
    engine: &Arc<Engine>,
    active_digest: &str,
    active_module_ids: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let packages_table = SystemTable::SourcePackages.table_name()?;
    let packages = query_system_documents_by_eq_async(
        engine,
        SystemTable::SourcePackages,
        [("status", json!("active"))],
    )
    .await?;
    for package in packages {
        let Some(digest) = package.fields.get("digest").and_then(Value::as_str) else {
            engine
                .delete_document_async(system_tenant.clone(), packages_table.clone(), package.id)
                .await?;
            continue;
        };
        if digest == active_digest {
            continue;
        }
        delete_modules_for_source_package_async(engine, digest, |_| true).await?;
        engine
            .delete_document_async(system_tenant.clone(), packages_table.clone(), package.id)
            .await?;
    }

    delete_modules_for_source_package_async(engine, active_digest, |module| {
        !active_module_ids.contains(&module.id.to_string())
    })
    .await?;

    Ok(())
}

async fn delete_modules_for_source_package_async(
    engine: &Arc<Engine>,
    digest: &str,
    should_delete: impl Fn(&Document) -> bool,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let modules_table = SystemTable::Modules.table_name()?;
    let modules = query_system_documents_by_eq_async(
        engine,
        SystemTable::Modules,
        [("sourcePackageId", json!(digest))],
    )
    .await?;
    for module in modules {
        if should_delete(&module) {
            engine
                .delete_document_async(system_tenant.clone(), modules_table.clone(), module.id)
                .await?;
        }
    }
    Ok(())
}

async fn delete_stale_scheduler_documents_async(
    engine: &Arc<Engine>,
    table: SystemTable,
    tenant_id: &TenantId,
    stale_status: &str,
    active_document_ids: &std::collections::BTreeSet<String>,
) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    let table_name = table.table_name()?;
    let documents = query_system_documents_by_eq_async(
        engine,
        table,
        [
            ("tenantId", json!(tenant_id.as_str())),
            ("status", json!(stale_status)),
        ],
    )
    .await?;
    for document in documents {
        if active_document_ids.contains(&document.id.to_string()) {
            continue;
        }
        engine
            .delete_document_async(system_tenant.clone(), table_name.clone(), document.id)
            .await?;
    }
    Ok(())
}

async fn query_system_documents_by_eq_async(
    engine: &Arc<Engine>,
    table: SystemTable,
    filters: impl IntoIterator<Item = (&'static str, Value)>,
) -> Result<Vec<Document>> {
    engine
        .query_documents_async(
            system_tenant_id()?,
            Query {
                table: table.table_name()?,
                filters: filters
                    .into_iter()
                    .map(|(field, value)| Filter {
                        field: field.to_owned(),
                        op: FilterOp::Eq,
                        value,
                    })
                    .collect(),
                order: None,
                limit: None,
            },
        )
        .await
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

fn deployment_bundle_sha256(input: &SystemDeploymentRecordInput<'_>) -> String {
    if let Some(fingerprint) = input.runtime_bundle_fingerprint {
        return fingerprint.to_owned();
    }

    let mut hasher = Sha256::new();
    hasher.update(b"nimbus-system-deployment-record-v1");
    for function in &input.functions {
        hasher.update(function.name.as_bytes());
        hasher.update([0]);
        hasher.update(function.kind.as_bytes());
        hasher.update([0]);
        hasher.update(function.fingerprint.as_bytes());
        hasher.update([0]);
    }
    for route in &input.http_routes {
        hasher.update(route.key.as_bytes());
        hasher.update([0]);
        hasher.update(route.fingerprint.as_bytes());
        hasher.update([0]);
    }
    if let Some(fingerprint) = input.schema_fingerprint {
        hasher.update(fingerprint.as_bytes());
    }
    hasher.update([0]);
    if let Some(fingerprint) = input.index_fingerprint {
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
    engine: &Arc<Engine>,
    listen_addr: Option<SocketAddr>,
) -> Result<()> {
    for route in route_inventory() {
        upsert_system_document_async(
            engine,
            SystemTable::Routes,
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
            engine,
            SystemTable::AdapterCapabilities,
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
            engine,
            SystemTable::Listeners,
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
    engine: &Arc<Engine>,
    table: SystemTable,
    document_id: &str,
    fields: Map<String, Value>,
) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    let table = table.table_name()?;
    let document_id = DocumentId::from_key(document_id.to_owned())?;

    match engine
        .get_document_async(tenant_id.clone(), table.clone(), document_id.clone())
        .await
    {
        Ok(document) if document.fields == fields => return Ok(()),
        Ok(_) => {
            engine
                .update_document_async(tenant_id, table, document_id, fields)
                .await?;
            return Ok(());
        }
        Err(Error::DocumentNotFound(_)) => {}
        Err(error) => return Err(error),
    }

    match engine
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
            engine
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

fn describe_machine_image_source(source: &nimbus_machine::MachineImageSource) -> String {
    match source {
        nimbus_machine::MachineImageSource::OciReference { reference } => reference.clone(),
        nimbus_machine::MachineImageSource::HttpUrl { url, sha256 } => {
            format!("{url}#sha256={sha256}")
        }
        nimbus_machine::MachineImageSource::LocalDisk { path } => path.display().to_string(),
    }
}

pub fn sandbox_backend(backend: SandboxBackendKind) -> &'static str {
    match backend {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    }
}

pub fn sandbox_status(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Starting => "starting",
        SandboxStatus::Ready => "ready",
        SandboxStatus::NotReady => "not_ready",
        SandboxStatus::Stopping => "stopping",
        SandboxStatus::Stopped => "stopped",
        SandboxStatus::Failed => "failed",
    }
}

pub fn endpoint_protocol(protocol: PublishedEndpointProtocol) -> &'static str {
    match protocol {
        PublishedEndpointProtocol::Tcp => "tcp",
        PublishedEndpointProtocol::Http => "http",
        PublishedEndpointProtocol::Https => "https",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn started_at_or_else_uses_persisted_value_without_calling_fallback() {
        let fields = object_fields(json!({ "startedAt": 42 }));

        let started_at = started_at_or_else(&fields, || {
            Err(Error::Internal(
                "fallback should not run when startedAt is present".to_owned(),
            ))
        })
        .expect("persisted startedAt should be used");

        assert_eq!(started_at, 42);
    }

    #[test]
    fn started_at_or_else_calls_fallback_when_started_at_is_missing_or_invalid() {
        let missing = object_fields(json!({}));
        let invalid = object_fields(json!({ "startedAt": "not-a-number" }));

        assert_eq!(started_at_or_else(&missing, || Ok(7)).unwrap(), 7);
        assert_eq!(started_at_or_else(&invalid, || Ok(9)).unwrap(), 9);
    }

    #[test]
    fn extract_error_location_lifts_remapped_source_location() {
        // The runtime remap appends ` (at module:line)` to the thrown message.
        let message = "runtime JavaScript error: Error: message body must not be empty (at messages:24)\n    at eval";
        assert_eq!(extract_error_location(message), Some("messages:24"));

        // Nested module paths (admin/users) and the first `(at ...)` win.
        assert_eq!(
            extract_error_location("Error: nope (at admin/users:7)"),
            Some("admin/users:7"),
        );
    }

    #[test]
    fn extract_error_location_returns_none_without_a_wellformed_location() {
        assert_eq!(extract_error_location("plain error, no location"), None);
        // Missing line number / malformed are rejected, not stored as garbage.
        assert_eq!(extract_error_location("boom (at messages)"), None);
        assert_eq!(extract_error_location("boom (at messages:abc)"), None);
        assert_eq!(extract_error_location("boom (at :24)"), None);
    }
}
