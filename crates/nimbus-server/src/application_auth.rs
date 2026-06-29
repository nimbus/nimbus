use std::sync::Arc;

use axum::http::{HeaderMap, header};
use nimbus_auth::{
    ApplicationAuthError, ResolvedApplicationAuth,
    firebase_emulator_verification_bypass_principal_from_bearer, normalize_principal_context,
    parse_bearer_value,
};
use nimbus_runtime::InvocationAuth;
use tonic::{Status, metadata::MetadataMap};

use crate::state::{AppError, AppState, DeploymentState};

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
        && let Some(principal) = firebase_emulator_verification_bypass_principal_from_bearer(bearer)
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
        .is_some_and(|config| config.allows_emulator_token_verification_bypass())
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
        app_error_from_application_auth(ApplicationAuthError::unauthorized(
            "no application auth providers are configured for the active deployment",
        ))
    })?;
    verifier
        .verify_bearer_token(bearer)
        .await
        .map(Some)
        .map_err(app_error_from_application_auth)
}

pub(crate) fn extract_bearer_token(headers: &HeaderMap) -> Result<Option<String>, AppError> {
    let Some(value) = headers.get(header::AUTHORIZATION) else {
        return Ok(None);
    };
    let value = value.to_str().map_err(|error| {
        AppError::unauthorized(format!("invalid authorization header: {error}"))
    })?;
    let token = parse_bearer_value(value).map_err(app_error_from_application_auth)?;
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
    let token = parse_bearer_value(value).map_err(app_error_from_application_auth)?;
    Ok(Some(token.to_string()))
}

pub(crate) fn app_error_from_application_auth(error: ApplicationAuthError) -> AppError {
    match error {
        ApplicationAuthError::Unauthorized(message) => AppError::unauthorized(message),
        ApplicationAuthError::Forbidden(message) => AppError::forbidden(message),
        ApplicationAuthError::Internal(message) => {
            AppError::Core(nimbus_core::Error::Internal(message))
        }
    }
}
