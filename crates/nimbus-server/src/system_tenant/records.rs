use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nimbus_core::{
    CronJob, CronSchedule, DocumentId, Error, Mutation, Result, ScheduledJob, ScheduledJobOutcome,
    ScheduledJobResult, TableName, TenantId,
};
use nimbus_engine::Service;
use nimbus_machine::{MachineConfigRecord, MachineLifecycle, MachineStateRecord};
use nimbus_sandbox::{PublishedEndpointProtocol, SandboxBackendKind, SandboxHandle, SandboxStatus};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

#[cfg(test)]
use crate::local_enforcement::{TenantSystemEvidenceProjection, TenantWorkloadStatus};
#[cfg(test)]
use crate::tenant::TenantIsolationContext;

use super::identity::{is_reserved_tenant_id, is_system_tenant_id, system_tenant_id};
use super::inventory::{adapter_capability_inventory, route_inventory};
#[cfg(test)]
use super::keys::workload_status_document_id;
use super::keys::{
    bundle_document_id, cron_job_document_id, function_document_id, listener_document_id,
    machine_document_id, machine_listener_document_id, machine_port_document_id,
    scheduled_job_document_id, service_document_id, service_port_document_id,
    subscription_document_id, table_document_id,
};
use super::schema::system_table_schemas;

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

#[cfg(test)]
pub(crate) async fn record_tenant_workload_status_async(
    service: &Arc<Service>,
    authority: &TenantIsolationContext,
    projection: &TenantSystemEvidenceProjection,
    status: &TenantWorkloadStatus,
) -> Result<()> {
    authority.ensure_system_or_operator_authority("_nimbus workload status projection")?;
    projection.ensure_status_matches(status)?;
    ensure_system_tenant_async(service).await?;

    let evidence = json!({
        "lifecycle": status.lifecycle_evidence(),
        "nodeObservation": status.node_observation_ids(),
        "cleanupProgress": status.cleanup_progress(),
        "correlationIds": status.evidence_correlation_ids(),
        "redactedFields": projection.redacted_fields(),
        "workloadStableId": projection.workload_stable_id(),
    });
    let diagnostics = serde_json::to_value(status.diagnostics())
        .map_err(|error| Error::Serialization(error.to_string()))?;
    upsert_system_document_async(
        service,
        "workload_status",
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
