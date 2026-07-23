//! Transport-free compute state (CP2): the compute half of what used to be
//! `nimbus-server`'s `AppState`. `ComputeState` owns the engine handle, the
//! active deployment snapshot, and the control-plane/node-services/runtime
//! composition config — no HTTP framework types, no HTTP framing.
//!
//! `nimbus-server`'s `AppState` wraps a `ComputeState` plus its
//! transport-only config and `Deref`s to it, so handler code written against
//! `state.engine`/`state.some_compute_method()` keeps compiling unchanged.
//! Errors surfaced from this crate use `ComputeError`, which carries no
//! transport-framework type; `nimbus-server` bridges it into its own `AppError`
//! (the crate's HTTP-response error) at the crate seam.

use std::path::Path;
use std::sync::{Arc, RwLock};

use nimbus_auth::ApplicationAuthVerifier;
use nimbus_cloud_functions::{CloudFunctionsHttpTenantBinding, CloudFunctionsRegistry};
use nimbus_convex::{ConvexRegistry, ConvexSiloAuthRegistry, ConvexTenancyConfig};
use nimbus_engine::Engine;
use nimbus_firebase::FirebaseConfig;
use nimbus_license::LicenseState;
use nimbus_operator::{LocalServerAuditEvent, LocalServerSecurityState};
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, HostCallCancellation, RuntimeAdaptiveControllerSettings,
    RuntimeHostResourceBudget, RuntimeScalingPlanSet,
};
use nimbus_services::{RuntimeServiceRegistry, ServiceManager};
use nimbus_tenant::TenantIsolationMode;
use tempfile::TempDir;
use tracing::warn;

use crate::cloudflare_config::CloudflareConfig;
use crate::config::control_plane::ControlPlaneConfig;
use crate::config::deployment::DeploymentConfig;
use crate::config::node_services::NodeServicesConfig;
use crate::config::runtime::RuntimeGovernorConfig;
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::runtime_manager::RuntimeManager;

pub struct ComputeStateConfig {
    pub engine: Arc<Engine>,
    pub deployment: DeploymentConfig,
    pub control_plane: ControlPlaneConfig,
    pub node_services: NodeServicesConfig,
    pub runtime: RuntimeGovernorConfig,
}

/// Transport-free compute state shared across adapters.
pub struct ComputeState {
    pub engine: Arc<Engine>,
    pub active_deployment: Arc<ActiveDeployment>,
    system_convex_registry: Option<Arc<ConvexRegistry>>,
    control_plane: ControlPlaneConfig,
    node_services: NodeServicesConfig,
    runtime: RuntimeGovernorConfig,
    runtime_manager: Arc<RuntimeManager>,
}

impl ComputeState {
    pub fn from_config(config: ComputeStateConfig) -> Self {
        let ComputeStateConfig {
            engine,
            deployment,
            control_plane,
            node_services,
            runtime,
        } = config;
        let node_services = node_services.resolve(engine.clone());
        let runtime_manager = RuntimeManager::new(engine.clone(), runtime.clone());
        let DeploymentConfig {
            convex_registry,
            system_convex_registry,
            application_auth_verifier,
            convex_silo_auth,
            cloud_functions_registry,
            cloud_functions_http_tenant,
            cloudflare_config,
            firebase_config,
            convex_tenancy,
        } = deployment;
        let base_runtime_limits = runtime.base_runtime_limits().clone();
        let convex_registry = convex_registry
            .map(|registry| Arc::new(registry.with_runtime_limits(base_runtime_limits.clone())));
        let system_convex_registry = system_convex_registry
            .map(|registry| Arc::new(registry.with_runtime_limits(base_runtime_limits.clone())));
        let initial_generation =
            u64::from(convex_registry.is_some() || cloud_functions_registry.is_some());
        let active_deployment = DeploymentState {
            generation: initial_generation,
            convex_registry,
            application_auth_verifier,
            convex_silo_auth,
            cloud_functions_registry: cloud_functions_registry.map(|registry| {
                Arc::new(registry.with_runtime_limits(base_runtime_limits.clone()))
            }),
            cloud_functions_http_tenant,
            convex_artifact_lease: None,
            cloud_functions_artifact_lease: None,
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
            runtime,
            runtime_manager,
        }
    }

    pub fn current_deployment(&self) -> Arc<DeploymentState> {
        self.active_deployment.current()
    }

    pub fn runtime_service_registry(&self) -> Arc<dyn RuntimeServiceRegistry> {
        self.node_services.runtime_service_registry()
    }

    pub fn runtime_manager(&self) -> Arc<RuntimeManager> {
        self.runtime_manager.clone()
    }

    pub fn runtime_host_resource_budget(&self) -> RuntimeHostResourceBudget {
        self.runtime.runtime_host_resource_budget()
    }

    pub fn runtime_adaptive_controller_settings(&self) -> RuntimeAdaptiveControllerSettings {
        self.runtime.runtime_adaptive_controller_settings()
    }

    pub fn effective_runtime_scaling_plan(&self) -> &EffectiveRuntimeScalingPlan {
        self.runtime.effective_runtime_scaling_plan()
    }

    pub fn effective_runtime_scaling_plans(&self) -> &RuntimeScalingPlanSet {
        self.runtime.effective_runtime_scaling_plans()
    }

    pub fn service_manager(&self) -> Option<Arc<ServiceManager>> {
        self.node_services.service_manager()
    }

    pub fn machine_lifecycle_manager(&self) -> Option<Arc<dyn MachineLifecycleManager>> {
        self.node_services.machine_lifecycle_manager()
    }

    pub fn tenant_isolation_mode(&self) -> TenantIsolationMode {
        self.node_services.tenant_isolation_mode()
    }

    pub fn system_convex_registry(&self) -> Option<Arc<ConvexRegistry>> {
        self.system_convex_registry.clone()
    }

    pub fn license_state(&self) -> &LicenseState {
        self.control_plane.license_state()
    }

    pub fn deploy_admin_token(&self) -> Option<&str> {
        self.control_plane.deploy_admin_token()
    }

    pub fn local_server_security(&self) -> Option<Arc<LocalServerSecurityState>> {
        self.control_plane.local_server_security()
    }

    pub fn record_local_server_audit(&self, event: LocalServerAuditEvent) {
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

    /// Creates a tenant and applies the active adapter schema through the
    /// compute-owned lifecycle entrypoint.
    pub async fn create_tenant_async(
        &self,
        tenant_id: nimbus_core::TenantId,
    ) -> Result<(), ComputeError> {
        self.engine.create_tenant_async(tenant_id.clone()).await?;
        if let Some(registry) = self.current_deployment().convex_registry() {
            registry
                .apply_schema_to_tenant_async(&self.engine, tenant_id)
                .await?;
        }
        Ok(())
    }

    /// Retires all runtime authority before tearing down tenant resources and
    /// allowing the engine/storage delete fence to complete.
    pub async fn delete_tenant(
        &self,
        tenant_id: nimbus_core::TenantId,
    ) -> Result<(), ComputeError> {
        let deletion = self
            .engine
            .begin_tenant_delete_async(tenant_id.clone())
            .await?;
        let (owner_id, _) = self
            .runtime_manager
            .retire_tenant_deletion(&deletion)
            .await?;
        self.runtime_service_registry()
            .teardown_tenant_async(&tenant_id)
            .await?;
        self.engine.finish_tenant_delete_async(deletion).await?;
        self.runtime_manager.forget_retired_owner(&owner_id);
        Ok(())
    }
}

/// Owns a private staging directory for as long as a deployment snapshot can
/// execute artifacts loaded from it.
pub(crate) struct DeploymentArtifactLease {
    app_dir: TempDir,
}

impl DeploymentArtifactLease {
    pub(crate) fn new(app_dir: TempDir) -> Self {
        Self { app_dir }
    }

    pub(crate) fn app_dir(&self) -> &Path {
        self.app_dir.path()
    }
}

#[derive(Clone)]
pub struct DeploymentState {
    pub generation: u64,
    pub convex_registry: Option<Arc<ConvexRegistry>>,
    pub application_auth_verifier: Option<Arc<dyn ApplicationAuthVerifier>>,
    pub convex_silo_auth: ConvexSiloAuthRegistry,
    pub cloud_functions_registry: Option<Arc<CloudFunctionsRegistry>>,
    pub cloud_functions_http_tenant: Option<CloudFunctionsHttpTenantBinding>,
    pub(crate) convex_artifact_lease: Option<Arc<DeploymentArtifactLease>>,
    pub(crate) cloud_functions_artifact_lease: Option<Arc<DeploymentArtifactLease>>,
    pub cloudflare_config: Option<Arc<CloudflareConfig>>,
    pub firebase_config: Option<Arc<FirebaseConfig>>,
    pub convex_tenancy: Option<Arc<ConvexTenancyConfig>>,
}

impl DeploymentState {
    pub fn convex_registry(&self) -> Option<Arc<ConvexRegistry>> {
        self.convex_registry.clone()
    }

    pub fn application_auth_verifier(&self) -> Option<Arc<dyn ApplicationAuthVerifier>> {
        self.application_auth_verifier.clone()
    }

    pub fn convex_silo_auth(&self) -> &ConvexSiloAuthRegistry {
        &self.convex_silo_auth
    }

    pub fn cloud_functions_registry(&self) -> Option<Arc<CloudFunctionsRegistry>> {
        self.cloud_functions_registry.clone()
    }

    pub fn cloud_functions_http_tenant(&self) -> Option<CloudFunctionsHttpTenantBinding> {
        self.cloud_functions_http_tenant.clone()
    }

    pub(crate) fn convex_artifact_lease(&self) -> Option<Arc<DeploymentArtifactLease>> {
        self.convex_artifact_lease.clone()
    }

    pub(crate) fn cloud_functions_artifact_lease(&self) -> Option<Arc<DeploymentArtifactLease>> {
        self.cloud_functions_artifact_lease.clone()
    }

    pub fn cloudflare_config(&self) -> Option<Arc<CloudflareConfig>> {
        self.cloudflare_config.clone()
    }

    pub fn firebase_config(&self) -> Option<Arc<FirebaseConfig>> {
        self.firebase_config.clone()
    }

    pub fn convex_tenancy(&self) -> Option<Arc<ConvexTenancyConfig>> {
        self.convex_tenancy.clone()
    }
}

pub struct ActiveDeployment {
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

    pub fn activate(&self, deployment: DeploymentState) -> Arc<DeploymentState> {
        let mut current = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::mem::replace(&mut *current, Arc::new(deployment))
    }
}

/// Axum-free compute error. `nimbus-server` bridges this into its own
/// `AppError` (the server's HTTP-response error) via `From<ComputeError> for AppError`
/// so the rendered HTTP status/body stays whatever `AppError` already
/// produces for each case.
#[derive(Debug)]
pub enum ComputeError {
    Core(nimbus_core::Error),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
}

impl From<nimbus_core::Error> for ComputeError {
    fn from(value: nimbus_core::Error) -> Self {
        Self::Core(value)
    }
}

impl ComputeError {
    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }
}

impl std::fmt::Display for ComputeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Core(error) => write!(f, "{error}"),
            Self::Unauthorized(message) => write!(f, "{message}"),
            Self::Forbidden(message) => write!(f, "{message}"),
            Self::NotFound(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for ComputeError {}

#[derive(Debug, Default)]
pub struct RequestCancellationGuard {
    token: HostCallCancellation,
}

impl RequestCancellationGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn token(&self) -> HostCallCancellation {
        self.token.clone()
    }
}

impl Drop for RequestCancellationGuard {
    fn drop(&mut self) {
        self.token.cancel_due_to_disconnect();
    }
}

pub async fn record_authenticated_usage(
    state: &ComputeState,
    auth: Option<&nimbus_core::InvocationAuth>,
) {
    let Some(token_identifier) = auth
        .and_then(nimbus_core::InvocationAuth::token_identifier)
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use nimbus_runtime::{InvocationServiceBinding, InvocationServices};
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn active_deployment_keeps_previous_snapshot_arc_alive_after_activation() {
        let deployment = ActiveDeployment::new(DeploymentState {
            generation: 1,
            convex_registry: Some(Arc::new(ConvexRegistry::empty())),
            application_auth_verifier: None,
            convex_silo_auth: ConvexSiloAuthRegistry::new(),
            cloud_functions_registry: None,
            cloud_functions_http_tenant: None,
            convex_artifact_lease: None,
            cloud_functions_artifact_lease: None,
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
            convex_silo_auth: previous.convex_silo_auth().clone(),
            cloud_functions_registry: None,
            cloud_functions_http_tenant: None,
            convex_artifact_lease: previous.convex_artifact_lease(),
            cloud_functions_artifact_lease: previous.cloud_functions_artifact_lease(),
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
    fn retired_deployment_releases_artifacts_only_after_its_last_snapshot_drops() {
        let app_dir = tempdir().expect("artifact tempdir should build");
        let artifact_path = app_dir.path().to_path_buf();
        let artifact_lease = Arc::new(DeploymentArtifactLease::new(app_dir));
        let deployment = ActiveDeployment::new(DeploymentState {
            generation: 1,
            convex_registry: None,
            application_auth_verifier: None,
            convex_silo_auth: ConvexSiloAuthRegistry::new(),
            cloud_functions_registry: None,
            cloud_functions_http_tenant: None,
            convex_artifact_lease: None,
            cloud_functions_artifact_lease: Some(artifact_lease.clone()),
            cloudflare_config: None,
            firebase_config: None,
            convex_tenancy: None,
        });
        drop(artifact_lease);
        let previous = deployment.current();
        let replaced = deployment.activate(DeploymentState {
            generation: 2,
            convex_registry: None,
            application_auth_verifier: None,
            convex_silo_auth: ConvexSiloAuthRegistry::new(),
            cloud_functions_registry: None,
            cloud_functions_http_tenant: None,
            convex_artifact_lease: None,
            cloud_functions_artifact_lease: None,
            cloudflare_config: None,
            firebase_config: None,
            convex_tenancy: None,
        });

        assert!(artifact_path.exists());
        drop(previous);
        assert!(
            artifact_path.exists(),
            "a second in-flight snapshot must retain the retired generation"
        );
        drop(replaced);
        assert!(
            !artifact_path.exists(),
            "the private staging directory should be reclaimed with the generation"
        );
    }

    #[test]
    fn compute_state_does_not_infer_application_auth_verifier_from_convex_registry() {
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let state = ComputeState::from_config(ComputeStateConfig {
            engine,
            deployment: DeploymentConfig::default().with_convex(ConvexRegistry::empty()),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: empty_node_services()
                .with_tenant_isolation_mode(TenantIsolationMode::LocalDevelopment),
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

    struct FailFirstTenantTeardown {
        attempts: AtomicUsize,
    }

    impl RuntimeServiceRegistry for FailFirstTenantTeardown {
        fn snapshot_for_tenant(&self, _tenant_id: &nimbus_core::TenantId) -> InvocationServices {
            InvocationServices::new()
        }

        fn resolve_service_binding(
            &self,
            _tenant_id: &nimbus_core::TenantId,
            _service_name: &str,
        ) -> Result<Option<InvocationServiceBinding>, nimbus_core::Error> {
            Ok(None)
        }

        fn teardown_tenant_async<'a>(
            &'a self,
            _tenant_id: &'a nimbus_core::TenantId,
        ) -> nimbus_services::RuntimeServiceTeardownFuture<'a> {
            Box::pin(async move {
                if self.attempts.fetch_add(1, Ordering::AcqRel) == 0 {
                    Err(nimbus_core::Error::Internal(
                        "injected tenant service teardown failure".to_owned(),
                    ))
                } else {
                    Ok(())
                }
            })
        }
    }

    #[tokio::test]
    async fn partial_tenant_delete_stays_fenced_and_retries_to_completion() {
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let teardown = Arc::new(FailFirstTenantTeardown {
            attempts: AtomicUsize::new(0),
        });
        let state = ComputeState::from_config(ComputeStateConfig {
            engine,
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::from_runtime_service_registry(teardown.clone()),
            runtime: RuntimeGovernorConfig::default(),
        });
        let tenant_id =
            nimbus_core::TenantId::new("retry-delete").expect("tenant id should be valid");
        state
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create");
        let original = state
            .runtime_manager()
            .acquire_invocation_lease(tenant_id.clone(), 0)
            .await
            .expect("original invocation lease should build");
        let original_owner = original.owner_lease().owner_id().clone();
        drop(original);

        let first_error = state
            .delete_tenant(tenant_id.clone())
            .await
            .expect_err("first service teardown should fail");
        assert!(
            first_error
                .to_string()
                .contains("injected tenant service teardown failure")
        );
        assert_eq!(teardown.attempts.load(Ordering::Acquire), 1);
        let rejected = state
            .runtime_manager()
            .acquire_invocation_lease(tenant_id.clone(), 0)
            .await
            .expect_err("partially deleted tenant must remain fail-closed");
        assert!(matches!(
            rejected,
            nimbus_core::Error::TenantNotFound(ref rejected_id) if rejected_id == &tenant_id
        ));

        state
            .delete_tenant(tenant_id.clone())
            .await
            .expect("retry should finish the fenced deletion");
        assert_eq!(teardown.attempts.load(Ordering::Acquire), 2);
        state
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should recreate after completed deletion");
        let recreated = state
            .runtime_manager()
            .acquire_invocation_lease(tenant_id, 0)
            .await
            .expect("recreated invocation lease should build");
        assert_ne!(recreated.owner_lease().owner_id(), &original_owner);
    }

    #[test]
    fn compute_state_carries_runtime_host_resource_budget() {
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let runtime_host_resource_budget = RuntimeHostResourceBudget::conservative_for_logical_cpus(
            std::num::NonZeroUsize::new(6).expect("fixture CPU count is nonzero"),
        );
        let state = ComputeState::from_config(ComputeStateConfig {
            engine,
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: empty_node_services()
                .with_tenant_isolation_mode(TenantIsolationMode::Production),
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

    #[test]
    fn compute_runtime_limits_override_adapter_registry_defaults_at_startup() {
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let adapter_limits = nimbus_runtime::RuntimeLimits {
            max_concurrent_runtime_instances: 2,
            ..nimbus_runtime::RuntimeLimits::default()
        };
        let canonical_limits = nimbus_runtime::RuntimeLimits {
            max_concurrent_runtime_instances: 7,
            ..nimbus_runtime::RuntimeLimits::default()
        };
        let state = ComputeState::from_config(ComputeStateConfig {
            engine,
            deployment: DeploymentConfig::default()
                .with_convex(ConvexRegistry::empty().with_runtime_limits(adapter_limits.clone()))
                .with_system_convex_registry(
                    ConvexRegistry::empty().with_runtime_limits(adapter_limits),
                ),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: empty_node_services(),
            runtime: RuntimeGovernorConfig::default()
                .with_base_runtime_limits(canonical_limits.clone()),
        });

        assert_eq!(
            state.runtime_manager().base_runtime_limits(),
            &canonical_limits
        );
        let mut expected_convex_limits = canonical_limits;
        expected_convex_limits.guest_semantics =
            nimbus_runtime::RuntimeGuestSemantics::ConvexDefault;
        assert_eq!(
            state
                .current_deployment()
                .convex_registry()
                .expect("application registry should exist")
                .runtime_limits(),
            expected_convex_limits
        );
        assert_eq!(
            state
                .system_convex_registry()
                .expect("system registry should exist")
                .runtime_limits(),
            expected_convex_limits
        );
    }
}
