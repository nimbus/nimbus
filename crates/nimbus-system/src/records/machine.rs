use std::sync::Arc;

use nimbus_core::Result;
use nimbus_engine::Engine;
use nimbus_machine::{MachineConfigRecord, MachineLifecycle, MachineStateRecord};
use serde_json::json;

use crate::keys::{machine_document_id, machine_listener_document_id, machine_port_document_id};
use crate::schema::SystemTable;

use super::{
    delete_system_document_if_exists_async, ensure_system_tenant_async, object_fields,
    upsert_system_document_async,
};

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
                "image": config.guest.image_source.as_source_string(),
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
