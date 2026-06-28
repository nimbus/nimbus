use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::{HeaderName, HeaderValue, Method, header};
use axum::middleware;
use axum::routing::{any, delete, get, post};
use axum::{Extension, Router};
use nimbus_engine::Engine;
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, NominalRuntimeHostPressureSource,
    RuntimeAdaptiveControllerSettings, RuntimeHostPressureSource, RuntimeHostResourceBudget,
    RuntimeScalingPlanSet,
};
use tokio::sync::watch;
use tower::ServiceBuilder;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;

use crate::adapters::cloud_functions;
use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::convex::{self, ConvexRegistry};
use crate::adapters::firebase::{self, FirebaseConfig};
use crate::license::LicenseState;
use crate::local_server::{
    LocalServerAccessPolicy, LocalServerSecurityState, origin_allowlist_middleware,
    route_family_gate_middleware, server_access_extract_middleware,
};
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::state::{AppState, AppStateConfig};
use crate::system::VersionCheck;
use crate::system::version_check::VersionCheckConfig;
use crate::tenant::TenantIsolationMode;
use crate::{http, ws};
use nimbus_auth::ApplicationAuthVerifier;
use nimbus_services::{EmptyServiceInstanceCatalog, ServiceInstanceCatalog, ServiceManager};
use nimbus_services::{RuntimeServiceRegistry, ServiceInstanceBindingRegistry};

const DEMOS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demos");

enum RuntimeServiceSource {
    ServiceInstanceCatalog(Arc<dyn ServiceInstanceCatalog>),
    ServiceManager(Arc<ServiceManager>),
}

impl RuntimeServiceSource {
    fn service_manager(&self) -> Option<Arc<ServiceManager>> {
        match self {
            Self::ServiceManager(service_manager) => Some(service_manager.clone()),
            Self::ServiceInstanceCatalog(_) => None,
        }
    }

    fn into_runtime_service_registry(
        self,
        system_state_engine: Arc<Engine>,
    ) -> Arc<dyn RuntimeServiceRegistry> {
        match self {
            Self::ServiceInstanceCatalog(service_instances) => {
                Arc::new(ServiceInstanceBindingRegistry::new(service_instances))
            }
            Self::ServiceManager(service_manager) => {
                crate::service_manager::attach_system_state_engine(
                    &service_manager,
                    system_state_engine,
                );
                service_manager
            }
        }
    }
}

/// Canonical public option bundle for building a Nimbus HTTP/WebSocket router.
pub struct RouterOptions {
    engine: Arc<Engine>,
    convex_registry: Option<ConvexRegistry>,
    system_convex_registry: Option<ConvexRegistry>,
    cloud_functions_registry: Option<CloudFunctionsRegistry>,
    firebase_config: Option<FirebaseConfig>,
    license_state: LicenseState,
    service_instances: Option<Arc<dyn ServiceInstanceCatalog>>,
    service_manager: Option<Arc<ServiceManager>>,
    machine_lifecycle_manager: Option<Arc<dyn MachineLifecycleManager>>,
    deploy_admin_token: Option<String>,
    local_server_security: Option<Arc<LocalServerSecurityState>>,
    tenant_isolation_mode: TenantIsolationMode,
    cors_allowed_origins: Vec<String>,
    runtime_host_resource_budget: RuntimeHostResourceBudget,
    runtime_host_pressure_source: Arc<dyn RuntimeHostPressureSource>,
    runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings,
    effective_runtime_scaling_plans: RuntimeScalingPlanSet,
}

impl RouterOptions {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            convex_registry: None,
            system_convex_registry: None,
            cloud_functions_registry: None,
            firebase_config: None,
            license_state: LicenseState::community(),
            service_instances: None,
            service_manager: None,
            machine_lifecycle_manager: None,
            deploy_admin_token: None,
            local_server_security: None,
            tenant_isolation_mode: TenantIsolationMode::default(),
            cors_allowed_origins: Vec::new(),
            runtime_host_resource_budget: default_runtime_host_resource_budget(),
            runtime_host_pressure_source: default_runtime_host_pressure_source(),
            runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings::default(),
            effective_runtime_scaling_plans: RuntimeScalingPlanSet::default(),
        }
    }

    pub fn with_convex_registry(mut self, convex_registry: ConvexRegistry) -> Self {
        self.convex_registry = Some(convex_registry);
        self
    }

    pub fn with_system_convex_registry(mut self, system_convex_registry: ConvexRegistry) -> Self {
        self.system_convex_registry = Some(system_convex_registry);
        self
    }

    pub fn with_cloud_functions_registry(
        mut self,
        cloud_functions_registry: CloudFunctionsRegistry,
    ) -> Self {
        self.cloud_functions_registry = Some(cloud_functions_registry);
        self
    }

    pub fn with_firebase_config(mut self, firebase_config: FirebaseConfig) -> Self {
        self.firebase_config = Some(firebase_config);
        self
    }

    pub fn with_license(mut self, license_state: LicenseState) -> Self {
        self.license_state = license_state;
        self
    }

    pub fn with_service_instance_catalog(
        mut self,
        service_instances: Arc<dyn ServiceInstanceCatalog>,
    ) -> Self {
        self.service_instances = Some(service_instances);
        self.service_manager = None;
        self
    }

    pub fn with_service_manager(mut self, service_manager: Arc<ServiceManager>) -> Self {
        self.service_manager = Some(service_manager);
        self.service_instances = None;
        self
    }

    pub fn with_machine_lifecycle_manager(
        mut self,
        machine_lifecycle_manager: Arc<dyn MachineLifecycleManager>,
    ) -> Self {
        self.machine_lifecycle_manager = Some(machine_lifecycle_manager);
        self
    }

    pub fn with_deploy_admin_token(mut self, token: impl Into<String>) -> Self {
        self.deploy_admin_token = Some(token.into());
        self
    }

    pub fn with_local_server_security(
        mut self,
        local_server_security: Arc<LocalServerSecurityState>,
    ) -> Self {
        self.local_server_security = Some(local_server_security);
        self
    }

    pub fn with_tenant_isolation_mode(mut self, mode: TenantIsolationMode) -> Self {
        self.tenant_isolation_mode = mode;
        self
    }

    /// Allow additional exact browser origins through the CORS layer.
    /// Loopback origins are always allowed. Values should be normalized via
    /// [`normalize_cors_origin`]; entries that fail normalization are
    /// ignored with a warning (fail closed).
    pub fn with_cors_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.cors_allowed_origins = origins;
        self
    }

    pub fn with_runtime_host_resource_budget(mut self, budget: RuntimeHostResourceBudget) -> Self {
        self.runtime_host_resource_budget = budget;
        self
    }

    pub fn with_runtime_host_pressure_source(
        mut self,
        pressure_source: Arc<dyn RuntimeHostPressureSource>,
    ) -> Self {
        self.runtime_host_pressure_source = pressure_source;
        self
    }

    pub fn with_runtime_adaptive_controller_settings(
        mut self,
        settings: RuntimeAdaptiveControllerSettings,
    ) -> Self {
        self.runtime_adaptive_controller_settings = settings;
        self
    }

    pub fn with_effective_runtime_scaling_plan(self, plan: EffectiveRuntimeScalingPlan) -> Self {
        self.with_effective_runtime_scaling_plans(RuntimeScalingPlanSet::single(plan))
    }

    pub fn with_effective_runtime_scaling_plans(mut self, plans: RuntimeScalingPlanSet) -> Self {
        self.effective_runtime_scaling_plans = plans;
        self
    }

    pub(crate) fn engine(&self) -> Arc<Engine> {
        Arc::clone(&self.engine)
    }

    pub(crate) fn has_system_convex_registry(&self) -> bool {
        self.system_convex_registry.is_some()
    }

    pub(crate) fn into_build_config(self) -> RouterBuildConfig {
        let mut config = RouterBuildConfig::core(self.engine).with_license(self.license_state);
        if let Some(system_convex_registry) = self.system_convex_registry {
            config = config.with_system_convex_registry(system_convex_registry);
        }
        if let Some(convex_registry) = self.convex_registry {
            config = config
                .with_application_auth_verifier(convex_application_auth_verifier(&convex_registry))
                .with_convex(convex_registry);
        }
        if let Some(cloud_functions_registry) = self.cloud_functions_registry {
            config = config.with_cloud_functions(cloud_functions_registry);
        }
        if let Some(firebase_config) = self.firebase_config {
            config = config.with_firebase(firebase_config);
        }
        if let Some(deploy_admin_token) = self.deploy_admin_token {
            config = config.with_deploy_admin_token(deploy_admin_token);
        }
        if let Some(local_server_security) = self.local_server_security {
            config = config.with_local_server_security(local_server_security);
        }
        config = config.with_tenant_isolation_mode(self.tenant_isolation_mode);
        if let Some(service_manager) = self.service_manager {
            config = config.with_service_manager(service_manager);
        } else if let Some(service_instances) = self.service_instances {
            config = config.with_service_instance_catalog(service_instances);
        }
        if let Some(machine_lifecycle_manager) = self.machine_lifecycle_manager {
            config = config.with_machine_lifecycle_manager(machine_lifecycle_manager);
        }
        config = config.with_cors_allowed_origins(self.cors_allowed_origins);
        config = config.with_runtime_host_resource_budget(self.runtime_host_resource_budget);
        config = config.with_runtime_host_pressure_source(self.runtime_host_pressure_source);
        config = config
            .with_runtime_adaptive_controller_settings(self.runtime_adaptive_controller_settings);
        config = config.with_effective_runtime_scaling_plans(self.effective_runtime_scaling_plans);
        config
    }
}

pub(crate) struct RouterBuildConfig {
    engine: Arc<Engine>,
    convex_registry: Option<ConvexRegistry>,
    system_convex_registry: Option<ConvexRegistry>,
    application_auth_verifier: Option<Arc<dyn ApplicationAuthVerifier>>,
    cloud_functions_registry: Option<CloudFunctionsRegistry>,
    firebase_config: Option<FirebaseConfig>,
    license_state: LicenseState,
    runtime_service_source: RuntimeServiceSource,
    machine_lifecycle_manager: Option<Arc<dyn MachineLifecycleManager>>,
    deploy_admin_token: Option<String>,
    local_server_security: Option<Arc<LocalServerSecurityState>>,
    tenant_isolation_mode: TenantIsolationMode,
    listen_addr: Option<SocketAddr>,
    server_shutdown: Option<watch::Sender<bool>>,
    cors_allowed_origins: Vec<String>,
    runtime_host_resource_budget: RuntimeHostResourceBudget,
    runtime_host_pressure_source: Arc<dyn RuntimeHostPressureSource>,
    runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings,
    effective_runtime_scaling_plans: RuntimeScalingPlanSet,
}

impl RouterBuildConfig {
    pub(crate) fn core(engine: Arc<Engine>) -> Self {
        Self {
            engine,
            convex_registry: None,
            system_convex_registry: None,
            application_auth_verifier: None,
            cloud_functions_registry: None,
            firebase_config: None,
            license_state: LicenseState::community(),
            runtime_service_source: RuntimeServiceSource::ServiceInstanceCatalog(Arc::new(
                EmptyServiceInstanceCatalog,
            )),
            machine_lifecycle_manager: None,
            deploy_admin_token: std::env::var("NIMBUS_DEPLOY_TOKEN").ok(),
            local_server_security: None,
            tenant_isolation_mode: TenantIsolationMode::default(),
            listen_addr: None,
            server_shutdown: None,
            cors_allowed_origins: Vec::new(),
            runtime_host_resource_budget: default_runtime_host_resource_budget(),
            runtime_host_pressure_source: default_runtime_host_pressure_source(),
            runtime_adaptive_controller_settings: RuntimeAdaptiveControllerSettings::default(),
            effective_runtime_scaling_plans: RuntimeScalingPlanSet::default(),
        }
    }

    pub(crate) fn with_cors_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.cors_allowed_origins = origins;
        self
    }

    pub(crate) fn with_runtime_host_resource_budget(
        mut self,
        budget: RuntimeHostResourceBudget,
    ) -> Self {
        self.runtime_host_resource_budget = budget;
        self
    }

    pub(crate) fn with_runtime_host_pressure_source(
        mut self,
        pressure_source: Arc<dyn RuntimeHostPressureSource>,
    ) -> Self {
        self.runtime_host_pressure_source = pressure_source;
        self
    }

    pub(crate) fn with_runtime_adaptive_controller_settings(
        mut self,
        settings: RuntimeAdaptiveControllerSettings,
    ) -> Self {
        self.runtime_adaptive_controller_settings = settings;
        self
    }

    pub(crate) fn with_effective_runtime_scaling_plans(
        mut self,
        plans: RuntimeScalingPlanSet,
    ) -> Self {
        self.effective_runtime_scaling_plans = plans;
        self
    }

    pub(crate) fn with_convex(mut self, convex_registry: ConvexRegistry) -> Self {
        self.convex_registry = Some(convex_registry);
        self
    }

    pub(crate) fn with_system_convex_registry(
        mut self,
        system_convex_registry: ConvexRegistry,
    ) -> Self {
        self.system_convex_registry = Some(system_convex_registry);
        self
    }

    pub(crate) fn with_application_auth_verifier(
        mut self,
        application_auth_verifier: Arc<dyn ApplicationAuthVerifier>,
    ) -> Self {
        self.application_auth_verifier = Some(application_auth_verifier);
        self
    }

    pub(crate) fn with_cloud_functions(
        mut self,
        cloud_functions_registry: CloudFunctionsRegistry,
    ) -> Self {
        self.cloud_functions_registry = Some(cloud_functions_registry);
        self
    }

    pub(crate) fn with_firebase(mut self, firebase_config: FirebaseConfig) -> Self {
        self.firebase_config = Some(firebase_config);
        self
    }

    pub(crate) fn with_license(mut self, license_state: LicenseState) -> Self {
        self.license_state = license_state;
        self
    }

    pub(crate) fn with_service_instance_catalog(
        mut self,
        service_instances: Arc<dyn ServiceInstanceCatalog>,
    ) -> Self {
        self.runtime_service_source =
            RuntimeServiceSource::ServiceInstanceCatalog(service_instances);
        self
    }

    pub(crate) fn with_deploy_admin_token(mut self, token: impl Into<String>) -> Self {
        self.deploy_admin_token = Some(token.into());
        self
    }

    pub(crate) fn with_local_server_security(
        mut self,
        local_server_security: Arc<LocalServerSecurityState>,
    ) -> Self {
        self.local_server_security = Some(local_server_security);
        self
    }

    pub(crate) fn with_tenant_isolation_mode(mut self, mode: TenantIsolationMode) -> Self {
        self.tenant_isolation_mode = mode;
        self
    }

    pub(crate) fn with_listen_addr(mut self, listen_addr: SocketAddr) -> Self {
        self.listen_addr = Some(listen_addr);
        self
    }

    pub(crate) fn with_server_shutdown(mut self, server_shutdown: watch::Sender<bool>) -> Self {
        self.server_shutdown = Some(server_shutdown);
        self
    }

    #[cfg(test)]
    pub(crate) fn without_deploy_admin_token(mut self) -> Self {
        self.deploy_admin_token = None;
        self
    }

    pub(crate) fn with_service_manager(mut self, service_manager: Arc<ServiceManager>) -> Self {
        self.runtime_service_source = RuntimeServiceSource::ServiceManager(service_manager);
        self
    }

    pub(crate) fn with_machine_lifecycle_manager(
        mut self,
        machine_lifecycle_manager: Arc<dyn MachineLifecycleManager>,
    ) -> Self {
        self.machine_lifecycle_manager = Some(machine_lifecycle_manager);
        self
    }

    pub(crate) async fn prepare_system_tenant(&self) -> nimbus_core::Result<()> {
        nimbus_system::prepare_system_tenant_async(&self.engine, self.listen_addr).await?;
        if let Some(registry) = self.convex_registry.as_ref() {
            let summary = registry.deploy_summary();
            let input = convex::convex_system_deployment_record_input(&summary, "startup");
            nimbus_system::record_deployment_state_async(&self.engine, &input).await?;
        }
        let Some(listen_addr) = self.listen_addr else {
            return Ok(());
        };
        let version = env!("CARGO_PKG_VERSION");
        if self.convex_registry.is_some() || self.system_convex_registry.is_some() {
            nimbus_system::record_listener_state_async(
                &self.engine,
                "convex",
                "websocket",
                &listen_addr.to_string(),
                "listening",
                Some(version),
                None,
            )
            .await?;
        }
        if self.firebase_config.is_some() {
            nimbus_system::record_listener_state_async(
                &self.engine,
                "firebase",
                "http+websocket",
                &listen_addr.to_string(),
                "listening",
                Some(version),
                None,
            )
            .await?;
        }
        if self.cloud_functions_registry.is_some() {
            nimbus_system::record_listener_state_async(
                &self.engine,
                "cloud-functions",
                "http",
                &listen_addr.to_string(),
                "listening",
                Some(version),
                None,
            )
            .await?;
        }
        Ok(())
    }

    pub(crate) fn build(self) -> Router {
        let engine = self.engine.clone();
        nimbus_system::install_table_projection_observer(&engine);
        let service_manager = self.runtime_service_source.service_manager();
        let version_check = build_version_check();
        let runtime_host_resource_budget = self.runtime_host_resource_budget;
        let runtime_host_pressure_source = self.runtime_host_pressure_source;
        let runtime_adaptive_controller_settings = self.runtime_adaptive_controller_settings;
        let effective_runtime_scaling_plans = self.effective_runtime_scaling_plans;
        let convex_registry = self.convex_registry.map(|registry| {
            registry
                .with_runtime_host_governor(
                    runtime_host_resource_budget,
                    runtime_host_pressure_source.clone(),
                    runtime_adaptive_controller_settings,
                )
                .with_effective_runtime_scaling_plans(effective_runtime_scaling_plans.clone())
        });
        let system_convex_registry = self.system_convex_registry.map(|registry| {
            registry
                .with_runtime_host_governor(
                    runtime_host_resource_budget,
                    runtime_host_pressure_source.clone(),
                    runtime_adaptive_controller_settings,
                )
                .with_effective_runtime_scaling_plans(effective_runtime_scaling_plans.clone())
        });
        let cloud_functions_registry = self.cloud_functions_registry.map(|registry| {
            registry
                .with_runtime_host_governor(
                    runtime_host_resource_budget,
                    runtime_host_pressure_source.clone(),
                    runtime_adaptive_controller_settings,
                )
                .with_effective_runtime_scaling_plans(effective_runtime_scaling_plans.clone())
        });
        let state = Arc::new(AppState::from_config(AppStateConfig {
            engine: self.engine,
            convex_registry,
            system_convex_registry,
            application_auth_verifier: self.application_auth_verifier,
            cloud_functions_registry,
            firebase_config: self.firebase_config,
            license_state: self.license_state,
            runtime_service_registry: self
                .runtime_service_source
                .into_runtime_service_registry(engine),
            service_manager,
            machine_lifecycle_manager: self.machine_lifecycle_manager,
            deploy_admin_token: self.deploy_admin_token,
            local_server_security: self.local_server_security,
            tenant_isolation_mode: self.tenant_isolation_mode,
            listen_addr: self.listen_addr,
            server_shutdown: self.server_shutdown,
            version_check,
            runtime_host_resource_budget: self.runtime_host_resource_budget,
            runtime_adaptive_controller_settings,
            effective_runtime_scaling_plans,
        }));
        let runtime_host_resource_budget = state.runtime_host_resource_budget();
        tracing::info!(
            host_millicpus = runtime_host_resource_budget.host_millicpus,
            system_reserved_millicpus = runtime_host_resource_budget.system_reserved_millicpus,
            nimbus_control_plane_reserved_millicpus =
                runtime_host_resource_budget.nimbus_control_plane_reserved_millicpus,
            runtime_hard_ceiling_millicpus =
                runtime_host_resource_budget.runtime_hard_ceiling_millicpus,
            runtime_seat_millicpus = runtime_host_resource_budget.runtime_seat_millicpus.get(),
            runtime_allocatable_millicpus =
                runtime_host_resource_budget.runtime_allocatable_millicpus(),
            "configured runtime host resource budget"
        );
        let runtime_adaptive_controller_settings = state.runtime_adaptive_controller_settings();
        tracing::info!(
            mode = ?runtime_adaptive_controller_settings.mode(),
            canary_remainders = runtime_adaptive_controller_settings
                .canary_policy()
                .admitted_remainders,
            canary_modulus = runtime_adaptive_controller_settings.canary_policy().hash_modulus.get(),
            rollback_to_static_defaults =
                runtime_adaptive_controller_settings.rollback_to_static_defaults(),
            live_adaptive_defaults_enabled =
                runtime_adaptive_controller_settings.live_adaptive_defaults_enabled(),
            "configured runtime adaptive controller"
        );
        let effective_runtime_scaling_plan = state.effective_runtime_scaling_plan();
        let effective_runtime_scaling_plans = state.effective_runtime_scaling_plans();
        tracing::info!(
            function = %effective_runtime_scaling_plan.function,
            min_warm = effective_runtime_scaling_plan.effective.min_warm,
            max_warm = effective_runtime_scaling_plan.effective.max_warm,
            autoscaling = effective_runtime_scaling_plan.effective.autoscaling,
            function_overrides = effective_runtime_scaling_plans.function_override_count(),
            pressure_adjustment = ?effective_runtime_scaling_plan.pressure_adjustment,
            "configured effective runtime scaling plans"
        );
        let deployment = state.current_deployment();
        if let Some(registry) = deployment.cloud_functions_registry() {
            state
                .install_cloud_functions_runtime_hooks(registry)
                .expect("cloud functions runtime hooks should install from active deployment");
        }
        let firebase_enabled = deployment.firebase_config().is_some();

        let local_admin_policy = LocalServerAccessPolicy::standard(state.clone());
        let deploy_admin_policy = LocalServerAccessPolicy::deploy(state.clone());

        let mut router = build_public_router()
            .merge(build_ui_router().route_layer(middleware::from_fn(http::ui_csp_middleware)))
            .merge(
                build_local_admin_router()
                    .route_layer(middleware::from_fn_with_state(
                        local_admin_policy.clone(),
                        route_family_gate_middleware,
                    ))
                    .route_layer(middleware::from_fn_with_state(
                        local_admin_policy,
                        server_access_extract_middleware,
                    )),
            )
            .merge(build_service_control_router())
            .merge(
                build_deploy_router()
                    .route_layer(middleware::from_fn_with_state(
                        deploy_admin_policy.clone(),
                        route_family_gate_middleware,
                    ))
                    .route_layer(middleware::from_fn_with_state(
                        deploy_admin_policy,
                        server_access_extract_middleware,
                    )),
            )
            .merge(
                // #41 fail-closed stopgap: the convex application surface selects
                // the tenant from the URL with no verified binding, so refuse it
                // on any non-loopback bind (covers all six route types by wrapping
                // the whole convex router). Replaced by the real binding fix.
                build_convex_router().route_layer(middleware::from_fn_with_state(
                    state.clone(),
                    convex::convex_application_network_bind_guard,
                )),
            );
        if firebase_enabled {
            router = router.merge(build_firebase_router(state.clone()));
        }
        if deployment.cloud_functions_registry().is_some() {
            router = router.fallback(any(cloud_functions::http_handler));
        }
        router
            .layer(build_cors_layer(&self.cors_allowed_origins))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                origin_allowlist_middleware,
            ))
            .with_state(state)
    }
}

pub(crate) fn convex_application_auth_verifier(
    convex_registry: &ConvexRegistry,
) -> Arc<dyn ApplicationAuthVerifier> {
    Arc::new(convex_registry.clone())
}

fn build_version_check() -> Arc<VersionCheck> {
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .unwrap_or_else(|_| semver::Version::new(0, 0, 0));
    let config = VersionCheckConfig::from_env(&current);
    VersionCheck::new(current, config)
}

/// Builds the Nimbus HTTP/WebSocket router from the canonical option bundle.
pub fn build_router(options: RouterOptions) -> Router {
    options.into_build_config().build()
}

fn default_runtime_host_resource_budget() -> RuntimeHostResourceBudget {
    let fallback_cpus = std::num::NonZeroUsize::new(1).expect("one logical CPU is nonzero");
    let host_logical_cpus = std::thread::available_parallelism().unwrap_or(fallback_cpus);
    RuntimeHostResourceBudget::conservative_for_logical_cpus(host_logical_cpus)
}

fn default_runtime_host_pressure_source() -> Arc<dyn RuntimeHostPressureSource> {
    #[cfg(target_os = "linux")]
    {
        match nimbus_node::CgroupV2HostPressureSource::for_current_process() {
            Ok(source) => return Arc::new(source),
            Err(error) => {
                tracing::debug!(
                    error = %error,
                    "cgroup v2 host pressure source unavailable; using nominal runtime host pressure source"
                );
            }
        }
    }
    Arc::new(NominalRuntimeHostPressureSource)
}

fn build_cors_layer(configured_origins: &[String]) -> CorsLayer {
    let mut allowed = std::collections::HashSet::new();
    for origin in configured_origins {
        match normalize_cors_origin(origin) {
            Ok(normalized) => {
                allowed.insert(normalized);
            }
            Err(reason) => {
                // Fail closed: a bad entry grants nothing extra; the origin
                // it was meant to allow will visibly fail CORS.
                tracing::warn!(%origin, %reason, "ignoring invalid configured CORS origin");
            }
        }
    }
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin, _request_head| {
            is_allowed_local_cors_origin(origin) || is_configured_cors_origin(origin, &allowed)
        }))
        .allow_headers([
            header::ACCEPT,
            header::AUTHORIZATION,
            header::CONTENT_TYPE,
            HeaderName::from_static("firebase-instance-id-token"),
            HeaderName::from_static("x-nimbus-admin-token"),
            HeaderName::from_static("google-cloud-resource-prefix"),
            HeaderName::from_static("x-goog-request-params"),
            HeaderName::from_static("x-goog-api-client"),
            HeaderName::from_static("x-goog-api-key"),
            HeaderName::from_static("x-firebase-gmpid"),
            HeaderName::from_static("x-firebase-appcheck"),
            HeaderName::from_static("x-grpc-web"),
            HeaderName::from_static("grpc-timeout"),
        ])
        .expose_headers([
            HeaderName::from_static("grpc-status"),
            HeaderName::from_static("grpc-message"),
            HeaderName::from_static("grpc-status-details-bin"),
        ])
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
}

pub(crate) fn is_allowed_local_cors_origin(origin: &HeaderValue) -> bool {
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Some(authority) = origin
        .strip_prefix("http://")
        .or_else(|| origin.strip_prefix("https://"))
    else {
        return false;
    };

    matches!(authority, "localhost" | "127.0.0.1" | "[::1]")
        || authority.starts_with("localhost:")
        || authority.starts_with("127.0.0.1:")
        || authority.starts_with("[::1]:")
}

pub(crate) fn is_configured_cors_origin(
    origin: &HeaderValue,
    allowed: &std::collections::HashSet<String>,
) -> bool {
    if allowed.is_empty() {
        return false;
    }
    let Ok(origin) = origin.to_str() else {
        return false;
    };
    let Ok(normalized) = normalize_cors_origin(origin) else {
        return false;
    };
    allowed.contains(&normalized)
}

/// Normalize a configured browser origin to the exact form browsers send in
/// the `Origin` header: lowercase `scheme://host`, default ports stripped,
/// no path/query/fragment. Wildcards are rejected — the CORS allowlist is
/// exact-match only.
pub fn normalize_cors_origin(origin: &str) -> Result<String, String> {
    let trimmed = origin.trim();
    if trimmed.is_empty() {
        return Err("CORS origin must not be empty".to_string());
    }
    if trimmed.contains('*') {
        return Err(
            "wildcard CORS origins are not supported; pass each origin explicitly".to_string(),
        );
    }
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err(format!(
            "CORS origin `{trimmed}` must include an http:// or https:// scheme"
        ));
    };
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "CORS origin `{trimmed}` must use the http or https scheme"
        ));
    }
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    if authority.is_empty() {
        return Err(format!("CORS origin `{trimmed}` is missing a host"));
    }
    if authority.contains('/') || authority.contains('?') || authority.contains('#') {
        return Err(format!(
            "CORS origin `{trimmed}` must not include a path, query, or fragment"
        ));
    }
    let authority = authority.to_ascii_lowercase();
    let (host, port) = split_origin_port(&authority);
    if host.is_empty() {
        return Err(format!("CORS origin `{trimmed}` is missing a host"));
    }
    match port {
        None => Ok(format!("{scheme}://{host}")),
        Some(port) => {
            let Ok(parsed) = port.parse::<u16>() else {
                return Err(format!("CORS origin `{trimmed}` has an invalid port"));
            };
            let is_default =
                (scheme == "http" && parsed == 80) || (scheme == "https" && parsed == 443);
            if is_default {
                Ok(format!("{scheme}://{host}"))
            } else {
                Ok(format!("{scheme}://{host}:{parsed}"))
            }
        }
    }
}

/// Split `host[:port]`, treating a bracketed IPv6 literal as the host
/// boundary so `[::1]:8080` does not split inside the address.
fn split_origin_port(authority: &str) -> (&str, Option<&str>) {
    if let Some(bracket_end) = authority.rfind(']') {
        match authority[bracket_end + 1..].strip_prefix(':') {
            Some(port) => (&authority[..=bracket_end], Some(port)),
            None => (authority, None),
        }
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (host, Some(port))
    } else {
        (authority, None)
    }
}

fn build_public_router() -> Router<Arc<AppState>> {
    let demos = ServeDir::new(DEMOS_DIR).append_index_html_on_directories(true);

    Router::new()
        .route("/health", get(http::health))
        .route("/demos", get(http::demos_redirect))
        .nest_service("/demos/", demos)
}

fn build_ui_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/ui", get(http::ui_root))
        .route("/ui/", get(http::ui_root))
        .route("/ui/auth", get(http::ui_auth))
        .route("/ui/auth.js", get(http::ui_auth_script))
        .route("/ui/auth/session", post(http::create_ui_session))
        .route("/ui/auth/launch-ticket", post(http::mint_ui_launch_ticket))
        .route("/ui/launch", get(http::consume_ui_launch_ticket))
        .route("/ui/{*path}", get(http::ui_path))
}

fn build_local_admin_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/debug/license/status", get(http::license_status))
        .route("/debug/encryption/status", get(http::encryption_status))
        .route(
            "/api/system/token/rotate",
            post(http::rotate_local_admin_token),
        )
        .route("/api/system/shutdown", post(http::shutdown_system))
        .route("/api/system/version-info", get(http::version_info))
        .route("/api/console/source", get(http::module_source))
        .route("/api/console/graph", get(http::call_graph))
        .route("/debug/runtime/metrics", get(http::runtime_diagnostics))
        .route(
            "/debug/tenants/{tenant_id}/consistency",
            get(http::tenant_consistency_report),
        )
        .route(
            "/debug/tenants/{tenant_id}/engine/metrics",
            get(http::tenant_engine_diagnostics),
        )
        .route(
            "/api/tenants",
            post(http::create_tenant).get(http::list_tenants),
        )
        .route("/api/tenants/{tenant_id}", delete(http::delete_tenant))
        .route(
            "/api/machines/{name}",
            delete(http::delete_machine).patch(http::update_machine),
        )
        .route("/api/machines/{name}/create", post(http::create_machine))
        .route("/api/machines/{name}/start", post(http::start_machine))
        .route("/api/machines/{name}/stop", post(http::stop_machine))
        .route("/api/machines/{name}/restart", post(http::restart_machine))
        .route(
            "/api/tenants/{tenant_id}/schedule",
            post(http::schedule_mutation).get(http::list_scheduled_jobs),
        )
        .route(
            "/api/tenants/{tenant_id}/schedule/{job_id}",
            delete(http::cancel_scheduled_job),
        )
        .route(
            "/api/tenants/{tenant_id}/schedule/history/{job_id}",
            get(http::get_scheduled_job_result),
        )
        .route(
            "/api/tenants/{tenant_id}/crons",
            post(http::create_cron_job).get(http::list_cron_jobs),
        )
        .route(
            "/api/tenants/{tenant_id}/crons/{name}",
            delete(http::delete_cron_job),
        )
        .route("/api/tenants/{tenant_id}/schema", get(http::get_schema))
        .route(
            "/api/tenants/{tenant_id}/schema/{table}",
            get(http::get_table_schema)
                .put(http::set_table_schema)
                .delete(http::delete_table_schema),
        )
        .route(
            "/api/tenants/{tenant_id}/journal/bootstrap",
            get(http::bootstrap_journal),
        )
        .route("/api/tenants/{tenant_id}/journal", get(http::read_journal))
        .route(
            "/api/tenants/{tenant_id}/documents",
            post(http::insert_document),
        )
        .route(
            "/api/tenants/{tenant_id}/documents/{table}",
            get(http::list_documents),
        )
        .route(
            "/api/tenants/{tenant_id}/documents/{table}/{document_id}",
            get(http::get_document)
                .patch(http::update_document)
                .delete(http::delete_document),
        )
        .route(
            "/api/tenants/{tenant_id}/query",
            post(http::query_documents),
        )
        .route(
            "/api/tenants/{tenant_id}/query/paginated",
            post(http::query_documents_paginated),
        )
        .route("/ws", get(ws::ws_handler))
}

fn build_service_control_router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/api/sessions",
            get(http::list_sessions).post(http::open_session),
        )
        .route("/api/sessions/{session_id}", get(http::get_session))
        .route(
            "/api/sessions/{session_id}/close",
            post(http::close_session),
        )
        .route(
            "/api/tenants/{tenant_id}/sandboxes",
            get(http::list_sandboxes).post(http::create_sandbox),
        )
        .route(
            "/api/tenants/{tenant_id}/sandboxes/{sandbox_id}",
            get(http::get_sandbox),
        )
        .route(
            "/api/tenants/{tenant_id}/sandboxes/{sandbox_id}/stop",
            post(http::stop_sandbox),
        )
        .route(
            "/api/tenants/{tenant_id}/services",
            get(http::list_service_definitions).post(http::create_service_definition),
        )
        .route(
            "/api/tenants/{tenant_id}/services/{service_name}",
            get(http::get_service)
                .put(http::update_service_definition)
                .delete(http::delete_service_definition),
        )
        .route(
            "/api/tenants/{tenant_id}/services/{service_name}/start",
            post(http::start_service),
        )
        .route(
            "/api/tenants/{tenant_id}/services/{service_name}/stop",
            post(http::stop_service),
        )
        .route(
            "/api/tenants/{tenant_id}/services/{service_name}/restart",
            post(http::restart_service),
        )
}

fn build_deploy_router() -> Router<Arc<AppState>> {
    Router::new().route("/api/admin/deploy", post(http::deploy_app))
}

fn build_convex_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/convex/{tenant_id}/query", post(convex::query))
        .route(
            "/convex/{tenant_id}/query/paginated",
            post(convex::paginated_query),
        )
        .route("/convex/{tenant_id}/mutation", post(convex::mutation))
        .route("/convex/{tenant_id}/action", post(convex::action))
        .route("/convex/{tenant_id}/http", any(convex::http_route_root))
        .route("/convex/{tenant_id}/http/{*path}", any(convex::http_route))
        .route(
            "/convex/{tenant_id}/schedule/run_after",
            post(convex::schedule_after),
        )
        .route(
            "/convex/{tenant_id}/schedule/run_at",
            post(convex::schedule_at),
        )
        .route(
            "/convex/{tenant_id}/schedule/{job_id}",
            delete(convex::cancel_scheduled_job),
        )
        .route("/convex/{tenant_id}/ws", get(convex::ws))
}

fn build_firebase_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
    // Keep one Firestore service instance so gRPC and WebSocket Listen share
    // retained target and write-stream state across reconnects.
    let firestore_service = firebase::grpc::FirestoreGrpcService::from_state(state.clone());
    let firestore_websocket_service = firestore_service.clone();
    let firestore_listen_service = ServiceBuilder::new()
        .layer(tonic_web::GrpcWebLayer::new())
        .service(firestore_service.clone().into_server());
    let firestore_grpc_service = ServiceBuilder::new()
        .layer(tonic_web::GrpcWebLayer::new())
        .service(firestore_service.into_server());
    // Every REST route lives behind the shared auth middleware, which
    // resolves the bearer once and injects the result as a request
    // extension — a REST route added here cannot skip auth. The gRPC and
    // WebSocket routes stay outside the layer: they resolve auth from
    // their own request metadata and must answer failures with gRPC
    // Status, not HTTP JSON.
    let rest_routes = Router::new()
        .route(
            "/v1/projects/{project_id}/databases/{database_id}/documents:commit",
            post(firebase::commit),
        )
        .route(
            "/v1/projects/{project_id}/databases/{database_id}/documents:batchWrite",
            post(firebase::batch_write),
        )
        .route(
            "/v1/projects/{project_id}/databases/{database_id}/documents:batchGet",
            post(firebase::batch_get_documents),
        )
        .route(
            "/v1/projects/{project_id}/databases/{database_id}/documents:beginTransaction",
            post(firebase::begin_transaction),
        )
        .route(
            "/v1/projects/{project_id}/databases/{database_id}/documents:rollback",
            post(firebase::rollback),
        )
        .route(
            "/v1/projects/{project_id}/databases/{database_id}/documents:listCollectionIds",
            post(firebase::list_collection_ids),
        )
        .route(
            "/v1/projects/{project_id}/databases/{database_id}/documents:runQuery",
            post(firebase::run_query),
        )
        .route(
            "/v1/projects/{project_id}/databases/{database_id}/documents:runAggregationQuery",
            post(firebase::run_aggregation_query),
        )
        .route(
            "/v1/projects/{project_id}/databases/{database_id}/documents/{*document_request}",
            post(firebase::run_document_action_under_parent_document),
        )
        .route_layer(middleware::from_fn_with_state(
            state,
            firebase::require_firebase_rest_auth,
        ));
    Router::new()
        .merge(rest_routes)
        .route(
            "/google.firestore.v1.Firestore/Listen",
            get(firebase::grpc::listen_websocket)
                .post_service(firestore_listen_service)
                .layer(Extension(firestore_websocket_service)),
        )
        .route_service(
            "/google.firestore.v1.Firestore/{*grpc_method}",
            firestore_grpc_service,
        )
}
