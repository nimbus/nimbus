//! Neovex server crate.

mod adapters;
mod http;
mod license;
mod protocol;
mod runtime;
mod state;
mod ws;

use std::sync::Arc;

use adapters::convex;
use axum::Router;
use axum::routing::{any, delete, get, post};
use neovex_engine::Service;
use state::AppState;
use tower_http::services::ServeDir;

const DEMOS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../demos");

pub use adapters::convex::ConvexRegistry;
pub use license::{
    DEFAULT_LICENSE_PATH, LICENSE_FILE_ENV, LicenseDocument, LicenseEntitlements, LicenseKind,
    LicenseLoadError, LicenseSnapshot, LicenseSourceInfo, LicenseSourceKind, LicenseState,
    LicenseStatus, LicenseUsageSnapshot,
};

/// Builds the Neovex HTTP/WebSocket router without Convex support.
pub fn build_router(service: Arc<Service>) -> Router {
    build_router_with_license(service, LicenseState::community())
}

/// Builds the Neovex HTTP/WebSocket router with an explicit license state.
pub fn build_router_with_license(service: Arc<Service>, license_state: LicenseState) -> Router {
    let state = Arc::new(AppState::with_license_state(service, license_state));
    build_core_router().with_state(state)
}

/// Builds the Neovex HTTP/WebSocket router with Convex support enabled.
pub fn build_router_with_convex(service: Arc<Service>, convex_registry: ConvexRegistry) -> Router {
    build_router_with_convex_and_license(service, convex_registry, LicenseState::community())
}

/// Builds the Neovex HTTP/WebSocket router with Convex support and an explicit license state.
pub fn build_router_with_convex_and_license(
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
) -> Router {
    let state = Arc::new(AppState::with_convex_registry_and_license_state(
        service,
        convex_registry,
        license_state,
    ));
    build_core_router()
        .merge(build_convex_router())
        .with_state(state)
}

fn build_core_router() -> Router<Arc<AppState>> {
    let demos = ServeDir::new(DEMOS_DIR).append_index_html_on_directories(true);

    Router::new()
        .route("/health", get(http::health))
        .route("/debug/license/status", get(http::license_status))
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
            "/api/tenants/{tenant_id}/commits",
            get(http::read_commit_log),
        )
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

/// Runs the Neovex HTTP/WebSocket server on an existing listener.
pub async fn serve(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
) -> std::io::Result<()> {
    axum::serve(listener, build_router(service)).await
}

/// Runs the Neovex HTTP/WebSocket server on an existing listener with an explicit license state.
pub async fn serve_with_license(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    license_state: LicenseState,
) -> std::io::Result<()> {
    axum::serve(listener, build_router_with_license(service, license_state)).await
}

/// Runs the Neovex HTTP/WebSocket server on an existing listener with Convex support.
pub async fn serve_with_convex(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
) -> std::io::Result<()> {
    serve_with_convex_and_license(
        listener,
        service,
        convex_registry,
        LicenseState::community(),
    )
    .await
}

/// Runs the Neovex HTTP/WebSocket server on an existing listener with Convex support and an explicit license state.
pub async fn serve_with_convex_and_license(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
) -> std::io::Result<()> {
    axum::serve(
        listener,
        build_router_with_convex_and_license(service, convex_registry, license_state),
    )
    .await
}

#[cfg(test)]
mod tests;
