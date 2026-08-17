use std::net::Ipv4Addr;
use std::num::NonZeroU16;
use std::sync::Arc;

use nimbus_core::{Error, Result};
use nimbus_engine::Engine;
use nimbus_machine::{
    MachineConfigRecord, MachineLifecycle, MachineSshPortLeaseIdentity, MachineStateRecord,
};
use nimbus_network::{
    ListenerId, NetworkCondition, NetworkConditionKind, NetworkConditionState,
    NetworkResourcePhase, PortBindRealm, PortBindTarget, PortBoundEndpoint, PortProtocol,
};
use serde_json::{Value, json};

use crate::identity::system_tenant_id;
use crate::keys::machine_document_id;
use crate::schema::SystemTable;

use super::{
    SystemPortListenerObservation, SystemUnixListenerObservation,
    delete_system_document_if_exists_async, ensure_system_tenant_async, object_fields,
    query_system_documents_by_eq_async, record_port_listener_observation_async,
    record_unix_listener_observation_async, upsert_system_document_async,
};

pub async fn record_machine_state_async(
    engine: &Arc<Engine>,
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<()> {
    let paths = config.roots.paths(&config.name);
    let connectivity = machine_connectivity_observations(config, state)?;
    ensure_system_tenant_async(engine).await?;
    delete_machine_connectivity_async(engine, &config.name).await?;
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

    if let Some((unix_listener, ssh_listener)) = connectivity {
        record_unix_listener_observation_async(engine, &unix_listener).await?;
        record_port_listener_observation_async(engine, &ssh_listener).await?;
    }
    Ok(())
}

pub async fn delete_machine_state_async(engine: &Arc<Engine>, name: &str) -> Result<()> {
    ensure_system_tenant_async(engine).await?;
    delete_machine_connectivity_async(engine, name).await?;
    delete_system_document_if_exists_async(
        engine,
        SystemTable::Machines,
        &machine_document_id(name),
    )
    .await
}

fn machine_connectivity_observations(
    config: &MachineConfigRecord,
    state: &MachineStateRecord,
) -> Result<Option<(SystemUnixListenerObservation, SystemPortListenerObservation)>> {
    if state.lifecycle != MachineLifecycle::Running {
        return Ok(None);
    }
    let Some(runtime) = state.runtime.as_ref() else {
        return Ok(None);
    };
    let port = NonZeroU16::new(runtime.ssh_port).ok_or_else(|| {
        Error::InvalidInput("running machine SSH observation contains port zero".to_owned())
    })?;
    let conditions = ready_conditions();
    let paths = config.roots.paths(&config.name);
    let unix_listener = SystemUnixListenerObservation::new(
        "machine",
        "unix",
        machine_api_listener_id(&config.name),
        runtime.forwarder_authority.generation(),
        Some(
            runtime
                .forwarder_authority
                .provider_instance()
                .provider_id()
                .clone(),
        ),
        paths.api_socket_path.display().to_string(),
        NetworkResourcePhase::Ready,
        conditions.clone(),
    )
    .map_err(connectivity_error)?
    .with_machine_id(&config.name)
    .with_version(env!("CARGO_PKG_VERSION"));
    let identity = MachineSshPortLeaseIdentity::for_listener(&runtime.ssh_listener_id);
    let endpoint = PortBoundEndpoint::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        PortBindTarget::ipv4_specific(Ipv4Addr::LOCALHOST),
        port,
    )
    .map_err(|error| Error::InvalidInput(error.to_string()))?;
    let ssh_listener = SystemPortListenerObservation::for_machine_ssh(
        &identity,
        endpoint,
        NetworkResourcePhase::Ready,
        conditions,
    )
    .map_err(connectivity_error)?
    .with_machine_id(&config.name)
    .with_version(env!("CARGO_PKG_VERSION"));
    Ok(Some((unix_listener, ssh_listener)))
}

async fn delete_machine_connectivity_async(engine: &Arc<Engine>, machine_id: &str) -> Result<()> {
    let system_tenant = system_tenant_id()?;
    for table in [SystemTable::Listeners, SystemTable::Ports] {
        let table_name = table.table_name()?;
        let documents = query_system_documents_by_eq_async(
            engine,
            table,
            [("machineId", Value::String(machine_id.to_owned()))],
        )
        .await?;
        for document in documents {
            engine
                .delete_document_async(system_tenant.clone(), table_name.clone(), document.id)
                .await?;
        }
    }
    Ok(())
}

fn machine_api_listener_id(machine_name: &str) -> ListenerId {
    ListenerId::for_workload_listener(&format!("managed-machine:{machine_name}"), "machine-api")
}

fn ready_conditions() -> Vec<NetworkCondition> {
    vec![
        NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::True),
        NetworkCondition::new(
            NetworkConditionKind::Published,
            NetworkConditionState::False,
        ),
        NetworkCondition::new(
            NetworkConditionKind::CleanupPending,
            NetworkConditionState::False,
        ),
    ]
}

fn connectivity_error(error: impl std::fmt::Display) -> Error {
    Error::InvalidInput(format!("invalid machine connectivity observation: {error}"))
}
