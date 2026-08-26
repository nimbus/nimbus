use std::path::PathBuf;
use std::sync::Arc;

use axum::middleware;
use axum::routing::{any, delete, get, post};
use axum::{Extension, Router};
use nimbus_compute::config::control_plane::ControlPlaneConfig;
use nimbus_compute::config::deployment::DeploymentConfig;
use nimbus_compute::config::node_services::NodeServicesConfig;
use nimbus_compute::config::runtime::RuntimeGovernorConfig;
use nimbus_engine::Engine;
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, RuntimeAdaptiveControllerSettings, RuntimeHostPressureSource,
    RuntimeHostResourceBudget, RuntimeLimits, RuntimeScalingPlanSet,
};
use tower::ServiceBuilder;
use tower_http::services::ServeDir;

use crate::adapters::cloud_functions::{CloudFunctionsHttpTenantBinding, CloudFunctionsRegistry};
use crate::adapters::cloudflare::CloudflareConfig;
use crate::adapters::convex::{self, ConvexRegistry, ConvexSiloAuthRegistry, ConvexTenancyConfig};
use crate::adapters::firebase::{self, FirebaseConfig};
use crate::adapters::http_mount::{
    CloudFunctionsHttpAdapter, CloudflareHttpAdapter, ConvexHttpAdapter, FirebaseHttpAdapter,
    mount_adapters,
};
use crate::config::transport::TransportConfig;
use crate::license::LicenseState;
use crate::local_server::{
    LocalServerAccessPolicy, LocalServerSecurityState, origin_allowlist_middleware,
    route_family_gate_middleware, server_access_extract_middleware,
};
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::state::{AppState, AppStateConfig};
use crate::tenant::TenantIsolationMode;
use crate::workload_composition::{ServerWorkloadComposition, ServerWorkloadProfile};
use crate::{http, ws};
use nimbus_auth::ApplicationAuthVerifier;
use nimbus_services::ServiceInstanceCatalog;
#[cfg(test)]
use nimbus_services::ServiceManager;

mod cors;

use self::cors::build_cors_layer;

pub use cors::normalize_cors_origin;
#[cfg(test)]
pub(crate) use cors::{is_allowed_local_cors_origin, is_configured_cors_origin};

/// Canonical public option bundle for building a Nimbus HTTP/WebSocket router.
pub struct RouterOptions {
    workload: ServerWorkloadProfile,
    deployment: DeploymentConfig,
    control_plane: ControlPlaneConfig,
    node_services: NodeServicesConfig,
    transport: TransportConfig,
    runtime: RuntimeGovernorConfig,
}

impl RouterOptions {
    /// Build a workload-capable router from one complete validated realm.
    pub fn managed(composition: ServerWorkloadComposition) -> Self {
        let service_manager = composition.service_manager();
        Self {
            workload: ServerWorkloadProfile::managed(composition),
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::default().with_service_manager(service_manager),
            transport: TransportConfig::default(),
            runtime: RuntimeGovernorConfig::default(),
        }
    }

    /// Build an explicitly protocol-only router with no workload lifecycle.
    ///
    /// This profile cannot install a service or machine lifecycle manager.
    pub fn protocol_only(engine: Arc<Engine>) -> Self {
        Self {
            workload: ServerWorkloadProfile::protocol_only(engine),
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::router_options_default(),
            node_services: NodeServicesConfig::default(),
            transport: TransportConfig::default(),
            runtime: RuntimeGovernorConfig::default(),
        }
    }

    pub fn with_convex_registry(mut self, convex_registry: ConvexRegistry) -> Self {
        self.deployment = self.deployment.with_convex(convex_registry);
        self
    }

    pub fn with_convex_registry_for_silo(
        mut self,
        silo: &nimbus_core::TenantId,
        convex_registry: ConvexRegistry,
    ) -> Self {
        let verifier = convex_application_auth_verifier(&convex_registry);
        self.deployment = self
            .deployment
            .with_convex(convex_registry)
            .with_convex_silo_auth_verifier(silo, verifier);
        self
    }

    pub fn with_convex_silo_auth(mut self, convex_silo_auth: ConvexSiloAuthRegistry) -> Self {
        self.deployment = self.deployment.with_convex_silo_auth(convex_silo_auth);
        self
    }

    pub fn with_system_convex_registry(mut self, system_convex_registry: ConvexRegistry) -> Self {
        self.deployment = self
            .deployment
            .with_system_convex_registry(system_convex_registry);
        self
    }

    pub fn with_cloud_functions_registry(
        mut self,
        cloud_functions_registry: CloudFunctionsRegistry,
    ) -> Self {
        self.deployment = self
            .deployment
            .with_cloud_functions(cloud_functions_registry);
        self
    }

    pub fn with_cloud_functions_http_tenant(
        mut self,
        binding: CloudFunctionsHttpTenantBinding,
    ) -> Self {
        self.deployment = self.deployment.with_cloud_functions_http_tenant(binding);
        self
    }

    pub fn with_firebase_config(mut self, firebase_config: FirebaseConfig) -> Self {
        self.deployment = self.deployment.with_firebase(firebase_config);
        self
    }

    pub fn with_cloudflare_config(mut self, cloudflare_config: CloudflareConfig) -> Self {
        self.deployment = self.deployment.with_cloudflare(cloudflare_config);
        self
    }

    pub fn with_convex_tenancy(mut self, convex_tenancy: ConvexTenancyConfig) -> Self {
        self.deployment = self.deployment.with_convex_tenancy(convex_tenancy);
        self
    }

    pub fn with_license(mut self, license_state: LicenseState) -> Self {
        self.control_plane = self.control_plane.with_license(license_state);
        self
    }

    pub fn with_service_instance_catalog(
        mut self,
        service_instances: Arc<dyn ServiceInstanceCatalog>,
    ) -> Self {
        self.node_services = self
            .node_services
            .with_service_instance_catalog(service_instances);
        self
    }

    pub fn with_machine_lifecycle_manager(
        mut self,
        machine_lifecycle_manager: Arc<dyn MachineLifecycleManager>,
    ) -> Self {
        self.require_managed("machine lifecycle manager");
        self.node_services = self
            .node_services
            .with_machine_lifecycle_manager(machine_lifecycle_manager);
        self
    }

    pub fn with_deploy_admin_token(mut self, token: impl Into<String>) -> Self {
        self.control_plane = self.control_plane.with_deploy_admin_token(token);
        self
    }

    pub fn with_local_server_security(
        mut self,
        local_server_security: Arc<LocalServerSecurityState>,
    ) -> Self {
        self.control_plane = self
            .control_plane
            .with_local_server_security(local_server_security);
        self
    }

    pub fn with_tenant_isolation_mode(mut self, mode: TenantIsolationMode) -> Self {
        self.node_services = self.node_services.with_tenant_isolation_mode(mode);
        self
    }

    /// Allow additional exact browser origins through the CORS layer.
    /// Loopback origins are always allowed. Values should be normalized via
    /// [`normalize_cors_origin`]; entries that fail normalization are
    /// ignored with a warning (fail closed).
    pub fn with_cors_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.transport = self.transport.with_cors_allowed_origins(origins);
        self
    }

    pub fn with_runtime_host_resource_budget(mut self, budget: RuntimeHostResourceBudget) -> Self {
        self.runtime = self.runtime.with_runtime_host_resource_budget(budget);
        self
    }

    /// Sets the canonical base limits from which the compute runtime manager
    /// derives every adapter lane.
    pub fn with_runtime_limits(mut self, limits: RuntimeLimits) -> Self {
        self.runtime = self.runtime.with_base_runtime_limits(limits);
        self
    }

    pub fn with_runtime_host_pressure_source(
        mut self,
        pressure_source: Arc<dyn RuntimeHostPressureSource>,
    ) -> Self {
        self.runtime = self
            .runtime
            .with_runtime_host_pressure_source(pressure_source);
        self
    }

    pub fn with_runtime_adaptive_controller_settings(
        mut self,
        settings: RuntimeAdaptiveControllerSettings,
    ) -> Self {
        self.runtime = self
            .runtime
            .with_runtime_adaptive_controller_settings(settings);
        self
    }

    pub fn with_effective_runtime_scaling_plan(self, plan: EffectiveRuntimeScalingPlan) -> Self {
        let mut this = self;
        this.runtime = this.runtime.with_effective_runtime_scaling_plan(plan);
        this
    }

    pub fn with_effective_runtime_scaling_plans(mut self, plans: RuntimeScalingPlanSet) -> Self {
        self.runtime = self.runtime.with_effective_runtime_scaling_plans(plans);
        self
    }

    pub(crate) fn engine(&self) -> Arc<Engine> {
        self.workload.engine()
    }

    pub(crate) fn has_system_convex_registry(&self) -> bool {
        self.deployment.has_system_convex_registry()
    }

    pub(crate) fn into_build_config(self) -> RouterBuildConfig {
        let mut config = RouterBuildConfig::from_workload(self.workload);
        config.deployment = self.deployment;
        config
            .control_plane
            .overlay_router_options(self.control_plane);
        config.node_services = self.node_services;
        config.transport = self.transport;
        config.runtime = self.runtime;
        config
    }

    fn require_managed(&self, component: &str) {
        assert!(
            self.workload.is_managed(),
            "{component} requires RouterOptions::managed with a complete workload composition; \
             protocol-only construction cannot own workload lifecycle"
        );
    }
}

pub(crate) struct RouterBuildConfig {
    workload: ServerWorkloadProfile,
    deployment: DeploymentConfig,
    control_plane: ControlPlaneConfig,
    node_services: NodeServicesConfig,
    transport: TransportConfig,
    runtime: RuntimeGovernorConfig,
}

pub(crate) struct PreparedRouterState {
    state: Arc<AppState>,
    cloud_functions_http_enabled: bool,
    cors_allowed_origins: Vec<String>,
}

impl RouterBuildConfig {
    #[cfg(test)]
    pub(crate) fn core(engine: Arc<Engine>) -> Self {
        Self::from_workload(ServerWorkloadProfile::protocol_only(engine))
    }

    fn from_workload(workload: ServerWorkloadProfile) -> Self {
        Self {
            workload,
            deployment: DeploymentConfig::default(),
            control_plane: ControlPlaneConfig::build_default(),
            node_services: NodeServicesConfig::default(),
            transport: TransportConfig::default(),
            runtime: RuntimeGovernorConfig::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_runtime_limits(mut self, limits: RuntimeLimits) -> Self {
        self.runtime = self.runtime.with_base_runtime_limits(limits);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_runtime_host_resource_budget(
        mut self,
        budget: RuntimeHostResourceBudget,
    ) -> Self {
        self.runtime = self.runtime.with_runtime_host_resource_budget(budget);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_runtime_host_pressure_source(
        mut self,
        pressure_source: Arc<dyn RuntimeHostPressureSource>,
    ) -> Self {
        self.runtime = self
            .runtime
            .with_runtime_host_pressure_source(pressure_source);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_convex(mut self, convex_registry: ConvexRegistry) -> Self {
        self.runtime = self
            .runtime
            .with_base_runtime_limits(convex_registry.runtime_limits());
        self.deployment = self.deployment.with_convex(convex_registry);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_system_convex_registry(
        mut self,
        system_convex_registry: ConvexRegistry,
    ) -> Self {
        self.deployment = self
            .deployment
            .with_system_convex_registry(system_convex_registry);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_application_auth_verifier(
        mut self,
        application_auth_verifier: Arc<dyn ApplicationAuthVerifier>,
    ) -> Self {
        self.deployment = self
            .deployment
            .with_application_auth_verifier(application_auth_verifier);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_convex_silo_auth_verifier(
        mut self,
        silo: &nimbus_core::TenantId,
        verifier: Arc<dyn ApplicationAuthVerifier>,
    ) -> Self {
        self.deployment = self
            .deployment
            .with_convex_silo_auth_verifier(silo, verifier);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_cloud_functions(
        mut self,
        cloud_functions_registry: CloudFunctionsRegistry,
    ) -> Self {
        self.runtime = self
            .runtime
            .with_base_runtime_limits(cloud_functions_registry.runtime_limits());
        self.deployment = self
            .deployment
            .with_cloud_functions(cloud_functions_registry);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_cloud_functions_http_tenant(
        mut self,
        binding: CloudFunctionsHttpTenantBinding,
    ) -> Self {
        self.deployment = self.deployment.with_cloud_functions_http_tenant(binding);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_firebase(mut self, firebase_config: FirebaseConfig) -> Self {
        self.deployment = self.deployment.with_firebase(firebase_config);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_cloudflare(mut self, cloudflare_config: CloudflareConfig) -> Self {
        self.deployment = self.deployment.with_cloudflare(cloudflare_config);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_convex_tenancy(mut self, convex_tenancy: ConvexTenancyConfig) -> Self {
        self.deployment = self.deployment.with_convex_tenancy(convex_tenancy);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_deploy_admin_token(mut self, token: impl Into<String>) -> Self {
        self.control_plane = self.control_plane.with_deploy_admin_token(token);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_local_server_security(
        mut self,
        local_server_security: Arc<LocalServerSecurityState>,
    ) -> Self {
        self.control_plane = self
            .control_plane
            .with_local_server_security(local_server_security);
        self
    }

    pub(crate) fn with_listen_addr(mut self, listen_addr: std::net::SocketAddr) -> Self {
        self.transport = self.transport.with_listen_addr(listen_addr);
        self
    }

    pub(crate) fn with_server_shutdown(
        mut self,
        server_shutdown: tokio::sync::watch::Sender<bool>,
    ) -> Self {
        self.transport = self.transport.with_server_shutdown(server_shutdown);
        self
    }

    #[cfg(test)]
    pub(crate) fn without_deploy_admin_token(mut self) -> Self {
        self.control_plane = self.control_plane.without_deploy_admin_token();
        self
    }

    #[cfg(test)]
    pub(crate) fn with_machine_lifecycle_manager(
        mut self,
        machine_lifecycle_manager: Arc<dyn MachineLifecycleManager>,
    ) -> Self {
        self.node_services = self
            .node_services
            .with_machine_lifecycle_manager(machine_lifecycle_manager);
        self
    }

    pub(crate) async fn prepare_system_tenant(&self) -> nimbus_core::Result<()> {
        let engine = self.workload.engine();
        nimbus_system::prepare_system_tenant_async(&engine, self.transport.listen_addr()).await?;
        if self.workload.is_managed() {
            crate::workload_saga_store::prepare_for_server(&engine).await?;
        }
        if let Some(registry) = self.deployment.convex_registry.as_ref() {
            let summary = registry.deploy_summary();
            let input =
                nimbus_compute::deploy::convex_system_deployment_record_input(&summary, "startup");
            nimbus_system::record_deployment_state_async(&engine, &input).await?;
        }
        Ok(())
    }

    /// Build transport state only after system schemas and the one bounded
    /// managed-workload recovery attempt are complete. No listener is served
    /// before this method returns.
    pub(crate) async fn prepare_for_serving(self) -> nimbus_core::Result<PreparedRouterState> {
        self.prepare_system_tenant().await?;
        let prepared = self.into_state();
        prepared
            .state
            .prepare_workload_lifecycle()
            .await
            .map_err(|error| nimbus_core::Error::Internal(error.to_string()))?;
        Ok(prepared)
    }

    fn into_state(self) -> PreparedRouterState {
        let RouterBuildConfig {
            workload,
            deployment,
            control_plane,
            node_services,
            transport,
            runtime,
        } = self;
        let engine = workload.engine();
        nimbus_system::install_table_projection_observer(&engine);
        let node_services = node_services.resolve();
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
        // The deploy API can activate the first Cloud Functions registry after
        // router construction. Mount its dynamic fallback whenever that API is
        // available so activation cannot publish an unreachable HTTP target.
        let cloud_functions_http_enabled =
            cloud_functions_registry.is_some() || control_plane.deploy_admin_token().is_some();
        let cors_allowed_origins = transport.cors_allowed_origins().to_owned();
        let state = Arc::new(AppState::from_config(AppStateConfig {
            workload,
            deployment: DeploymentConfig {
                convex_registry,
                system_convex_registry,
                application_auth_verifier,
                convex_silo_auth,
                cloud_functions_registry,
                cloud_functions_http_tenant,
                cloudflare_config,
                firebase_config,
                convex_tenancy,
            },
            control_plane,
            node_services,
            transport: transport.ensure_version_check(),
            runtime,
        }));
        PreparedRouterState {
            state,
            cloud_functions_http_enabled,
            cors_allowed_origins,
        }
    }

    fn build_prepared(prepared: PreparedRouterState) -> Router {
        let PreparedRouterState {
            state,
            cloud_functions_http_enabled,
            cors_allowed_origins,
        } = prepared;
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
        let cloudflare_config = deployment.cloudflare_config();

        let local_admin_policy = LocalServerAccessPolicy::standard(state.clone());
        let deploy_admin_policy = LocalServerAccessPolicy::deploy(state.clone());

        let router = build_public_router()
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
            );
        let router = mount_adapters(
            router,
            vec![
                Box::new(ConvexHttpAdapter),
                Box::new(FirebaseHttpAdapter::new(firebase_enabled, state.clone())),
                Box::new(CloudflareHttpAdapter::new(cloudflare_config)),
                Box::new(CloudFunctionsHttpAdapter::new(cloud_functions_http_enabled)),
            ],
        );
        router
            .layer(build_cors_layer(&cors_allowed_origins))
            .layer(middleware::from_fn_with_state(
                state.clone(),
                origin_allowlist_middleware,
            ))
            .with_state(state)
    }

    pub(crate) fn build(self) -> Router {
        Self::build_prepared(self.into_state())
    }

    pub(crate) fn build_serving(prepared: PreparedRouterState) -> Router {
        Self::build_prepared(prepared)
    }
}

pub(crate) fn convex_application_auth_verifier(
    convex_registry: &ConvexRegistry,
) -> Arc<dyn ApplicationAuthVerifier> {
    Arc::new(convex_registry.clone())
}

/// Builds the Nimbus HTTP/WebSocket router from the canonical option bundle.
pub fn build_router(options: RouterOptions) -> Router {
    let config = options.into_build_config();
    assert!(
        !config.workload.is_managed(),
        "managed workload routers require the asynchronous serving or foreground readiness boundary"
    );
    config.build()
}

fn build_public_router() -> Router<Arc<AppState>> {
    let examples = ServeDir::new(examples_dir()).append_index_html_on_directories(true);

    Router::new()
        .route("/health", get(http::health))
        .route("/examples", get(http::examples_redirect))
        .nest_service("/examples/", examples)
}

fn examples_dir() -> PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(|manifest_dir| PathBuf::from(manifest_dir).join("../../examples"))
        .unwrap_or_else(|| PathBuf::from("examples"))
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
            get(http::tenant_consistency_report).delete(http::clear_tenant_consistency_session),
        )
        .route(
            "/debug/tenants/{tenant_id}/engine/metrics",
            get(http::tenant_engine_diagnostics),
        )
        .route(
            "/debug/tenants/{tenant_id}/engine/retention",
            post(http::run_tenant_metadata_retention),
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

pub(crate) fn build_convex_router() -> Router<Arc<AppState>> {
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

pub(crate) fn build_firebase_router(state: Arc<AppState>) -> Router<Arc<AppState>> {
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

#[cfg(test)]
mod network_manager_tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use nimbus_sandbox::SandboxBackendKind;
    use nimbus_services::EmptyServiceDefinitionCatalog;

    use super::*;

    struct EffectForbiddenMachineLifecycleManager;

    impl MachineLifecycleManager for EffectForbiddenMachineLifecycleManager {
        fn create_machine<'a>(
            &'a self,
            _request: crate::machine_lifecycle::MachineCreateRequest,
        ) -> crate::machine_lifecycle::MachineLifecycleFuture<'a> {
            panic!("protocol-only refusal must happen before machine effects")
        }

        fn start_machine<'a>(
            &'a self,
            _name: &'a str,
        ) -> crate::machine_lifecycle::MachineLifecycleFuture<'a> {
            panic!("protocol-only refusal must happen before machine effects")
        }

        fn stop_machine<'a>(
            &'a self,
            _name: &'a str,
        ) -> crate::machine_lifecycle::MachineLifecycleFuture<'a> {
            panic!("protocol-only refusal must happen before machine effects")
        }

        fn update_machine<'a>(
            &'a self,
            _request: crate::machine_lifecycle::MachineUpdateRequest,
        ) -> crate::machine_lifecycle::MachineLifecycleFuture<'a> {
            panic!("protocol-only refusal must happen before machine effects")
        }

        fn delete_machine<'a>(
            &'a self,
            _name: &'a str,
        ) -> crate::machine_lifecycle::MachineLifecycleFuture<'a> {
            panic!("protocol-only refusal must happen before machine effects")
        }
    }

    fn app_state_from_build(build: RouterBuildConfig) -> Arc<AppState> {
        build.into_state().state
    }

    #[test]
    fn protocol_only_state_refuses_uncomposed_workload_manager_families() {
        let root = tempfile::tempdir().expect("fixture root should build");
        let engine = Arc::new(Engine::new(root.path()).expect("engine should initialize"));
        let protocol_state = app_state_from_build(
            RouterOptions::protocol_only(Arc::clone(&engine)).into_build_config(),
        );
        assert!(protocol_state.network_manager().is_none());

        let service_manager = Arc::new(ServiceManager::new(
            Arc::new(EmptyServiceDefinitionCatalog),
            SandboxBackendKind::Krun,
        ));
        let service_state_refusal = catch_unwind(AssertUnwindSafe(|| {
            AppState::from_config(AppStateConfig {
                workload: ServerWorkloadProfile::protocol_only(Arc::clone(&engine)),
                deployment: DeploymentConfig::default(),
                control_plane: ControlPlaneConfig::router_options_default(),
                node_services: NodeServicesConfig::default().with_service_manager(service_manager),
                transport: TransportConfig::default(),
                runtime: RuntimeGovernorConfig::default(),
            });
        }));
        assert!(
            service_state_refusal.is_err(),
            "compute state must reject a service manager without the shared network manager"
        );

        let machine_manager: Arc<dyn MachineLifecycleManager> =
            Arc::new(EffectForbiddenMachineLifecycleManager);
        let machine_refusal = catch_unwind(AssertUnwindSafe(|| {
            RouterOptions::protocol_only(Arc::clone(&engine))
                .with_machine_lifecycle_manager(Arc::clone(&machine_manager));
        }));
        assert!(
            machine_refusal.is_err(),
            "protocol-only machine-manager builder must refuse before installation"
        );
        let machine_state_refusal = catch_unwind(AssertUnwindSafe(|| {
            AppState::from_config(AppStateConfig {
                workload: ServerWorkloadProfile::protocol_only(engine),
                deployment: DeploymentConfig::default(),
                control_plane: ControlPlaneConfig::router_options_default(),
                node_services: NodeServicesConfig::default()
                    .with_machine_lifecycle_manager(machine_manager),
                transport: TransportConfig::default(),
                runtime: RuntimeGovernorConfig::default(),
            });
        }));
        assert!(
            machine_state_refusal.is_err(),
            "compute state must reject a machine manager without the shared network manager"
        );
    }
}
