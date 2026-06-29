use std::net::SocketAddr;
use std::sync::{Arc, RwLock};

use axum::response::{IntoResponse, Response};
use nimbus_core::Error;
use nimbus_engine::Engine;
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, HostCallCancellation, InvocationAuth,
    RuntimeAdaptiveControllerSettings, RuntimeHostResourceBudget, RuntimeScalingPlanSet,
};
use tokio::sync::watch;
use tracing::warn;

use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::cloudflare::CloudflareConfig;
use crate::adapters::convex::ConvexRegistry;
use crate::adapters::firebase::FirebaseConfig;
use crate::error_envelope::StructuredHttpError;
use crate::license::LicenseState;
use crate::local_server::{
    LocalServerAuditEvent, LocalServerPolicyError, LocalServerSecurityState,
};
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::system::VersionCheck;
use crate::tenant::TenantIsolationMode;
use nimbus_auth::ApplicationAuthVerifier;
use nimbus_services::RuntimeServiceRegistry;
use nimbus_services::ServiceManager;

pub(crate) struct AppStateConfig {
    pub(crate) engine: Arc<Engine>,
    pub(crate) convex_registry: Option<ConvexRegistry>,
    pub(crate) system_convex_registry: Option<ConvexRegistry>,
    pub(crate) application_auth_verifier: Option<Arc<dyn ApplicationAuthVerifier>>,
    pub(crate) cloud_functions_registry: Option<CloudFunctionsRegistry>,
    pub(crate) cloudflare_config: Option<CloudflareConfig>,
    pub(crate) firebase_config: Option<FirebaseConfig>,
    pub(crate) license_state: LicenseState,
    pub(crate) runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    pub(crate) service_manager: Option<Arc<ServiceManager>>,
    pub(crate) machine_lifecycle_manager: Option<Arc<dyn MachineLifecycleManager>>,
    pub(crate) deploy_admin_token: Option<String>,
    pub(crate) local_server_security: Option<Arc<LocalServerSecurityState>>,
    pub(crate) tenant_isolation_mode: TenantIsolationMode,
    pub(crate) listen_addr: Option<SocketAddr>,
    pub(crate) server_shutdown: Option<watch::Sender<bool>>,
    pub(crate) version_check: Arc<VersionCheck>,
    pub(crate) runtime_host_resource_budget: RuntimeHostResourceBudget,
    pub(crate) runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings,
    pub(crate) effective_runtime_scaling_plans: RuntimeScalingPlanSet,
}

/// Shared application state.
pub(crate) struct AppState {
    pub(crate) engine: Arc<Engine>,
    pub(crate) active_deployment: Arc<ActiveDeployment>,
    system_convex_registry: Option<Arc<ConvexRegistry>>,
    pub(crate) license_state: Arc<LicenseState>,
    pub(crate) runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    service_manager: Option<Arc<ServiceManager>>,
    machine_lifecycle_manager: Option<Arc<dyn MachineLifecycleManager>>,
    pub(crate) deploy_admin_token: Option<String>,
    pub(crate) local_server_security: Option<Arc<LocalServerSecurityState>>,
    pub(crate) tenant_isolation_mode: TenantIsolationMode,
    pub(crate) listen_addr: Option<SocketAddr>,
    server_shutdown: Option<watch::Sender<bool>>,
    pub(crate) version_check: Arc<VersionCheck>,
    runtime_host_resource_budget: RuntimeHostResourceBudget,
    runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings,
    effective_runtime_scaling_plans: RuntimeScalingPlanSet,
}

impl AppState {
    pub(crate) fn from_config(config: AppStateConfig) -> Self {
        let AppStateConfig {
            engine,
            convex_registry,
            system_convex_registry,
            application_auth_verifier,
            cloud_functions_registry,
            cloudflare_config,
            firebase_config,
            license_state,
            runtime_service_registry,
            service_manager,
            machine_lifecycle_manager,
            deploy_admin_token,
            local_server_security,
            tenant_isolation_mode,
            listen_addr,
            server_shutdown,
            version_check,
            runtime_host_resource_budget,
            runtime_adaptive_controller_settings,
            effective_runtime_scaling_plans,
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
            cloudflare_config: cloudflare_config.map(Arc::new),
            firebase_config: firebase_config.map(Arc::new),
        };
        Self {
            engine,
            active_deployment: Arc::new(ActiveDeployment::new(active_deployment)),
            system_convex_registry,
            license_state: Arc::new(license_state),
            runtime_service_registry,
            service_manager,
            machine_lifecycle_manager,
            deploy_admin_token,
            local_server_security,
            tenant_isolation_mode,
            listen_addr,
            server_shutdown,
            version_check,
            runtime_host_resource_budget,
            runtime_adaptive_controller_settings,
            effective_runtime_scaling_plans,
        }
    }

    pub(crate) fn current_deployment(&self) -> Arc<DeploymentState> {
        self.active_deployment.current()
    }

    pub(crate) fn runtime_service_registry(&self) -> Arc<dyn RuntimeServiceRegistry> {
        self.runtime_service_registry.clone()
    }

    pub(crate) fn runtime_host_resource_budget(&self) -> RuntimeHostResourceBudget {
        self.runtime_host_resource_budget
    }

    pub(crate) fn runtime_adaptive_controller_settings(&self) -> RuntimeAdaptiveControllerSettings {
        self.runtime_adaptive_controller_settings
    }

    pub(crate) fn effective_runtime_scaling_plan(&self) -> &EffectiveRuntimeScalingPlan {
        self.effective_runtime_scaling_plans.default_plan()
    }

    pub(crate) fn effective_runtime_scaling_plans(&self) -> &RuntimeScalingPlanSet {
        &self.effective_runtime_scaling_plans
    }

    pub(crate) fn service_manager(&self) -> Option<Arc<ServiceManager>> {
        self.service_manager.clone()
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
        let deployment_generation = self.current_deployment().generation;
        self.engine
            .install_trigger_registrations(registry.trigger_registrations()?)?;
        self.engine.install_trigger_invocation_executor(Arc::new(
            crate::adapters::cloud_functions::CloudFunctionsTriggerExecutor::new(
                self.engine.clone(),
                registry,
                deployment_generation,
                self.runtime_service_registry(),
                self.tenant_isolation_mode,
                Arc::new(crate::adapters::cloud_functions::ServerCloudFunctionsRuntimeInvoker),
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
    pub(crate) cloudflare_config: Option<Arc<CloudflareConfig>>,
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

    pub(crate) fn cloudflare_config(&self) -> Option<Arc<CloudflareConfig>> {
        self.cloudflare_config.clone()
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

impl From<LocalServerPolicyError> for AppError {
    fn from(value: LocalServerPolicyError) -> Self {
        if value.is_forbidden() {
            Self::Forbidden(value.into_message())
        } else {
            Self::Unauthorized(value.into_message())
        }
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
            cloudflare_config: None,
            firebase_config: Some(Arc::new(FirebaseConfig::new())),
        });
        let previous = deployment.current();
        let previous_ptr = Arc::as_ptr(&previous);

        let replaced = deployment.activate(DeploymentState {
            generation: 2,
            convex_registry: Some(Arc::new(ConvexRegistry::empty())),
            application_auth_verifier: None,
            cloud_functions_registry: None,
            cloudflare_config: previous.cloudflare_config(),
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
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let state = AppState::from_config(AppStateConfig {
            engine,
            convex_registry: Some(ConvexRegistry::empty()),
            system_convex_registry: None,
            application_auth_verifier: None,
            cloud_functions_registry: None,
            cloudflare_config: None,
            firebase_config: None,
            license_state: LicenseState::community(),
            runtime_service_registry: Arc::new(
                nimbus_services::ServiceInstanceBindingRegistry::new(Arc::new(
                    nimbus_services::EmptyServiceInstanceCatalog,
                )),
            ),
            service_manager: None,
            machine_lifecycle_manager: None,
            deploy_admin_token: None,
            local_server_security: None,
            tenant_isolation_mode: TenantIsolationMode::LocalDevelopment,
            listen_addr: None,
            server_shutdown: None,
            version_check: test_version_check(),
            runtime_host_resource_budget: RuntimeHostResourceBudget::conservative_for_logical_cpus(
                std::num::NonZeroUsize::new(4).expect("fixture CPU count is nonzero"),
            ),
            runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings::default(),
            effective_runtime_scaling_plans: RuntimeScalingPlanSet::single(
                EffectiveRuntimeScalingPlan::baked_standard("__default__", 4),
            ),
        });

        assert!(
            state
                .current_deployment()
                .application_auth_verifier()
                .is_none()
        );
        assert_eq!(state.runtime_host_resource_budget().host_millicpus, 4000);
    }

    fn test_version_check() -> Arc<crate::system::VersionCheck> {
        let current = semver::Version::new(0, 0, 0);
        let config = crate::system::version_check::VersionCheckConfig {
            cache_path: None,
            releases_url: "http://127.0.0.1:1/".to_owned(),
            user_agent: "nimbus/test".to_owned(),
            ttl: std::time::Duration::from_secs(60),
            disabled: true,
            host_label: "test".to_owned(),
            current_exe: None,
        };
        crate::system::VersionCheck::new(current, config)
    }

    #[test]
    fn app_state_carries_runtime_host_resource_budget() {
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let runtime_host_resource_budget = RuntimeHostResourceBudget::conservative_for_logical_cpus(
            std::num::NonZeroUsize::new(6).expect("fixture CPU count is nonzero"),
        );
        let state = AppState::from_config(AppStateConfig {
            engine,
            convex_registry: None,
            system_convex_registry: None,
            application_auth_verifier: None,
            cloud_functions_registry: None,
            cloudflare_config: None,
            firebase_config: None,
            license_state: LicenseState::community(),
            runtime_service_registry: Arc::new(
                nimbus_services::ServiceInstanceBindingRegistry::new(Arc::new(
                    nimbus_services::EmptyServiceInstanceCatalog,
                )),
            ),
            service_manager: None,
            machine_lifecycle_manager: None,
            deploy_admin_token: None,
            local_server_security: None,
            tenant_isolation_mode: TenantIsolationMode::Production,
            listen_addr: None,
            server_shutdown: None,
            version_check: test_version_check(),
            runtime_host_resource_budget,
            runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings::shadow(
                nimbus_runtime::RuntimeControllerReplayConfig::default(),
            ),
            effective_runtime_scaling_plans: RuntimeScalingPlanSet::single(
                EffectiveRuntimeScalingPlan::baked_standard("messages:send", 6),
            ),
        });

        assert_eq!(
            state.runtime_host_resource_budget(),
            runtime_host_resource_budget
        );
        assert_eq!(state.runtime_host_resource_budget().host_millicpus, 6000);
        assert_eq!(
            state.runtime_adaptive_controller_settings().mode(),
            nimbus_runtime::RuntimeAdaptiveControllerMode::Shadow
        );
        assert_eq!(
            state.effective_runtime_scaling_plan().function,
            "messages:send"
        );
        assert_eq!(state.effective_runtime_scaling_plan().effective.max_warm, 6);
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

    let service = state.engine.clone();
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
