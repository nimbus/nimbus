use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use axum::response::{IntoResponse, Response};
use nimbus_core::Error;
use nimbus_engine::Service;
use nimbus_runtime::{HostCallCancellation, InvocationAuth};
use tokio::sync::watch;
use tracing::warn;

use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::convex::ConvexRegistry;
use crate::adapters::firebase::FirebaseConfig;
use crate::application_auth::ApplicationAuthVerifier;
use crate::error_envelope::StructuredHttpError;
use crate::license::LicenseState;
use crate::local_server::{LocalServerAuditEvent, LocalServerSecurityState};
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::service_manager::SandboxServiceManager;
use crate::service_registry::RuntimeServiceRegistry;

pub(crate) struct AppStateConfig {
    pub(crate) service: Arc<Service>,
    pub(crate) convex_registry: Option<ConvexRegistry>,
    pub(crate) system_convex_registry: Option<ConvexRegistry>,
    pub(crate) application_auth_verifier: Option<Arc<dyn ApplicationAuthVerifier>>,
    pub(crate) cloud_functions_registry: Option<CloudFunctionsRegistry>,
    pub(crate) firebase_config: Option<FirebaseConfig>,
    pub(crate) license_state: LicenseState,
    pub(crate) runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    pub(crate) sandbox_service_manager: Option<Arc<SandboxServiceManager>>,
    pub(crate) machine_lifecycle_manager: Option<Arc<dyn MachineLifecycleManager>>,
    pub(crate) deploy_admin_token: Option<String>,
    pub(crate) local_server_security: Option<Arc<LocalServerSecurityState>>,
    pub(crate) listen_addr: Option<SocketAddr>,
    pub(crate) server_shutdown: Option<watch::Sender<bool>>,
}

/// Shared application state.
pub(crate) struct AppState {
    pub(crate) service: Arc<Service>,
    pub(crate) active_deployment: Arc<ActiveDeployment>,
    system_convex_registry: Option<Arc<ConvexRegistry>>,
    pub(crate) license_state: Arc<LicenseState>,
    pub(crate) runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    sandbox_service_manager: Option<Arc<SandboxServiceManager>>,
    machine_lifecycle_manager: Option<Arc<dyn MachineLifecycleManager>>,
    pub(crate) deploy_admin_token: Option<String>,
    pub(crate) local_server_security: Option<Arc<LocalServerSecurityState>>,
    pub(crate) listen_addr: Option<SocketAddr>,
    server_shutdown: Option<watch::Sender<bool>>,
}

impl AppState {
    pub(crate) fn from_config(config: AppStateConfig) -> Self {
        let AppStateConfig {
            service,
            convex_registry,
            system_convex_registry,
            application_auth_verifier,
            cloud_functions_registry,
            firebase_config,
            license_state,
            runtime_service_registry,
            sandbox_service_manager,
            machine_lifecycle_manager,
            deploy_admin_token,
            local_server_security,
            listen_addr,
            server_shutdown,
        } = config;
        let convex_registry = convex_registry.map(Arc::new);
        let system_convex_registry = system_convex_registry.map(Arc::new);
        let initial_generation =
            u64::from(convex_registry.is_some() || cloud_functions_registry.is_some());
        let active_deployment = DeploymentState {
            generation: initial_generation,
            convex_registry,
            application_auth_verifier,
            cloud_functions_registry: cloud_functions_registry.map(Arc::new),
            firebase_config: firebase_config.map(Arc::new),
        };
        Self {
            service,
            active_deployment: Arc::new(ActiveDeployment::new(active_deployment)),
            system_convex_registry,
            license_state: Arc::new(license_state),
            runtime_service_registry,
            sandbox_service_manager,
            machine_lifecycle_manager,
            deploy_admin_token,
            local_server_security,
            listen_addr,
            server_shutdown,
        }
    }

    pub(crate) fn current_deployment(&self) -> Arc<DeploymentState> {
        self.active_deployment.current()
    }

    pub(crate) fn runtime_service_registry(&self) -> Arc<dyn RuntimeServiceRegistry> {
        self.runtime_service_registry.clone()
    }

    pub(crate) fn sandbox_service_manager(&self) -> Option<Arc<SandboxServiceManager>> {
        self.sandbox_service_manager.clone()
    }

    pub(crate) fn machine_lifecycle_manager(&self) -> Option<Arc<dyn MachineLifecycleManager>> {
        self.machine_lifecycle_manager.clone()
    }

    pub(crate) fn system_convex_registry(&self) -> Option<Arc<ConvexRegistry>> {
        self.system_convex_registry.clone()
    }

    pub(crate) fn request_server_shutdown(&self) -> std::result::Result<(), AppError> {
        let sender = self.server_shutdown.as_ref().ok_or_else(|| {
            AppError::from(Error::Internal(
                "server shutdown is unavailable for this router".to_owned(),
            ))
        })?;
        sender.send_replace(true);
        Ok(())
    }

    pub(crate) fn install_cloud_functions_runtime_hooks(
        &self,
        registry: Arc<CloudFunctionsRegistry>,
    ) -> std::result::Result<(), AppError> {
        self.service
            .install_trigger_registrations(registry.trigger_registrations()?)?;
        self.service.install_trigger_invocation_executor(Arc::new(
            crate::adapters::cloud_functions::CloudFunctionsTriggerExecutor::new(
                self.service.clone(),
                registry,
                self.runtime_service_registry(),
            ),
        ))?;
        Ok(())
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

#[derive(Clone)]
pub(crate) struct DeploymentState {
    pub(crate) generation: u64,
    pub(crate) convex_registry: Option<Arc<ConvexRegistry>>,
    pub(crate) application_auth_verifier: Option<Arc<dyn ApplicationAuthVerifier>>,
    pub(crate) cloud_functions_registry: Option<Arc<CloudFunctionsRegistry>>,
    pub(crate) firebase_config: Option<Arc<FirebaseConfig>>,
}

impl DeploymentState {
    pub(crate) fn convex_registry(&self) -> Option<Arc<ConvexRegistry>> {
        self.convex_registry.clone()
    }

    pub(crate) fn application_auth_verifier(&self) -> Option<Arc<dyn ApplicationAuthVerifier>> {
        self.application_auth_verifier.clone()
    }

    pub(crate) fn cloud_functions_registry(&self) -> Option<Arc<CloudFunctionsRegistry>> {
        self.cloud_functions_registry.clone()
    }

    pub(crate) fn firebase_config(&self) -> Option<Arc<FirebaseConfig>> {
        self.firebase_config.clone()
    }
}

pub(crate) struct ActiveDeployment {
    inner: RwLock<Arc<DeploymentState>>,
}

impl ActiveDeployment {
    fn new(initial: DeploymentState) -> Self {
        Self {
            inner: RwLock::new(Arc::new(initial)),
        }
    }

    pub(crate) fn current(&self) -> Arc<DeploymentState> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(crate) fn activate(&self, deployment: DeploymentState) -> Arc<DeploymentState> {
        let mut current = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(&mut *current, Arc::new(deployment))
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
            nimbus_core::StorageErrorKind::Unavailable,
            "postgres pool unavailable",
        ))
        .into_response();

        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[test]
    fn active_deployment_keeps_previous_snapshot_arc_alive_after_activation() {
        let deployment = ActiveDeployment::new(DeploymentState {
            generation: 1,
            convex_registry: Some(Arc::new(ConvexRegistry::empty())),
            application_auth_verifier: None,
            cloud_functions_registry: None,
            firebase_config: Some(Arc::new(FirebaseConfig::new())),
        });
        let previous = deployment.current();
        let previous_ptr = Arc::as_ptr(&previous);

        let replaced = deployment.activate(DeploymentState {
            generation: 2,
            convex_registry: Some(Arc::new(ConvexRegistry::empty())),
            application_auth_verifier: None,
            cloud_functions_registry: None,
            firebase_config: previous.firebase_config(),
        });
        let current = deployment.current();

        assert_eq!(current.generation, 2);
        assert_eq!(Arc::as_ptr(&replaced), previous_ptr);
        assert_ne!(Arc::as_ptr(&current), previous_ptr);
        assert_eq!(Arc::as_ptr(&previous), previous_ptr);
    }

    #[test]
    fn app_state_does_not_infer_application_auth_verifier_from_convex_registry() {
        let temp = tempdir().expect("service tempdir should build");
        let service = Arc::new(Service::new(temp.path()).expect("service should build"));
        let state = AppState::from_config(AppStateConfig {
            service,
            convex_registry: Some(ConvexRegistry::empty()),
            system_convex_registry: None,
            application_auth_verifier: None,
            cloud_functions_registry: None,
            firebase_config: None,
            license_state: LicenseState::community(),
            runtime_service_registry: Arc::new(
                crate::service_registry::SandboxCatalogRuntimeServiceRegistry::new(Arc::new(
                    crate::sandbox::EmptySandboxCatalog,
                )),
            ),
            sandbox_service_manager: None,
            machine_lifecycle_manager: None,
            deploy_admin_token: None,
            local_server_security: None,
            listen_addr: None,
            server_shutdown: None,
        });

        assert!(
            state
                .current_deployment()
                .application_auth_verifier()
                .is_none()
        );
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
