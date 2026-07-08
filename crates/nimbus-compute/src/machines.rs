//! Machine lifecycle orchestration extracted from `nimbus-server`'s
//! `http::machines` handlers (CP3). Machine routes carry no HeaderMap/authz
//! coupling at all (operator-only local admin surface), so the handler
//! reduces to transport-input extraction, machine-name parsing, and wrapping the
//! response — every manager call and state/event recording call lives here.

use std::path::PathBuf;
use std::sync::Arc;

use nimbus_machine::{
    MachineConfigRecord, MachineGuestProvisioning, MachineImageSource, MachineLifecycle,
    MachineManagerState, MachineRuntimeState, MachineStateRecord, MachineVolume,
};
use serde::{Deserialize, Serialize};

use crate::machine_lifecycle::{
    MachineCreateRequest, MachineLifecycleManager, MachineUpdateRequest,
};
use crate::state::{ComputeError, ComputeState};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineLifecycleResponse {
    name: String,
    provider: String,
    state: String,
    manager: String,
    resources: MachineResourcesResponse,
    guest: MachineGuestResponse,
    runtime: Option<MachineRuntimeResponse>,
    last_error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineResourcesResponse {
    cpus: u8,
    #[serde(rename = "memoryMiB")]
    memory_mib: u32,
    #[serde(rename = "diskGiB")]
    disk_gib: u32,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineGuestResponse {
    image: String,
    provisioning: String,
    ssh_user: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineRuntimeResponse {
    image_path: String,
    ssh_port: u16,
    rest_uri: String,
    ready_vsock_port: u32,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineCreateRequestBody {
    pub cpus: Option<u8>,
    #[serde(rename = "memoryMiB", alias = "memoryMib")]
    pub memory_mib: Option<u32>,
    #[serde(rename = "diskGiB", alias = "diskGib")]
    pub disk_gib: Option<u32>,
    pub image: Option<String>,
    #[serde(alias = "sshIdentityPath")]
    pub ssh_identity: Option<PathBuf>,
    #[serde(alias = "ignitionFilePath")]
    pub ignition_file: Option<PathBuf>,
    pub bootc_native: Option<bool>,
    #[serde(alias = "efiStorePath")]
    pub efi_store: Option<PathBuf>,
    pub volumes: Option<Vec<MachineVolume>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineUpdateRequestBody {
    pub cpus: Option<u8>,
    #[serde(rename = "memoryMiB", alias = "memoryMib")]
    pub memory_mib: Option<u32>,
    #[serde(rename = "diskGiB", alias = "diskGib")]
    pub disk_gib: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineDeleteResponse {
    name: String,
    state: String,
    previous_state: String,
}

pub async fn create_machine(
    compute: &ComputeState,
    name: String,
    request: MachineCreateRequestBody,
) -> Result<MachineLifecycleResponse, ComputeError> {
    let manager = machine_lifecycle_manager(compute)?;
    let snapshot = manager
        .create_machine(MachineCreateRequest {
            name,
            cpus: request.cpus,
            memory_mib: request.memory_mib,
            disk_gib: request.disk_gib,
            image: request.image,
            ssh_identity: request.ssh_identity,
            ignition_file: request.ignition_file,
            bootc_native: request.bootc_native.unwrap_or(false),
            efi_store: request.efi_store,
            volumes: request.volumes.unwrap_or_default(),
        })
        .await?;
    record_machine_snapshot(compute, &snapshot.config, &snapshot.state).await?;
    record_machine_event(compute, "create", &snapshot.config, &snapshot.state).await?;
    Ok(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    ))
}

pub async fn start_machine(
    compute: &ComputeState,
    name: &str,
) -> Result<MachineLifecycleResponse, ComputeError> {
    let manager = machine_lifecycle_manager(compute)?;
    let snapshot = manager.start_machine(name).await?;
    record_machine_snapshot(compute, &snapshot.config, &snapshot.state).await?;
    record_machine_event(compute, "start", &snapshot.config, &snapshot.state).await?;
    Ok(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    ))
}

pub async fn stop_machine(
    compute: &ComputeState,
    name: &str,
) -> Result<MachineLifecycleResponse, ComputeError> {
    let manager = machine_lifecycle_manager(compute)?;
    let snapshot = manager.stop_machine(name).await?;
    record_machine_snapshot(compute, &snapshot.config, &snapshot.state).await?;
    record_machine_event(compute, "stop", &snapshot.config, &snapshot.state).await?;
    Ok(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    ))
}

pub async fn restart_machine(
    compute: &ComputeState,
    name: &str,
) -> Result<MachineLifecycleResponse, ComputeError> {
    let manager = machine_lifecycle_manager(compute)?;
    let snapshot = manager.restart_machine(name).await?;
    record_machine_snapshot(compute, &snapshot.config, &snapshot.state).await?;
    record_machine_event(compute, "restart", &snapshot.config, &snapshot.state).await?;
    Ok(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    ))
}

pub async fn update_machine(
    compute: &ComputeState,
    name: String,
    request: MachineUpdateRequestBody,
) -> Result<MachineLifecycleResponse, ComputeError> {
    let manager = machine_lifecycle_manager(compute)?;
    let snapshot = manager
        .update_machine(MachineUpdateRequest {
            name,
            cpus: request.cpus,
            memory_mib: request.memory_mib,
            disk_gib: request.disk_gib,
        })
        .await?;
    record_machine_snapshot(compute, &snapshot.config, &snapshot.state).await?;
    record_machine_event(compute, "update", &snapshot.config, &snapshot.state).await?;
    Ok(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    ))
}

pub async fn delete_machine(
    compute: &ComputeState,
    name: &str,
) -> Result<MachineDeleteResponse, ComputeError> {
    let manager = machine_lifecycle_manager(compute)?;
    let snapshot = manager.delete_machine(name).await?;
    nimbus_system::delete_machine_state_async(&compute.engine, &snapshot.config.name)
        .await
        .map_err(ComputeError::from)?;
    record_machine_delete_event(compute, &snapshot.config, &snapshot.state).await?;
    Ok(MachineDeleteResponse {
        name: snapshot.config.name,
        state: "deleted".to_owned(),
        previous_state: snapshot.state.lifecycle.as_str().to_owned(),
    })
}

impl MachineLifecycleResponse {
    fn from_snapshot(config: &MachineConfigRecord, state: &MachineStateRecord) -> Self {
        Self {
            name: config.name.clone(),
            provider: config.provider.as_str().to_owned(),
            state: machine_lifecycle(state.lifecycle).to_owned(),
            manager: machine_manager_state(state.manager).to_owned(),
            resources: MachineResourcesResponse {
                cpus: config.resources.cpus,
                memory_mib: config.resources.memory_mib,
                disk_gib: config.resources.disk_gib,
            },
            guest: MachineGuestResponse {
                image: describe_image_source(&config.guest.image_source),
                provisioning: machine_provisioning(config.guest.provisioning).to_owned(),
                ssh_user: config.guest.ssh_user.clone(),
            },
            runtime: state.runtime.as_ref().map(MachineRuntimeResponse::from),
            last_error: state.last_error.clone(),
        }
    }
}

impl From<&MachineRuntimeState> for MachineRuntimeResponse {
    fn from(runtime: &MachineRuntimeState) -> Self {
        Self {
            image_path: runtime.image_path.display().to_string(),
            ssh_port: runtime.ssh_port,
            rest_uri: runtime.rest_uri.clone(),
            ready_vsock_port: runtime.ready_vsock_port,
        }
    }
}

async fn record_machine_snapshot(
    compute: &ComputeState,
    config: &MachineConfigRecord,
    snapshot: &MachineStateRecord,
) -> Result<(), ComputeError> {
    nimbus_system::record_machine_state_async(&compute.engine, config, snapshot)
        .await
        .map_err(ComputeError::from)
}

async fn record_machine_event(
    compute: &ComputeState,
    action: &str,
    config: &MachineConfigRecord,
    snapshot: &MachineStateRecord,
) -> Result<(), ComputeError> {
    let message = format!(
        "machine `{}` {} completed with state {}",
        config.name,
        action,
        snapshot.lifecycle.as_str()
    );
    let correlation_id = format!("machine:{}:{action}", config.name);
    nimbus_system::record_system_event_async(
        &compute.engine,
        "machine",
        "info",
        "machine.lifecycle",
        &message,
        serde_json::json!({
            "action": action,
            "machineId": config.name.as_str(),
            "state": snapshot.lifecycle.as_str(),
            "manager": snapshot.manager.as_str(),
            "provider": config.provider.as_str(),
        }),
        Some(&correlation_id),
    )
    .await
    .map_err(ComputeError::from)
}

async fn record_machine_delete_event(
    compute: &ComputeState,
    config: &MachineConfigRecord,
    snapshot: &MachineStateRecord,
) -> Result<(), ComputeError> {
    let message = format!(
        "machine `{}` delete completed from state {}",
        config.name,
        snapshot.lifecycle.as_str()
    );
    let correlation_id = format!("machine:{}:delete", config.name);
    nimbus_system::record_system_event_async(
        &compute.engine,
        "machine",
        "info",
        "machine.lifecycle",
        &message,
        serde_json::json!({
            "action": "delete",
            "machineId": config.name.as_str(),
            "state": "deleted",
            "previousState": snapshot.lifecycle.as_str(),
            "manager": snapshot.manager.as_str(),
            "provider": config.provider.as_str(),
        }),
        Some(&correlation_id),
    )
    .await
    .map_err(ComputeError::from)
}

fn machine_lifecycle_manager(
    compute: &ComputeState,
) -> Result<Arc<dyn MachineLifecycleManager>, ComputeError> {
    compute.machine_lifecycle_manager().ok_or_else(|| {
        ComputeError::not_found(
            "machine lifecycle endpoints require a server-owned machine manager",
        )
    })
}

fn describe_image_source(source: &MachineImageSource) -> String {
    match source {
        MachineImageSource::OciReference { reference } => reference.clone(),
        MachineImageSource::HttpUrl { url, sha256 } => format!("{url}#sha256={sha256}"),
        MachineImageSource::LocalDisk { path } => path.display().to_string(),
    }
}

fn machine_lifecycle(state: MachineLifecycle) -> &'static str {
    state.as_str()
}

fn machine_manager_state(state: MachineManagerState) -> &'static str {
    state.as_str()
}

fn machine_provisioning(provisioning: MachineGuestProvisioning) -> &'static str {
    match provisioning {
        MachineGuestProvisioning::Ignition => "ignition",
        MachineGuestProvisioning::BootcMachineConfig => "bootc-machine-config",
    }
}
