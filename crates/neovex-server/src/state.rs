use std::sync::Arc;

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use neovex_core::Error;
use neovex_engine::Service;
use neovex_runtime::{HostCallCancellation, InvocationAuth};
use serde_json::json;
use tracing::warn;

use crate::convex::ConvexRegistry;
use crate::license::LicenseState;

/// Shared application state.
pub(crate) struct AppState {
    pub(crate) service: Arc<Service>,
    pub(crate) convex_registry: Option<Arc<ConvexRegistry>>,
    pub(crate) license_state: Arc<LicenseState>,
}

impl AppState {
    pub(crate) fn with_license_state(service: Arc<Service>, license_state: LicenseState) -> Self {
        Self {
            service,
            convex_registry: None,
            license_state: Arc::new(license_state),
        }
    }

    pub(crate) fn with_convex_registry_and_license_state(
        service: Arc<Service>,
        convex_registry: ConvexRegistry,
        license_state: LicenseState,
    ) -> Self {
        Self {
            service,
            convex_registry: Some(Arc::new(convex_registry)),
            license_state: Arc::new(license_state),
        }
    }
}

/// HTTP-facing application error wrapper.
#[derive(Debug)]
pub(crate) enum AppError {
    Core(Error),
    Unauthorized(String),
}

impl From<Error> for AppError {
    fn from(value: Error) -> Self {
        Self::Core(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match self {
            Self::Core(error) => {
                let status = match error {
                    Error::Cancelled => StatusCode::REQUEST_TIMEOUT,
                    Error::TenantNotFound(_)
                    | Error::DocumentNotFound(_)
                    | Error::ScheduledJobNotFound(_)
                    | Error::SchemaNotFound(_) => StatusCode::NOT_FOUND,
                    Error::InvalidInput(_) => StatusCode::BAD_REQUEST,
                    Error::SchemaValidation(_) => StatusCode::UNPROCESSABLE_ENTITY,
                    Error::AlreadyExists(_) => StatusCode::CONFLICT,
                    Error::Storage(_) | Error::Serialization(_) | Error::Internal(_) => {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                (status, Json(json!({ "error": error.to_string() }))).into_response()
            }
            Self::Unauthorized(message) => {
                (StatusCode::UNAUTHORIZED, Json(json!({ "error": message }))).into_response()
            }
        }
    }
}

impl AppError {
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core(error) => write!(f, "{error}"),
            Self::Unauthorized(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AppError {}

#[derive(Debug, Default)]
pub(crate) struct RequestCancellationGuard {
    token: HostCallCancellation,
}

impl RequestCancellationGuard {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn token(&self) -> HostCallCancellation {
        self.token.clone()
    }
}

impl Drop for RequestCancellationGuard {
    fn drop(&mut self) {
        self.token.cancel();
    }
}

pub(crate) async fn record_authenticated_usage(
    state: &Arc<AppState>,
    auth: Option<&InvocationAuth>,
) {
    let Some(token_identifier) = auth
        .and_then(InvocationAuth::token_identifier)
        .map(str::to_owned)
    else {
        return;
    };

    let service = state.service.clone();
    if let Err(error) = service
        .record_monthly_active_user_async(token_identifier)
        .await
    {
        warn!(
            error = %error,
            "failed to record monthly active user usage"
        );
    }
}
