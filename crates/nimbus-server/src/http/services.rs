use nimbus_runtime::HostCallCancellation;
use nimbus_sandbox::SandboxHandle;
use serde::Serialize;

use super::*;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceLifecycleResponse {
    pub(crate) tenant_id: String,
    pub(crate) name: String,
    pub(crate) sandbox_id: String,
    pub(crate) backend: String,
    pub(crate) state: String,
    pub(crate) endpoints: Vec<ServiceEndpointResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceEndpointResponse {
    pub(crate) name: String,
    pub(crate) protocol: String,
    pub(crate) host: String,
    pub(crate) port: u16,
}

pub(crate) async fn start_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
) -> Result<Json<ServiceLifecycleResponse>, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let manager = sandbox_service_manager(&state)?;
    let handle = manager
        .start_service_async(&tenant_id, &service_name, HostCallCancellation::default())
        .await?
        .ok_or_else(|| service_not_found(&tenant_id, &service_name))?;
    record_service_event(&state, &tenant_id, "start", &handle).await?;

    Ok(Json(ServiceLifecycleResponse::from_handle(
        &tenant_id, &handle,
    )))
}

pub(crate) async fn stop_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
) -> Result<Json<ServiceLifecycleResponse>, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let manager = sandbox_service_manager(&state)?;
    let handle = manager
        .stop_service_async(&tenant_id, &service_name)
        .await?
        .ok_or_else(|| service_not_found(&tenant_id, &service_name))?;
    record_service_event(&state, &tenant_id, "stop", &handle).await?;

    Ok(Json(ServiceLifecycleResponse::from_handle(
        &tenant_id, &handle,
    )))
}

pub(crate) async fn restart_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
) -> Result<Json<ServiceLifecycleResponse>, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let manager = sandbox_service_manager(&state)?;
    let handle = manager
        .restart_service_async(&tenant_id, &service_name, HostCallCancellation::default())
        .await?
        .ok_or_else(|| service_not_found(&tenant_id, &service_name))?;
    record_service_event(&state, &tenant_id, "restart", &handle).await?;

    Ok(Json(ServiceLifecycleResponse::from_handle(
        &tenant_id, &handle,
    )))
}

impl ServiceLifecycleResponse {
    fn from_handle(tenant_id: &TenantId, handle: &SandboxHandle) -> Self {
        Self {
            tenant_id: tenant_id.as_str().to_owned(),
            name: handle.name.clone(),
            sandbox_id: handle.id.as_str().to_owned(),
            backend: crate::system_tenant::sandbox_backend(handle.backend).to_owned(),
            state: crate::system_tenant::sandbox_status(handle.status).to_owned(),
            endpoints: handle
                .published_endpoints
                .iter()
                .map(|endpoint| ServiceEndpointResponse {
                    name: endpoint.name.as_str().to_owned(),
                    protocol: crate::system_tenant::endpoint_protocol(endpoint.protocol).to_owned(),
                    host: endpoint.address.ip().to_string(),
                    port: endpoint.address.port(),
                })
                .collect(),
        }
    }
}

fn sandbox_service_manager(
    state: &AppState,
) -> Result<Arc<crate::service_manager::SandboxServiceManager>, AppError> {
    state.sandbox_service_manager().ok_or_else(|| {
        AppError::not_found(
            "service lifecycle endpoints require a server-owned sandbox service manager",
        )
    })
}

async fn record_service_event(
    state: &AppState,
    tenant_id: &TenantId,
    action: &str,
    handle: &SandboxHandle,
) -> Result<(), AppError> {
    let service_state = crate::system_tenant::sandbox_status(handle.status);
    let message = format!(
        "service `{}` for tenant `{}` {} completed with state {}",
        handle.name, tenant_id, action, service_state
    );
    let correlation_id = format!("service:{}:{}:{action}", tenant_id, handle.name);
    crate::system_tenant::record_system_event_async(
        &state.service,
        "service",
        "info",
        "service.lifecycle",
        &message,
        serde_json::json!({
            "action": action,
            "tenantId": tenant_id.as_str(),
            "serviceName": handle.name.as_str(),
            "sandboxId": handle.id.as_str(),
            "state": service_state,
            "backend": crate::system_tenant::sandbox_backend(handle.backend),
        }),
        Some(&correlation_id),
    )
    .await
    .map_err(AppError::from)
}

fn service_not_found(tenant_id: &TenantId, service_name: &str) -> AppError {
    AppError::not_found(format!(
        "service `{service_name}` is not declared for tenant `{tenant_id}`"
    ))
}
