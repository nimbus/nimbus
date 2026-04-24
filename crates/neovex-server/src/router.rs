use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::http::{HeaderName, HeaderValue, Method, header};
use axum::middleware;
use axum::routing::{any, delete, get, post};
use neovex_engine::Service;
use tower_http::cors::{AllowOrigin, CorsLayer};
use tower_http::services::ServeDir;

use crate::adapters::convex::{self, ConvexRegistry};
use crate::license::LicenseState;
use crate::local_server::{
    LocalServerAccessPolicy, LocalServerSecurityState, origin_allowlist_middleware,
    route_family_gate_middleware, server_access_extract_middleware,
};
use crate::sandbox::{EmptySandboxCatalog, SandboxCatalog};
use crate::service_manager::SandboxServiceManager;
use crate::service_registry::{RuntimeServiceRegistry, SandboxCatalogRuntimeServiceRegistry};
use crate::state::{AppState, AppStateConfig};
use crate::{http, ws};

const DEMOS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demos");

enum RuntimeServiceSource {
    SandboxCatalog(Arc<dyn SandboxCatalog>),
    SandboxServiceManager(Arc<SandboxServiceManager>),
    #[cfg(test)]
    RuntimeServiceRegistry(Arc<dyn RuntimeServiceRegistry>),
}

impl RuntimeServiceSource {
    fn into_runtime_service_registry(self) -> Arc<dyn RuntimeServiceRegistry> {
        match self {
            Self::SandboxCatalog(sandbox_catalog) => {
                Arc::new(SandboxCatalogRuntimeServiceRegistry::new(sandbox_catalog))
            }
            Self::SandboxServiceManager(sandbox_service_manager) => sandbox_service_manager,
            #[cfg(test)]
            Self::RuntimeServiceRegistry(runtime_service_registry) => runtime_service_registry,
        }
    }
}

pub(crate) struct RouterBuildConfig {
    service: Arc<Service>,
    convex_registry: Option<ConvexRegistry>,
    license_state: LicenseState,
    runtime_service_source: RuntimeServiceSource,
    deploy_admin_token: Option<String>,
    local_server_security: Option<Arc<LocalServerSecurityState>>,
    listen_addr: Option<SocketAddr>,
}

impl RouterBuildConfig {
    pub(crate) fn core(service: Arc<Service>) -> Self {
        Self {
            service,
            convex_registry: None,
            license_state: LicenseState::community(),
            runtime_service_source: RuntimeServiceSource::SandboxCatalog(Arc::new(
                EmptySandboxCatalog,
            )),
            deploy_admin_token: std::env::var("NEOVEX_DEPLOY_TOKEN").ok(),
            local_server_security: None,
            listen_addr: None,
        }
    }

    pub(crate) fn with_convex(mut self, convex_registry: ConvexRegistry) -> Self {
        self.convex_registry = Some(convex_registry);
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
        let state = Arc::new(AppState::from_config(AppStateConfig {
            service: self.service,
            convex_registry: self.convex_registry,
            license_state: self.license_state,
            runtime_service_registry: self.runtime_service_source.into_runtime_service_registry(),
            deploy_admin_token: self.deploy_admin_token,
            local_server_security: self.local_server_security,
            listen_addr: self.listen_addr,
        }));

        let local_admin_policy = LocalServerAccessPolicy::standard(state.clone());
        let deploy_admin_policy = LocalServerAccessPolicy::deploy(state.clone());

        build_public_router()
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
            .merge(build_convex_router())
            .layer(build_cors_layer())
            .layer(middleware::from_fn_with_state(
                state.clone(),
                origin_allowlist_middleware,
            ))
            .with_state(state)
    }
}

/// Builds the Neovex HTTP/WebSocket router without Convex support.
pub fn build_router(service: Arc<Service>) -> Router {
    RouterBuildConfig::core(service).build()
}

/// Builds the Neovex HTTP/WebSocket router without Convex support and with an explicit sandbox catalog.
pub fn build_router_with_sandbox_catalog(
    service: Arc<Service>,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_sandbox_catalog(sandbox_catalog)
        .build()
}

/// Builds the Neovex HTTP/WebSocket router with an explicit license state.
pub fn build_router_with_license(service: Arc<Service>, license_state: LicenseState) -> Router {
    RouterBuildConfig::core(service)
        .with_license(license_state)
        .build()
}

/// Builds the Neovex HTTP/WebSocket router with an explicit license state and sandbox catalog.
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

/// Builds the Neovex HTTP/WebSocket router with Convex support enabled.
pub fn build_router_with_convex(service: Arc<Service>, convex_registry: ConvexRegistry) -> Router {
    RouterBuildConfig::core(service)
        .with_convex(convex_registry)
        .build()
}

/// Builds the Neovex HTTP/WebSocket router with Convex support enabled and with an explicit sandbox catalog.
pub fn build_router_with_convex_and_sandbox_catalog(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_convex(convex_registry)
        .with_sandbox_catalog(sandbox_catalog)
        .build()
}

/// Builds the Neovex HTTP/WebSocket router with Convex support enabled and a
/// server-owned sandbox service manager capable of start-on-first-reference activation.
pub fn build_router_with_convex_and_sandbox_service_manager(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    sandbox_service_manager: Arc<SandboxServiceManager>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_convex(convex_registry)
        .with_sandbox_service_manager(sandbox_service_manager)
        .build()
}

/// Builds the Neovex HTTP/WebSocket router with Convex support and an explicit license state.
pub fn build_router_with_convex_and_license(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
) -> Router {
    RouterBuildConfig::core(service)
        .with_convex(convex_registry)
        .with_license(license_state)
        .build()
}

/// Builds the Neovex HTTP/WebSocket router with Convex support, license state,
/// and a server-owned sandbox service manager.
pub fn build_router_with_convex_and_license_and_sandbox_service_manager(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
    sandbox_service_manager: Arc<SandboxServiceManager>,
) -> Router {
    RouterBuildConfig::core(service)
        .with_convex(convex_registry)
        .with_license(license_state)
        .with_sandbox_service_manager(sandbox_service_manager)
        .build()
}

/// Builds the Neovex HTTP/WebSocket router with Convex support, license state, and sandbox catalog.
pub fn build_router_with_convex_and_license_and_sandbox_catalog(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> Router {
    RouterBuildConfig::core(service)
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
            HeaderName::from_static("x-neovex-admin-token"),
        ])
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
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
            "/api/admin/token/rotate",
            post(http::rotate_local_admin_token),
        )
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
