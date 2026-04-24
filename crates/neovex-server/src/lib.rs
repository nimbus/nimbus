//! Neovex server crate.

mod adapters;
mod execution;
mod http;
mod license;
mod local_server;
mod owned_tasks;
mod protocol;
mod router;
mod sandbox;
mod service_manager;
mod service_registry;
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
pub use local_server::{
    LOCAL_ADMIN_TOKEN_SCOPE, LocalAdminTokenRecord, LocalServerPaths, LocalServerPlatform,
    LocalServerSecurityState, SERVER_DISCOVERY_PROTOCOL_VERSIONS, ServerDiscoveryLease,
    ServerDiscoveryRecord, load_local_admin_token, load_or_create_local_admin_token,
    read_live_server_discovery, rotate_local_admin_token_offline,
};
use router::RouterBuildConfig;
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

/// Optional server runtime surfaces layered on top of the core service.
pub struct ServeOptions {
    convex_registry: Option<ConvexRegistry>,
    license_state: LicenseState,
    sandbox_catalog: Option<Arc<dyn SandboxCatalog>>,
    sandbox_service_manager: Option<Arc<SandboxServiceManager>>,
    deploy_admin_token: Option<String>,
    local_server_security: Option<Arc<LocalServerSecurityState>>,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            convex_registry: None,
            license_state: LicenseState::community(),
            sandbox_catalog: None,
            sandbox_service_manager: None,
            deploy_admin_token: None,
            local_server_security: None,
        }
    }
}

impl ServeOptions {
    pub fn with_convex_registry(mut self, convex_registry: ConvexRegistry) -> Self {
        self.convex_registry = Some(convex_registry);
        self
    }

    pub fn with_license(mut self, license_state: LicenseState) -> Self {
        self.license_state = license_state;
        self
    }

    pub fn with_sandbox_catalog(mut self, sandbox_catalog: Arc<dyn SandboxCatalog>) -> Self {
        self.sandbox_catalog = Some(sandbox_catalog);
        self.sandbox_service_manager = None;
        self
    }

    pub fn with_sandbox_service_manager(
        mut self,
        sandbox_service_manager: Arc<SandboxServiceManager>,
    ) -> Self {
        self.sandbox_service_manager = Some(sandbox_service_manager);
        self.sandbox_catalog = None;
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
}

async fn serve_with_router_config(
    listener: tokio::net::TcpListener,
    config: RouterBuildConfig,
) -> std::io::Result<()> {
    let listen_addr = listener.local_addr()?;
    axum::serve(listener, config.with_listen_addr(listen_addr).build()).await
}

/// Runs the Neovex HTTP/WebSocket server on an existing listener.
pub async fn serve(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
) -> std::io::Result<()> {
    serve_with_router_config(listener, RouterBuildConfig::core(service)).await
}

/// Runs the Neovex HTTP/WebSocket server on an existing listener with an explicit license state.
pub async fn serve_with_license(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    license_state: LicenseState,
) -> std::io::Result<()> {
    serve_with_router_config(
        listener,
        RouterBuildConfig::core(service).with_license(license_state),
    )
    .await
}

/// Runs the Neovex HTTP/WebSocket server on an existing listener with an
/// explicit license state and sandbox catalog.
pub async fn serve_with_license_and_sandbox_catalog(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    license_state: LicenseState,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> std::io::Result<()> {
    serve_with_router_config(
        listener,
        RouterBuildConfig::core(service)
            .with_license(license_state)
            .with_sandbox_catalog(sandbox_catalog),
    )
    .await
}

/// Runs the Neovex HTTP/WebSocket server on an existing listener with an
/// explicit option bundle.
pub async fn serve_with_options(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    options: ServeOptions,
) -> std::io::Result<()> {
    let mut config = RouterBuildConfig::core(service).with_license(options.license_state);
    if let Some(convex_registry) = options.convex_registry {
        config = config.with_convex(convex_registry);
    }
    if let Some(deploy_admin_token) = options.deploy_admin_token {
        config = config.with_deploy_admin_token(deploy_admin_token);
    }
    if let Some(local_server_security) = options.local_server_security {
        config = config.with_local_server_security(local_server_security);
    }
    if let Some(sandbox_service_manager) = options.sandbox_service_manager {
        config = config.with_sandbox_service_manager(sandbox_service_manager);
    } else if let Some(sandbox_catalog) = options.sandbox_catalog {
        config = config.with_sandbox_catalog(sandbox_catalog);
    }
    serve_with_router_config(listener, config).await
}

/// Runs the Neovex HTTP/WebSocket server on an existing listener with Convex support.
pub async fn serve_with_convex(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
) -> std::io::Result<()> {
    serve_with_router_config(
        listener,
        RouterBuildConfig::core(service).with_convex(convex_registry),
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
    serve_with_router_config(
        listener,
        RouterBuildConfig::core(service)
            .with_convex(convex_registry)
            .with_license(license_state),
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
    serve_with_router_config(
        listener,
        RouterBuildConfig::core(service)
            .with_convex(convex_registry)
            .with_license(license_state)
            .with_sandbox_service_manager(sandbox_service_manager),
    )
    .await
}

#[cfg(test)]
mod tests;
