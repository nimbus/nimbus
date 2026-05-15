use nimbus_machine::{
    MachineConfigRecord, MachineGuestProvisioning, MachineImageSource, MachineLifecycle,
    MachineManagerState, MachineRuntimeState, MachineStateRecord, MachineVolume,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::*;
use crate::machine_lifecycle::{MachineCreateRequest, MachineUpdateRequest};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MachineLifecycleResponse {
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
pub(crate) struct MachineCreateRequestBody {
    cpus: Option<u8>,
    #[serde(rename = "memoryMiB", alias = "memoryMib")]
    memory_mib: Option<u32>,
    #[serde(rename = "diskGiB", alias = "diskGib")]
    disk_gib: Option<u32>,
    image: Option<String>,
    #[serde(alias = "sshIdentityPath")]
    ssh_identity: Option<PathBuf>,
    #[serde(alias = "ignitionFilePath")]
    ignition_file: Option<PathBuf>,
    bootc_native: Option<bool>,
    #[serde(alias = "efiStorePath")]
    efi_store: Option<PathBuf>,
    volumes: Option<Vec<MachineVolume>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MachineUpdateRequestBody {
    cpus: Option<u8>,
    #[serde(rename = "memoryMiB", alias = "memoryMib")]
    memory_mib: Option<u32>,
    #[serde(rename = "diskGiB", alias = "diskGib")]
    disk_gib: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MachineDeleteResponse {
    name: String,
    state: String,
    previous_state: String,
}

pub(crate) async fn create_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<MachineCreateRequestBody>,
) -> Result<Json<MachineLifecycleResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let manager = machine_lifecycle_manager(&state)?;
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
    record_machine_snapshot(&state, &snapshot.config, &snapshot.state).await?;
    record_machine_event(&state, "create", &snapshot.config, &snapshot.state).await?;
    Ok(Json(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    )))
}

pub(crate) async fn start_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<MachineLifecycleResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let manager = machine_lifecycle_manager(&state)?;
    let snapshot = manager.start_machine(&name).await?;
    record_machine_snapshot(&state, &snapshot.config, &snapshot.state).await?;
    record_machine_event(&state, "start", &snapshot.config, &snapshot.state).await?;
    Ok(Json(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    )))
}

pub(crate) async fn stop_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<MachineLifecycleResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let manager = machine_lifecycle_manager(&state)?;
    let snapshot = manager.stop_machine(&name).await?;
    record_machine_snapshot(&state, &snapshot.config, &snapshot.state).await?;
    record_machine_event(&state, "stop", &snapshot.config, &snapshot.state).await?;
    Ok(Json(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    )))
}

pub(crate) async fn restart_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<MachineLifecycleResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let manager = machine_lifecycle_manager(&state)?;
    let snapshot = manager.restart_machine(&name).await?;
    record_machine_snapshot(&state, &snapshot.config, &snapshot.state).await?;
    record_machine_event(&state, "restart", &snapshot.config, &snapshot.state).await?;
    Ok(Json(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    )))
}

pub(crate) async fn update_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<MachineUpdateRequestBody>,
) -> Result<Json<MachineLifecycleResponse>, AppError> {
    let name = parse_machine_name(name)?;
    if request.cpus.is_none() && request.memory_mib.is_none() && request.disk_gib.is_none() {
        return Err(AppError::from(nimbus_core::Error::InvalidInput(
            "machine update requires at least one of `cpus`, `memoryMiB`, or `diskGiB`".to_owned(),
        )));
    }
    let manager = machine_lifecycle_manager(&state)?;
    let snapshot = manager
        .update_machine(MachineUpdateRequest {
            name,
            cpus: request.cpus,
            memory_mib: request.memory_mib,
            disk_gib: request.disk_gib,
        })
        .await?;
    record_machine_snapshot(&state, &snapshot.config, &snapshot.state).await?;
    record_machine_event(&state, "update", &snapshot.config, &snapshot.state).await?;
    Ok(Json(MachineLifecycleResponse::from_snapshot(
        &snapshot.config,
        &snapshot.state,
    )))
}

pub(crate) async fn delete_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<MachineDeleteResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let manager = machine_lifecycle_manager(&state)?;
    let snapshot = manager.delete_machine(&name).await?;
    crate::system_tenant::delete_machine_state_async(&state.service, &snapshot.config.name)
        .await
        .map_err(AppError::from)?;
    record_machine_delete_event(&state, &snapshot.config, &snapshot.state).await?;
    Ok(Json(MachineDeleteResponse {
        name: snapshot.config.name,
        state: "deleted".to_owned(),
        previous_state: snapshot.state.lifecycle.as_str().to_owned(),
    }))
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
    state: &AppState,
    config: &MachineConfigRecord,
    snapshot: &MachineStateRecord,
) -> Result<(), AppError> {
    crate::system_tenant::record_machine_state_async(&state.service, config, snapshot)
        .await
        .map_err(AppError::from)
}

async fn record_machine_event(
    state: &AppState,
    action: &str,
    config: &MachineConfigRecord,
    snapshot: &MachineStateRecord,
) -> Result<(), AppError> {
    let message = format!(
        "machine `{}` {} completed with state {}",
        config.name,
        action,
        snapshot.lifecycle.as_str()
    );
    let correlation_id = format!("machine:{}:{action}", config.name);
    crate::system_tenant::record_system_event_async(
        &state.service,
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
    .map_err(AppError::from)
}

async fn record_machine_delete_event(
    state: &AppState,
    config: &MachineConfigRecord,
    snapshot: &MachineStateRecord,
) -> Result<(), AppError> {
    let message = format!(
        "machine `{}` delete completed from state {}",
        config.name,
        snapshot.lifecycle.as_str()
    );
    let correlation_id = format!("machine:{}:delete", config.name);
    crate::system_tenant::record_system_event_async(
        &state.service,
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
    .map_err(AppError::from)
}

fn machine_lifecycle_manager(
    state: &AppState,
) -> Result<Arc<dyn crate::machine_lifecycle::MachineLifecycleManager>, AppError> {
    state.machine_lifecycle_manager().ok_or_else(|| {
        AppError::not_found("machine lifecycle endpoints require a server-owned machine manager")
    })
}

fn parse_machine_name(value: String) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty()
        || matches!(value, "." | "..")
        || value
            .chars()
            .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.')))
    {
        return Err(AppError::from(nimbus_core::Error::InvalidInput(format!(
            "invalid machine name `{value}`; expected letters, numbers, dots, dashes, or underscores"
        ))));
    }
    Ok(value.to_owned())
}

fn describe_image_source(source: &MachineImageSource) -> String {
    match source {
        MachineImageSource::OciReference { reference } => reference.clone(),
        MachineImageSource::HttpUrl { url } => url.clone(),
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
