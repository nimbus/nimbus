//! Nimbus server crate.

mod adapters;
mod application_auth;
mod error_envelope;
mod execution;
mod http;
mod license;
mod local_server;
mod machine_lifecycle;
mod owned_tasks;
mod protocol;
mod provider_family;
mod router;
mod runtime_host;
mod sandbox;
mod service_manager;
mod service_registry;
mod state;
mod system_tenant;
mod ws;

use std::sync::Arc;

use nimbus_engine::Service;

pub use adapters::cloud_functions::CloudFunctionsRegistry;
pub use adapters::convex::ConvexRegistry;
pub use adapters::firebase::FirebaseConfig;
pub use adapters::mongodb::{AuthConfig as MongoDbAuthConfig, MongoDbConfig};
pub mod adapters_mongodb {
    pub use super::adapters::mongodb::bson_bridge;
    pub use super::adapters::mongodb::listener;
    pub use super::adapters::mongodb::wire;
}
pub use license::{
    LICENSE_FILE_ENV, LicenseDocument, LicenseEntitlements, LicenseKind, LicenseLoadError,
    LicenseSnapshot, LicenseSourceInfo, LicenseSourceKind, LicenseState, LicenseStatus,
    LicenseUsageSnapshot,
};
pub use local_server::{
    LOCAL_ADMIN_TOKEN_SCOPE, LocalAdminTokenRecord, LocalServerPaths, LocalServerPlatform,
    LocalServerSecurityState, SERVER_DISCOVERY_PROTOCOL_VERSIONS, ServerDiscoveryLease,
    ServerDiscoveryRecord, load_local_admin_token, load_or_create_local_admin_token,
    read_live_server_discovery, rotate_local_admin_token_offline,
};
pub use machine_lifecycle::{
    MachineCreateRequest, MachineLifecycleFuture, MachineLifecycleManager,
    MachineLifecycleSnapshot, MachineUpdateRequest,
};
use router::{RouterBuildConfig, convex_application_auth_verifier};
pub use router::{
    build_router, build_router_with_convex, build_router_with_convex_and_license,
    build_router_with_convex_and_license_and_sandbox_catalog,
    build_router_with_convex_and_license_and_sandbox_service_manager,
    build_router_with_convex_and_sandbox_catalog,
    build_router_with_convex_and_sandbox_service_manager, build_router_with_firebase,
    build_router_with_license, build_router_with_license_and_sandbox_catalog,
    build_router_with_sandbox_catalog,
};
pub use sandbox::{
    EmptySandboxCatalog, EmptySandboxServiceCatalog, SandboxCatalog, SandboxServiceCatalog,
    SandboxServiceLaunch,
};
pub use service_manager::SandboxServiceManager;

/// Optional server runtime surfaces layered on top of the core service.
pub struct ServeOptions {
    convex_registry: Option<ConvexRegistry>,
    system_convex_registry: Option<ConvexRegistry>,
    cloud_functions_registry: Option<CloudFunctionsRegistry>,
    firebase_config: Option<FirebaseConfig>,
    mongodb_config: Option<MongoDbConfig>,
    license_state: LicenseState,
    sandbox_catalog: Option<Arc<dyn SandboxCatalog>>,
    sandbox_service_manager: Option<Arc<SandboxServiceManager>>,
    machine_lifecycle_manager: Option<Arc<dyn MachineLifecycleManager>>,
    deploy_admin_token: Option<String>,
    local_server_security: Option<Arc<LocalServerSecurityState>>,
}

impl Default for ServeOptions {
    fn default() -> Self {
        Self {
            convex_registry: None,
            system_convex_registry: None,
            cloud_functions_registry: None,
            firebase_config: None,
            mongodb_config: None,
            license_state: LicenseState::community(),
            sandbox_catalog: None,
            sandbox_service_manager: None,
            machine_lifecycle_manager: None,
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

    pub fn with_mongodb(mut self, mongodb_config: MongoDbConfig) -> Self {
        self.mongodb_config = Some(mongodb_config);
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
}

async fn serve_with_router_config(
    listener: tokio::net::TcpListener,
    config: RouterBuildConfig,
) -> std::io::Result<()> {
    let listen_addr = listener.local_addr()?;
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let config = config
        .with_listen_addr(listen_addr)
        .with_server_shutdown(shutdown_tx);
    config
        .prepare_system_tenant()
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    axum::serve(listener, config.build())
        .with_graceful_shutdown(async move {
            while !*shutdown_rx.borrow() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
        })
        .await
}

/// Runs the Nimbus HTTP/WebSocket server on an existing listener.
pub async fn serve(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
) -> std::io::Result<()> {
    serve_with_options(listener, service, ServeOptions::default()).await
}

/// Runs the Nimbus HTTP/WebSocket server on an existing listener with an explicit license state.
pub async fn serve_with_license(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    license_state: LicenseState,
) -> std::io::Result<()> {
    serve_with_options(
        listener,
        service,
        ServeOptions::default().with_license(license_state),
    )
    .await
}

/// Runs the Nimbus HTTP/WebSocket server on an existing listener with an
/// explicit license state and sandbox catalog.
pub async fn serve_with_license_and_sandbox_catalog(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    license_state: LicenseState,
    sandbox_catalog: Arc<dyn SandboxCatalog>,
) -> std::io::Result<()> {
    serve_with_options(
        listener,
        service,
        ServeOptions::default()
            .with_license(license_state)
            .with_sandbox_catalog(sandbox_catalog),
    )
    .await
}

fn load_default_system_convex_registry() -> std::io::Result<ConvexRegistry> {
    ConvexRegistry::from_embedded_system_bundle()
        .map_err(|error| std::io::Error::other(error.to_string()))
}

fn apply_optional_system_convex_registry(
    config: RouterBuildConfig,
    system_convex_registry: Option<ConvexRegistry>,
) -> std::io::Result<RouterBuildConfig> {
    Ok(
        config.with_system_convex_registry(match system_convex_registry {
            Some(registry) => registry,
            None => load_default_system_convex_registry()?,
        }),
    )
}

/// Runs the Nimbus HTTP/WebSocket server on an existing listener with an
/// explicit option bundle.
pub async fn serve_with_options(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    options: ServeOptions,
) -> std::io::Result<()> {
    let mut config = apply_optional_system_convex_registry(
        RouterBuildConfig::core(Arc::clone(&service)).with_license(options.license_state),
        options.system_convex_registry,
    )?;
    if let Some(convex_registry) = options.convex_registry {
        config = config
            .with_application_auth_verifier(convex_application_auth_verifier(&convex_registry))
            .with_convex(convex_registry);
    }
    if let Some(cloud_functions_registry) = options.cloud_functions_registry {
        config = config.with_cloud_functions(cloud_functions_registry);
    }
    if let Some(firebase_config) = options.firebase_config {
        config = config.with_firebase(firebase_config);
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
    if let Some(machine_lifecycle_manager) = options.machine_lifecycle_manager {
        config = config.with_machine_lifecycle_manager(machine_lifecycle_manager);
    }

    if let Some(mongodb_config) = options.mongodb_config {
        let mongodb_listener = tokio::net::TcpListener::bind(mongodb_config.bind_addr).await?;
        let mongodb_addr = mongodb_listener.local_addr()?;
        crate::system_tenant::record_listener_state_async(
            &service,
            "mongodb",
            "tcp",
            &mongodb_addr.to_string(),
            "listening",
            Some(env!("CARGO_PKG_VERSION")),
            None,
        )
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
        let mongodb_service = Arc::clone(&service);
        let mongodb_auth = mongodb_config.auth;
        let mongodb_handle = tokio::spawn(async move {
            adapters::mongodb::listener::run_listener_with_auth(
                mongodb_listener,
                mongodb_service,
                mongodb_auth,
            )
            .await;
        });
        let http_result = serve_with_router_config(listener, config).await;
        mongodb_handle.abort();
        return http_result;
    }

    serve_with_router_config(listener, config).await
}

/// Runs the Nimbus HTTP/WebSocket server on an existing listener with Convex support.
pub async fn serve_with_convex(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
) -> std::io::Result<()> {
    serve_with_options(
        listener,
        service,
        ServeOptions::default().with_convex_registry(convex_registry),
    )
    .await
}

/// Runs the Nimbus HTTP/WebSocket server on an existing listener with Firebase REST support.
pub async fn serve_with_firebase(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    firebase_config: FirebaseConfig,
) -> std::io::Result<()> {
    serve_with_options(
        listener,
        service,
        ServeOptions::default().with_firebase_config(firebase_config),
    )
    .await
}

/// Runs the Nimbus HTTP/WebSocket server on an existing listener with Convex support and an explicit license state.
pub async fn serve_with_convex_and_license(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
) -> std::io::Result<()> {
    serve_with_options(
        listener,
        service,
        ServeOptions::default()
            .with_convex_registry(convex_registry)
            .with_license(license_state),
    )
    .await
}

/// Runs the Nimbus HTTP/WebSocket server on an existing listener with Convex
/// support, an explicit license state, and a server-owned sandbox service
/// manager.
pub async fn serve_with_convex_and_license_and_sandbox_service_manager(
    listener: tokio::net::TcpListener,
    service: Arc<Service>,
    convex_registry: ConvexRegistry,
    license_state: LicenseState,
    sandbox_service_manager: Arc<SandboxServiceManager>,
) -> std::io::Result<()> {
    serve_with_options(
        listener,
        service,
        ServeOptions::default()
            .with_convex_registry(convex_registry)
            .with_license(license_state)
            .with_sandbox_service_manager(sandbox_service_manager),
    )
    .await
}

#[cfg(test)]
mod tests;
