use std::net::SocketAddr;
use std::sync::{Arc, OnceLock};

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
    let projection_epoch = DocumentId::new().to_string();
    record_table_state_for_generation_async(engine, tenant_id, table, &projection_epoch, 0).await
}

pub(crate) async fn record_table_state_for_generation_async(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
    projection_epoch: &str,
    projection_generation: u64,
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
    let delete = schema.is_none() && row_count == 0;

    let mut fields = object_fields(json!({
        "tenantId": tenant_id.as_str(),
        "name": table.as_str(),
        "rowCount": row_count,
        "lastWriteAt": unix_time_millis()?,
        "projectionEpoch": projection_epoch,
        "projectionGeneration": projection_generation,
    }));
    if let Some(schema) = schema {
        fields.insert(
            "schema".to_owned(),
            serde_json::to_value(schema)
                .map_err(|error| Error::Serialization(error.to_string()))?,
        );
    }
    write_table_projection_if_current_async(
        engine,
        &document_id,
        fields,
        projection_epoch,
        projection_generation,
        delete,
    )
    .await
}

/// Fence left behind by a table projection whose row has been deleted.
///
/// A live projection carries its own fence in the stored row, so deleting the
/// row would otherwise discard the only record of how far the projection had
/// advanced. A writer that sampled the table before the delete would then find
/// no row to lose against and recreate the table with its stale row count.
///
/// The fence is deliberately in-process rather than a stored tombstone. The
/// guard rejects a writer only when it carries the *same* epoch, and an epoch
/// identifies one projection runtime in one process: a restart mints a fresh
/// epoch, which must win despite its lower generation. So no writer able to
/// consult this fence can outlive the process that recorded it, and a durable
/// tombstone would add no fencing power while becoming visible to every reader
/// of the `tables` system table, none of which distinguishes a tombstone from a
/// live table.
struct DeletedTableProjectionFence {
    epoch: String,
    generation: u64,
}

async fn write_table_projection_if_current_async(
    engine: &Arc<Engine>,
    document_id: &str,
    fields: Map<String, Value>,
    projection_epoch: &str,
    projection_generation: u64,
    delete: bool,
) -> Result<()> {
    /// Serializes table projection writes and holds the fences retained for
    /// projections whose row is currently deleted.
    static TABLE_PROJECTION_WRITE_STATE: OnceLock<
        tokio::sync::Mutex<std::collections::HashMap<String, DeletedTableProjectionFence>>,
    > = OnceLock::new();

    let document_id = DocumentId::from_key(document_id.to_owned())?;
    let table = SystemTable::Tables.table_name()?;
    let tenant_id = system_tenant_id()?;
    let mut deleted_fences = TABLE_PROJECTION_WRITE_STATE
        .get_or_init(|| tokio::sync::Mutex::new(std::collections::HashMap::new()))
        .lock()
        .await;
    let current = match engine
        .get_document_async(tenant_id.clone(), table.clone(), document_id.clone())
        .await
    {
        Ok(document) => Some(document),
        Err(Error::DocumentNotFound(_)) => None,
        Err(error) => return Err(error),
    };
    let stale = {
        let (current_epoch, current_generation) = match current.as_ref() {
            Some(document) => (
                document
                    .fields
                    .get("projectionEpoch")
                    .and_then(Value::as_str),
                document
                    .fields
                    .get("projectionGeneration")
                    .and_then(Value::as_u64)
                    .unwrap_or(0),
            ),
            // The row is gone, so the fence its delete retained stands in for it.
            None => match deleted_fences.get(document_id.as_str()) {
                Some(fence) => (Some(fence.epoch.as_str()), fence.generation),
                None => (None, 0),
            },
        };
        current_epoch == Some(projection_epoch) && current_generation > projection_generation
    };
    if stale {
        return Ok(());
    }

    if delete {
        // Retained before the delete, and even when there is no row to delete,
        // so the fence is never absent while the row is.
        deleted_fences.insert(
            document_id.as_str().to_owned(),
            DeletedTableProjectionFence {
                epoch: projection_epoch.to_owned(),
                generation: projection_generation,
            },
        );
        if current.is_none() {
            return Ok(());
        }
        engine
            .delete_document_async(tenant_id, table, document_id)
            .await
    } else {
        upsert_system_document_async(engine, SystemTable::Tables, document_id.as_str(), fields)
            .await?;
        // The stored row carries the fence again, so the retained one is
        // redundant. Dropping it here bounds the map by the number of table
        // projections currently deleted and not recreated.
        deleted_fences.remove(document_id.as_str());
        Ok(())
    }
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

pub fn endpoint_protocol(protocol: PublishedEndpointProtocol) -> &'static str {
    match protocol {
        PublishedEndpointProtocol::Tcp => "tcp",
        PublishedEndpointProtocol::Http => "http",
        PublishedEndpointProtocol::Https => "https",
    }
}

#[cfg(test)]
mod tests {
    use nimbus_testing::EngineFixture;

    use super::*;

    /// Builds the payload a projection sampling `row_count` rows would write.
    fn sampled_fields(
        tenant_id: &TenantId,
        table: &TableName,
        row_count: u64,
        projection_epoch: &str,
        projection_generation: u64,
    ) -> Map<String, Value> {
        object_fields(json!({
            "tenantId": tenant_id.as_str(),
            "name": table.as_str(),
            "rowCount": row_count,
            "lastWriteAt": 1_700_000_000_000_u64,
            "projectionEpoch": projection_epoch,
            "projectionGeneration": projection_generation,
        }))
    }

    /// Reads the projection the way every consumer does: an unfiltered listing
    /// of the `tables` system table, matched by tenant and table name. This is
    /// the shape of the `tables:list` Convex query the console reads through,
    /// so a row visible here is a row the console renders as a live table.
    async fn listed_projection_row(
        engine: &Arc<Engine>,
        tenant_id: &TenantId,
        table: &TableName,
    ) -> Option<Document> {
        engine
            .list_documents_async(
                system_tenant_id().expect("system tenant id should build"),
                SystemTable::Tables
                    .table_name()
                    .expect("system tables name should build"),
            )
            .await
            .expect("projected table records should list")
            .into_iter()
            .find(|row| {
                row.fields.get("tenantId") == Some(&json!(tenant_id.as_str()))
                    && row.fields.get("name") == Some(&json!(table.as_str()))
            })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn same_epoch_stale_write_does_not_resurrect_a_deleted_table_projection() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-delete-fence", Engine::create_tenant);
        let table = TableName::new("tasks").expect("table name should build");
        let document_id = table_document_id(&tenant_id, &table);
        let epoch = "projection-delete-fence-epoch";
        ensure_system_tenant_async(&engine)
            .await
            .expect("system tenant should prepare");

        // An observer samples two rows at generation 5 and projects them.
        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 2, epoch, 5),
            epoch,
            5,
            false,
        )
        .await
        .expect("the sampled projection should record two rows");
        assert_eq!(
            listed_projection_row(&engine, &tenant_id, &table)
                .await
                .and_then(|row| row.fields.get("rowCount").and_then(Value::as_u64)),
            Some(2),
        );

        // The table empties, and generation 6 deletes the projection row.
        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 0, epoch, 6),
            epoch,
            6,
            true,
        )
        .await
        .expect("the emptied table should delete its projection row");
        assert!(
            listed_projection_row(&engine, &tenant_id, &table)
                .await
                .is_none(),
            "deleting the projection must remove the table from every consumer's view"
        );

        // A writer from the same epoch that sampled before the delete now
        // lands. Its generation is older, so it must lose even though the
        // delete took away the row that used to carry the fence.
        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 2, epoch, 5),
            epoch,
            5,
            false,
        )
        .await
        .expect("the stale same-epoch write should be rejected without an error");

        assert!(
            listed_projection_row(&engine, &tenant_id, &table)
                .await
                .is_none(),
            "a stale same-epoch write must not resurrect a deleted table projection"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn fresh_epoch_writes_over_a_deleted_projection_despite_a_lower_generation() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-delete-restart", Engine::create_tenant);
        let table = TableName::new("tasks").expect("table name should build");
        let document_id = table_document_id(&tenant_id, &table);
        let epoch = "projection-delete-restart-epoch";
        let restarted_epoch = "projection-delete-restart-fresh-epoch";
        ensure_system_tenant_async(&engine)
            .await
            .expect("system tenant should prepare");

        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 3, epoch, 9),
            epoch,
            9,
            false,
        )
        .await
        .expect("the sampled projection should record three rows");
        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 0, epoch, 10),
            epoch,
            10,
            true,
        )
        .await
        .expect("the emptied table should delete its projection row");

        // A restarted process mints a fresh epoch and resets its generation
        // counter, so it must win against the retained fence.
        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 4, restarted_epoch, 1),
            restarted_epoch,
            1,
            false,
        )
        .await
        .expect("a fresh process epoch should project despite its lower generation");
        assert_eq!(
            listed_projection_row(&engine, &tenant_id, &table)
                .await
                .and_then(|row| row.fields.get("rowCount").and_then(Value::as_u64)),
            Some(4),
            "the retained delete fence must not block a fresh epoch"
        );

        // The restored row carries the fence again, so a stale writer from the
        // fresh epoch still loses to it.
        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 8, restarted_epoch, 0),
            restarted_epoch,
            0,
            false,
        )
        .await
        .expect("the stale same-epoch write should be rejected without an error");
        assert_eq!(
            listed_projection_row(&engine, &tenant_id, &table)
                .await
                .and_then(|row| row.fields.get("rowCount").and_then(Value::as_u64)),
            Some(4),
            "an older generation must not overwrite a newer live projection row"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_deleted_table_projection_leaves_nothing_for_consumers_to_read() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let engine = fixture.engine();
        let tenant_id = fixture.create_tenant("projection-delete-consumers", Engine::create_tenant);
        let table = TableName::new("tasks").expect("table name should build");
        let document_id = table_document_id(&tenant_id, &table);
        let epoch = "projection-delete-consumers-epoch";
        ensure_system_tenant_async(&engine)
            .await
            .expect("system tenant should prepare");

        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 5, epoch, 2),
            epoch,
            2,
            false,
        )
        .await
        .expect("the sampled projection should record five rows");
        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 0, epoch, 3),
            epoch,
            3,
            true,
        )
        .await
        .expect("the emptied table should delete its projection row");
        write_table_projection_if_current_async(
            &engine,
            &document_id,
            sampled_fields(&tenant_id, &table, 5, epoch, 2),
            epoch,
            2,
            false,
        )
        .await
        .expect("the stale same-epoch write should be rejected without an error");

        // Consumers list the `tables` system table and treat row existence as
        // table liveness, so the fence must leave no row of any kind behind.
        let rows = engine
            .list_documents_async(
                system_tenant_id().expect("system tenant id should build"),
                SystemTable::Tables
                    .table_name()
                    .expect("system tables name should build"),
            )
            .await
            .expect("projected table records should list");
        assert!(
            !rows.iter().any(|row| {
                row.fields.get("tenantId") == Some(&json!(tenant_id.as_str()))
                    && row.fields.get("name") == Some(&json!(table.as_str()))
            }),
            "a deleted table must not be listed, counted, or rendered as a live table"
        );
        assert!(
            !rows
                .iter()
                .any(|row| row.id.as_str() == document_id.as_str()),
            "the delete fence must not be stored where consumers can read it"
        );
    }

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
