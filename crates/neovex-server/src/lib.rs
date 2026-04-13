//! Neovex server crate.

mod adapters;
mod execution;
mod http;
mod license;
mod owned_tasks;
mod protocol;
mod router;
mod sandbox;
mod service_manager;
pub mod service_registry;
mod state;
mod ws;

use std::sync::Arc;

use neovex_engine::Service;

pub use adapters::convex::ConvexRegistry;
pub use license::{
    DEFAULT_LICENSE_PATH, LICENSE_FILE_ENV, LicenseDocument, LicenseEntitlements, LicenseKind,
    LicenseLoadError, LicenseSnapshot, LicenseSourceInfo, LicenseSourceKind, LicenseState,
    LicenseStatus, LicenseUsageSnapshot,
};
pub use router::{
    build_router, build_router_with_convex, build_router_with_convex_and_license,
    build_router_with_convex_and_license_and_sandbox_catalog,
    build_router_with_convex_and_license_and_sandbox_service_manager,
    build_router_with_convex_and_sandbox_catalog,
    build_router_with_convex_and_sandbox_service_manager, build_router_with_license,
    build_router_with_license_and_sandbox_catalog, build_router_with_sandbox_catalog,
};
pub use sandbox::{
    EmptySandboxCatalog, EmptySandboxServiceCatalog, SandboxCatalog, SandboxServiceCatalog,
    SandboxServiceLaunch,
};
pub use service_manager::SandboxServiceManager;

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

/// Runs the Neovex HTTP/WebSocket server on an existing listener with an
/// explicit license state and sandbox catalog.
pub async fn serve_with_license_and_sandbox_catalog(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    license_state: LicenseState,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> std::io::Result<()> {
    axum::serve(
        listener,
        build_router_with_license_and_sandbox_catalog(service, license_state, sandbox_catalog),
    )
    .await
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

/// Runs the Neovex HTTP/WebSocket server on an existing listener with Convex
/// support, an explicit license state, and a server-owned sandbox service
/// manager.
pub async fn serve_with_convex_and_license_and_sandbox_service_manager(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
    sandbox_service_manager: Arc<SandboxServiceManager>,
) -> std::io::Result<()> {
    axum::serve(
        listener,
        build_router_with_convex_and_license_and_sandbox_service_manager(
            service,
            convex_registry,
            license_state,
            sandbox_service_manager,
        ),
    )
    .await
}

#[cfg(test)]
mod tests;
