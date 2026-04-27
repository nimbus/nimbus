use std::sync::Arc;

use axum::http::{HeaderMap, header};
use futures::future::BoxFuture;
use neovex_core::PrincipalContext;
use neovex_runtime::InvocationAuth;
use serde::Serialize;
use serde_json::{Map, Value};
use tonic::{Status, metadata::MetadataMap};

use crate::state::{AppError, AppState, DeploymentState};

pub(crate) trait ApplicationAuthVerifier: Send + Sync {
    fn verify_bearer_token<'a>(
        &'a self,
        token: &'a str,
    ) -> BoxFuture<'a, Result<InvocationAuth, AppError>>;
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedApplicationAuth {
    pub(crate) auth: Option<InvocationAuth>,
    pub(crate) principal: PrincipalContext,
}

impl ResolvedApplicationAuth {
    fn anonymous() -> Self {
        Self {
            auth: None,
            principal: PrincipalContext::anonymous(),
        }
    }
}

pub(crate) async fn resolve_application_auth_from_headers(
    state: &Arc<AppState>,
    headers: &HeaderMap,
) -> Result<ResolvedApplicationAuth, AppError> {
    let bearer = extract_bearer_token(headers)?;
    let deployment = state.current_deployment();
    resolve_application_auth_from_bearer_in_deployment(deployment.as_ref(), bearer.as_deref()).await
}

pub(crate) async fn resolve_application_auth_from_bearer(
    state: &Arc<AppState>,
    bearer: Option<&str>,
) -> Result<ResolvedApplicationAuth, AppError> {
    let deployment = state.current_deployment();
    resolve_application_auth_from_bearer_in_deployment(deployment.as_ref(), bearer).await
}

pub(crate) async fn resolve_application_auth_from_bearer_in_deployment(
    deployment: &DeploymentState,
    bearer: Option<&str>,
) -> Result<ResolvedApplicationAuth, AppError> {
    let Some(bearer) = bearer else {
        return Ok(ResolvedApplicationAuth::anonymous());
    };

    if firebase_emulator_mock_auth_enabled(deployment)
        && let Some(principal) = emulator_principal_from_bearer(bearer)
    {
        return Ok(ResolvedApplicationAuth {
            auth: None,
            principal,
        });
    }

    let Some(auth) =
        verify_optional_application_auth_from_bearer_in_deployment(deployment, Some(bearer))
            .await?
    else {
        return Ok(ResolvedApplicationAuth::anonymous());
    };
    let principal = normalize_principal_context(Some(&auth));
    Ok(ResolvedApplicationAuth {
        auth: Some(auth),
        principal,
    })
}

fn firebase_emulator_mock_auth_enabled(deployment: &DeploymentState) -> bool {
    deployment
        .firebase_config()
        .as_deref()
        .is_some_and(|config| config.allows_emulator_mock_user_token_auth())
}

fn emulator_principal_from_bearer(token: &str) -> Option<PrincipalContext> {
    let Value::Object(mut claims) = serde_json::from_str::<Value>(token).ok()? else {
        return None;
    };
    normalize_subject_aliases(&mut claims);
    Some(PrincipalContext {
        authenticated: true,
        claims,
        verified_claims: Map::new(),
    })
}

fn normalize_subject_aliases(claims: &mut Map<String, Value>) {
    let canonical = claims
        .get("subject")
        .cloned()
        .or_else(|| claims.get("sub").cloned())
        .or_else(|| claims.get("user_id").cloned())
        .or_else(|| claims.get("uid").cloned());
    let Some(subject) = canonical else {
        return;
    };
    claims
        .entry("subject".to_string())
        .or_insert_with(|| subject.clone());
    claims.entry("sub".to_string()).or_insert(subject);
}

pub(crate) fn normalize_principal_context(auth: Option<&InvocationAuth>) -> PrincipalContext {
    let Some(auth) = auth else {
        return PrincipalContext::anonymous();
    };

    PrincipalContext {
        authenticated: auth.identity.is_some() || auth.verified_identity.is_some(),
        claims: claims_map(auth.identity.as_ref()),
        verified_claims: claims_map(auth.verified_identity.as_ref()),
    }
}

fn claims_map<T>(value: Option<&T>) -> Map<String, Value>
where
    T: Serialize,
{
    value
        .and_then(|value| serde_json::to_value(value).ok())
        .and_then(|value| match value {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

pub(crate) fn grpc_status_from_app_error(error: AppError) -> Status {
    match error {
        AppError::Unauthorized(message) => Status::unauthenticated(message),
        AppError::Forbidden(message) => Status::permission_denied(message),
        AppError::NotFound(message) => Status::not_found(message),
        AppError::Core(error) => Status::internal(error.to_string()),
        AppError::Structured(error) => {
            let status = error.status();
            let message = error.message().to_string();
            match status.as_u16() {
                400 => Status::invalid_argument(message),
                401 => Status::unauthenticated(message),
                403 => Status::permission_denied(message),
                404 => Status::not_found(message),
                409 => Status::aborted(message),
                429 => Status::resource_exhausted(message),
                501 => Status::unimplemented(message),
                503 => Status::unavailable(message),
                504 => Status::deadline_exceeded(message),
                _ => Status::internal(message),
            }
        }
    }
}

pub(crate) async fn verify_optional_application_auth_from_headers_in_deployment(
    deployment: &DeploymentState,
    headers: &HeaderMap,
) -> Result<Option<InvocationAuth>, AppError> {
    let bearer = extract_bearer_token(headers)?;
    verify_optional_application_auth_from_bearer_in_deployment(deployment, bearer.as_deref()).await
}

pub(crate) async fn verify_optional_application_auth_from_bearer_in_deployment(
    deployment: &DeploymentState,
    bearer: Option<&str>,
) -> Result<Option<InvocationAuth>, AppError> {
    let Some(bearer) = bearer else {
        return Ok(None);
    };
    let verifier = deployment.application_auth_verifier().ok_or_else(|| {
        AppError::unauthorized(
            "no application auth providers are configured for the active deployment",
        )
    })?;
    verifier.verify_bearer_token(bearer).await.map(Some)
}

pub(crate) fn extract_bearer_token(headers: &HeaderMap) -> Result<Option<String>, AppError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|error| {
        AppError::unauthorized(format!("invalid authorization header: {error}"))
    })?;
    let token =
        parse_bearer_value(value).map_err(|message| AppError::unauthorized(message.to_string()))?;
    Ok(Some(token.to_string()))
}

pub(crate) fn extract_bearer_token_from_metadata(
    metadata: &MetadataMap,
) -> Result<Option<String>, AppError> {
    let Some(value) = metadata.get("authorization") else {
        return Ok(None);
    };
    let value = value
        .to_str()
        .map_err(|_| AppError::unauthorized("authorization metadata must be valid ASCII text"))?;
    let token =
        parse_bearer_value(value).map_err(|message| AppError::unauthorized(message.to_string()))?;
    Ok(Some(token.to_string()))
}

fn parse_bearer_value(value: &str) -> Result<&str, &'static str> {
    let (scheme, token) = value
        .split_once(' ')
        .ok_or("authorization header must use the Bearer scheme")?;
    if !scheme.eq_ignore_ascii_case("bearer") {
        return Err("authorization header must use the Bearer scheme");
    }
    let token = token.trim();
    if token.is_empty() {
        return Err("authorization header is missing a token");
    }
    Ok(token)
}
