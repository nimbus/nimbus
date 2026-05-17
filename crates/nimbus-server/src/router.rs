use std::net::SocketAddr;
use std::sync::Arc;

use axum::http::{HeaderName, HeaderValue, Method, header};
use axum::middleware;
use axum::routing::{any, delete, get, post};
use axum::{Extension, Router};
use nimbus_engine::Service;
use tokio::sync::watch;
use tower::ServiceBuilder;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;

use crate::adapters::cloud_functions;
use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::convex::{self, ConvexRegistry};
use crate::adapters::firebase::{self, FirebaseConfig};
use crate::application_auth::ApplicationAuthVerifier;
use crate::license::LicenseState;
use crate::local_server::{
    LocalServerAccessPolicy, LocalServerSecurityState, origin_allowlist_middleware,
    route_family_gate_middleware, server_access_extract_middleware,
};
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::sandbox::{EmptySandboxCatalog, SandboxCatalog};
use crate::service_manager::SandboxServiceManager;
use crate::service_registry::{RuntimeServiceRegistry, SandboxCatalogRuntimeServiceRegistry};
use crate::state::{AppState, AppStateConfig};
use crate::system::VersionCheck;
use crate::system::version_check::VersionCheckConfig;
use crate::{http, ws};

const DEMOS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demos");

enum RuntimeServiceSource {
    SandboxCatalog(Arc<dyn SandboxCatalog>),
    SandboxServiceManager(Arc<SandboxServiceManager>),
    #[cfg(test)]
    RuntimeServiceRegistry(Arc<dyn RuntimeServiceRegistry>),
}

impl RuntimeServiceSource {
    fn sandbox_service_manager(&self) -> Option<Arc<SandboxServiceManager>> {
        match self {
            Self::SandboxServiceManager(sandbox_service_manager) => {
                Some(sandbox_service_manager.clone())
            }
            Self::SandboxCatalog(_) => None,
            #[cfg(test)]
            Self::RuntimeServiceRegistry(_) => None,
        }
    }

    fn into_runtime_service_registry(
        self,
        system_state_service: Arc<Service>,
    ) -> Arc<dyn RuntimeServiceRegistry> {
        match self {
            Self::SandboxCatalog(sandbox_catalog) => {
                Arc::new(SandboxCatalogRuntimeServiceRegistry::new(sandbox_catalog))
            }
            Self::SandboxServiceManager(sandbox_service_manager) => {
                sandbox_service_manager.attach_system_state_service(system_state_service);
                sandbox_service_manager
            }
            #[cfg(test)]
            Self::RuntimeServiceRegistry(runtime_service_registry) => runtime_service_registry,
        }
    }
}

pub(crate) struct RouterBuildConfig {
    service: Arc<Service>,
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
    listen_addr: Option<SocketAddr>,
    server_shutdown: Option<watch::Sender<bool>>,
}

impl RouterBuildConfig {
    pub(crate) fn core(service: Arc<Service>) -> Self {
        Self {
            service,
            convex_registry: None,
            system_convex_registry: None,
            application_auth_verifier: None,
            cloud_functions_registry: None,
            firebase_config: None,
            license_state: LicenseState::community(),
            runtime_service_source: RuntimeServiceSource::SandboxCatalog(Arc::new(
                EmptySandboxCatalog,
            )),
            machine_lifecycle_manager: None,
            deploy_admin_token: std::env::var("NIMBUS_DEPLOY_TOKEN").ok(),
            local_server_security: None,
            listen_addr: None,
            server_shutdown: None,
        }
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

    pub(crate) fn with_sandbox_catalog(mut self, sandbox_catalog: Arc<dyn SandboxCatalog>) -> Self {
        self.runtime_service_source = RuntimeServiceSource::SandboxCatalog(sandbox_catalog);
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

    pub(crate) fn with_sandbox_service_manager(
        mut self,
        sandbox_service_manager: Arc<SandboxServiceManager>,
    ) -> Self {
        self.runtime_service_source =
            RuntimeServiceSource::SandboxServiceManager(sandbox_service_manager);
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
        crate::system_tenant::prepare_system_tenant_async(&self.service, self.listen_addr).await?;
        if let Some(registry) = self.convex_registry.as_ref() {
            crate::system_tenant::record_convex_deployment_state_async(
                &self.service,
                &registry.deploy_summary(),
                "startup",
            )
            .await?;
        }
        let Some(listen_addr) = self.listen_addr else {
            return Ok(());
        };
        let version = env!("CARGO_PKG_VERSION");
        if self.convex_registry.is_some() || self.system_convex_registry.is_some() {
            crate::system_tenant::record_listener_state_async(
                &self.service,
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
            crate::system_tenant::record_listener_state_async(
                &self.service,
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
            crate::system_tenant::record_listener_state_async(
                &self.service,
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

    #[cfg(test)]
    pub(crate) fn with_runtime_service_registry(
        mut self,
        runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
    ) -> Self {
        self.runtime_service_source =
            RuntimeServiceSource::RuntimeServiceRegistry(runtime_service_registry);
        self
    }

    pub(crate) fn build(self) -> Router {
        let service = self.service.clone();
        crate::system_tenant::install_table_projection_observer(&service);
        let sandbox_service_manager = self.runtime_service_source.sandbox_service_manager();
        let version_check = build_version_check();
        let state = Arc::new(AppState::from_config(AppStateConfig {
            service: self.service,
            convex_registry: self.convex_registry,
            system_convex_registry: self.system_convex_registry,
            application_auth_verifier: self.application_auth_verifier,
            cloud_functions_registry: self.cloud_functions_registry,
            firebase_config: self.firebase_config,
            license_state: self.license_state,
            runtime_service_registry: self
                .runtime_service_source
                .into_runtime_service_registry(service),
            sandbox_service_manager,
            machine_lifecycle_manager: self.machine_lifecycle_manager,
            deploy_admin_token: self.deploy_admin_token,
            local_server_security: self.local_server_security,
            listen_addr: self.listen_addr,
            server_shutdown: self.server_shutdown,
            version_check,
        }));
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
            .merge(build_convex_router());
        if firebase_enabled {
            router = router.merge(build_firebase_router(state.clone()));
        }
        if deployment.cloud_functions_registry().is_some() {
            router = router.fallback(any(cloud_functions::http_handler));
        }
        router
            .layer(build_cors_layer())
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

/// Builds the Nimbus HTTP/WebSocket router without Convex support.
pub fn build_router(service: Arc<Service>) -> Router {
    RouterBuildConfig::core(service).build()
}

/// Builds the Nimbus HTTP/WebSocket router without Convex support and with an explicit sandbox catalog.
pub fn build_router_with_sandbox_catalog(
    service: Arc<Service>,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_sandbox_catalog(sandbox_catalog)
        .build()
}

/// Builds the Nimbus HTTP/WebSocket router with an explicit license state.
pub fn build_router_with_license(service: Arc<Service>, license_state: LicenseState) -> Router {
    RouterBuildConfig::core(service)
        .with_license(license_state)
        .build()
}

/// Builds the Nimbus HTTP/WebSocket router with an explicit license state and sandbox catalog.
pub fn build_router_with_license_and_sandbox_catalog(
    service: Arc<Service>,
    license_state: LicenseState,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_license(license_state)
        .with_sandbox_catalog(sandbox_catalog)
        .build()
}

/// Builds the Nimbus HTTP/WebSocket router with Convex support enabled.
pub fn build_router_with_convex(service: Arc<Service>, convex_registry: ConvexRegistry) -> Router {
    RouterBuildConfig::core(service)
        .with_application_auth_verifier(convex_application_auth_verifier(&convex_registry))
        .with_convex(convex_registry)
        .build()
}

/// Builds the Nimbus HTTP/WebSocket router with Firebase REST support enabled.
pub fn build_router_with_firebase(
    service: Arc<Service>,
    firebase_config: FirebaseConfig,
) -> Router {
    RouterBuildConfig::core(service)
        .with_firebase(firebase_config)
        .build()
}

/// Builds the Nimbus HTTP/WebSocket router with Convex support enabled and with an explicit sandbox catalog.
pub fn build_router_with_convex_and_sandbox_catalog(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_application_auth_verifier(convex_application_auth_verifier(&convex_registry))
        .with_convex(convex_registry)
        .with_sandbox_catalog(sandbox_catalog)
        .build()
}

/// Builds the Nimbus HTTP/WebSocket router with Convex support enabled and a
/// server-owned sandbox service manager capable of start-on-first-reference activation.
pub fn build_router_with_convex_and_sandbox_service_manager(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    sandbox_service_manager: Arc<SandboxServiceManager>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_application_auth_verifier(convex_application_auth_verifier(&convex_registry))
        .with_convex(convex_registry)
        .with_sandbox_service_manager(sandbox_service_manager)
        .build()
}

/// Builds the Nimbus HTTP/WebSocket router with Convex support and an explicit license state.
pub fn build_router_with_convex_and_license(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
) -> Router {
    RouterBuildConfig::core(service)
        .with_application_auth_verifier(convex_application_auth_verifier(&convex_registry))
        .with_convex(convex_registry)
        .with_license(license_state)
        .build()
}

/// Builds the Nimbus HTTP/WebSocket router with Convex support, license state,
/// and a server-owned sandbox service manager.
pub fn build_router_with_convex_and_license_and_sandbox_service_manager(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
    sandbox_service_manager: Arc<SandboxServiceManager>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_application_auth_verifier(convex_application_auth_verifier(&convex_registry))
        .with_convex(convex_registry)
        .with_license(license_state)
        .with_sandbox_service_manager(sandbox_service_manager)
        .build()
}

/// Builds the Nimbus HTTP/WebSocket router with Convex support, license state, and sandbox catalog.
pub fn build_router_with_convex_and_license_and_sandbox_catalog(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_application_auth_verifier(convex_application_auth_verifier(&convex_registry))
        .with_convex(convex_registry)
        .with_license(license_state)
        .with_sandbox_catalog(sandbox_catalog)
        .build()
}

#[cfg(test)]
pub(crate) fn build_router_with_convex_and_runtime_service_registry(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    runtime_service_registry: Arc<dyn RuntimeServiceRegistry>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_application_auth_verifier(convex_application_auth_verifier(&convex_registry))
        .with_convex(convex_registry)
        .with_runtime_service_registry(runtime_service_registry)
        .build()
}

fn build_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _request_head| {
            is_allowed_local_cors_origin(origin)
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

fn is_allowed_local_cors_origin(origin: &HeaderValue) -> bool {
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
        .route("/ui/auth/session", post(http::create_ui_session))
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
    Router::new()
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
