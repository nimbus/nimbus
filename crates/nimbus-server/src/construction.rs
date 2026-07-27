use std::sync::Arc;

use nimbus_engine::Engine;
use nimbus_runtime::{
    EffectiveRuntimeScalingPlan, RuntimeAdaptiveControllerSettings, RuntimeHostResourceBudget,
    RuntimeLimits, RuntimeScalingPlanSet,
};

use crate::adapters::cloud_functions::{CloudFunctionsHttpTenantBinding, CloudFunctionsRegistry};
use crate::adapters::cloudflare::CloudflareConfig;
use crate::adapters::convex::{ConvexRegistry, ConvexTenancyConfig};
use crate::adapters::dynamodb::DynamoDbConfig;
use crate::adapters::firebase::FirebaseConfig;
use crate::adapters::mongodb::MongoDbConfig;
use crate::adapters::s3::S3Config;
use crate::adapters::wire::WireProtocolAdapter;
use crate::license::LicenseState;
use crate::listener_lease::{
    ActiveServerListenerLease, LeasedServerListener, PreparedServerListener,
    RecordedListenerBindFailure, ServerListenerLeaseAuthority,
    abandon_prepared_after_guard_failure,
};
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
    listener_leases: ServerListenerLeaseAuthority,
}

impl ServeOptions {
    pub fn new(engine: Arc<Engine>) -> Self {
        Self::from_router_options(RouterOptions::new(engine))
    }

    pub fn from_router_options(router_options: RouterOptions) -> Self {
        let listener_leases =
            ServerListenerLeaseAuthority::new(router_options.engine().data_dir().to_path_buf());
        Self {
            router_options,
            wire_adapters: Vec::new(),
            tls_config: None,
            listener_leases,
        }
    }

    /// Use this node-local root for durable listener leases.
    ///
    /// The default is the engine data directory. Composition roots that share
    /// one host-global network authority with other providers may override it.
    pub fn with_network_state_root(mut self, state_root: impl Into<std::path::PathBuf>) -> Self {
        self.listener_leases = self.listener_leases.with_state_root(state_root);
        self
    }

    /// Reserve and claim the main TCP listener before the caller-owned bind.
    pub fn prepare_main_listener(
        &self,
        requested_addr: std::net::SocketAddr,
    ) -> std::io::Result<PreparedServerListener> {
        self.listener_leases.prepare_main(requested_addr)
    }

    /// Adopt a listener supplied by systemd, an embedder, or another owner.
    pub fn adopt_external_main_listener(
        &self,
        listener: tokio::net::TcpListener,
    ) -> std::io::Result<LeasedServerListener> {
        self.listener_leases.adopt_external_main(listener)
    }

    fn with_router_options(mut self, update: impl FnOnce(RouterOptions) -> RouterOptions) -> Self {
        self.router_options = update(self.router_options);
        self
    }

    pub fn with_convex_registry(self, convex_registry: ConvexRegistry) -> Self {
        self.with_router_options(|options| options.with_convex_registry(convex_registry))
    }

    pub fn with_convex_registry_for_silo(
        self,
        silo: &nimbus_core::TenantId,
        convex_registry: ConvexRegistry,
    ) -> Self {
        self.with_router_options(|options| {
            options.with_convex_registry_for_silo(silo, convex_registry)
        })
    }

    pub fn with_convex_silo_auth(self, convex_silo_auth: crate::ConvexSiloAuthRegistry) -> Self {
        self.with_router_options(|options| options.with_convex_silo_auth(convex_silo_auth))
    }

    pub fn with_convex_tenancy(self, convex_tenancy: ConvexTenancyConfig) -> Self {
        self.with_router_options(|options| options.with_convex_tenancy(convex_tenancy))
    }

    pub fn with_system_convex_registry(self, system_convex_registry: ConvexRegistry) -> Self {
        self.with_router_options(|options| {
            options.with_system_convex_registry(system_convex_registry)
        })
    }

    pub fn with_cloud_functions_registry(
        self,
        cloud_functions_registry: CloudFunctionsRegistry,
    ) -> Self {
        self.with_router_options(|options| {
            options.with_cloud_functions_registry(cloud_functions_registry)
        })
    }

    pub fn with_cloud_functions_http_tenant(
        self,
        binding: CloudFunctionsHttpTenantBinding,
    ) -> Self {
        self.with_router_options(|options| options.with_cloud_functions_http_tenant(binding))
    }

    pub fn with_firebase_config(self, firebase_config: FirebaseConfig) -> Self {
        self.with_router_options(|options| options.with_firebase_config(firebase_config))
    }

    pub fn with_cloudflare(self, cloudflare_config: CloudflareConfig) -> Self {
        self.with_router_options(|options| options.with_cloudflare_config(cloudflare_config))
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

    /// Register a sibling S3 HTTP listener. Each call adds a listener;
    /// call at most once per adapter.
    pub fn with_s3(mut self, s3_config: S3Config) -> Self {
        self.wire_adapters.push(Box::new(s3_config));
        self
    }

    pub fn with_license(self, license_state: LicenseState) -> Self {
        self.with_router_options(|options| options.with_license(license_state))
    }

    pub fn with_service_instance_catalog(
        self,
        service_instances: Arc<dyn ServiceInstanceCatalog>,
    ) -> Self {
        self.with_router_options(|options| options.with_service_instance_catalog(service_instances))
    }

    pub fn with_service_manager(self, service_manager: Arc<ServiceManager>) -> Self {
        self.with_router_options(|options| options.with_service_manager(service_manager))
    }

    pub fn with_machine_lifecycle_manager(
        self,
        machine_lifecycle_manager: Arc<dyn MachineLifecycleManager>,
    ) -> Self {
        self.with_router_options(|options| {
            options.with_machine_lifecycle_manager(machine_lifecycle_manager)
        })
    }

    pub fn with_deploy_admin_token(self, token: impl Into<String>) -> Self {
        self.with_router_options(|options| options.with_deploy_admin_token(token))
    }

    pub fn with_local_server_security(
        self,
        local_server_security: Arc<LocalServerSecurityState>,
    ) -> Self {
        self.with_router_options(|options| {
            options.with_local_server_security(local_server_security)
        })
    }

    pub fn with_tenant_isolation_mode(self, mode: TenantIsolationMode) -> Self {
        self.with_router_options(|options| options.with_tenant_isolation_mode(mode))
    }

    /// Allow additional exact browser origins through the CORS layer
    /// (loopback origins are always allowed). See
    /// [`crate::normalize_cors_origin`] for the accepted form.
    pub fn with_cors_allowed_origins(self, origins: Vec<String>) -> Self {
        self.with_router_options(|options| options.with_cors_allowed_origins(origins))
    }

    /// Set the aggregate host CPU budget available to in-process runtime work.
    /// Tenant quotas remain separate; this is the node-allocatable-style host
    /// guard that later runtime admission consumes.
    pub fn with_runtime_host_resource_budget(self, budget: RuntimeHostResourceBudget) -> Self {
        self.with_router_options(|options| options.with_runtime_host_resource_budget(budget))
    }

    pub fn with_runtime_limits(self, limits: RuntimeLimits) -> Self {
        self.with_router_options(|options| options.with_runtime_limits(limits))
    }

    pub fn with_runtime_adaptive_controller_settings(
        self,
        settings: RuntimeAdaptiveControllerSettings,
    ) -> Self {
        self.with_router_options(|options| {
            options.with_runtime_adaptive_controller_settings(settings)
        })
    }

    pub fn with_effective_runtime_scaling_plan(self, plan: EffectiveRuntimeScalingPlan) -> Self {
        self.with_router_options(|options| options.with_effective_runtime_scaling_plan(plan))
    }

    pub fn with_effective_runtime_scaling_plans(self, plans: RuntimeScalingPlanSet) -> Self {
        self.with_router_options(|options| options.with_effective_runtime_scaling_plans(plans))
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

/// Runs the Nimbus HTTP/WebSocket server on an externally supplied listener.
///
/// Embedders that own the socket effect may use this convenience entry point;
/// it adopts the already-bound socket with external provenance before serving.
/// Nimbus-owned bind paths should call [`ServeOptions::prepare_main_listener`]
/// before their effect and then use [`serve_leased`].
pub async fn serve(
    listener: tokio::net::TcpListener,
    options: ServeOptions,
) -> std::io::Result<()> {
    let listener = options.adopt_external_main_listener(listener)?;
    serve_leased(listener, options).await
}

/// Runs Nimbus only after the main listener has Active durable lease authority.
pub async fn serve_leased(
    listener: LeasedServerListener,
    options: ServeOptions,
) -> std::io::Result<()> {
    if !options.listener_leases.owns(listener.owner_incarnation()) {
        return match listener.close_and_settle() {
            Ok(()) => Err(std::io::Error::other(
                "leased main listener belongs to a different ServeOptions incarnation",
            )),
            Err(cleanup_error) => Err(std::io::Error::other(format!(
                "leased main listener belongs to a different ServeOptions incarnation; \
                 failed to settle its lease after the confirmed local socket close: \
                 {cleanup_error}"
            ))),
        };
    }
    let (listener, main_lease, _) = listener.into_parts();
    let ServeOptions {
        mut router_options,
        wire_adapters,
        tls_config,
        listener_leases,
    } = options;
    let mut main_listener = Some(listener);
    let mut adapter_handles: Vec<tokio::task::JoinHandle<()>> = Vec::new();
    let mut adapter_leases: Vec<ActiveServerListenerLease> = Vec::new();

    let mut result = async {
        let engine = router_options.engine();
        if !router_options.has_system_convex_registry() {
            router_options =
                router_options.with_system_convex_registry(load_default_system_convex_registry()?);
        }
        let config = router_options.into_build_config();

        // Sibling adapter listeners share the same `Arc<Engine>`. Keep every
        // task and lease outside this fallible setup block so all synchronous
        // returns converge through one confirmed-close cleanup path.
        for (ordinal, adapter) in wire_adapters.into_iter().enumerate() {
            let requested_addr = adapter.bind_addr();
            let prepared =
                listener_leases.prepare_sibling(ordinal, adapter.name(), requested_addr)?;
            let adapter_listener = match tokio::net::TcpListener::bind(requested_addr).await {
                Ok(listener) => listener,
                Err(error) => {
                    return Err(bind_failure_error(prepared.record_bind_failure(error)));
                }
            };
            let adapter_addr = match adapter_listener.local_addr() {
                Ok(addr) => addr,
                Err(error) => {
                    return match abandon_prepared_after_guard_failure(prepared, adapter_listener) {
                        Ok(()) => Err(error),
                        Err(cleanup_error) => append_cleanup_error(
                            Err(error),
                            "failed to settle the claimed sibling after its bound address \
                             could not be observed",
                            cleanup_error,
                        ),
                    };
                }
            };
            // Fail closed: the adapter's guard refuses unsafe bind shapes
            // before the listener serves a single byte.
            if let Err(guard_error) = adapter.guard(adapter_addr) {
                return match abandon_prepared_after_guard_failure(prepared, adapter_listener) {
                    Ok(()) => Err(guard_error),
                    Err(cleanup_error) => append_cleanup_error(
                        Err(guard_error),
                        "failed to settle the claimed listener after its guard refused the bind",
                        cleanup_error,
                    ),
                };
            }
            let leased_listener = prepared.adopt(adapter_listener)?;
            let (adapter_listener, adapter_lease, listener_owner) = leased_listener.into_parts();
            debug_assert!(
                listener_leases.owns(listener_owner.as_ref()),
                "sibling listener must belong to the serving incarnation"
            );
            if let Err(error) = crate::system_tenant::record_listener_state_async(
                &engine,
                adapter.name(),
                adapter.protocol(),
                &adapter_addr.to_string(),
                "listening",
                Some(env!("CARGO_PKG_VERSION")),
                None,
            )
            .await
            {
                drop(adapter_listener);
                let mut result = Err(std::io::Error::other(error.to_string()));
                if let Err(cleanup_error) = adapter_lease.settle_after_confirmed_local_close() {
                    result = append_cleanup_error(
                        result,
                        "failed to settle the sibling lease after its projection failed",
                        cleanup_error,
                    );
                }
                return result;
            }
            adapter_handles.extend(adapter.spawn(adapter_listener, Arc::clone(&engine)));
            adapter_leases.push(adapter_lease);
        }

        let listener = main_listener
            .take()
            .expect("the main listener must be consumed exactly once");
        serve_with_router_config(listener, config, tls_config).await
    }
    .await;

    let main_was_served = main_listener.is_none();
    // A synchronous setup error leaves the main socket here. Dropping it
    // proves local closure before the lease is settled below.
    drop(main_listener.take());

    if main_was_served {
        // Abort every sibling before awaiting any one cancellation. A
        // cancellation-resistant first task must not delay closure of later
        // listeners.
        for handle in &adapter_handles {
            handle.abort();
        }
        for handle in adapter_handles {
            let _ = handle.await;
        }
        for lease in adapter_leases {
            if let Err(error) = lease.settle_after_confirmed_local_close() {
                result = append_cleanup_error(
                    result,
                    "failed to settle a sibling listener lease after confirmed task closure",
                    error,
                );
            }
        }
    } else {
        // NNC7.1a owns atomic preparation/unwind of siblings that started
        // before a later synchronous setup failure. Dropping these handles
        // deliberately preserves the pre-existing detached-task behavior and
        // their Active durable fences; NNC3.5 must not manufacture provider
        // absence or release those leases.
        drop(adapter_handles);
        drop(adapter_leases);
    }
    if let Err(error) = main_lease.settle_after_confirmed_local_close() {
        result = append_cleanup_error(
            result,
            "failed to settle the main listener lease after confirmed local closure",
            error,
        );
    }
    result
}

fn bind_failure_error(
    result: Result<RecordedListenerBindFailure, std::io::Error>,
) -> std::io::Error {
    match result {
        Ok(recorded) => recorded.into_error(),
        Err(error) => error,
    }
}

fn append_cleanup_error(
    result: std::io::Result<()>,
    context: &str,
    cleanup_error: std::io::Error,
) -> std::io::Result<()> {
    match result {
        Ok(()) => Err(std::io::Error::new(
            cleanup_error.kind(),
            format!("{context}: {cleanup_error}"),
        )),
        Err(primary) => Err(std::io::Error::new(
            primary.kind(),
            format!("{primary}; {context}: {cleanup_error}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nimbus_network::{LocalPortLeaseAuthority, PortLeasePhase};
    use nimbus_testing::EngineFixture;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::task::AbortHandle;

    use super::*;

    struct ProbeAdapter {
        bound_addr: Arc<Mutex<Option<SocketAddr>>>,
        abort_handle: Arc<Mutex<Option<AbortHandle>>>,
    }

    impl WireProtocolAdapter for ProbeAdapter {
        fn name(&self) -> &'static str {
            "nnc0-7-probe"
        }

        fn protocol(&self) -> &'static str {
            "tcp"
        }

        fn bind_addr(&self) -> SocketAddr {
            "127.0.0.1:0".parse().expect("probe address should parse")
        }

        fn guard(&self, addr: SocketAddr) -> std::io::Result<()> {
            *self
                .bound_addr
                .lock()
                .expect("probe address lock should remain healthy") = Some(addr);
            Ok(())
        }

        fn spawn(
            self: Box<Self>,
            listener: tokio::net::TcpListener,
            _engine: Arc<Engine>,
        ) -> Vec<tokio::task::JoinHandle<()>> {
            let task = tokio::spawn(async move {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let _ = stream.write_all(b"still-live").await;
                }
            });
            *self
                .abort_handle
                .lock()
                .expect("abort handle lock should remain healthy") = Some(task.abort_handle());
            vec![task]
        }
    }

    struct OccupiedAdapter {
        addr: SocketAddr,
    }

    impl WireProtocolAdapter for OccupiedAdapter {
        fn name(&self) -> &'static str {
            "nnc0-7-occupied"
        }

        fn protocol(&self) -> &'static str {
            "tcp"
        }

        fn bind_addr(&self) -> SocketAddr {
            self.addr
        }

        fn guard(&self, _addr: SocketAddr) -> std::io::Result<()> {
            unreachable!("the occupied address must fail before guard")
        }

        fn spawn(
            self: Box<Self>,
            _listener: tokio::net::TcpListener,
            _engine: Arc<Engine>,
        ) -> Vec<tokio::task::JoinHandle<()>> {
            unreachable!("the occupied address must fail before spawn")
        }
    }

    struct LeaseAwareAdapter {
        state_root: PathBuf,
        bound_addr: Arc<Mutex<Option<SocketAddr>>>,
        claim_observed: Arc<AtomicBool>,
    }

    impl WireProtocolAdapter for LeaseAwareAdapter {
        fn name(&self) -> &'static str {
            "nnc3-5-lease-aware"
        }

        fn protocol(&self) -> &'static str {
            "tcp"
        }

        fn bind_addr(&self) -> SocketAddr {
            "127.0.0.1:0".parse().expect("probe address should parse")
        }

        fn guard(&self, addr: SocketAddr) -> std::io::Result<()> {
            *self
                .bound_addr
                .lock()
                .expect("probe address lock should remain healthy") = Some(addr);
            let claim_observed = LocalPortLeaseAuthority::open(&self.state_root)
                .and_then(|authority| authority.list())
                .is_ok_and(|records| {
                    records.iter().any(|record| {
                        record.phase() == PortLeasePhase::Reserved && record.bind_claim().is_some()
                    })
                });
            self.claim_observed.store(claim_observed, Ordering::Release);
            if claim_observed {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "sibling bind reached its guard without a claimed port lease",
                ))
            }
        }

        fn spawn(
            self: Box<Self>,
            listener: tokio::net::TcpListener,
            _engine: Arc<Engine>,
        ) -> Vec<tokio::task::JoinHandle<()>> {
            vec![tokio::spawn(async move {
                if let Ok((mut stream, _)) = listener.accept().await {
                    let _ = stream.write_all(b"lease-owned").await;
                }
            })]
        }
    }

    fn active_lease_matches_addr(state_root: &Path, addr: SocketAddr) -> bool {
        LocalPortLeaseAuthority::open(state_root)
            .and_then(|authority| authority.list())
            .is_ok_and(|records| {
                records.iter().any(|record| {
                    record.phase() == PortLeasePhase::Active
                        && record
                            .binding()
                            .is_some_and(|binding| binding.actual_port().get() == addr.port())
                })
            })
    }

    #[tokio::test]
    async fn nnc3_5_main_listener_is_active_in_port_authority_while_serving() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("main listener should bind");
        let addr = listener.local_addr().expect("main address should resolve");
        let task = tokio::spawn(serve(listener, ServeOptions::new(fixture.engine())));

        let mut active = false;
        for _ in 0..100 {
            if active_lease_matches_addr(fixture.data_dir(), addr) {
                active = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        task.abort();
        let _ = task.await;
        assert!(
            active,
            "NNC3.5: the main server accepted a listener without an Active durable port lease"
        );
    }

    #[tokio::test]
    async fn nnc3_5_mismatched_options_close_socket_and_release_lease() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let preparing_options = ServeOptions::new(fixture.engine());
        let prepared = preparing_options
            .prepare_main_listener("127.0.0.1:0".parse().expect("fixture address should parse"))
            .expect("main listener should reserve");
        let raw = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("main listener should bind");
        let leased = prepared.adopt(raw).expect("main listener should activate");
        let serving_options = ServeOptions::new(fixture.engine());

        let error = serve_leased(leased, serving_options)
            .await
            .expect_err("another ServeOptions incarnation must not consume the lease");
        assert!(
            error
                .to_string()
                .contains("different ServeOptions incarnation")
        );
        let records = LocalPortLeaseAuthority::open(fixture.data_dir())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase(), PortLeasePhase::Released);
    }

    #[tokio::test]
    async fn nnc3_5_synchronous_sibling_failure_closes_and_releases_owned_main_listener() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let mut options = ServeOptions::new(fixture.engine());
        let prepared = options
            .prepare_main_listener("127.0.0.1:0".parse().expect("fixture address should parse"))
            .expect("main listener should reserve");
        let raw = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("main listener should bind");
        let main_addr = raw.local_addr().expect("main address should resolve");
        let leased = prepared.adopt(raw).expect("main listener should activate");
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("external sibling owner should bind");
        let occupied_addr = occupied
            .local_addr()
            .expect("occupied sibling address should resolve");
        options.wire_adapters.push(Box::new(OccupiedAdapter {
            addr: occupied_addr,
        }));

        let error = serve_leased(leased, options)
            .await
            .expect_err("occupied sibling must fail synchronous setup");
        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
        let records = LocalPortLeaseAuthority::open(fixture.data_dir())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        let main = records
            .iter()
            .find(|record| {
                record
                    .binding()
                    .is_some_and(|binding| binding.actual_port().get() == main_addr.port())
            })
            .expect("main lease should remain inspectable");
        assert_eq!(main.phase(), PortLeasePhase::Released);
        assert!(
            records.iter().any(|record| {
                record.phase() == PortLeasePhase::Failed
                    && record.failure().is_some_and(|failure| {
                        failure.kind() == nimbus_network::PortBindFailureKind::AddrInUse
                    })
            }),
            "the sibling collision must retain its durable no-effect evidence"
        );
        tokio::net::TcpListener::bind(main_addr)
            .await
            .expect("the synchronously failed start must close the owned main socket");
    }

    #[test]
    fn cleanup_error_is_aggregated_without_hiding_the_primary_failure() {
        let error = append_cleanup_error(
            Err(std::io::Error::new(
                std::io::ErrorKind::AddrInUse,
                "primary serve failure",
            )),
            "main lease cleanup",
            std::io::Error::other("durable cleanup failed"),
        )
        .expect_err("the primary result is already an error");
        assert_eq!(error.kind(), std::io::ErrorKind::AddrInUse);
        assert!(error.to_string().contains("primary serve failure"));
        assert!(error.to_string().contains("main lease cleanup"));
        assert!(error.to_string().contains("durable cleanup failed"));
    }

    #[tokio::test]
    async fn nnc3_5_sibling_bind_is_claimed_before_guard_and_serves_identical_bytes() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("main listener should bind");
        let bound_addr = Arc::new(Mutex::new(None));
        let claim_observed = Arc::new(AtomicBool::new(false));
        let mut options = ServeOptions::new(fixture.engine());
        options.wire_adapters.push(Box::new(LeaseAwareAdapter {
            state_root: fixture.data_dir().to_path_buf(),
            bound_addr: Arc::clone(&bound_addr),
            claim_observed: Arc::clone(&claim_observed),
        }));
        let task = tokio::spawn(serve(main_listener, options));

        let sibling_addr = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(addr) = *bound_addr
                    .lock()
                    .expect("probe address lock should remain healthy")
                {
                    break addr;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("sibling guard should run");
        let claimed_before_guard = claim_observed.load(Ordering::Acquire);
        let bytes = if claimed_before_guard {
            tokio::time::timeout(Duration::from_secs(1), async {
                let mut stream = tokio::net::TcpStream::connect(sibling_addr).await?;
                let mut bytes = [0_u8; 11];
                stream.read_exact(&mut bytes).await?;
                Ok::<_, std::io::Error>(bytes)
            })
            .await
            .ok()
            .and_then(Result::ok)
        } else {
            None
        };

        task.abort();
        let _ = task.await;
        assert!(
            claimed_before_guard,
            "NNC3.5: the sibling kernel bind reached its guard without an exact durable claim"
        );
        assert_eq!(
            bytes.as_ref().map(<[u8; 11]>::as_slice),
            Some(b"lease-owned".as_slice()),
            "the lease migration must preserve sibling protocol bytes"
        );
    }

    #[tokio::test]
    // This is the NNC0.7 fail-before executable baseline for NNCF17. A later
    // adapter bind fails after the first adapter owns a live socket/task.
    // NNC7.1a must turn it green with structured group preparation/unwind and
    // remove the ignore marker.
    #[ignore = "NNC0.7 expected red until partial sibling-listener startup unwinds and joins every earlier task"]
    async fn nnc0_7_kth_adapter_failure_must_not_leave_prior_listener_live() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("main listener should bind");
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("external owner should bind");
        let occupied_addr = occupied
            .local_addr()
            .expect("external owner address should resolve");
        let bound_addr = Arc::new(Mutex::new(None));
        let abort_handle = Arc::new(Mutex::new(None));
        let mut options = ServeOptions::new(fixture.engine());
        options.wire_adapters.push(Box::new(ProbeAdapter {
            bound_addr: Arc::clone(&bound_addr),
            abort_handle: Arc::clone(&abort_handle),
        }));
        options.wire_adapters.push(Box::new(OccupiedAdapter {
            addr: occupied_addr,
        }));

        let error = serve(main_listener, options)
            .await
            .expect_err("the kth occupied adapter must fail startup");
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::AddrInUse,
            "the baseline must reach the exact kth-adapter bind failure"
        );
        let first_addr = bound_addr
            .lock()
            .expect("probe address lock should remain healthy")
            .expect("the first adapter must bind and pass its guard");
        let survivor = tokio::time::timeout(Duration::from_secs(1), async {
            let mut stream = tokio::net::TcpStream::connect(first_addr).await?;
            let mut bytes = [0_u8; 10];
            stream.read_exact(&mut bytes).await?;
            Ok::<_, std::io::Error>(bytes)
        })
        .await;
        let prior_listener_served = matches!(survivor, Ok(Ok(bytes)) if &bytes == b"still-live");

        if let Some(handle) = abort_handle
            .lock()
            .expect("abort handle lock should remain healthy")
            .take()
        {
            handle.abort();
        }

        assert!(
            !prior_listener_served,
            "NNCF17: startup returned {error}, but the earlier sibling listener still \
             accepted and served bytes after the listener-group failure"
        );
    }
}
