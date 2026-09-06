use std::net::SocketAddr;
use std::sync::Arc;

use nimbus_core::{
    Document, DocumentId, Error, Filter, FilterOp, Query, Result, TableName, TenantId,
};
use nimbus_engine::{Engine, ProjectionToken};
use nimbus_network::EndpointProtocol;
use nimbus_node::{
    HostLifecycleFuture, StatusEvidenceWrite, StatusEvidenceWriter, TenantWorkloadStatus,
    ensure_status_matches_projection,
};
use nimbus_sandbox::{SandboxBackendKind, SandboxStatus};
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::TenantSystemEvidenceProjection;
use serde_json::{Map, Value, json};

use crate::identity::system_tenant_id;
use crate::inventory::{adapter_capability_inventory, route_inventory};
use crate::keys::workload_status_document_id;
use crate::projection::publication::{
    ProjectionPublication, ProjectionPublicationOutcome, publish_table_projection_async,
};
use crate::schema::{SystemTable, projection_fence_table_schema, system_table_schemas};

mod connectivity;
mod deployment;
mod machine;
mod run;
mod scheduler;
mod source;
mod subscription;

pub(crate) use connectivity::replace_server_port_listener_observations_async;
pub use connectivity::{
    SystemConnectivityObservationError, SystemPortListenerObservation,
    SystemPublishedEndpointObservation, SystemServiceConnectivityObservation,
    SystemUnixListenerObservation, claim_server_listener_projection_async,
    record_port_listener_observation_async, record_service_connectivity_observation_async,
    record_unix_listener_observation_async,
};
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
    delete_subscription_state_async, record_subscription_delivery_async,
    record_subscription_error_async, record_subscription_state_async,
};

pub async fn ensure_system_tenant_async(engine: &Arc<Engine>) -> Result<()> {
    let tenant_id = system_tenant_id()?;
    engine.ensure_tenant_ready_async(tenant_id.clone()).await?;

    for schema in system_table_schemas()? {
        engine
            .set_table_schema_async(tenant_id.clone(), schema)
            .await?;
    }
    engine
        .set_table_schema_async(tenant_id, projection_fence_table_schema()?)
        .await?;

    Ok(())
}

pub async fn prepare_system_tenant_async(
    engine: &Arc<Engine>,
    listen_addr: Option<SocketAddr>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    record_system_status_async(engine, listen_addr).await?;
    seed_system_documents_async(engine).await?;
    scheduler::sync_all_scheduler_state_async(engine).await
}

pub(crate) async fn record_system_status_async(
    engine: &Arc<Engine>,
    listen_addr: Option<SocketAddr>,
) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    let started_at = unix_time_millis()?;
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
    let fields = system_event_fields(
        source,
        level,
        category,
        message,
        data,
        correlation_id,
        unix_time_millis()?,
    );
    engine
        .insert_document_async(
            system_tenant_id()?,
            SystemTable::Events.table_name()?,
            fields,
        )
        .await?;
    Ok(())
}

fn system_event_fields(
    source: &str,
    level: &str,
    category: &str,
    message: &str,
    data: Value,
    correlation_id: Option<&str>,
    created_at: u64,
) -> Map<String, Value> {
    let mut fields = object_fields(json!({
        "source": source,
        "level": level,
        "category": category,
        "message": message,
        "data": data,
        "createdAt": created_at,
    }));
    if let Some(correlation_id) = correlation_id {
        fields.insert("correlationId".to_owned(), json!(correlation_id));
    }
    fields
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
    let execution_id = status.execution_id();
    upsert_system_document_async(
        engine,
        SystemTable::WorkloadStatus,
        &workload_status_document_id(projection.tenant_id(), projection.workload_uid().as_str()),
        object_fields(json!({
            "tenantId": projection.tenant_id().as_str(),
            "workloadUid": projection.workload_uid().as_str(),
            "executionId": execution_id.as_str(),
            "decisionId": projection.decision_id().as_str(),
            "observedGeneration": status.observed_generation().to_string(),
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
    let projection_epoch = DocumentId::new().to_string();
    let projection_token = engine.projection_token_for_tenant_async(tenant_id).await?;
    record_table_state_for_generation_async(
        engine,
        tenant_id,
        table,
        projection_token,
        &projection_epoch,
        0,
    )
    .await
    .map(|_| ())
}

pub(crate) async fn record_table_state_for_generation_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
    projection_token: ProjectionToken,
    projection_epoch: &str,
    projection_generation: u64,
) -> Result<ProjectionPublicationOutcome> {
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
    let delete = schema.is_none() && row_count == 0;

    let mut fields = object_fields(json!({
        "tenantId": tenant_id.as_str(),
        "name": table.as_str(),
        "rowCount": row_count,
        "lastWriteAt": unix_time_millis()?,
        "projectionEpoch": projection_epoch,
        "projectionGeneration": projection_generation,
        "sourceTenantIncarnation": projection_token.tenant_incarnation,
        "sourceLeaseEpoch": projection_token.lease_epoch,
        "sourceDurableSequence": projection_token.durable_sequence.0,
    }));
    if let Some(schema) = schema {
        fields.insert(
            "schema".to_owned(),
            serde_json::to_value(schema)
                .map_err(|error| Error::Serialization(error.to_string()))?,
        );
    }
    publish_table_projection_async(
        engine,
        ProjectionPublication {
            tenant_id: tenant_id.clone(),
            table: table.clone(),
            token: projection_token,
            visible_fields: fields,
            delete_visible: delete,
        },
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

async fn seed_system_documents_async(engine: &Arc<Engine>) -> Result<()> {
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

    Ok(())
}

fn unix_time_millis() -> Result<u64> {
    Ok(nimbus_core::clock::system_now_millis())
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

pub fn endpoint_protocol(protocol: EndpointProtocol) -> &'static str {
    match protocol {
        EndpointProtocol::Tcp => "tcp",
        EndpointProtocol::Http => "http",
        EndpointProtocol::Https => "https",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recording_system_status_replaces_a_persisted_process_start_time() {
        let fixture = nimbus_testing::EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        ensure_system_tenant_async(&engine)
            .await
            .expect("system tenant should prepare");
        upsert_system_document_async(
            &engine,
            SystemTable::SystemStatus,
            "system:server",
            object_fields(json!({
                "name": "server",
                "version": "old",
                "health": "ok",
                "startedAt": 42,
                "updatedAt": 42,
                "details": {},
            })),
        )
        .await
        .expect("old process status should seed");

        record_system_status_async(&engine, None)
            .await
            .expect("new process status should replace the old start time");

        let status = engine
            .get_document_async(
                system_tenant_id().expect("system tenant id should validate"),
                SystemTable::SystemStatus
                    .table_name()
                    .expect("system status table should validate"),
                DocumentId::from_key("system:server").expect("status id should validate"),
            )
            .await
            .expect("system status should exist");
        assert!(
            status.fields["startedAt"]
                .as_u64()
                .is_some_and(|value| value > 42),
            "server uptime must begin with the current process: {status:?}"
        );
    }

    #[test]
    fn system_event_omits_absent_optional_correlation_id() {
        let absent = system_event_fields(
            "system",
            "info",
            "lifecycle",
            "server shutdown requested",
            json!({}),
            None,
            42,
        );
        assert!(
            !absent.contains_key("correlationId"),
            "an absent optional field must be omitted instead of written as schema-invalid null"
        );

        let present = system_event_fields(
            "system",
            "info",
            "lifecycle",
            "server shutdown requested",
            json!({}),
            Some("request-7"),
            42,
        );
        assert_eq!(present.get("correlationId"), Some(&json!("request-7")));
    }
}
