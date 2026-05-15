use super::*;

/// Creates a tenant explicitly.
pub(crate) async fn create_tenant(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateTenantRequest>,
) -> Result<(StatusCode, Json<TenantResponse>), AppError> {
    let tenant_id = parse_user_tenant_id(request.id)?;
    let service = state.service.clone();
    service.create_tenant_async(tenant_id.clone()).await?;
    if let Some(registry) = state.current_deployment().convex_registry() {
        registry
            .apply_schema_to_tenant_async(&service, tenant_id.clone())
            .await?;
    }
    let id = tenant_id.to_string();
    Ok((StatusCode::CREATED, Json(TenantResponse { id })))
}

/// Lists known tenants.
pub(crate) async fn list_tenants(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TenantListResponse>, AppError> {
    let service = state.service.clone();
    let tenants = service.list_tenants_async().await?;
    Ok(Json(TenantListResponse {
        tenants: tenants
            .into_iter()
            .filter(|tenant| !crate::system_tenant::is_reserved_tenant_id(tenant))
            .map(|tenant| tenant.to_string())
            .collect(),
    }))
}

/// Deletes a tenant.
pub(crate) async fn delete_tenant(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<StatusCode, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let service = state.service.clone();
    state
        .runtime_service_registry()
        .teardown_tenant(&tenant_id)?;
    service.delete_tenant_async(tenant_id).await?;
    Ok(StatusCode::NO_CONTENT)
}
