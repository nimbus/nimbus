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
use nimbus_network::{
    LocalNetworkManager, NetworkCapabilitySelection, NetworkSovereigntyRequirements,
};
use nimbus_operator::{LocalServerAuditEvent, LocalServerSecurityState};
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, HostCallCancellation, RuntimeAdaptiveControllerSettings,
    RuntimeHostResourceBudget, RuntimeScalingPlanSet,
};
use nimbus_services::{RuntimeServiceRegistry, ServiceManager, TenantServiceRetirement};
use nimbus_tenant::TenantIsolationMode;
use nimbus_workloads::{NodeIdentity, WorkloadSagaStore};
use tempfile::TempDir;
use tracing::warn;

use crate::cloudflare_config::CloudflareConfig;
use crate::config::control_plane::ControlPlaneConfig;
use crate::config::deployment::DeploymentConfig;
use crate::config::node_services::NodeServicesConfig;
use crate::config::runtime::RuntimeGovernorConfig;
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::node_workloads::NodeWorkloadCoordinator;
use crate::runtime_manager::RuntimeManager;
use crate::workload_projection::WorkloadProjectionSink;
use crate::workload_provisioner::WorkloadProvisioner;
use crate::workload_saga::WorkloadTeardownRuntime;
use crate::workload_saga::restart_runtime::WorkloadRestartRuntime;
use crate::workload_saga::{
    ExactWorkloadTeardownCapabilityRealm, WorkloadDesireAdmissionGuard,
    WorkloadProvisionCapabilityRegistry, WorkloadProvisionSourceAuthority,
    WorkloadRestartCapabilityRegistry, WorkloadSagaCoordinator,
};

/// Explicit workload-lifecycle capabilities available to a compute state.
pub enum ComputeWorkloadComposition {
    /// Protocol serving without workload lifecycle authority.
    ProtocolOnly,
    /// Workload-capable serving over one network manager and durable saga store.
    Managed {
        network_manager: Arc<LocalNetworkManager>,
        local_node: NodeIdentity,
        capability_selection: Box<NetworkCapabilitySelection>,
        execution_provider_id: nimbus_workloads::WorkloadExecutionProviderId,
        sovereignty: NetworkSovereigntyRequirements,
        saga_store: Arc<dyn WorkloadSagaStore>,
        source_authority: Arc<dyn WorkloadProvisionSourceAuthority>,
        provision_capabilities: Box<WorkloadProvisionCapabilityRegistry>,
        restart_capabilities: Box<WorkloadRestartCapabilityRegistry>,
        teardown_capabilities: Option<Box<ExactWorkloadTeardownCapabilityRealm>>,
        desire_admission_guard: Option<Arc<dyn WorkloadDesireAdmissionGuard>>,
        projection_sink: Arc<dyn WorkloadProjectionSink>,
    },
}

pub struct ComputeStateConfig {
    pub engine: Arc<Engine>,
    pub workload_composition: ComputeWorkloadComposition,
    pub deployment: DeploymentConfig,
    pub control_plane: ControlPlaneConfig,
    pub node_services: NodeServicesConfig,
    pub runtime: RuntimeGovernorConfig,
}

/// Transport-free compute state shared across adapters.
pub struct ComputeState {
    pub engine: Arc<Engine>,
    pub active_deployment: Arc<ActiveDeployment>,
    network_manager: Option<Arc<LocalNetworkManager>>,
    workload_saga_coordinator: Option<Arc<WorkloadSagaCoordinator>>,
    workload_provisioner: Option<Arc<WorkloadProvisioner>>,
    workload_restart_runtime: Option<Arc<WorkloadRestartRuntime>>,
    workload_teardown_runtime: Option<Arc<WorkloadTeardownRuntime>>,
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
            workload_composition,
            deployment,
            control_plane,
            node_services,
            runtime,
        } = config;
        let (
            network_manager,
            workload_saga_coordinator,
            workload_provisioner,
            workload_restart_runtime,
            workload_teardown_runtime,
        ) = match workload_composition {
            ComputeWorkloadComposition::ProtocolOnly => {
                Self::require_protocol_only_node_services(&node_services);
                (None, None, None, None, None)
            }
            ComputeWorkloadComposition::Managed {
                network_manager,
                local_node,
                capability_selection,
                execution_provider_id,
                sovereignty,
                saga_store,
                source_authority,
                provision_capabilities,
                restart_capabilities,
                teardown_capabilities,
                desire_admission_guard,
                projection_sink,
            } => {
                assert!(
                    node_services.tenant_service_retirement().is_some(),
                    "managed workload composition requires an exact tenant-retirement owner"
                );
                let teardown_capabilities = teardown_capabilities.map(|capabilities| {
                        capabilities
                            .into_registry_for(&capability_selection, &execution_provider_id)
                            .expect(
                                "managed workload composition requires the exact configured teardown provider realm",
                            )
                    });
                let coordinator = Arc::new(match desire_admission_guard {
                    Some(guard) => {
                        WorkloadSagaCoordinator::with_desire_admission_guard(saga_store, guard)
                    }
                    None => WorkloadSagaCoordinator::new(saga_store),
                });
                let provider_reports = network_manager.capability_registry().clone();
                let provision_capabilities = Arc::new(*provision_capabilities);
                let provisioner = Arc::new(
                    WorkloadProvisioner::new(
                        local_node,
                        provider_reports.clone(),
                        *capability_selection,
                        sovereignty,
                        Arc::clone(&coordinator),
                        Arc::clone(&source_authority),
                        (*provision_capabilities).clone(),
                        projection_sink,
                    )
                    .expect(
                        "managed workload composition requires an exact provider-report selection",
                    ),
                );
                let restart_runtime = Arc::new(
                    WorkloadRestartRuntime::start(
                        Arc::clone(&coordinator),
                        Arc::clone(&source_authority),
                        provider_reports.clone(),
                        provision_capabilities,
                        Arc::new(*restart_capabilities),
                    )
                    .expect("managed workload composition requires a retained restart watch"),
                );
                let teardown_runtime = teardown_capabilities.map(|capabilities| {
                    Arc::new(WorkloadTeardownRuntime::new(
                        Arc::clone(&coordinator),
                        source_authority,
                        provider_reports,
                        Arc::new(capabilities),
                    ))
                });
                (
                    Some(network_manager),
                    Some(coordinator),
                    Some(provisioner),
                    Some(restart_runtime),
                    teardown_runtime,
                )
            }
        };
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
            network_manager,
            workload_saga_coordinator,
            workload_provisioner,
            workload_restart_runtime,
            workload_teardown_runtime,
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

    /// The one process-owned local network composition, when this compute
    /// state was built for workload-capable serving.
    ///
    /// Protocol-only routers deliberately return `None`. The returned Arc is
    /// the injected manager itself. Compute never reopens or reconstructs its
    /// store and never reselects providers; it snapshots the manager's exact
    /// immutable reports only when constructing the sole provisioner.
    pub fn network_manager(&self) -> Option<Arc<LocalNetworkManager>> {
        self.network_manager.clone()
    }

    /// The one compute-owned workload-saga coordinator, when this state was
    /// built for workload-capable serving.
    pub fn workload_saga_coordinator(&self) -> Option<Arc<WorkloadSagaCoordinator>> {
        self.workload_saga_coordinator.clone()
    }

    /// The sole product-level workload provision authority for this realm.
    pub fn workload_provisioner(&self) -> Option<Arc<WorkloadProvisioner>> {
        self.workload_provisioner.clone()
    }

    /// The sole compute-owned restart runtime, when workload lifecycle is
    /// enabled. Callers use its transport-free explicit submission seam.
    pub(crate) fn workload_restart_runtime(&self) -> Option<Arc<WorkloadRestartRuntime>> {
        self.workload_restart_runtime.clone()
    }

    /// The sole exact teardown runtime for the managed provider realm.
    /// Protocol-only or partially composed realms return `None` and native
    /// retirement fails before source, store, or provider mutation.
    pub(crate) fn workload_teardown_runtime(&self) -> Option<Arc<WorkloadTeardownRuntime>> {
        self.workload_teardown_runtime.clone()
    }

    fn require_protocol_only_node_services(node_services: &NodeServicesConfig) {
        assert!(
            node_services.service_manager().is_none()
                && node_services.tenant_service_retirement().is_none()
                && node_services.machine_lifecycle_manager().is_none()
                && node_services.node_workload_coordinator().is_none(),
            "service and machine workload lifecycle requires managed workload composition"
        );
    }

    pub fn runtime_service_registry(&self) -> Arc<dyn RuntimeServiceRegistry> {
        self.node_services.runtime_service_registry()
    }

    pub fn tenant_service_retirement(&self) -> Option<Arc<dyn TenantServiceRetirement>> {
        self.node_services.tenant_service_retirement()
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

    pub fn node_workload_coordinator(&self) -> Option<Arc<NodeWorkloadCoordinator>> {
        self.node_services.node_workload_coordinator()
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
        if let Some(retirement) = self.tenant_service_retirement() {
            retirement.retire_tenant_async(&tenant_id).await?;
        }
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

    use nimbus_node::{
        HostLifecycleBackendCapabilities, HostLifecycleBackendKind, HostLifecycleFuture,
        HostLifecycleStatus, NodeAgentAssignment, NodeAgentReconcileReport,
        NodeWorkloadReconcileCapability, NodeWorkloadReconcileOutcome,
    };
    use nimbus_runtime::{InvocationServiceBinding, InvocationServices};
    use tempfile::tempdir;

    use super::*;
    use crate::workload_saga::{
        ConfirmedWorkloadTeardownCommand, FinalIngressWithdrawalCapability,
        IngressTeardownCapabilities, NetworkAttachmentTeardownCapabilities,
        NetworkDetachmentCapability, NetworkReleaseCapability, WorkloadExecutionDrainCapability,
        WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
        WorkloadTeardownCapabilityFuture, WorkloadTeardownCapabilityRegistry,
        sandbox_execution_provider_id,
    };

    fn managed_network_manager_test_lock() -> &'static tokio::sync::Mutex<()> {
        static LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> = std::sync::OnceLock::new();
        LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
    }

    #[derive(Default)]
    struct EffectForbiddenNodeCapability {
        calls: AtomicUsize,
    }

    #[derive(Default)]
    struct EffectForbiddenWorkloadSagaStore {
        calls: AtomicUsize,
        restart_watch_calls: AtomicUsize,
    }

    #[derive(Default)]
    struct EffectForbiddenSourceAuthority {
        calls: AtomicUsize,
    }

    #[derive(Default)]
    struct EffectForbiddenProjectionSink {
        calls: AtomicUsize,
    }

    #[derive(Default)]
    struct EffectForbiddenTeardownCapability;

    #[derive(Default)]
    struct EffectForbiddenSandboxBackend;

    impl nimbus_sandbox::SandboxBackend for EffectForbiddenSandboxBackend {
        fn kind(&self) -> nimbus_sandbox::SandboxBackendKind {
            nimbus_sandbox::SandboxBackendKind::Krun
        }

        fn inspect(
            &self,
            _id: &nimbus_sandbox::SandboxId,
        ) -> nimbus_sandbox::SandboxFuture<Option<nimbus_sandbox::SandboxInspection>> {
            panic!("unmanaged definition deletion must not inspect a sandbox provider")
        }

        fn stop(&self, _id: &nimbus_sandbox::SandboxId) -> nimbus_sandbox::SandboxFuture<()> {
            panic!("unmanaged definition deletion must not stop a sandbox provider")
        }
    }

    macro_rules! impl_effect_forbidden_teardown_capability {
        ($capability:ident) => {
            impl $capability for EffectForbiddenTeardownCapability {
                fn execute<'a>(
                    &'a self,
                    _command: &'a ConfirmedWorkloadTeardownCommand,
                ) -> WorkloadTeardownCapabilityFuture<'a> {
                    Box::pin(async { panic!("effect-forbidden teardown must not execute") })
                }

                fn inspect<'a>(
                    &'a self,
                    _command: &'a ConfirmedWorkloadTeardownCommand,
                ) -> WorkloadTeardownCapabilityFuture<'a> {
                    Box::pin(async { panic!("effect-forbidden teardown must not inspect") })
                }
            }
        };
    }

    impl_effect_forbidden_teardown_capability!(FinalIngressWithdrawalCapability);
    impl_effect_forbidden_teardown_capability!(WorkloadExecutionDrainCapability);
    impl_effect_forbidden_teardown_capability!(WorkloadExecutionStopCapability);
    impl_effect_forbidden_teardown_capability!(NetworkDetachmentCapability);
    impl_effect_forbidden_teardown_capability!(NetworkReleaseCapability);

    impl crate::workload_projection::WorkloadProjectionSink for EffectForbiddenProjectionSink {
        fn project<'a>(
            &'a self,
            _projection: &'a crate::workload_projection::WorkloadObservedProjection,
        ) -> crate::workload_projection::WorkloadProjectionSinkFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                panic!("effect-forbidden projection sink must not be called")
            })
        }
    }

    impl WorkloadProvisionSourceAuthority for EffectForbiddenSourceAuthority {
        fn current_source<'a>(
            &'a self,
            _key: &'a nimbus_workloads::WorkloadSagaKey,
            _identity: &'a nimbus_workloads::WorkloadProvisionSourceIdentity,
        ) -> crate::workload_saga::WorkloadProvisionSourceFuture<'a> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                Err(crate::workload_saga::WorkloadProvisionSourceAuthorityError::Unavailable)
            })
        }
    }

    fn effect_free_provider_realm() -> (
        nimbus_network::NetworkCapabilityRegistry,
        nimbus_network::NetworkCapabilitySelection,
    ) {
        let requirements = nimbus_sandbox::sandbox_network_plan_requirements(
            nimbus_sandbox::SandboxBackendKind::Krun,
        );
        let ingress_provider =
            nimbus_network::NetworkProviderId::for_registration_key("state-fixture-ingress");
        let lifecycle = nimbus_network::NetworkLifecycleCapabilitySet::new([]);
        let attachment = nimbus_network::NetworkAttachmentProviderRegistration::new(
            requirements.required_attachment_provider_id().clone(),
            requirements.capability_requirements().attachment().clone(),
            [nimbus_network::NetworkAddressFamily::Ipv4],
            lifecycle.clone(),
            nimbus_network::NetworkSovereigntyCapabilities::new(
                nimbus_network::NetworkControlPlaneLocality::LocalOnly,
                [],
                true,
            ),
        );
        let ingress = nimbus_network::NetworkIngressProviderRegistration::new(
            ingress_provider.clone(),
            nimbus_network::NetworkEndpointCapabilitySet::new(
                [nimbus_network::NetworkAddressFamily::Ipv4],
                [nimbus_network::NetworkBindRealmKind::Host],
                [nimbus_network::NetworkExposure::Loopback],
                [nimbus_network::PortProtocol::Tcp],
                [nimbus_network::NetworkPortAssignmentMode::ProviderAssigned],
            ),
            nimbus_network::NetworkIngressCapabilitySet::new([]),
            nimbus_network::NetworkForwardingCapabilitySet::new([]),
            lifecycle,
            nimbus_network::NetworkSovereigntyCapabilities::new(
                nimbus_network::NetworkControlPlaneLocality::LocalOnly,
                [],
                true,
            ),
        );
        let selection = nimbus_network::NetworkCapabilitySelection::new(
            requirements.required_attachment_provider_id().clone(),
            ingress_provider,
        );
        (
            nimbus_network::NetworkCapabilityRegistry::new([
                nimbus_network::NetworkCapabilityBundle::new(attachment, ingress),
            ])
            .expect("effect-free provider reports should validate"),
            selection,
        )
    }

    fn effect_forbidden_exact_teardown_realm(
        selection: &NetworkCapabilitySelection,
    ) -> ExactWorkloadTeardownCapabilityRealm {
        let capability = Arc::new(EffectForbiddenTeardownCapability);
        let execution_provider_id =
            sandbox_execution_provider_id(nimbus_sandbox::SandboxBackendKind::Krun);
        let registry = WorkloadTeardownCapabilityRegistry::new(
            [NetworkAttachmentTeardownCapabilities::new(
                selection.attachment_provider_id().clone(),
                capability.clone(),
                capability.clone(),
            )],
            [WorkloadExecutionTeardownCapabilities::new(
                execution_provider_id.clone(),
                capability.clone(),
                capability.clone(),
            )],
            [IngressTeardownCapabilities::new(
                selection.ingress_provider_id().clone(),
                capability,
            )],
        )
        .expect("effect-forbidden teardown registry should validate");
        ExactWorkloadTeardownCapabilityRealm::new(registry, selection, &execution_provider_id)
            .expect("effect-forbidden exact teardown realm should validate")
    }

    impl WorkloadSagaStore for EffectForbiddenWorkloadSagaStore {
        fn load<'a>(
            &'a self,
            _key: &'a nimbus_workloads::WorkloadSagaKey,
        ) -> nimbus_workloads::WorkloadSagaFuture<'a, Option<nimbus_workloads::WorkloadSagaRecord>>
        {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                Err(nimbus_workloads::WorkloadSagaStoreError::Unavailable)
            })
        }

        fn compare_and_swap<'a>(
            &'a self,
            _expected: nimbus_workloads::WorkloadSagaExpected,
            _next: nimbus_workloads::WorkloadSagaRecord,
        ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadSagaCommit>
        {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                Err(nimbus_workloads::WorkloadSagaStoreError::Unavailable)
            })
        }

        fn list_recoverable<'a>(
            &'a self,
            _request: nimbus_workloads::WorkloadSagaPageRequest,
        ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadSagaPage> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                Err(nimbus_workloads::WorkloadSagaStoreError::Unavailable)
            })
        }

        fn list_restart_candidates<'a>(
            &'a self,
            request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
        ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
        {
            Box::pin(async move {
                self.restart_watch_calls.fetch_add(1, Ordering::AcqRel);
                nimbus_workloads::WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
            })
        }

        fn list_for_tenant<'a>(
            &'a self,
            _tenant_id: &'a nimbus_core::TenantId,
            _request: nimbus_workloads::WorkloadSagaTenantPageRequest,
        ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadSagaTenantPage>
        {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                Err(nimbus_workloads::WorkloadSagaStoreError::Unavailable)
            })
        }
    }

    impl NodeWorkloadReconcileCapability for EffectForbiddenNodeCapability {
        fn backend_capabilities(&self) -> Vec<HostLifecycleBackendCapabilities> {
            vec![HostLifecycleBackendCapabilities::new(
                HostLifecycleBackendKind::DirectProcess,
                true,
            )]
        }

        fn reconcile_assignment<'a>(
            &'a self,
            _assignment: NodeAgentAssignment,
        ) -> HostLifecycleFuture<'a, NodeWorkloadReconcileOutcome> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                Err(nimbus_core::Error::Internal(
                    "node reconcile effect must not run".to_owned(),
                ))
            })
        }

        fn reconcile_assignments<'a>(
            &'a self,
            _assignments: Vec<NodeAgentAssignment>,
        ) -> HostLifecycleFuture<'a, NodeAgentReconcileReport> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                Err(nimbus_core::Error::Internal(
                    "node reconcile effect must not run".to_owned(),
                ))
            })
        }

        fn inspect_assignment<'a>(
            &'a self,
            _assignment: &'a NodeAgentAssignment,
        ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::AcqRel);
                Err(nimbus_core::Error::Internal(
                    "node inspect effect must not run".to_owned(),
                ))
            })
        }
    }

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
            workload_composition: ComputeWorkloadComposition::ProtocolOnly,
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
    }

    impl TenantServiceRetirement for FailFirstTenantTeardown {
        fn retire_tenant_async<'a>(
            &'a self,
            _tenant_id: &'a nimbus_core::TenantId,
        ) -> nimbus_services::TenantServiceRetirementFuture<'a> {
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
        let _network_manager_guard = managed_network_manager_test_lock().lock().await;
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let teardown = Arc::new(FailFirstTenantTeardown {
            attempts: AtomicUsize::new(0),
        });
        let (provider_reports, capability_selection) = effect_free_provider_realm();
        let state = ComputeState::from_config(ComputeStateConfig {
            engine,
            workload_composition: ComputeWorkloadComposition::Managed {
                network_manager: LocalNetworkManager::open(
                    temp.path().join("network"),
                    provider_reports,
                )
                .expect("network manager should build"),
                local_node: crate::embedded_local_node_identity(),
                capability_selection: Box::new(capability_selection),
                execution_provider_id: sandbox_execution_provider_id(
                    nimbus_sandbox::SandboxBackendKind::Krun,
                ),
                sovereignty: nimbus_network::NetworkSovereigntyRequirements::new(
                    nimbus_network::NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
                saga_store: Arc::new(EffectForbiddenWorkloadSagaStore::default()),
                source_authority: Arc::new(EffectForbiddenSourceAuthority::default()),
                provision_capabilities: Box::new(
                    WorkloadProvisionCapabilityRegistry::new([], [], [])
                        .expect("empty provision registry should validate"),
                ),
                restart_capabilities: Box::new(
                    WorkloadRestartCapabilityRegistry::new([])
                        .expect("empty restart registry should validate"),
                ),
                teardown_capabilities: None,
                desire_admission_guard: None,
                projection_sink: Arc::new(EffectForbiddenProjectionSink::default()),
            },
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::from_runtime_service_registry_and_retirement(
                teardown.clone(),
                teardown.clone(),
            ),
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
            workload_composition: ComputeWorkloadComposition::ProtocolOnly,
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
            workload_composition: ComputeWorkloadComposition::ProtocolOnly,
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

    #[tokio::test]
    async fn compute_state_retains_the_exact_managed_workload_composition() {
        let _network_manager_guard = managed_network_manager_test_lock().lock().await;
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let (provider_reports, capability_selection) = effect_free_provider_realm();
        let manager =
            LocalNetworkManager::open(temp.path().join("network"), provider_reports.clone())
                .expect("network manager should build");
        assert!(
            !manager.authority_path().exists(),
            "an empty manager should not materialize durable authority"
        );
        let capability = Arc::new(EffectForbiddenNodeCapability::default());
        let coordinator = Arc::new(NodeWorkloadCoordinator::new(capability.clone()));
        let saga_store = Arc::new(EffectForbiddenWorkloadSagaStore::default());
        let source_authority = Arc::new(EffectForbiddenSourceAuthority::default());
        let projection_sink = Arc::new(EffectForbiddenProjectionSink::default());
        let teardown_capabilities = effect_forbidden_exact_teardown_realm(&capability_selection);
        let retirement = Arc::new(FailFirstTenantTeardown {
            attempts: AtomicUsize::new(0),
        });
        let state = ComputeState::from_config(ComputeStateConfig {
            engine,
            workload_composition: ComputeWorkloadComposition::Managed {
                network_manager: Arc::clone(&manager),
                local_node: crate::embedded_local_node_identity(),
                capability_selection: Box::new(capability_selection),
                execution_provider_id: sandbox_execution_provider_id(
                    nimbus_sandbox::SandboxBackendKind::Krun,
                ),
                sovereignty: nimbus_network::NetworkSovereigntyRequirements::new(
                    nimbus_network::NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
                saga_store: saga_store.clone(),
                source_authority: source_authority.clone(),
                provision_capabilities: Box::new(
                    WorkloadProvisionCapabilityRegistry::new([], [], [])
                        .expect("empty provision registry should fail closed"),
                ),
                restart_capabilities: Box::new(
                    WorkloadRestartCapabilityRegistry::new([])
                        .expect("empty restart registry should fail closed"),
                ),
                teardown_capabilities: Some(Box::new(teardown_capabilities)),
                desire_admission_guard: None,
                projection_sink: projection_sink.clone(),
            },
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::from_runtime_service_registry_and_retirement(
                empty_node_services().runtime_service_registry(),
                retirement,
            )
            .with_node_workload_coordinator(Arc::clone(&coordinator)),
            runtime: RuntimeGovernorConfig::default(),
        });

        let first = state
            .network_manager()
            .expect("managed compute state should expose its manager");
        let second = state
            .network_manager()
            .expect("repeated access should expose its manager");
        assert!(Arc::ptr_eq(&first, &manager));
        assert!(Arc::ptr_eq(&second, &manager));
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.capability_registry().selections().count(), 1);
        let injected_coordinator = state
            .node_workload_coordinator()
            .expect("managed compute state should expose its node coordinator");
        assert!(Arc::ptr_eq(&injected_coordinator, &coordinator));
        let first_saga_coordinator = state
            .workload_saga_coordinator()
            .expect("managed compute state should expose its saga coordinator");
        let second_saga_coordinator = state
            .workload_saga_coordinator()
            .expect("managed compute state should retain one saga coordinator");
        assert!(Arc::ptr_eq(
            &first_saga_coordinator,
            &second_saga_coordinator
        ));
        let first_provisioner = state
            .workload_provisioner()
            .expect("managed compute state should expose its provisioner");
        let second_provisioner = state
            .workload_provisioner()
            .expect("managed compute state should retain one provisioner");
        assert!(Arc::ptr_eq(&first_provisioner, &second_provisioner));
        assert_eq!(
            first_provisioner.local_node(),
            &crate::embedded_local_node_identity()
        );
        assert_eq!(first_provisioner.provider_reports().selections().count(), 1);
        let first_teardown_runtime = state
            .workload_teardown_runtime()
            .expect("managed exact composition should retain its teardown runtime");
        let second_teardown_runtime = state
            .workload_teardown_runtime()
            .expect("repeated access should reuse the sole teardown runtime");
        assert!(Arc::ptr_eq(
            &first_teardown_runtime,
            &second_teardown_runtime
        ));
        let saga_key = nimbus_workloads::WorkloadSagaKey::new(
            nimbus_core::TenantId::new("managed-composition").expect("fixture tenant is valid"),
            nimbus_core::WorkloadId::new("managed-composition").expect("fixture workload is valid"),
        );
        assert_eq!(
            first_saga_coordinator.load(&saga_key).await,
            Err(nimbus_workloads::WorkloadSagaStoreError::Unavailable)
        );
        assert_eq!(saga_store.calls.load(Ordering::Acquire), 1);
        assert_eq!(source_authority.calls.load(Ordering::Acquire), 0);
        assert_eq!(projection_sink.calls.load(Ordering::Acquire), 0);
        assert_eq!(capability.calls.load(Ordering::Acquire), 0);
        assert!(
            !manager.authority_path().exists(),
            "read-only manager access must not materialize durable authority"
        );
    }

    #[tokio::test]
    async fn compute_state_rejects_crossed_teardown_realm_before_authority_or_effects() {
        let _network_manager_guard = managed_network_manager_test_lock().lock().await;
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let (provider_reports, capability_selection) = effect_free_provider_realm();
        let manager = LocalNetworkManager::open(temp.path().join("network"), provider_reports)
            .expect("network manager should build");
        let saga_store = Arc::new(EffectForbiddenWorkloadSagaStore::default());
        let source_authority = Arc::new(EffectForbiddenSourceAuthority::default());
        let projection_sink = Arc::new(EffectForbiddenProjectionSink::default());
        let teardown_capabilities = effect_forbidden_exact_teardown_realm(&capability_selection);
        let retirement = Arc::new(FailFirstTenantTeardown {
            attempts: AtomicUsize::new(0),
        });

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ComputeState::from_config(ComputeStateConfig {
                engine,
                workload_composition: ComputeWorkloadComposition::Managed {
                    network_manager: Arc::clone(&manager),
                    local_node: crate::embedded_local_node_identity(),
                    capability_selection: Box::new(capability_selection),
                    execution_provider_id: sandbox_execution_provider_id(
                        nimbus_sandbox::SandboxBackendKind::Container,
                    ),
                    sovereignty: nimbus_network::NetworkSovereigntyRequirements::new(
                        nimbus_network::NetworkControlPlaneLocality::LocalOnly,
                        [],
                        true,
                    ),
                    saga_store: saga_store.clone(),
                    source_authority: source_authority.clone(),
                    provision_capabilities: Box::new(
                        WorkloadProvisionCapabilityRegistry::new([], [], [])
                            .expect("empty provision registry should validate"),
                    ),
                    restart_capabilities: Box::new(
                        WorkloadRestartCapabilityRegistry::new([])
                            .expect("empty restart registry should validate"),
                    ),
                    teardown_capabilities: Some(Box::new(teardown_capabilities)),
                    desire_admission_guard: None,
                    projection_sink: projection_sink.clone(),
                },
                deployment: DeploymentConfig::default(),
                control_plane: ControlPlaneConfig::router_options_default(),
                node_services: NodeServicesConfig::from_runtime_service_registry_and_retirement(
                    empty_node_services().runtime_service_registry(),
                    retirement,
                ),
                runtime: RuntimeGovernorConfig::default(),
            })
        }));

        assert!(
            result.is_err(),
            "crossed teardown realm must fail at composition"
        );
        assert_eq!(saga_store.calls.load(Ordering::Acquire), 0);
        assert_eq!(saga_store.restart_watch_calls.load(Ordering::Acquire), 0);
        assert_eq!(source_authority.calls.load(Ordering::Acquire), 0);
        assert_eq!(projection_sink.calls.load(Ordering::Acquire), 0);
        assert!(!manager.authority_path().exists());
    }

    #[tokio::test]
    async fn unmanaged_definition_deletion_does_not_require_teardown_composition() {
        let _network_manager_guard = managed_network_manager_test_lock().lock().await;
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let services = Arc::new(nimbus_services::ServiceManager::new(
            Arc::new(nimbus_services::EmptyServiceDefinitionCatalog),
            Arc::new(EffectForbiddenSandboxBackend),
        ));
        let tenant_id =
            nimbus_core::TenantId::new("unmanaged-delete").expect("fixture tenant should validate");
        let built_in = services
            .create_service_definition(
                &tenant_id,
                "built-in",
                nimbus_services::ServiceBackend::built_in("browser"),
                std::collections::BTreeMap::new(),
            )
            .expect("built-in definition should create");
        let external = services
            .create_service_definition(
                &tenant_id,
                "external",
                nimbus_services::ServiceBackend::external(
                    "https://example.invalid",
                    nimbus_services::ExternalAuthPolicy::None,
                    nimbus_services::HealthCheckPolicy::Http {
                        path: "/health".to_owned(),
                    },
                ),
                std::collections::BTreeMap::new(),
            )
            .expect("external definition should create");
        let (provider_reports, capability_selection) = effect_free_provider_realm();
        let state = ComputeState::from_config(ComputeStateConfig {
            engine,
            workload_composition: ComputeWorkloadComposition::Managed {
                network_manager: LocalNetworkManager::open(
                    temp.path().join("network"),
                    provider_reports,
                )
                .expect("network manager should build"),
                local_node: crate::embedded_local_node_identity(),
                capability_selection: Box::new(capability_selection),
                execution_provider_id: sandbox_execution_provider_id(
                    nimbus_sandbox::SandboxBackendKind::Krun,
                ),
                sovereignty: nimbus_network::NetworkSovereigntyRequirements::new(
                    nimbus_network::NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
                saga_store: Arc::new(EffectForbiddenWorkloadSagaStore::default()),
                source_authority: Arc::new(EffectForbiddenSourceAuthority::default()),
                provision_capabilities: Box::new(
                    WorkloadProvisionCapabilityRegistry::new([], [], [])
                        .expect("empty provision registry should validate"),
                ),
                restart_capabilities: Box::new(
                    WorkloadRestartCapabilityRegistry::new([])
                        .expect("empty restart registry should validate"),
                ),
                teardown_capabilities: None,
                desire_admission_guard: None,
                projection_sink: Arc::new(EffectForbiddenProjectionSink::default()),
            },
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::default().with_service_manager(services.clone()),
            runtime: RuntimeGovernorConfig::default(),
        });
        let context = nimbus_tenant::TenantIsolationContext::system(
            tenant_id.clone(),
            "service.definition.delete",
        );

        crate::services::delete_service_definition(
            &state,
            &context,
            "built-in",
            built_in.generation,
            false,
        )
        .await
        .expect("built-in deletion should use only services authority");
        crate::services::delete_service_definition(
            &state,
            &context,
            "external",
            external.generation,
            false,
        )
        .await
        .expect("external deletion should use only services authority");

        assert!(
            services
                .service_definition_for_tenant(&tenant_id, "built-in")
                .is_none()
        );
        assert!(
            services
                .service_definition_for_tenant(&tenant_id, "external")
                .is_none()
        );
        assert!(state.resource_retirer().is_err());
    }

    #[test]
    fn protocol_only_compute_rejects_a_node_coordinator_before_capability_use() {
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let capability = Arc::new(EffectForbiddenNodeCapability::default());
        let coordinator = Arc::new(NodeWorkloadCoordinator::new(capability.clone()));

        let rejected = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ComputeState::from_config(ComputeStateConfig {
                engine,
                workload_composition: ComputeWorkloadComposition::ProtocolOnly,
                deployment: DeploymentConfig::default(),
                control_plane: ControlPlaneConfig::router_options_default(),
                node_services: empty_node_services().with_node_workload_coordinator(coordinator),
                runtime: RuntimeGovernorConfig::default(),
            })
        }));

        assert!(rejected.is_err());
        assert_eq!(capability.calls.load(Ordering::Acquire), 0);
    }

    #[test]
    fn protocol_only_compute_reports_no_node_workload_coordinator() {
        let temp = tempdir().expect("service tempdir should build");
        let engine = Arc::new(Engine::new(temp.path()).expect("engine should build"));
        let state = ComputeState::from_config(ComputeStateConfig {
            engine,
            workload_composition: ComputeWorkloadComposition::ProtocolOnly,
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: empty_node_services(),
            runtime: RuntimeGovernorConfig::default(),
        });

        assert!(state.node_workload_coordinator().is_none());
        assert!(state.network_manager().is_none());
        assert!(state.workload_saga_coordinator().is_none());
        assert!(state.workload_provisioner().is_none());
        assert!(state.resource_provisioner().is_err());
    }
}
