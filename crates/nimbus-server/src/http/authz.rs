use axum::http::HeaderMap;
use nimbus_core::{PrincipalContext, TenantId};
use nimbus_operator::{
    ExtractedServerAccessStatus, LocalServerCredentialMode, extract_server_access,
};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use super::AppError;
use crate::local_server::{
    LocalServerAuditEvent, LocalServerRouteFamily, LocalServerSecurityState, origin_from_headers,
};
use crate::state::AppState;
use crate::tenant::TenantIsolationContext;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PrincipalClass {
    Operator,
    Tenant,
    SpawnedWorkload,
}

impl PrincipalClass {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Operator => "operator",
            Self::Tenant => "tenant",
            Self::SpawnedWorkload => "spawned_workload",
        }
    }
}

pub(super) fn principal_class_from_principal(
    principal: &PrincipalContext,
    route_family: &'static str,
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
            "unknown {route_family} route principal class `{other}`"
        ))),
    }
}

pub(super) fn principal_claim_string<'a>(
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

pub(super) fn permission_claim_values<'a>(
    principal: &'a PrincipalContext,
    claim_names: &[&str],
) -> Vec<&'a Value> {
    let mut values = Vec::new();
    for claims in [&principal.verified_claims, &principal.claims] {
        for claim_name in claim_names {
            let Some(value) = claims.get(*claim_name) else {
                continue;
            };
            match value {
                Value::Array(items) => values.extend(items),
                value => values.push(value),
            }
        }
    }
    values
}

pub(super) fn permission_actions_allow(permission: &Value, required_action: &str) -> bool {
    let Some(actions) = permission.get("actions") else {
        return false;
    };
    match actions {
        Value::String(value) => value == required_action,
        Value::Array(values) => values
            .iter()
            .any(|value| value.as_str() == Some(required_action)),
        _ => false,
    }
}

pub(super) fn format_millis_rfc3339(millis: u64) -> String {
    let nanos = (millis as i128).saturating_mul(1_000_000);
    match OffsetDateTime::from_unix_timestamp_nanos(nanos) {
        Ok(timestamp) => timestamp
            .format(&Rfc3339)
            .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned()),
        Err(_) => "1970-01-01T00:00:00Z".to_owned(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperatorRouteAccess {
    Authorized { auth_method: Option<&'static str> },
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperatorRouteRejection {
    Revoked { auth_method: Option<&'static str> },
    Expired { auth_method: Option<&'static str> },
    Invalid { auth_method: Option<&'static str> },
}

impl OperatorRouteRejection {
    pub(super) fn auth_method(self) -> Option<&'static str> {
        match self {
            Self::Revoked { auth_method }
            | Self::Expired { auth_method }
            | Self::Invalid { auth_method } => auth_method,
        }
    }

    pub(super) fn reason(self) -> &'static str {
        match self {
            Self::Revoked { .. } => "auth.token_revoked",
            Self::Expired { .. } => "auth.session_expired",
            Self::Invalid { .. } => "invalid local admin credential",
        }
    }

    pub(super) fn app_error(self) -> AppError {
        match self {
            Self::Revoked { .. } => AppError::unauthorized("auth.token_revoked"),
            Self::Expired { .. } => AppError::unauthorized("auth.session_expired"),
            Self::Invalid { .. } => AppError::unauthorized(
                LocalServerCredentialMode::AuthorizationOrAdminHeader.unauthorized_message(),
            ),
        }
    }
}

pub(super) fn extract_operator_route_access(
    headers: &HeaderMap,
    local_server_security: Option<&LocalServerSecurityState>,
) -> Result<Result<OperatorRouteAccess, OperatorRouteRejection>, AppError> {
    if local_server_security.is_none() {
        return Ok(Ok(OperatorRouteAccess::Missing));
    }

    let extracted = extract_server_access(
        headers,
        LocalServerCredentialMode::AuthorizationOrAdminHeader,
        local_server_security,
    )
    .map_err(AppError::from)?;
    match extracted.status {
        ExtractedServerAccessStatus::Authorized => Ok(Ok(OperatorRouteAccess::Authorized {
            auth_method: extracted.auth_method,
        })),
        ExtractedServerAccessStatus::Missing => Ok(Ok(OperatorRouteAccess::Missing)),
        ExtractedServerAccessStatus::Invalid
            if extracted.auth_method == Some("local_admin_bearer") =>
        {
            Ok(Ok(OperatorRouteAccess::Missing))
        }
        ExtractedServerAccessStatus::Revoked => Ok(Err(OperatorRouteRejection::Revoked {
            auth_method: extracted.auth_method,
        })),
        ExtractedServerAccessStatus::Expired => Ok(Err(OperatorRouteRejection::Expired {
            auth_method: extracted.auth_method,
        })),
        ExtractedServerAccessStatus::Invalid => Ok(Err(OperatorRouteRejection::Invalid {
            auth_method: extracted.auth_method,
        })),
    }
}

/// Which resource family an operator-route authorization decision (and its
/// audit trail) belongs to. Carries the `auth_scope` audit literal so call
/// sites never pass a raw string for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperatorAuthScope {
    Session,
    Service,
    ServiceDefinition,
    Sandbox,
}

impl OperatorAuthScope {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session_principal_class",
            Self::Service => "service_principal_class",
            Self::ServiceDefinition => "service_definition_principal_class",
            Self::Sandbox => "sandbox_principal_class",
        }
    }

    fn operator_route_noun(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Service => "service",
            Self::ServiceDefinition => "service definition",
            Self::Sandbox => "sandbox",
        }
    }
}

/// What an authorized operator-route check hands back to the caller's
/// `build` closure: enough to construct any of the per-resource
/// authorization results without the shared core knowing their shapes.
pub(super) struct OperatorGrant {
    pub(super) tenant_context: TenantIsolationContext,
    pub(super) auth_method: Option<&'static str>,
}

/// Single implementation of the operator-route extract → authorized /
/// missing / rejected flow shared by the session, service, and sandbox
/// route families. `tenant` must already be a validated [`TenantId`].
/// Returns `Ok(None)` when there is no operator credential on the route
/// (the caller should fall through to application-auth), `Ok(Some(_))`
/// when authorized, and `Err` (with the rejection already audited) when an
/// operator credential was presented but rejected.
pub(super) fn authorize_operator_route<A>(
    headers: &HeaderMap,
    state: &AppState,
    tenant: &TenantId,
    surface: &'static str,
    scope: OperatorAuthScope,
    build: impl FnOnce(OperatorGrant) -> A,
) -> Result<Option<A>, AppError> {
    match extract_operator_route_access(headers, state.local_server_security().as_deref())? {
        Ok(OperatorRouteAccess::Authorized { auth_method }) => {
            let tenant_context = TenantIsolationContext::operator(tenant.clone(), surface);
            Ok(Some(build(OperatorGrant {
                tenant_context,
                auth_method,
            })))
        }
        Ok(OperatorRouteAccess::Missing) => Ok(None),
        Err(rejection) => {
            record_operator_authorization_audit(
                state,
                headers,
                tenant.as_str(),
                scope,
                PrincipalClass::Operator,
                rejection.auth_method(),
                false,
                format!(
                    "operator {} route rejected: {}",
                    scope.operator_route_noun(),
                    rejection.reason()
                ),
            );
            Err(rejection.app_error())
        }
    }
}

/// Single parameterized audit recorder for operator/application principal
/// class authorization outcomes across the session, service, service
/// definition, and sandbox route families.
// 20+ call sites each vary every field independently (flat audit-event
// shape); a struct would only move the same 8 fields into a literal at every
// call site without adding a meaningful sub-concept.
#[allow(clippy::too_many_arguments)]
pub(super) fn record_operator_authorization_audit(
    state: &AppState,
    headers: &HeaderMap,
    tenant_id: &str,
    scope: OperatorAuthScope,
    principal_class: PrincipalClass,
    auth_method: Option<&'static str>,
    success: bool,
    reason: impl Into<String>,
) {
    state.record_local_server_audit(LocalServerAuditEvent {
        route_family: LocalServerRouteFamily::NativeApi,
        tenant_id: Some(tenant_id.to_owned()),
        auth_scope: scope.as_str(),
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

#[cfg(test)]
mod tests {
    use axum::response::IntoResponse;
    use nimbus_core::PrincipalContext;
    use serde_json::{Map, Value, json};

    use super::*;

    fn principal(
        claims: Map<String, Value>,
        verified_claims: Map<String, Value>,
    ) -> PrincipalContext {
        PrincipalContext {
            authenticated: true,
            claims,
            verified_claims,
        }
    }

    #[test]
    fn principal_class_prefers_verified_claims_and_rejects_operator_identity() {
        let mut claims = Map::new();
        claims.insert(
            "principalClass".to_string(),
            Value::String("tenant".to_string()),
        );
        let mut verified_claims = Map::new();
        verified_claims.insert(
            "principalClass".to_string(),
            Value::String("spawned_workload".to_string()),
        );
        let context = principal(claims, verified_claims);
        assert_eq!(
            principal_class_from_principal(&context, "session").expect("class should parse"),
            PrincipalClass::SpawnedWorkload
        );

        let mut claims = Map::new();
        claims.insert(
            "principalClass".to_string(),
            Value::String("operator".to_string()),
        );
        let error = principal_class_from_principal(&principal(claims, Map::new()), "sandbox")
            .expect_err("operator class should be rejected for application credentials");
        assert_eq!(
            error.into_response().status(),
            axum::http::StatusCode::FORBIDDEN
        );
    }

    #[test]
    fn permission_claim_values_flattens_arrays_and_preserves_scalar_permissions() {
        let mut claims = Map::new();
        claims.insert(
            "permissions".to_string(),
            json!([
                {"actions": ["get"]},
                {"actions": "list"}
            ]),
        );
        let mut verified_claims = Map::new();
        verified_claims.insert("permissions".to_string(), json!({"actions": "create"}));
        let principal = principal(claims, verified_claims);

        let values = permission_claim_values(&principal, &["permissions"]);
        assert_eq!(values.len(), 3);
        assert!(
            values
                .iter()
                .any(|value| permission_actions_allow(value, "create"))
        );
        assert!(
            values
                .iter()
                .any(|value| permission_actions_allow(value, "get"))
        );
        assert!(
            values
                .iter()
                .any(|value| permission_actions_allow(value, "list"))
        );
        assert!(
            !values
                .iter()
                .any(|value| permission_actions_allow(value, "delete"))
        );
    }

    #[test]
    fn format_millis_rfc3339_saturates_invalid_timestamps_to_epoch() {
        assert_eq!(format_millis_rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(format_millis_rfc3339(u64::MAX), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn operator_route_access_treats_missing_security_as_missing() {
        let headers = HeaderMap::new();
        assert_eq!(
            extract_operator_route_access(&headers, None).expect("extraction should succeed"),
            Ok(OperatorRouteAccess::Missing)
        );
    }
}
