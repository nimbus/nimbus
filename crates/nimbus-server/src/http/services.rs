use std::collections::BTreeMap;

use axum::http::HeaderMap;
use nimbus_core::PrincipalContext;
use nimbus_operator::{
    ExtractedServerAccessStatus, LocalServerCredentialMode, extract_server_access,
};
use nimbus_runtime::HostCallCancellation;
use nimbus_sandbox::{SandboxHandle, SandboxStatus};
use nimbus_services::{
    ExternalAuthPolicy, HealthCheckPolicy, ServiceBackend, ServiceDefinition,
    ServiceDefinitionSource,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::sandbox_spec::{SandboxSpecInput, SandboxSpecResponse};
use super::service_grants::principal_has_exact_service_grant;
use super::*;
use crate::local_server::{LocalServerAuditEvent, LocalServerRouteFamily, origin_from_headers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrincipalClass {
    Operator,
    Tenant,
    SpawnedWorkload,
}

impl PrincipalClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Tenant => "tenant",
            Self::SpawnedWorkload => "spawned_workload",
        }
    }
}

#[derive(Debug)]
struct ServiceRouteAuthorization {
    principal_class: PrincipalClass,
    tenant_context: TenantIsolationContext,
    auth_method: Option<&'static str>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceResourceResponse {
    pub(crate) tenant_id: String,
    pub(crate) name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sandbox_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) backend: Option<String>,
    pub(crate) state: String,
    pub(crate) lifecycle_state: String,
    pub(crate) readiness: String,
    pub(crate) health: String,
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
    let manager = service_manager(&state)?;
    let mut definitions = manager.service_definitions_for_tenant(&authorized_tenant_id);
    definitions.sort_by(|left, right| left.name.cmp(&right.name));
    if let Some(token) = query.page_token.as_deref() {
        definitions.retain(|definition| definition.name.as_str() > token);
    }
    if !is_operator {
        definitions.retain(|definition| {
            authorization.allows_service_definition(ServiceDefinitionAction::List, &definition.name)
        });
    }

    let limit = query.limit.unwrap_or(100).clamp(1, 100);
    let remaining_count = definitions.len().saturating_sub(limit);
    let next_page_token = if remaining_count > 0 {
        definitions
            .get(limit.saturating_sub(1))
            .map(|definition| definition.name.clone())
    } else {
        None
    };
    definitions.truncate(limit);

    record_service_definition_authorization_audit(
        &state,
        &headers,
        &authorized_tenant_id,
        authorization.principal_class,
        authorization.auth_method,
        true,
        "service definition list authorized",
    );

    Ok(Json(ServiceDefinitionCollectionResponse {
        metadata: ServiceDefinitionCollectionMetadataResponse {
            tenant_id: authorized_tenant_id.as_str().to_owned(),
            resource_version: format!(
                "services:{}:{}",
                authorized_tenant_id,
                definitions.len() + remaining_count
            ),
            limit,
            next_page_token,
            remaining_count,
        },
        items: definitions
            .into_iter()
            .map(|definition| {
                let projection = if authorization
                    .allows_service_definition(ServiceDefinitionAction::Inspect, &definition.name)
                {
                    ServiceDefinitionProjection::Inspect
                } else {
                    ServiceDefinitionProjection::List
                };
                ServiceDefinitionResourceResponse::from_definition(definition, projection)
            })
            .collect(),
    }))
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
    let manager = service_manager(&state)?;
    let backend = service_backend_from_input(
        tenant_context.tenant_id(),
        service_name,
        request.spec.backend,
    )?;
    let definition = manager.create_service_definition(
        tenant_context.tenant_id(),
        service_name,
        backend,
        request.metadata.labels.unwrap_or_default(),
    )?;
    record_service_definition_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("service definition create authorized for `{service_name}`"),
    );

    Ok((
        StatusCode::CREATED,
        Json(ServiceDefinitionResourceResponse::from_definition(
            definition,
            ServiceDefinitionProjection::Inspect,
        )),
    ))
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
    let manager = service_manager(&state)?;
    let backend = service_backend_from_input(
        tenant_context.tenant_id(),
        &service_name,
        request.spec.backend,
    )?;
    let definition = manager.update_service_definition(
        tenant_context.tenant_id(),
        &service_name,
        expected_generation,
        backend,
        request.metadata.labels.unwrap_or_default(),
    )?;
    record_service_definition_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!("service definition update authorized for `{service_name}`"),
    );

    Ok(Json(ServiceDefinitionResourceResponse::from_definition(
        definition,
        ServiceDefinitionProjection::Inspect,
    )))
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
    let manager = service_manager(&state)?;
    let force = query.force.unwrap_or(false);
    if force && !authorization.allows_force_delete(&service_name) {
        record_service_definition_authorization_audit(
            &state,
            &headers,
            tenant_context.tenant_id(),
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
    manager
        .delete_service_definition_async(
            tenant_context.tenant_id(),
            &service_name,
            expected_generation,
            force,
        )
        .await?;
    record_service_definition_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
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

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ServiceBackendInput {
    #[serde(rename = "sandbox")]
    Sandbox { sandbox: SandboxSpecInput },
    #[serde(rename = "builtIn")]
    BuiltIn { provider: String },
    #[serde(rename = "external")]
    External {
        endpoint: ExternalEndpointPolicyInput,
        auth: ExternalAuthPolicyInput,
        health: HealthCheckPolicyInput,
    },
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ExternalEndpointPolicyInput {
    url: String,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum ExternalAuthPolicyInput {
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum HealthCheckPolicyInput {
    #[serde(rename = "http")]
    Http { path: String },
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceDefinitionCollectionResponse {
    metadata: ServiceDefinitionCollectionMetadataResponse,
    items: Vec<ServiceDefinitionResourceResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceDefinitionCollectionMetadataResponse {
    tenant_id: String,
    resource_version: String,
    limit: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_page_token: Option<String>,
    remaining_count: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceDefinitionResourceResponse {
    metadata: ServiceDefinitionMetadataResponse,
    spec: ServiceDefinitionSpecResponse,
    status: ServiceDefinitionStatusResponse,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceDefinitionMetadataResponse {
    tenant_id: String,
    name: String,
    generation: u64,
    resource_version: String,
    created_at: String,
    updated_at: String,
    labels: BTreeMap<String, String>,
    source: &'static str,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceDefinitionSpecResponse {
    backend: ServiceBackendResponse,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ServiceBackendResponse {
    #[serde(rename = "sandbox")]
    Sandbox { sandbox: SandboxSpecResponse },
    #[serde(rename = "builtIn")]
    BuiltIn { provider: String },
    #[serde(rename = "external")]
    External {
        endpoint: ExternalEndpointPolicyResponse,
        auth: ExternalAuthPolicyResponse,
        health: HealthCheckPolicyResponse,
    },
    #[serde(rename = "redacted")]
    Redacted {
        backend: &'static str,
        redacted: bool,
        reason: &'static str,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ExternalEndpointPolicyResponse {
    url: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ExternalAuthPolicyResponse {
    #[serde(rename = "none")]
    None,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum HealthCheckPolicyResponse {
    #[serde(rename = "http")]
    Http { path: String },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceDefinitionStatusResponse {
    backend: &'static str,
    lifecycle_state: String,
    readiness: String,
    health: String,
    conditions: Vec<ServiceConditionResponse>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ServiceConditionResponse {
    #[serde(rename = "type")]
    condition_type: &'static str,
    status: &'static str,
    reason: &'static str,
    message: String,
    observed_generation: u64,
    last_transition_time: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceDefinitionProjection {
    List,
    Inspect,
}

pub(crate) async fn get_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    let authorization = authorize_service_route(
        &state,
        &headers,
        tenant_id,
        &service_name,
        "native_http.service.get",
    )
    .await?;
    let tenant_context = authorization.tenant_context;
    let manager = service_manager(&state)?;
    if !manager.service_declared_for_tenant(tenant_context.tenant_id(), &service_name) {
        return Err(service_not_found(tenant_context.tenant_id(), &service_name));
    }
    let handle = manager
        .inspect_service_for_context_async(&tenant_context, &service_name)
        .await?;
    record_service_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal authorized with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok(Json(match handle {
        Some(handle) => ServiceResourceResponse::from_handle(tenant_context.tenant_id(), &handle),
        None => {
            ServiceResourceResponse::declared_inactive(tenant_context.tenant_id(), &service_name)
        }
    }))
}

pub(crate) async fn start_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    let authorization = authorize_service_route(
        &state,
        &headers,
        tenant_id,
        &service_name,
        "native_http.service.start",
    )
    .await?;
    let tenant_context = authorization.tenant_context;
    let manager = service_manager(&state)?;
    let handle = manager
        .start_service_for_context_async(
            &tenant_context,
            &service_name,
            HostCallCancellation::default(),
        )
        .await?
        .ok_or_else(|| service_not_found(tenant_context.tenant_id(), &service_name))?;
    record_service_event(&state, tenant_context.tenant_id(), "start", &handle).await?;
    record_service_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal authorized with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok(Json(ServiceResourceResponse::from_handle(
        tenant_context.tenant_id(),
        &handle,
    )))
}

pub(crate) async fn stop_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    let authorization = authorize_service_route(
        &state,
        &headers,
        tenant_id,
        &service_name,
        "native_http.service.stop",
    )
    .await?;
    let tenant_context = authorization.tenant_context;
    let manager = service_manager(&state)?;
    let handle = manager
        .stop_service_for_context_async(&tenant_context, &service_name)
        .await?
        .ok_or_else(|| service_not_found(tenant_context.tenant_id(), &service_name))?;
    record_service_event(&state, tenant_context.tenant_id(), "stop", &handle).await?;
    record_service_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal authorized with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok(Json(ServiceResourceResponse::from_handle(
        tenant_context.tenant_id(),
        &handle,
    )))
}

pub(crate) async fn restart_service(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, service_name)): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<ServiceResourceResponse>, AppError> {
    let authorization = authorize_service_route(
        &state,
        &headers,
        tenant_id,
        &service_name,
        "native_http.service.restart",
    )
    .await?;
    let tenant_context = authorization.tenant_context;
    let manager = service_manager(&state)?;
    let handle = manager
        .restart_service_for_context_async(
            &tenant_context,
            &service_name,
            HostCallCancellation::default(),
        )
        .await?
        .ok_or_else(|| service_not_found(tenant_context.tenant_id(), &service_name))?;
    record_service_event(&state, tenant_context.tenant_id(), "restart", &handle).await?;
    record_service_authorization_audit(
        &state,
        &headers,
        tenant_context.tenant_id(),
        authorization.principal_class,
        authorization.auth_method,
        true,
        format!(
            "{} principal authorized with exact service grant or operator authority",
            authorization.principal_class.as_str()
        ),
    );

    Ok(Json(ServiceResourceResponse::from_handle(
        tenant_context.tenant_id(),
        &handle,
    )))
}

impl ServiceResourceResponse {
    fn from_handle(tenant_id: &TenantId, handle: &SandboxHandle) -> Self {
        let state = crate::system_tenant::sandbox_status(handle.status).to_owned();
        Self {
            tenant_id: tenant_id.as_str().to_owned(),
            name: handle.name.clone(),
            sandbox_id: Some(handle.id.as_str().to_owned()),
            backend: Some(crate::system_tenant::sandbox_backend(handle.backend).to_owned()),
            state: state.clone(),
            lifecycle_state: state,
            readiness: readiness_from_status(handle.status).to_owned(),
            health: health_from_status(handle.status).to_owned(),
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

    fn declared_inactive(tenant_id: &TenantId, service_name: &str) -> Self {
        Self {
            tenant_id: tenant_id.as_str().to_owned(),
            name: service_name.to_owned(),
            sandbox_id: None,
            backend: None,
            state: "stopped".to_owned(),
            lifecycle_state: "stopped".to_owned(),
            readiness: "stopped".to_owned(),
            health: "unknown".to_owned(),
            endpoints: Vec::new(),
        }
    }
}

impl ServiceDefinitionResourceResponse {
    fn from_definition(
        definition: ServiceDefinition,
        projection: ServiceDefinitionProjection,
    ) -> Self {
        let backend = match projection {
            ServiceDefinitionProjection::Inspect => {
                ServiceBackendResponse::from_backend(definition.backend.clone())
            }
            ServiceDefinitionProjection::List => {
                ServiceBackendResponse::redacted_from_backend(&definition.backend)
            }
        };
        let backend_kind = service_backend_wire_kind(&definition.backend);
        let lifecycle_state = match &definition.backend {
            ServiceBackend::Sandbox(_) => "stopped",
            ServiceBackend::BuiltIn(_) | ServiceBackend::External(_) => "declared",
        }
        .to_owned();
        let readiness = match &definition.backend {
            ServiceBackend::Sandbox(_) => "stopped",
            ServiceBackend::BuiltIn(_) | ServiceBackend::External(_) => "unknown",
        }
        .to_owned();
        let health = "unknown".to_owned();
        let updated_at = format_millis_rfc3339(definition.updated_at_millis);
        Self {
            metadata: ServiceDefinitionMetadataResponse {
                tenant_id: definition.tenant_id.as_str().to_owned(),
                name: definition.name.clone(),
                generation: definition.generation,
                resource_version: definition.resource_version,
                created_at: format_millis_rfc3339(definition.created_at_millis),
                updated_at: updated_at.clone(),
                labels: definition.labels,
                source: match definition.source {
                    ServiceDefinitionSource::StaticCatalog => "staticCatalog",
                    ServiceDefinitionSource::Dynamic => "dynamic",
                },
            },
            spec: ServiceDefinitionSpecResponse { backend },
            status: ServiceDefinitionStatusResponse {
                backend: backend_kind,
                lifecycle_state,
                readiness,
                health,
                conditions: vec![ServiceConditionResponse {
                    condition_type: "Admitted",
                    status: "True",
                    reason: "DefinitionValidated",
                    message: format!(
                        "service definition `{}` uses {backend_kind} backend",
                        definition.name
                    ),
                    observed_generation: definition.generation,
                    last_transition_time: updated_at,
                }],
            },
        }
    }
}

impl ServiceBackendResponse {
    fn from_backend(backend: ServiceBackend) -> Self {
        match backend {
            ServiceBackend::Sandbox(sandbox) => Self::Sandbox {
                sandbox: SandboxSpecResponse::from_spec(sandbox),
            },
            ServiceBackend::BuiltIn(spec) => Self::BuiltIn {
                provider: spec.provider().to_owned(),
            },
            ServiceBackend::External(spec) => Self::External {
                endpoint: ExternalEndpointPolicyResponse {
                    url: spec.endpoint().to_owned(),
                },
                auth: external_auth_policy_response(spec.auth()),
                health: health_check_policy_response(spec.health()),
            },
        }
    }

    fn redacted_from_backend(backend: &ServiceBackend) -> Self {
        Self::Redacted {
            backend: service_backend_wire_kind(backend),
            redacted: true,
            reason: "requiresInspectPermission",
        }
    }
}

fn external_auth_policy_response(policy: ExternalAuthPolicy) -> ExternalAuthPolicyResponse {
    match policy {
        ExternalAuthPolicy::None => ExternalAuthPolicyResponse::None,
    }
}

fn health_check_policy_response(policy: &HealthCheckPolicy) -> HealthCheckPolicyResponse {
    match policy {
        HealthCheckPolicy::Http { path } => HealthCheckPolicyResponse::Http { path: path.clone() },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServiceDefinitionAction {
    Create,
    List,
    Inspect,
    Update,
    Delete,
    ForceDelete,
}

impl ServiceDefinitionAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::List => "list",
            Self::Inspect => "inspect",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::ForceDelete => "forceDelete",
        }
    }
}

#[derive(Debug)]
struct ServiceDefinitionAuthorization {
    principal_class: PrincipalClass,
    tenant_context: TenantIsolationContext,
    auth_method: Option<&'static str>,
    principal: Option<PrincipalContext>,
}

impl ServiceDefinitionAuthorization {
    fn is_operator(&self) -> bool {
        self.principal_class == PrincipalClass::Operator
    }

    fn allows_service_definition(
        &self,
        action: ServiceDefinitionAction,
        service_name: &str,
    ) -> bool {
        self.is_operator()
            || self.principal.as_ref().is_some_and(|principal| {
                principal_has_service_definition_permission(principal, action, service_name)
            })
    }

    fn allows_force_delete(&self, service_name: &str) -> bool {
        self.is_operator()
            || self.principal.as_ref().is_some_and(|principal| {
                principal_has_service_definition_permission(
                    principal,
                    ServiceDefinitionAction::ForceDelete,
                    service_name,
                ) && principal_has_exact_service_grant(principal, service_name)
            })
    }
}

fn service_manager(state: &AppState) -> Result<Arc<nimbus_services::ServiceManager>, AppError> {
    state.service_manager().ok_or_else(|| {
        AppError::not_found("service lifecycle endpoints require a server-owned service manager")
    })
}

async fn authorize_service_definition_route(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    tenant_id: String,
    service_name: Option<&str>,
    action: ServiceDefinitionAction,
    surface: &'static str,
) -> Result<ServiceDefinitionAuthorization, AppError> {
    if let Some(operator) = authorize_operator_service_route(state, headers, &tenant_id, surface)? {
        return Ok(ServiceDefinitionAuthorization {
            principal_class: operator.principal_class,
            tenant_context: operator.tenant_context,
            auth_method: operator.auth_method,
            principal: None,
        });
    }

    let resolved = crate::application_auth::resolve_application_auth_from_headers(state, headers)
        .await
        .map_err(|error| {
            record_service_definition_authorization_audit(
                state,
                headers,
                &parse_user_tenant_id_lossy(&tenant_id),
                PrincipalClass::Tenant,
                Some("application_bearer"),
                false,
                format!("tenant/spawned service definition authorization failed: {error}"),
            );
            error
        })?;
    if !resolved.principal.authenticated {
        let tenant = parse_user_tenant_id(tenant_id)?;
        record_service_definition_authorization_audit(
            state,
            headers,
            &tenant,
            PrincipalClass::Tenant,
            None,
            false,
            "service definition route requires operator credentials or authenticated tenant/spawned workload identity",
        );
        return Err(AppError::unauthorized(
            "service definition route requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }

    let tenant = parse_user_tenant_id(tenant_id)?;
    let principal_class = principal_class_from_principal(&resolved.principal)?;
    let tenant_context =
        TenantIsolationContext::application(tenant.clone(), resolved.principal.clone(), surface);
    if let Err(error) =
        tenant_context.require_matching_principal_claim("service definition route policy")
    {
        record_service_definition_authorization_audit(
            state,
            headers,
            &tenant,
            principal_class,
            Some("application_bearer"),
            false,
            format!(
                "{} cross-tenant service definition route rejected: {error}",
                principal_class.as_str()
            ),
        );
        return Err(AppError::from(error));
    }

    let allowed = match service_name {
        Some(service_name) => {
            principal_has_service_definition_permission(&resolved.principal, action, service_name)
        }
        None => principal_has_any_service_definition_permission(&resolved.principal, action),
    };
    if !allowed {
        record_service_definition_authorization_audit(
            state,
            headers,
            &tenant,
            principal_class,
            Some("application_bearer"),
            false,
            format!(
                "{} principal lacks service definition `{}` permission",
                principal_class.as_str(),
                action.as_str()
            ),
        );
        return Err(AppError::forbidden(format!(
            "{} principal requires service definition `{}` permission",
            principal_class.as_str(),
            action.as_str()
        )));
    }

    Ok(ServiceDefinitionAuthorization {
        principal_class,
        tenant_context,
        auth_method: Some("application_bearer"),
        principal: Some(resolved.principal),
    })
}

async fn authorize_service_route(
    state: &Arc<AppState>,
    headers: &HeaderMap,
    tenant_id: String,
    service_name: &str,
    surface: &'static str,
) -> Result<ServiceRouteAuthorization, AppError> {
    if let Some(operator) = authorize_operator_service_route(state, headers, &tenant_id, surface)? {
        return Ok(operator);
    }

    let resolved = crate::application_auth::resolve_application_auth_from_headers(state, headers)
        .await
        .map_err(|error| {
            record_service_authorization_audit(
                state,
                headers,
                &parse_user_tenant_id_lossy(&tenant_id),
                PrincipalClass::Tenant,
                Some("application_bearer"),
                false,
                format!("tenant/spawned service authorization failed: {error}"),
            );
            error
        })?;
    if !resolved.principal.authenticated {
        let tenant = parse_user_tenant_id(tenant_id)?;
        record_service_authorization_audit(
            state,
            headers,
            &tenant,
            PrincipalClass::Tenant,
            None,
            false,
            "service lifecycle route requires operator credentials or authenticated tenant/spawned workload identity",
        );
        return Err(AppError::unauthorized(
            "service lifecycle route requires operator credentials or authenticated tenant/spawned workload identity",
        ));
    }

    let tenant = parse_user_tenant_id(tenant_id)?;
    let principal_class = principal_class_from_principal(&resolved.principal)?;
    let tenant_context =
        TenantIsolationContext::application(tenant.clone(), resolved.principal.clone(), surface);
    if let Err(error) = tenant_context
        .require_matching_principal_claim("service lifecycle principal-class route policy")
    {
        record_service_authorization_audit(
            state,
            headers,
            &tenant,
            principal_class,
            Some("application_bearer"),
            false,
            format!(
                "{} cross-tenant service route rejected: {error}",
                principal_class.as_str()
            ),
        );
        return Err(AppError::from(error));
    }

    if !principal_has_exact_service_grant(&resolved.principal, service_name) {
        record_service_authorization_audit(
            state,
            headers,
            &tenant,
            principal_class,
            Some("application_bearer"),
            false,
            format!(
                "{} principal lacks exact service grant for `{service_name}`",
                principal_class.as_str()
            ),
        );
        return Err(AppError::forbidden(format!(
            "{} principal requires an exact service grant for `{service_name}`",
            principal_class.as_str()
        )));
    }

    Ok(ServiceRouteAuthorization {
        principal_class,
        tenant_context,
        auth_method: Some("application_bearer"),
    })
}

fn authorize_operator_service_route(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &str,
    surface: &'static str,
) -> Result<Option<ServiceRouteAuthorization>, AppError> {
    let route_tenant = parse_user_tenant_id(tenant_id)?;
    if state.local_server_security.is_none() {
        return Ok(None);
    }

    let extracted = extract_server_access(
        headers,
        LocalServerCredentialMode::AuthorizationOrAdminHeader,
        state.local_server_security.as_deref(),
    )
    .map_err(AppError::from)?;
    match extracted.status {
        ExtractedServerAccessStatus::Authorized => Ok(Some(ServiceRouteAuthorization {
            principal_class: PrincipalClass::Operator,
            tenant_context: parse_operator_tenant_context(tenant_id.to_owned(), surface)?,
            auth_method: extracted.auth_method,
        })),
        ExtractedServerAccessStatus::Missing => Ok(None),
        ExtractedServerAccessStatus::Invalid
            if extracted.auth_method == Some("local_admin_bearer") =>
        {
            Ok(None)
        }
        ExtractedServerAccessStatus::Revoked => {
            record_service_authorization_audit(
                state,
                headers,
                &route_tenant,
                PrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator service route rejected: auth.token_revoked",
            );
            Err(AppError::unauthorized("auth.token_revoked"))
        }
        ExtractedServerAccessStatus::Expired => {
            record_service_authorization_audit(
                state,
                headers,
                &route_tenant,
                PrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator service route rejected: auth.session_expired",
            );
            Err(AppError::unauthorized("auth.session_expired"))
        }
        ExtractedServerAccessStatus::Invalid => {
            record_service_authorization_audit(
                state,
                headers,
                &route_tenant,
                PrincipalClass::Operator,
                extracted.auth_method,
                false,
                "operator service route rejected: invalid local admin credential",
            );
            Err(AppError::unauthorized(
                LocalServerCredentialMode::AuthorizationOrAdminHeader.unauthorized_message(),
            ))
        }
    }
}

fn readiness_from_status(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Ready => "ready",
        SandboxStatus::Stopped => "stopped",
        SandboxStatus::Failed => "failed",
        SandboxStatus::Starting | SandboxStatus::NotReady | SandboxStatus::Stopping => "not_ready",
    }
}

fn health_from_status(status: SandboxStatus) -> &'static str {
    match status {
        SandboxStatus::Ready => "healthy",
        SandboxStatus::Failed => "unhealthy",
        SandboxStatus::Starting
        | SandboxStatus::NotReady
        | SandboxStatus::Stopping
        | SandboxStatus::Stopped => "unknown",
    }
}

fn principal_class_from_principal(
    principal: &PrincipalContext,
) -> Result<PrincipalClass, AppError> {
    let Some(value) = principal_claim_string(
        principal,
        &[
            "nimbus_principal_class",
            "nimbusPrincipalClass",
            "principal_class",
            "principalClass",
        ],
    ) else {
        return Ok(PrincipalClass::Tenant);
    };
    match value {
        "tenant" => Ok(PrincipalClass::Tenant),
        "spawned" | "spawned_workload" | "spawnedWorkload" | "workload" | "workload_identity" => {
            Ok(PrincipalClass::SpawnedWorkload)
        }
        "operator" => Err(AppError::forbidden(
            "application credentials cannot resolve to operator principal class",
        )),
        other => Err(AppError::forbidden(format!(
            "unknown service route principal class `{other}`"
        ))),
    }
}

fn principal_claim_string<'a>(
    principal: &'a PrincipalContext,
    claim_names: &[&str],
) -> Option<&'a str> {
    for claims in [&principal.verified_claims, &principal.claims] {
        for claim_name in claim_names {
            if let Some(value) = claims.get(*claim_name).and_then(Value::as_str) {
                return Some(value);
            }
        }
    }
    None
}

fn principal_has_any_service_definition_permission(
    principal: &PrincipalContext,
    action: ServiceDefinitionAction,
) -> bool {
    service_definition_permission_values(principal)
        .any(|permission| service_definition_permission_actions_allow(permission, action))
}

fn principal_has_service_definition_permission(
    principal: &PrincipalContext,
    action: ServiceDefinitionAction,
    service_name: &str,
) -> bool {
    service_definition_permission_values(principal).any(|permission| {
        service_definition_permission_actions_allow(permission, action)
            && service_definition_permission_scope_allows(permission, service_name)
    })
}

fn service_definition_permission_values(
    principal: &PrincipalContext,
) -> impl Iterator<Item = &Value> {
    [&principal.verified_claims, &principal.claims]
        .into_iter()
        .flat_map(|claims| {
            [
                "nimbus_service_definition_permissions",
                "nimbusServiceDefinitionPermissions",
                "service_definition_permissions",
                "serviceDefinitionPermissions",
            ]
            .into_iter()
            .filter_map(|claim_name| claims.get(claim_name))
        })
        .flat_map(|value| match value {
            Value::Array(values) => values.iter().collect::<Vec<_>>(),
            value => vec![value],
        })
}

fn service_definition_permission_actions_allow(
    permission: &Value,
    action: ServiceDefinitionAction,
) -> bool {
    let Some(actions) = permission.get("actions") else {
        return false;
    };
    match actions {
        Value::String(value) => value == action.as_str(),
        Value::Array(values) => values
            .iter()
            .any(|value| value.as_str() == Some(action.as_str())),
        _ => false,
    }
}

fn service_definition_permission_scope_allows(permission: &Value, service_name: &str) -> bool {
    let Some(scope) = permission.get("scope") else {
        return false;
    };
    let kind = scope.get("kind").and_then(Value::as_str);
    match kind {
        Some("exactName") => scope
            .get("name")
            .and_then(Value::as_str)
            .is_some_and(|name| name == service_name),
        Some("namePrefix") => scope
            .get("prefix")
            .and_then(Value::as_str)
            .is_some_and(|prefix| service_name.starts_with(prefix)),
        _ => false,
    }
}

fn service_backend_from_input(
    tenant_id: &TenantId,
    service_name: &str,
    input: ServiceBackendInput,
) -> Result<ServiceBackend, AppError> {
    match input {
        ServiceBackendInput::Sandbox { sandbox } => Ok(ServiceBackend::sandbox(
            sandbox.into_spec(tenant_id, Some(service_name))?,
        )),
        ServiceBackendInput::BuiltIn { provider } => Ok(ServiceBackend::built_in(provider)),
        ServiceBackendInput::External {
            endpoint,
            auth,
            health,
        } => Ok(ServiceBackend::external(
            endpoint.url,
            external_auth_policy_from_input(auth),
            health_check_policy_from_input(health),
        )),
    }
}

fn external_auth_policy_from_input(input: ExternalAuthPolicyInput) -> ExternalAuthPolicy {
    match input {
        ExternalAuthPolicyInput::None => ExternalAuthPolicy::None,
    }
}

fn health_check_policy_from_input(input: HealthCheckPolicyInput) -> HealthCheckPolicy {
    match input {
        HealthCheckPolicyInput::Http { path } => HealthCheckPolicy::Http { path },
    }
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

fn service_backend_wire_kind(backend: &ServiceBackend) -> &'static str {
    match backend {
        ServiceBackend::Sandbox(_) => "sandbox",
        ServiceBackend::BuiltIn(_) => "builtIn",
        ServiceBackend::External(_) => "external",
    }
}

fn format_millis_rfc3339(millis: u64) -> String {
    let nanos = (millis as i128).saturating_mul(1_000_000);
    match OffsetDateTime::from_unix_timestamp_nanos(nanos) {
        Ok(timestamp) => timestamp
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned()),
        Err(_) => "1970-01-01T00:00:00Z".to_owned(),
    }
}

fn record_service_authorization_audit(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &TenantId,
    principal_class: PrincipalClass,
    auth_method: Option<&'static str>,
    success: bool,
    reason: impl Into<String>,
) {
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::NativeApi,
        tenant_id: Some(tenant_id.as_str().to_owned()),
        auth_scope: "service_principal_class",
        auth_method,
        success,
        origin: origin_from_headers(headers),
        reason: format!(
            "principal_class={} {}",
            principal_class.as_str(),
            reason.into()
        ),
    });
}

fn record_service_definition_authorization_audit(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &TenantId,
    principal_class: PrincipalClass,
    auth_method: Option<&'static str>,
    success: bool,
    reason: impl Into<String>,
) {
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::NativeApi,
        tenant_id: Some(tenant_id.as_str().to_owned()),
        auth_scope: "service_definition_principal_class",
        auth_method,
        success,
        origin: origin_from_headers(headers),
        reason: format!(
            "principal_class={} {}",
            principal_class.as_str(),
            reason.into()
        ),
    });
}

fn parse_user_tenant_id_lossy(value: &str) -> TenantId {
    parse_user_tenant_id(value.to_owned()).unwrap_or_else(|_| {
        TenantId::new("invalid-tenant").expect("fallback tenant id should parse")
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
        &state.engine,
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
