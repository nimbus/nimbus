use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use axum::response::{IntoResponse, Response};
use neovex_core::Error;
use neovex_engine::Service;
use neovex_runtime::{HostCallCancellation, InvocationAuth};
use tracing::warn;

use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::convex::ConvexRegistry;
use crate::adapters::firebase::FirebaseConfig;
use crate::application_auth::ApplicationAuthVerifier;
use crate::error_envelope::StructuredHttpError;
use crate::license::LicenseState;
use crate::local_server::{LocalServerAuditEvent, LocalServerSecurityState};
use crate::service_registry::RuntimeServiceRegistry;

pub(crate) struct AppStateConfig {
    pub(crate) service: Arc<Service>,
    pub(crate) convex_registry: Option<ConvexRegistry>,
    pub(crate) application_auth_verifier: Option<Arc<dyn ApplicationAuthVerifier>>,
    pub(crate) cloud_functions_registry: Option<CloudFunctionsRegistry>,
    pub(crate) firebase_config: Option<FirebaseConfig>,
    pub(crate) license_state: LicenseState,
    pub(crate) runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    pub(crate) deploy_admin_token: Option<String>,
    pub(crate) local_server_security: Option<Arc<LocalServerSecurityState>>,
    pub(crate) listen_addr: Option<SocketAddr>,
}

/// Shared application state.
pub(crate) struct AppState {
    pub(crate) service: Arc<Service>,
    pub(crate) convex_registry: Arc<ActiveConvexRegistry>,
    pub(crate) application_auth_verifier: Arc<ActiveApplicationAuthVerifier>,
    pub(crate) cloud_functions_registry: Arc<ActiveCloudFunctionsRegistry>,
    pub(crate) firebase_config: Arc<ActiveFirebaseConfig>,
    pub(crate) deploy_generation: Arc<ActiveDeployGeneration>,
    pub(crate) license_state: Arc<LicenseState>,
    pub(crate) runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    pub(crate) deploy_admin_token: Option<String>,
    pub(crate) local_server_security: Option<Arc<LocalServerSecurityState>>,
    pub(crate) listen_addr: Option<SocketAddr>,
}

impl AppState {
    pub(crate) fn from_config(config: AppStateConfig) -> Self {
        let AppStateConfig {
            service,
            convex_registry,
            application_auth_verifier,
            cloud_functions_registry,
            firebase_config,
            license_state,
            runtime_service_registry,
            deploy_admin_token,
            local_server_security,
            listen_addr,
        } = config;
        let convex_registry = convex_registry.map(Arc::new);
        let initial_generation =
            u64::from(convex_registry.is_some() || cloud_functions_registry.is_some());
        Self {
            service,
            convex_registry: Arc::new(ActiveConvexRegistry::from_arc(convex_registry)),
            application_auth_verifier: Arc::new(ActiveApplicationAuthVerifier::new(
                application_auth_verifier,
            )),
            cloud_functions_registry: Arc::new(ActiveCloudFunctionsRegistry::new(
                cloud_functions_registry,
            )),
            firebase_config: Arc::new(ActiveFirebaseConfig::new(firebase_config)),
            deploy_generation: Arc::new(ActiveDeployGeneration::new(initial_generation)),
            license_state: Arc::new(license_state),
            runtime_service_registry,
            deploy_admin_token,
            local_server_security,
            listen_addr,
        }
    }

    pub(crate) fn runtime_service_registry(&self) -> Arc<dyn RuntimeServiceRegistry> {
        self.runtime_service_registry.clone()
    }

    pub(crate) fn record_local_server_audit(&self, event: LocalServerAuditEvent) {
        let Some(local_server_security) = self.local_server_security.as_ref() else {
            return;
        };
        if let Err(error) = local_server_security.record_audit_event(event) {
            warn!(
                audit_log_path = %local_server_security.paths().audit_log_path.display(),
                error = %error,
                "failed to append local server audit log"
            );
        }
    }
}

pub(crate) struct ActiveApplicationAuthVerifier {
    inner: RwLock<Option<Arc<dyn ApplicationAuthVerifier>>>,
}

impl ActiveApplicationAuthVerifier {
    fn new(verifier: Option<Arc<dyn ApplicationAuthVerifier>>) -> Self {
        Self {
            inner: RwLock::new(verifier),
        }
    }

    pub(crate) fn current(&self) -> Option<Arc<dyn ApplicationAuthVerifier>> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(crate) fn activate(
        &self,
        verifier: Arc<dyn ApplicationAuthVerifier>,
    ) -> Option<Arc<dyn ApplicationAuthVerifier>> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .replace(verifier)
    }
}

#[derive(Debug)]
pub(crate) struct ActiveCloudFunctionsRegistry {
    inner: RwLock<Option<Arc<CloudFunctionsRegistry>>>,
}

impl ActiveCloudFunctionsRegistry {
    fn new(registry: Option<CloudFunctionsRegistry>) -> Self {
        Self {
            inner: RwLock::new(registry.map(Arc::new)),
        }
    }

    pub(crate) fn current(&self) -> Option<Arc<CloudFunctionsRegistry>> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(crate) fn activate(
        &self,
        registry: CloudFunctionsRegistry,
    ) -> Option<Arc<CloudFunctionsRegistry>> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .replace(Arc::new(registry))
    }
}

#[derive(Debug)]
pub(crate) struct ActiveDeployGeneration {
    current: RwLock<u64>,
}

impl ActiveDeployGeneration {
    fn new(initial_generation: u64) -> Self {
        Self {
            current: RwLock::new(initial_generation),
        }
    }

    pub(crate) fn current(&self) -> u64 {
        *self
            .current
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn advance(&self) -> (u64, u64) {
        let mut generation = self
            .current
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = *generation;
        *generation = generation.saturating_add(1);
        (*generation, previous)
    }
}

#[derive(Debug)]
pub(crate) struct ActiveFirebaseConfig {
    inner: RwLock<Option<Arc<FirebaseConfig>>>,
}

impl ActiveFirebaseConfig {
    fn new(config: Option<FirebaseConfig>) -> Self {
        Self {
            inner: RwLock::new(config.map(Arc::new)),
        }
    }

    pub(crate) fn current(&self) -> Option<Arc<FirebaseConfig>> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
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
    #[cfg(test)]
    fn new(registry: Option<ConvexRegistry>) -> Self {
        Self::from_arc(registry.map(Arc::new))
    }

    fn from_arc(registry: Option<Arc<ConvexRegistry>>) -> Self {
        let generation = u64::from(registry.is_some());
        Self {
            inner: RwLock::new(ActiveConvexRegistryState {
                generation,
                registry,
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

    #[cfg(test)]
    pub(crate) fn activate(&self, registry: ConvexRegistry) -> (u64, Option<Arc<ConvexRegistry>>) {
        self.activate_shared(Arc::new(registry))
    }

    pub(crate) fn activate_shared(
        &self,
        registry: Arc<ConvexRegistry>,
    ) -> (u64, Option<Arc<ConvexRegistry>>) {
        let mut state = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let previous = state.registry.replace(registry);
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
    Forbidden(String),
    NotFound(String),
    Structured(Box<StructuredHttpError>),
}

impl From<Error> for AppError {
    fn from(value: Error) -> Self {
        Self::Core(value)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        StructuredHttpError::from_app_error(self).into_response()
    }
}

impl AppError {
    pub(crate) fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub(crate) fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Structured(error) => write!(f, "{error}"),
            Self::Core(error) => write!(f, "{error}"),
            Self::Unauthorized(message) => write!(f, "{message}"),
            Self::Forbidden(message) => write!(f, "{message}"),
            Self::NotFound(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for AppError {}

#[cfg(test)]
mod tests {
    use axum::http::StatusCode;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn unavailable_storage_error_maps_to_service_unavailable() {
        let response = AppError::from(Error::storage(
            neovex_core::StorageErrorKind::Unavailable,
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

    #[test]
    fn active_deploy_generation_advances_from_initial_state() {
        let generation = ActiveDeployGeneration::new(1);

        assert_eq!(generation.current(), 1);
        assert_eq!(generation.advance(), (2, 1));
        assert_eq!(generation.current(), 2);
    }

    #[test]
    fn active_firebase_config_returns_current_config_when_present() {
        let config = ActiveFirebaseConfig::new(Some(FirebaseConfig::new()));
        assert!(config.current().is_some());

        let missing = ActiveFirebaseConfig::new(None);
        assert!(missing.current().is_none());
    }

    #[test]
    fn app_state_does_not_infer_application_auth_verifier_from_convex_registry() {
        let temp = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(temp.path()).expect("service should build"));
        let state = AppState::from_config(AppStateConfig {
            service,
            convex_registry: Some(ConvexRegistry::empty()),
            application_auth_verifier: None,
            cloud_functions_registry: None,
            firebase_config: None,
            license_state: LicenseState::community(),
            runtime_service_registry: Arc::new(
                crate::service_registry::SandboxCatalogRuntimeServiceRegistry::new(Arc::new(
                    crate::sandbox::EmptySandboxCatalog,
                )),
            ),
            deploy_admin_token: None,
            local_server_security: None,
            listen_addr: None,
        });

        assert!(state.application_auth_verifier.current().is_none());
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
