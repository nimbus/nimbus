use std::sync::{Arc, RwLock};

use axum::response::{IntoResponse, Response};
use nimbus_core::{Error, InvocationAuth};
use nimbus_engine::Engine;
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, HostCallCancellation, RuntimeAdaptiveControllerSettings,
    RuntimeHostResourceBudget, RuntimeScalingPlanSet,
};
use tracing::warn;

use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::cloudflare::CloudflareConfig;
use crate::adapters::convex::{ConvexRegistry, ConvexTenancyConfig};
use crate::adapters::firebase::FirebaseConfig;
use crate::config::control_plane::ControlPlaneConfig;
use crate::config::deployment::DeploymentConfig;
use crate::config::node_services::NodeServicesConfig;
use crate::config::runtime::RuntimeGovernorConfig;
use crate::config::transport::TransportConfig;
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
    pub(crate) deployment: DeploymentConfig,
    pub(crate) control_plane: ControlPlaneConfig,
    pub(crate) node_services: NodeServicesConfig,
    pub(crate) transport: TransportConfig,
    pub(crate) runtime: RuntimeGovernorConfig,
}

/// Shared application state.
pub(crate) struct AppState {
    pub(crate) engine: Arc<Engine>,
    pub(crate) active_deployment: Arc<ActiveDeployment>,
    system_convex_registry: Option<Arc<ConvexRegistry>>,
    control_plane: ControlPlaneConfig,
    node_services: NodeServicesConfig,
    transport: TransportConfig,
    runtime: RuntimeGovernorConfig,
}

impl AppState {
    pub(crate) fn from_config(config: AppStateConfig) -> Self {
        let AppStateConfig {
            engine,
            deployment,
            control_plane,
            node_services,
            transport,
            runtime,
        } = config;
        let node_services = node_services.resolve(engine.clone());
        let DeploymentConfig {
            convex_registry,
            system_convex_registry,
            application_auth_verifier,
            cloud_functions_registry,
            cloudflare_config,
            firebase_config,
            convex_tenancy,
        } = deployment;
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
            convex_tenancy: convex_tenancy.map(Arc::new),
        };
        Self {
            engine,
            active_deployment: Arc::new(ActiveDeployment::new(active_deployment)),
            system_convex_registry,
            control_plane,
            node_services,
            transport,
            runtime,
        }
    }

    pub(crate) fn current_deployment(&self) -> Arc<DeploymentState> {
        self.active_deployment.current()
    }

    pub(crate) fn runtime_service_registry(&self) -> Arc<dyn RuntimeServiceRegistry> {
        self.node_services.runtime_service_registry()
    }

    pub(crate) fn runtime_host_resource_budget(&self) -> RuntimeHostResourceBudget {
        self.runtime.runtime_host_resource_budget()
    }

    pub(crate) fn runtime_adaptive_controller_settings(&self) -> RuntimeAdaptiveControllerSettings {
        self.runtime.runtime_adaptive_controller_settings()
    }

    pub(crate) fn effective_runtime_scaling_plan(&self) -> &EffectiveRuntimeScalingPlan {
        self.runtime.effective_runtime_scaling_plan()
    }

    pub(crate) fn effective_runtime_scaling_plans(&self) -> &RuntimeScalingPlanSet {
        self.runtime.effective_runtime_scaling_plans()
    }

    pub(crate) fn service_manager(&self) -> Option<Arc<ServiceManager>> {
        self.node_services.service_manager()
    }

    pub(crate) fn machine_lifecycle_manager(&self) -> Option<Arc<dyn MachineLifecycleManager>> {
        self.node_services.machine_lifecycle_manager()
    }

    pub(crate) fn tenant_isolation_mode(&self) -> TenantIsolationMode {
        self.node_services.tenant_isolation_mode()
    }

    pub(crate) fn system_convex_registry(&self) -> Option<Arc<ConvexRegistry>> {
        self.system_convex_registry.clone()
    }

    pub(crate) fn license_state(&self) -> &LicenseState {
        self.control_plane.license_state()
    }

    pub(crate) fn deploy_admin_token(&self) -> Option<&str> {
        self.control_plane.deploy_admin_token()
    }

    pub(crate) fn local_server_security(&self) -> Option<Arc<LocalServerSecurityState>> {
        self.control_plane.local_server_security()
    }

    pub(crate) fn listen_addr(&self) -> Option<std::net::SocketAddr> {
        self.transport.listen_addr()
    }

    pub(crate) fn version_check(&self) -> Arc<VersionCheck> {
        self.transport.version_check()
    }

    pub(crate) fn request_server_shutdown(&self) -> std::result::Result<(), AppError> {
        let sender = self.transport.server_shutdown().ok_or_else(|| {
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
                self.tenant_isolation_mode(),
                Arc::new(crate::adapters::cloud_functions::ServerCloudFunctionsRuntimeInvoker),
            ),
        ))?;
        Ok(())
    }

    pub(crate) fn record_local_server_audit(&self, event: LocalServerAuditEvent) {
        let Some(local_server_security) = self.local_server_security() else {
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
    pub(crate) convex_tenancy: Option<Arc<ConvexTenancyConfig>>,
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

    pub(crate) fn convex_tenancy(&self) -> Option<Arc<ConvexTenancyConfig>> {
        self.convex_tenancy.clone()
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
            convex_tenancy: None,
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
            convex_tenancy: previous.convex_tenancy(),
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
            deployment: DeploymentConfig::default().with_convex(ConvexRegistry::empty()),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: empty_node_services()
                .with_tenant_isolation_mode(TenantIsolationMode::LocalDevelopment),
            transport: TransportConfig::default().with_version_check(test_version_check()),
            runtime: RuntimeGovernorConfig::default()
                .with_runtime_host_resource_budget(
                    RuntimeHostResourceBudget::conservative_for_logical_cpus(
                        std::num::NonZeroUsize::new(4).expect("fixture CPU count is nonzero"),
                    ),
                )
                .with_effective_runtime_scaling_plans(RuntimeScalingPlanSet::single(
                    EffectiveRuntimeScalingPlan::baked_standard("__default__", 4),
                )),
        });

        assert!(
            state
                .current_deployment()
                .application_auth_verifier()
                .is_none()
        );
        assert_eq!(state.runtime_host_resource_budget().host_millicpus, 4000);
    }

    fn empty_node_services() -> NodeServicesConfig {
        NodeServicesConfig::from_runtime_service_registry(Arc::new(
            nimbus_services::ServiceInstanceBindingRegistry::new(Arc::new(
                nimbus_services::EmptyServiceInstanceCatalog,
            )),
        ))
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
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: empty_node_services()
                .with_tenant_isolation_mode(TenantIsolationMode::Production),
            transport: TransportConfig::default().with_version_check(test_version_check()),
            runtime: RuntimeGovernorConfig::default()
                .with_runtime_host_resource_budget(runtime_host_resource_budget)
                .with_runtime_adaptive_controller_settings(
                    RuntimeAdaptiveControllerSettings::shadow(
                        nimbus_runtime::RuntimeControllerReplayConfig::default(),
                    ),
                )
                .with_effective_runtime_scaling_plans(RuntimeScalingPlanSet::single(
                    EffectiveRuntimeScalingPlan::baked_standard("messages:send", 6),
                )),
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
