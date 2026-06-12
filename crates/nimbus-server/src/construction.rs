use std::sync::Arc;

use nimbus_engine::Engine;

use crate::adapters::cloud_functions::CloudFunctionsRegistry;
use crate::adapters::convex::ConvexRegistry;
use crate::adapters::dynamodb::DynamoDbConfig;
use crate::adapters::firebase::FirebaseConfig;
use crate::adapters::mongodb::MongoDbConfig;
use crate::adapters::wire::WireProtocolAdapter;
use crate::license::LicenseState;
use crate::local_server::LocalServerSecurityState;
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::router::{RouterBuildConfig, RouterOptions};
use crate::tenant::TenantIsolationMode;
use crate::tls::TlsConfig;
use nimbus_services::ServiceInstanceCatalog;
use nimbus_services::ServiceManager;

/// Canonical public option bundle for serving Nimbus on a listener.
pub struct ServeOptions {
    router_options: RouterOptions,
    wire_adapters: Vec<Box<dyn WireProtocolAdapter>>,
    tls_config: Option<TlsConfig>,
}

impl ServeOptions {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self::from_router_options(RouterOptions::new(engine))
    }

    pub fn from_router_options(router_options: RouterOptions) -> Self {
        Self {
            router_options,
            wire_adapters: Vec::new(),
            tls_config: None,
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

    /// Register a sibling MongoDB wire-protocol listener. Each call adds a
    /// listener; call at most once per adapter.
    pub fn with_mongodb(mut self, mongodb_config: MongoDbConfig) -> Self {
        self.wire_adapters.push(Box::new(mongodb_config));
        self
    }

    /// Register a sibling DynamoDB HTTP listener. Each call adds a listener;
    /// call at most once per adapter.
    pub fn with_dynamodb(mut self, dynamodb_config: DynamoDbConfig) -> Self {
        self.wire_adapters.push(Box::new(dynamodb_config));
        self
    }

    pub fn with_license(mut self, license_state: LicenseState) -> Self {
        self.router_options = self.router_options.with_license(license_state);
        self
    }

    pub fn with_service_instance_catalog(
        mut self,
        service_instances: Arc<dyn ServiceInstanceCatalog>,
    ) -> Self {
        self.router_options = self
            .router_options
            .with_service_instance_catalog(service_instances);
        self
    }

    pub fn with_service_manager(mut self, service_manager: Arc<ServiceManager>) -> Self {
        self.router_options = self.router_options.with_service_manager(service_manager);
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

    /// Allow additional exact browser origins through the CORS layer
    /// (loopback origins are always allowed). See
    /// [`crate::normalize_cors_origin`] for the accepted form.
    pub fn with_cors_allowed_origins(mut self, origins: Vec<String>) -> Self {
        self.router_options = self.router_options.with_cors_allowed_origins(origins);
        self
    }

    /// Terminate TLS on the main HTTP listener with this PEM pair. The
    /// pair is loaded and validated at startup; sibling adapter listeners
    /// stay plain TCP (see docs/private/decisions/adapter-listener-tls.md).
    pub fn with_tls(mut self, tls_config: TlsConfig) -> Self {
        self.tls_config = Some(tls_config);
        self
    }
}

async fn serve_with_router_config(
    listener: tokio::net::TcpListener,
    config: RouterBuildConfig,
    tls_config: Option<TlsConfig>,
) -> std::io::Result<()> {
    // Load and validate the TLS identity before any engine work so a bad
    // certificate fails the boot, not the first connection.
    let rustls_config = tls_config
        .as_ref()
        .map(crate::tls::load_rustls_server_config)
        .transpose()?;
    let listen_addr = listener.local_addr()?;
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::watch::channel(false);
    let config = config
        .with_listen_addr(listen_addr)
        .with_server_shutdown(shutdown_tx);
    config
        .prepare_system_tenant()
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    match rustls_config {
        Some(rustls_config) => {
            crate::tls::serve_tls(listener, config.build(), rustls_config, shutdown_rx).await
        }
        None => {
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
    }
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
        wire_adapters,
        tls_config,
    } = options;
    let engine = router_options.engine();
    if !router_options.has_system_convex_registry() {
        router_options =
            router_options.with_system_convex_registry(load_default_system_convex_registry()?);
    }
    let config = router_options.into_build_config();

    // Sibling adapter listeners share the same `Arc<Engine>`; collect their
    // task handles so the main HTTP server's return aborts every one of them.
    let mut adapter_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();

    for adapter in wire_adapters {
        let adapter_listener = tokio::net::TcpListener::bind(adapter.bind_addr()).await?;
        let adapter_addr = adapter_listener.local_addr()?;
        // Fail closed: the adapter's guard refuses unsafe bind shapes before
        // the listener serves a single byte.
        adapter.guard(adapter_addr)?;
        crate::system_tenant::record_listener_state_async(
            &engine,
            adapter.name(),
            adapter.protocol(),
            &adapter_addr.to_string(),
            "listening",
            Some(env!("CARGO_PKG_VERSION")),
            None,
        )
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
        adapter_handles.extend(adapter.spawn(adapter_listener, Arc::clone(&engine)));
    }

    let http_result = serve_with_router_config(listener, config, tls_config).await;
    for handle in adapter_handles {
        handle.abort();
    }
    http_result
}
