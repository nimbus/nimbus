use std::sync::Arc;

use nimbus_engine::Service;

use crate::adapters;
use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::convex::ConvexRegistry;
use crate::adapters::firebase::FirebaseConfig;
use crate::adapters::mongodb::MongoDbConfig;
use crate::license::LicenseState;
use crate::local_server::LocalServerSecurityState;
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::router::{RouterBuildConfig, RouterOptions};
use crate::sandbox::SandboxCatalog;
use crate::service_manager::SandboxServiceManager;
use crate::tenant::TenantIsolationMode;

/// Canonical public option bundle for serving Nimbus on a listener.
pub struct ServeOptions {
    router_options: RouterOptions,
    mongodb_config: Option<MongoDbConfig>,
}

impl ServeOptions {
    pub fn new(service: Arc<Service>) -> Self {
        Self::from_router_options(RouterOptions::new(service))
    }

    pub fn from_router_options(router_options: RouterOptions) -> Self {
        Self {
            router_options,
            mongodb_config: None,
        }
    }

    pub fn with_convex_registry(mut self, convex_registry: ConvexRegistry) -> Self {
        self.router_options = self.router_options.with_convex_registry(convex_registry);
        self
    }

    pub fn with_system_convex_registry(mut self, system_convex_registry: ConvexRegistry) -> Self {
        self.router_options = self
            .router_options
            .with_system_convex_registry(system_convex_registry);
        self
    }

    pub fn with_cloud_functions_registry(
        mut self,
        cloud_functions_registry: CloudFunctionsRegistry,
    ) -> Self {
        self.router_options = self
            .router_options
            .with_cloud_functions_registry(cloud_functions_registry);
        self
    }

    pub fn with_firebase_config(mut self, firebase_config: FirebaseConfig) -> Self {
        self.router_options = self.router_options.with_firebase_config(firebase_config);
        self
    }

    pub fn with_mongodb(mut self, mongodb_config: MongoDbConfig) -> Self {
        self.mongodb_config = Some(mongodb_config);
        self
    }

    pub fn with_license(mut self, license_state: LicenseState) -> Self {
        self.router_options = self.router_options.with_license(license_state);
        self
    }

    pub fn with_sandbox_catalog(mut self, sandbox_catalog: Arc<dyn SandboxCatalog>) -> Self {
        self.router_options = self.router_options.with_sandbox_catalog(sandbox_catalog);
        self
    }

    pub fn with_sandbox_service_manager(
        mut self,
        sandbox_service_manager: Arc<SandboxServiceManager>,
    ) -> Self {
        self.router_options = self
            .router_options
            .with_sandbox_service_manager(sandbox_service_manager);
        self
    }

    pub fn with_machine_lifecycle_manager(
        mut self,
        machine_lifecycle_manager: Arc<dyn MachineLifecycleManager>,
    ) -> Self {
        self.router_options = self
            .router_options
            .with_machine_lifecycle_manager(machine_lifecycle_manager);
        self
    }

    pub fn with_deploy_admin_token(mut self, token: impl Into<String>) -> Self {
        self.router_options = self.router_options.with_deploy_admin_token(token);
        self
    }

    pub fn with_local_server_security(
        mut self,
        local_server_security: Arc<LocalServerSecurityState>,
    ) -> Self {
        self.router_options = self
            .router_options
            .with_local_server_security(local_server_security);
        self
    }

    pub fn with_tenant_isolation_mode(mut self, mode: TenantIsolationMode) -> Self {
        self.router_options = self.router_options.with_tenant_isolation_mode(mode);
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

fn load_default_system_convex_registry() -> std::io::Result<ConvexRegistry> {
    ConvexRegistry::from_embedded_system_bundle()
        .map_err(|error| std::io::Error::other(error.to_string()))
}

/// Runs the Nimbus HTTP/WebSocket server on an existing listener.
pub async fn serve(
    listener: tokio::net::TcpListener,
    options: ServeOptions,
) -> std::io::Result<()> {
    let ServeOptions {
        mut router_options,
        mongodb_config,
    } = options;
    let service = router_options.service();
    if !router_options.has_system_convex_registry() {
        router_options =
            router_options.with_system_convex_registry(load_default_system_convex_registry()?);
    }
    let config = router_options.into_build_config();

    if let Some(mongodb_config) = mongodb_config {
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
