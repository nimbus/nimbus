use std::sync::Arc;

use axum::Router;
use axum::routing::{any, delete, get, post};
use neovex_engine::Service;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::adapters::convex::{self, ConvexRegistry};
use crate::license::LicenseState;
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
        let include_convex = self.convex_registry.is_some();
        let state = Arc::new(AppState::from_config(AppStateConfig {
            service: self.service,
            convex_registry: self.convex_registry,
            license_state: self.license_state,
            runtime_service_registry: self.runtime_service_source.into_runtime_service_registry(),
        }));

        let router = build_core_router();
        let router = if include_convex {
            router.merge(build_convex_router())
        } else {
            router
        };

        router.layer(build_cors_layer()).with_state(state)
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
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any)
}

fn build_core_router() -> Router<Arc<AppState>> {
    let demos = ServeDir::new(DEMOS_DIR).append_index_html_on_directories(true);

    Router::new()
        .route("/health", get(http::health))
        .route("/debug/license/status", get(http::license_status))
        .route("/debug/encryption/status", get(http::encryption_status))
        .route(
            "/debug/tenants/{tenant_id}/consistency",
            get(http::tenant_consistency_report),
        )
        .route(
            "/debug/tenants/{tenant_id}/engine/metrics",
            get(http::tenant_engine_diagnostics),
        )
        .route("/demos", get(http::demos_redirect))
        .nest_service("/demos/", demos)
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

fn build_convex_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/debug/runtime/metrics", get(http::runtime_diagnostics))
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
