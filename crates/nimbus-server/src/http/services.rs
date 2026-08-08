use std::collections::BTreeMap;

use axum::http::HeaderMap;
use nimbus_compute::services::{
    ServiceBackendInput, ServiceDefinitionCollectionResponse, ServiceDefinitionResourceResponse,
    ServiceLifecycleVerb, ServiceResourceResponse, ServiceRestartResponse,
};
use serde::Deserialize;

use super::authz::{OperatorAuthScope, record_operator_authorization_audit};
use super::resource_control::services::{
    ServiceDefinitionAction, authorize_service_definition_route, authorize_service_route,
};
use super::*;

pub(crate) async fn list_service_definitions(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    QueryParams(query): QueryParams<ServiceDefinitionListQuery>,
    headers: HeaderMap,
) -> Result<Json<ServiceDefinitionCollectionResponse>, AppError> {
    let authorization = authorize_service_definition_route(
        &state,
        &headers,
        tenant_id,
        None,
        ServiceDefinitionAction::List,
        "native_http.service_definition.list",
    )
    .await?;
    let is_operator = authorization.is_operator();
    let authorized_tenant_id = authorization.tenant_context.tenant_id().clone();
    let response = nimbus_compute::services::list_service_definitions(
        &state,
        &authorized_tenant_id,
        is_operator,
        |name| authorization.allows_service_definition(ServiceDefinitionAction::List, name),
        |name| authorization.allows_service_definition(ServiceDefinitionAction::Inspect, name),
        query.page_token.as_deref(),
        query.limit,
    )?;

    record_operator_authorization_audit(
        &state,
        &headers,
        authorized_tenant_id.as_str(),
        OperatorAuthScope::ServiceDefinition,
        authorization.principal_class,
        authorization.auth_method,
        true,
        "service definition list authorized",
    );

    Ok(Json(response))
}

pub(crate) async fn create_service_definition(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    headers: HeaderMap,
    Json(request): Json<ServiceDefinitionWriteRequest>,
) -> Result<(StatusCode, Json<ServiceDefinitionResourceResponse>), AppError> {
    let route_tenant = parse_user_tenant_id(tenant_id)?;
    validate_body_tenant(request.metadata.tenant_id.as_deref(), &route_tenant)?;
    let service_name = request.metadata.name.as_deref().ok_or_else(|| {
        AppError::from(Error::InvalidInput(
            "service definition create requires metadata.name".to_owned(),
        ))
    })?;
    let authorization = authorize_service_definition_route(
        &state,
        &headers,
        route_tenant.as_str().to_owned(),
        Some(service_name),
        ServiceDefinitionAction::Create,
        "native_http.service_definition.create",
    )
    .await?;
    let tenant_context = authorization.tenant_context.clone();
    let response = nimbus_compute::services::create_service_definition(
        &state,
        tenant_context.tenant_id(),
        service_name,
        request.spec.backend,
        request.metadata.labels.unwrap_or_default(),
    )?;
    record_operator_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id().as_str(),
        OperatorAuthScope::ServiceDefinition,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("service definition create authorized for `{service_name}`"),
    );

    Ok((StatusCode::CREATED, Json(response)))
}

pub(crate) async fn update_service_definition(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<ServiceDefinitionWriteRequest>,
) -> Result<Json<ServiceDefinitionResourceResponse>, AppError> {
    let route_tenant = parse_user_tenant_id(tenant_id)?;
    validate_body_tenant(request.metadata.tenant_id.as_deref(), &route_tenant)?;
    validate_body_service_name(request.metadata.name.as_deref(), &service_name)?;
    let authorization = authorize_service_definition_route(
        &state,
        &headers,
        route_tenant.as_str().to_owned(),
        Some(&service_name),
        ServiceDefinitionAction::Update,
        "native_http.service_definition.update",
    )
    .await?;
    let tenant_context = authorization.tenant_context.clone();
    let expected_generation = request.metadata.generation.ok_or_else(|| {
        AppError::from(Error::InvalidInput(
            "service definition update requires metadata.generation precondition".to_owned(),
        ))
    })?;
    let response = nimbus_compute::services::update_service_definition(
        &state,
        tenant_context.tenant_id(),
        &service_name,
        expected_generation,
        request.spec.backend,
        request.metadata.labels.unwrap_or_default(),
    )?;
    record_operator_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id().as_str(),
        OperatorAuthScope::ServiceDefinition,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("service definition update authorized for `{service_name}`"),
    );

    Ok(Json(response))
}

pub(crate) async fn delete_service_definition(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    QueryParams(query): QueryParams<ServiceDefinitionDeleteQuery>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let authorization = authorize_service_definition_route(
        &state,
        &headers,
        tenant_id,
        Some(&service_name),
        ServiceDefinitionAction::Delete,
        "native_http.service_definition.delete",
    )
    .await?;
    let tenant_context = authorization.tenant_context.clone();
    let expected_generation = query.if_match_generation.ok_or_else(|| {
        AppError::from(Error::InvalidInput(
            "service definition delete requires ifMatchGeneration query precondition".to_owned(),
        ))
    })?;
    // Preserve pre-extraction error precedence: the old handler resolved the
    // service manager before evaluating the force-delete permission, so a
    // manager-less server answers not_found ahead of the force 403/audit.
    nimbus_compute::services::ensure_service_manager_available(&state)?;
    let force = query.force.unwrap_or(false);
    if force && !authorization.allows_force_delete(&service_name) {
        record_operator_authorization_audit(
            &state,
            &headers,
            tenant_context.tenant_id().as_str(),
            OperatorAuthScope::ServiceDefinition,
            authorization.principal_class,
            authorization.auth_method,
            false,
            format!(
                "{} principal lacks separate force-delete policy and exact service grant for `{service_name}`",
                authorization.principal_class.as_str()
            ),
        );
        return Err(AppError::forbidden(format!(
            "{} principal requires service definition `forceDelete` permission plus an exact service grant for `{service_name}`",
            authorization.principal_class.as_str()
        )));
    }
    nimbus_compute::services::delete_service_definition(
        &state,
        tenant_context.tenant_id(),
        &service_name,
        expected_generation,
        force,
    )
    .await?;
    record_operator_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id().as_str(),
        OperatorAuthScope::ServiceDefinition,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("service definition delete authorized for `{service_name}` force={force}"),
    );

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServiceDefinitionWriteRequest {
    metadata: ServiceDefinitionMetadataInput,
    spec: ServiceDefinitionSpecInput,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServiceDefinitionMetadataInput {
    tenant_id: Option<String>,
    name: Option<String>,
    generation: Option<u64>,
    labels: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ServiceDefinitionSpecInput {
    backend: ServiceBackendInput,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServiceDefinitionDeleteQuery {
    if_match_generation: Option<u64>,
    force: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServiceDefinitionListQuery {
    limit: Option<usize>,
    page_token: Option<String>,
}

pub(crate) async fn get_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    service_lifecycle_route(
        state,
        headers,
        tenant_id,
        service_name,
        ServiceLifecycleVerb::Get,
    )
    .await
}

pub(crate) async fn start_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    service_lifecycle_route(
        state,
        headers,
        tenant_id,
        service_name,
        ServiceLifecycleVerb::Start,
    )
    .await
}

pub(crate) async fn stop_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    service_lifecycle_route(
        state,
        headers,
        tenant_id,
        service_name,
        ServiceLifecycleVerb::Stop,
    )
    .await
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServiceRestartRequest {
    source_generation: u64,
    request_id: String,
}

pub(crate) async fn restart_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
    Json(request): Json<ServiceRestartRequest>,
) -> Result<(StatusCode, Json<ServiceRestartResponse>), AppError> {
    let authorization = authorize_service_route(
        &state,
        &headers,
        tenant_id,
        &service_name,
        "native_http.service.restart",
    )
    .await?;
    let tenant_context = authorization.tenant_context;
    let response = nimbus_compute::services::submit_service_restart(
        &state,
        &tenant_context,
        &service_name,
        request.source_generation,
        &request.request_id,
    )
    .await?;
    record_operator_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id().as_str(),
        OperatorAuthScope::Service,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal admitted fenced service restart with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok((StatusCode::ACCEPTED, Json(response)))
}

async fn service_lifecycle_route(
    state: Arc<AppState>,
    headers: HeaderMap,
    tenant_id: String,
    service_name: String,
    verb: ServiceLifecycleVerb,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    let authorization =
        authorize_service_route(&state, &headers, tenant_id, &service_name, verb.surface()).await?;
    let tenant_context = authorization.tenant_context;
    let response =
        nimbus_compute::services::service_lifecycle(&state, &tenant_context, &service_name, verb)
            .await?;
    record_operator_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id().as_str(),
        OperatorAuthScope::Service,
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal authorized with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok(Json(response))
}

fn validate_body_tenant(value: Option<&str>, route_tenant: &TenantId) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let body_tenant = parse_user_tenant_id(value.to_owned())?;
    if &body_tenant != route_tenant {
        return Err(AppError::from(Error::InvalidInput(format!(
            "request body metadata.tenantId `{body_tenant}` does not match route tenant `{route_tenant}`"
        ))));
    }
    Ok(())
}

fn validate_body_service_name(
    value: Option<&str>,
    route_service_name: &str,
) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    if value != route_service_name {
        return Err(AppError::from(Error::InvalidInput(format!(
            "request body metadata.name `{value}` does not match route service `{route_service_name}`"
        ))));
    }
    Ok(())
}
