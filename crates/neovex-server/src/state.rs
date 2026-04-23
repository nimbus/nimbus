use std::sync::{Arc, RwLock};

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use neovex_core::{Error, StorageErrorKind};
use neovex_engine::Service;
use neovex_runtime::{HostCallCancellation, InvocationAuth};
use serde_json::json;
use tracing::warn;

use crate::adapters::convex::ConvexRegistry;
use crate::license::LicenseState;
use crate::service_registry::RuntimeServiceRegistry;

pub(crate) struct AppStateConfig {
    pub(crate) service: Arc<Service>,
    pub(crate) convex_registry: Option<ConvexRegistry>,
    pub(crate) license_state: LicenseState,
    pub(crate) runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    pub(crate) deploy_admin_token: Option<String>,
}

/// Shared application state.
pub(crate) struct AppState {
    pub(crate) service: Arc<Service>,
    pub(crate) convex_registry: Arc<ActiveConvexRegistry>,
    pub(crate) license_state: Arc<LicenseState>,
    pub(crate) runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    pub(crate) deploy_admin_token: Option<String>,
}

impl AppState {
    pub(crate) fn from_config(config: AppStateConfig) -> Self {
        let AppStateConfig {
            service,
            convex_registry,
            license_state,
            runtime_service_registry,
            deploy_admin_token,
        } = config;
        Self {
            service,
            convex_registry: Arc::new(ActiveConvexRegistry::new(convex_registry)),
            license_state: Arc::new(license_state),
            runtime_service_registry,
            deploy_admin_token,
        }
    }

    pub(crate) fn runtime_service_registry(&self) -> Arc<dyn RuntimeServiceRegistry> {
        self.runtime_service_registry.clone()
    }
}

#[derive(Debug)]
pub(crate) struct ActiveConvexRegistry {
    inner: RwLock<ActiveConvexRegistryState>,
}

#[derive(Debug, Default)]
struct ActiveConvexRegistryState {
    generation: u64,
    registry: Option<Arc<ConvexRegistry>>,
}

impl ActiveConvexRegistry {
    fn new(registry: Option<ConvexRegistry>) -> Self {
        let generation = u64::from(registry.is_some());
        Self {
            inner: RwLock::new(ActiveConvexRegistryState {
                generation,
                registry: registry.map(Arc::new),
            }),
        }
    }

    pub(crate) fn current(&self) -> Option<Arc<ConvexRegistry>> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .registry
            .clone()
    }

    pub(crate) fn snapshot(&self) -> (u64, Option<Arc<ConvexRegistry>>) {
        let state = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (state.generation, state.registry.clone())
    }

    pub(crate) fn activate(&self, registry: ConvexRegistry) -> (u64, Option<Arc<ConvexRegistry>>) {
        let mut state = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = state.registry.replace(Arc::new(registry));
        state.generation = state.generation.saturating_add(1);
        (state.generation, previous)
    }

    #[cfg(test)]
    pub(crate) fn generation(&self) -> u64 {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .generation
    }
}

/// HTTP-facing application error wrapper.
#[derive(Debug)]
pub(crate) enum AppError {
    Core(Error),
    Unauthorized(String),
    NotFound(String),
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
                    Error::Conflict(_) => StatusCode::CONFLICT,
                    Error::ResourceExhausted(_) => StatusCode::TOO_MANY_REQUESTS,
                    Error::PermissionDenied(_) => StatusCode::FORBIDDEN,
                    Error::InvalidInput(_) => StatusCode::BAD_REQUEST,
                    Error::SchemaValidation(_) => StatusCode::UNPROCESSABLE_ENTITY,
                    Error::AlreadyExists(_) => StatusCode::CONFLICT,
                    Error::Storage { kind, .. } => match kind {
                        StorageErrorKind::Busy
                        | StorageErrorKind::Transient
                        | StorageErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                        StorageErrorKind::Corruption
                        | StorageErrorKind::Io
                        | StorageErrorKind::Other => StatusCode::INTERNAL_SERVER_ERROR,
                    },
                    Error::Serialization(_) | Error::Internal(_) => {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                (status, Json(json!({ "error": error.to_string() }))).into_response()
            }
            Self::Unauthorized(message) => {
                (StatusCode::UNAUTHORIZED, Json(json!({ "error": message }))).into_response()
            }
            Self::NotFound(message) => {
                (StatusCode::NOT_FOUND, Json(json!({ "error": message }))).into_response()
            }
        }
    }
}

impl AppError {
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core(error) => write!(f, "{error}"),
            Self::Unauthorized(message) => write!(f, "{message}"),
            Self::NotFound(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_storage_error_maps_to_service_unavailable() {
        let response = AppError::from(Error::storage(
            StorageErrorKind::Unavailable,
            "postgres pool unavailable",
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn active_convex_registry_keeps_previous_generation_arc_alive_after_activation() {
        let registry = ActiveConvexRegistry::new(Some(ConvexRegistry::empty()));
        let previous = registry
            .current()
            .expect("initial generation should be present");
        let previous_ptr = Arc::as_ptr(&previous);

        let (generation, replaced) = registry.activate(ConvexRegistry::empty());
        let current = registry
            .current()
            .expect("activated generation should be present");

        assert_eq!(generation, 2);
        assert_eq!(registry.generation(), 2);
        assert_eq!(
            Arc::as_ptr(&replaced.expect("previous generation should return")),
            previous_ptr
        );
        assert_ne!(Arc::as_ptr(&current), previous_ptr);
        assert_eq!(Arc::as_ptr(&previous), previous_ptr);
    }
}

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
        self.token.cancel_due_to_disconnect();
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
