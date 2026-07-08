use nimbus_compute::machines::{
    MachineCreateRequestBody, MachineDeleteResponse, MachineLifecycleResponse,
    MachineUpdateRequestBody,
};

use super::*;

pub(crate) async fn create_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Json(request): Json<MachineCreateRequestBody>,
) -> Result<Json<MachineLifecycleResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let response = nimbus_compute::machines::create_machine(&state, name, request).await?;
    Ok(Json(response))
}

pub(crate) async fn start_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<MachineLifecycleResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let response = nimbus_compute::machines::start_machine(&state, &name).await?;
    Ok(Json(response))
}

pub(crate) async fn stop_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<MachineLifecycleResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let response = nimbus_compute::machines::stop_machine(&state, &name).await?;
    Ok(Json(response))
}

pub(crate) async fn restart_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<MachineLifecycleResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let response = nimbus_compute::machines::restart_machine(&state, &name).await?;
    Ok(Json(response))
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
    let response = nimbus_compute::machines::update_machine(&state, name, request).await?;
    Ok(Json(response))
}

pub(crate) async fn delete_machine(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<MachineDeleteResponse>, AppError> {
    let name = parse_machine_name(name)?;
    let response = nimbus_compute::machines::delete_machine(&state, &name).await?;
    Ok(Json(response))
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
