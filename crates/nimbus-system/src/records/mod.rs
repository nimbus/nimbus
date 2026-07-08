use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use nimbus_core::{
    Document, DocumentId, Error, Filter, FilterOp, Query, Result, TableName, TenantId,
};
use nimbus_engine::Engine;
use nimbus_node::{
    HostLifecycleFuture, StatusEvidenceWrite, StatusEvidenceWriter, TenantWorkloadStatus,
    ensure_status_matches_projection,
};
use nimbus_sandbox::{PublishedEndpointProtocol, SandboxBackendKind, SandboxHandle, SandboxStatus};
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::TenantSystemEvidenceProjection;
use serde_json::{Map, Value, json};

use crate::identity::system_tenant_id;
use crate::inventory::{adapter_capability_inventory, route_inventory};
use crate::keys::{
    service_document_id, service_port_document_id, table_document_id, workload_status_document_id,
};
use crate::schema::{SystemTable, system_table_schemas};

mod deployment;
mod machine;
mod run;
mod scheduler;
mod source;
mod subscription;

pub use deployment::{
    SystemDeploymentFunctionRecordInput, SystemDeploymentHttpRouteRecordInput,
    SystemDeploymentRecordInput, record_deployment_state_async,
};
pub use machine::{delete_machine_state_async, record_machine_state_async};
pub use run::{RunRecord, record_run_async};
#[cfg(test)]
pub(crate) use scheduler::record_scheduled_job_state_async;
pub use scheduler::{
    delete_cron_job_state_async, delete_scheduled_job_state_async,
    record_scheduled_job_result_state_async, sync_scheduler_state_for_tenant_async,
};
pub use source::{
    ModuleSource, SystemModuleRecordInput, SystemSourcePackageRecordInput,
    read_active_source_package_modules_async, read_module_source_async,
    read_source_package_modules_async, record_source_package_state_async,
};
pub use subscription::{
    delete_subscription_state_async, record_listener_state_async,
    record_subscription_delivery_async, record_subscription_error_async,
    record_subscription_state_async,
};

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
    scheduler::sync_all_scheduler_state_async(engine).await
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
}
