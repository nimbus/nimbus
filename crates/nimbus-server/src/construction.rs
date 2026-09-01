use std::sync::Arc;

use nimbus_engine::Engine;
use nimbus_network::{
    LocalNetworkAuthority, NetworkCondition, NetworkConditionKind, NetworkConditionState,
    NetworkResourceId, NetworkResourcePhase,
};
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
use crate::listener_group::{WireListenerGroup, append_cleanup_error};
use crate::listener_lease::{
    ActiveServerListenerEvidence, ExternalServerListenerContext, LeasedServerListener,
    PreboundServerListener, PreboundServerListeners, PreparedServerListener,
    RecordedListenerBindFailure, ServerListenerLeaseAuthority,
    abandon_prepared_after_guard_failure,
};
use crate::local_server::LocalServerSecurityState;
use crate::machine_lifecycle::MachineLifecycleManager;
use crate::router::{RouterBuildConfig, RouterOptions};
use crate::tenant::TenantIsolationMode;
use crate::tls::TlsConfig;
use crate::workload_boot::ServerWorkloadBootPlan;
use crate::workload_composition::ServerWorkloadComposition;
use nimbus_services::ServiceInstanceCatalog;
use tokio::sync::watch;

/// Cloneable authority for requesting an orderly server shutdown.
///
/// The server and its callers share this handle so local administration,
/// process signals, and embedders all enter the same graceful listener and
/// workload cleanup path.
#[derive(Clone, Debug)]
pub struct ServerShutdownHandle {
    sender: watch::Sender<bool>,
}

impl ServerShutdownHandle {
    fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self { sender }
    }

    /// Request shutdown. Repeated requests are idempotent.
    pub fn request_shutdown(&self) {
        self.sender.send_replace(true);
    }

    fn sender(&self) -> watch::Sender<bool> {
        self.sender.clone()
    }

    fn subscribe(&self) -> watch::Receiver<bool> {
        self.sender.subscribe()
    }
}

/// Canonical public option bundle for serving Nimbus on a listener.
pub struct ServeOptions {
    router_options: RouterOptions,
    wire_adapters: Vec<Box<dyn WireProtocolAdapter>>,
    tls_config: Option<TlsConfig>,
    listener_leases: ServerListenerLeaseAuthority,
    prebound_wire_listeners: Option<PreboundServerListeners>,
    server_shutdown: ServerShutdownHandle,
}

impl ServeOptions {
    /// Construct workload-capable server options from one complete realm.
    pub fn managed(composition: ServerWorkloadComposition) -> Self {
        let network_manager = composition.network_manager();
        let listener_leases = ServerListenerLeaseAuthority::new(network_manager.authority());
        Self {
            router_options: RouterOptions::managed(composition),
            wire_adapters: Vec::new(),
            tls_config: None,
            listener_leases,
            prebound_wire_listeners: None,
            server_shutdown: ServerShutdownHandle::new(),
        }
    }

    /// Construct an explicitly protocol-only server on an already-claimed
    /// local network authority.
    ///
    /// This is the production counterpart to [`Self::reconstruct_direct`]
    /// for callers that staged one process-wide network composition before
    /// deciding whether workloads were present. It reuses that exact
    /// authority for main and pre-bound sibling listeners, but does not expose
    /// the frozen capability registry to compute or install workload lifecycle
    /// managers.
    pub fn protocol_only_with_authority(
        engine: Arc<Engine>,
        network_authority: LocalNetworkAuthority,
    ) -> Self {
        Self {
            router_options: RouterOptions::protocol_only(engine),
            wire_adapters: Vec::new(),
            tls_config: None,
            listener_leases: ServerListenerLeaseAuthority::new(network_authority),
            prebound_wire_listeners: None,
            server_shutdown: ServerShutdownHandle::new(),
        }
    }

    /// Explicitly reconstruct the primitive listener authority once.
    ///
    /// This protocol-only embedder/test seam does not claim process-manager
    /// composition and cannot install workload lifecycle managers. Production
    /// workload composition should use [`Self::managed`].
    pub fn reconstruct_direct(engine: Arc<Engine>) -> std::io::Result<Self> {
        let state_root = engine.data_dir().to_path_buf();
        Self::reconstruct_direct_at(engine, state_root)
    }

    /// Explicitly reconstruct the primitive listener authority at a root
    /// independent of engine persistence.
    ///
    /// This is a protocol-only direct embedder/test seam. Production workload
    /// composition should use [`Self::managed`].
    pub fn reconstruct_direct_at(
        engine: Arc<Engine>,
        state_root: impl AsRef<std::path::Path>,
    ) -> std::io::Result<Self> {
        let router_options = RouterOptions::protocol_only(engine);
        let listener_leases = ServerListenerLeaseAuthority::reconstruct_direct(state_root)?;
        Ok(Self {
            router_options,
            wire_adapters: Vec::new(),
            tls_config: None,
            listener_leases,
            prebound_wire_listeners: None,
            server_shutdown: ServerShutdownHandle::new(),
        })
    }

    /// Authenticate one externally owned main-listener provider incarnation.
    ///
    /// Embedders that inherit a socket across process restarts must persist
    /// and replay the same context with that exact descriptor. Omitting this
    /// option remains safe but makes a new [`ServeOptions`] incarnation
    /// ineligible to reclaim earlier external-listener authority.
    pub fn with_external_main_listener_context(
        mut self,
        context: ExternalServerListenerContext,
    ) -> Self {
        self.listener_leases = self.listener_leases.with_external_main_context(context);
        self
    }

    /// Use the same authority incarnation that owns an upcoming pre-bound
    /// sibling-listener bundle.
    ///
    /// Call this before preparing the main listener. The bundle itself may
    /// remain with the composition owner until every other startup step has
    /// succeeded, allowing exact cleanup on an earlier error.
    pub fn with_prebound_listener_authority(
        mut self,
        listeners: &PreboundServerListeners,
    ) -> std::io::Result<Self> {
        self.listener_leases = self
            .listener_leases
            .authenticate_prebound_authority(&listeners.authority())?;
        Ok(self)
    }

    /// Transfer continuously held sibling sockets into server ownership.
    ///
    /// Their authority must already have been selected with
    /// [`with_prebound_listener_authority`](Self::with_prebound_listener_authority)
    /// before the main listener was prepared.
    pub fn with_prebound_wire_listeners(
        mut self,
        listeners: PreboundServerListeners,
    ) -> std::io::Result<Self> {
        let listener_authority = listeners.authority();
        self.listener_leases
            .authenticate_prebound_bundle(&listener_authority)?;
        self.prebound_wire_listeners = Some(listeners);
        Ok(self)
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

    /// Submit exact declared services after managed recovery and before the
    /// main listener begins serving requests.
    pub fn with_workload_boot_plan(self, plan: ServerWorkloadBootPlan) -> Self {
        self.with_router_options(|options| options.with_workload_boot_plan(plan))
    }

    #[cfg(test)]
    pub(crate) fn with_test_wire_adapter(mut self, adapter: Box<dyn WireProtocolAdapter>) -> Self {
        self.wire_adapters.push(adapter);
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

    /// Obtain a handle that requests shutdown through the same graceful path
    /// as the authenticated local administration endpoint.
    #[must_use]
    pub fn shutdown_handle(&self) -> ServerShutdownHandle {
        self.server_shutdown.clone()
    }
}

async fn serve_with_router_config(
    listener: tokio::net::TcpListener,
    config: RouterBuildConfig,
    tls_config: Option<TlsConfig>,
    server_shutdown: ServerShutdownHandle,
) -> std::io::Result<()> {
    // Load and validate the TLS identity before any engine work so a bad
    // certificate fails the boot, not the first connection.
    let rustls_config = tls_config
        .as_ref()
        .map(crate::tls::load_rustls_server_config)
        .transpose()?;
    let listen_addr = listener.local_addr()?;
    let shutdown_tx = server_shutdown.sender();
    let mut shutdown_rx = server_shutdown.subscribe();
    let config = config
        .with_listen_addr(listen_addr)
        .with_server_shutdown(shutdown_tx);
    let prepared = Box::pin(config.prepare_for_serving())
        .await
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    match rustls_config {
        Some(rustls_config) => {
            crate::tls::serve_tls(
                listener,
                RouterBuildConfig::build_serving(prepared),
                rustls_config,
                shutdown_rx,
            )
            .await
        }
        None => {
            axum::serve(listener, RouterBuildConfig::build_serving(prepared))
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
    mut options: ServeOptions,
) -> std::io::Result<()> {
    if !options
        .listener_leases
        .owns(listener.owner_incarnation(), listener.network_authority())
    {
        let mut result = match listener.close_and_settle() {
            Ok(()) => Err(std::io::Error::other(
                "leased main listener belongs to a different ServeOptions incarnation",
            )),
            Err(cleanup_error) => Err(std::io::Error::other(format!(
                "leased main listener belongs to a different ServeOptions incarnation; \
                 failed to settle its lease after the confirmed local socket close: \
                 {cleanup_error}"
            ))),
        };
        settle_unconsumed_prebound_listeners(
            &mut result,
            &mut options.prebound_wire_listeners,
            "main-listener authority mismatch",
        );
        return result;
    }
    let (listener, main_lease, _) = listener.into_parts();
    let ServeOptions {
        mut router_options,
        wire_adapters,
        tls_config,
        listener_leases,
        mut prebound_wire_listeners,
        server_shutdown,
    } = options;
    let mut main_listener = Some(listener);
    let mut listener_group = WireListenerGroup::new();

    let mut result = async {
        let engine = router_options.engine();
        let system_connectivity_projection =
            nimbus_system::SystemConnectivityProjectionRuntime::new(&engine);
        let main_evidence = main_lease.observation_evidence().ok_or_else(|| {
            std::io::Error::other("main listener carries crossed active lease evidence")
        })?;
        let main_protocol = if tls_config.is_some() {
            "https+websocket"
        } else {
            "http+websocket"
        };
        let main_observation =
            physical_listener_observation("nimbus-server", main_protocol, &main_evidence)?;
        system_connectivity_projection.project_port_listener(main_observation);
        if !router_options.has_system_convex_registry() {
            router_options =
                router_options.with_system_convex_registry(load_default_system_convex_registry()?);
        }
        let config = router_options.into_build_config();

        // Sibling adapter listeners share the same `Arc<Engine>`. The group
        // retains every prepared or spawned task and active lease across this
        // fallible setup block so every return uses one cleanup path.
        for (ordinal, adapter) in wire_adapters.into_iter().enumerate() {
            let requested_addr = adapter.bind_addr();
            let leased_listener = if let Some(prebound) = prebound_wire_listeners
                .as_mut()
                .and_then(|listeners| listeners.remove(adapter.name()))
            {
                if !listener_leases.owns(prebound.owner_incarnation(), prebound.network_authority())
                {
                    return close_prebound_after_error(
                        prebound,
                        std::io::Error::other(format!(
                            "pre-bound {} listener belongs to a different ServeOptions \
                                 incarnation",
                            adapter.name()
                        )),
                        "failed to settle the mismatched pre-bound sibling",
                    );
                }
                let adapter_addr = match prebound.local_addr() {
                    Ok(addr) => addr,
                    Err(error) => {
                        return close_prebound_after_error(
                            prebound,
                            error,
                            "failed to settle the pre-bound sibling after its address could \
                                 not be observed",
                        );
                    }
                };
                if adapter_addr != requested_addr {
                    return close_prebound_after_error(
                        prebound,
                        std::io::Error::other(format!(
                            "pre-bound {} listener address {adapter_addr} does not match \
                                 configured address {requested_addr}",
                            adapter.name()
                        )),
                        "failed to settle the mismatched pre-bound sibling address",
                    );
                }
                if let Err(guard_error) = adapter.guard(adapter_addr) {
                    return close_prebound_after_error(
                        prebound,
                        guard_error,
                        "failed to settle the pre-bound listener after its guard refused the \
                             bind",
                    );
                }
                prebound.into_leased()?
            } else {
                let prepared = listener_leases
                    .prepare_sibling(ordinal, adapter.name(), requested_addr)
                    .map_err(|error| {
                        wire_listener_setup_error(adapter.name(), requested_addr, error)
                    })?;
                let adapter_listener = match tokio::net::TcpListener::bind(requested_addr).await {
                    Ok(listener) => listener,
                    Err(error) => {
                        return Err(wire_listener_setup_error(
                            adapter.name(),
                            requested_addr,
                            bind_failure_error(prepared.record_bind_failure(error)),
                        ));
                    }
                };
                let adapter_addr = match adapter_listener.local_addr() {
                    Ok(addr) => addr,
                    Err(error) => {
                        return match abandon_prepared_after_guard_failure(
                            prepared,
                            adapter_listener,
                        ) {
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
                            "failed to settle the claimed listener after its guard refused \
                                 the bind",
                            cleanup_error,
                        ),
                    };
                }
                prepared.adopt(adapter_listener)?
            };
            if let Err(error) = leased_listener.local_addr() {
                return match leased_listener.close_and_settle() {
                    Ok(()) => Err(error),
                    Err(cleanup_error) => append_cleanup_error(
                        Err(error),
                        "failed to settle the sibling lease after its adopted address could \
                             not be observed",
                        cleanup_error,
                    ),
                };
            }
            debug_assert!(
                listener_leases.owns(
                    leased_listener.owner_incarnation(),
                    leased_listener.network_authority(),
                ),
                "sibling listener must belong to the serving incarnation"
            );
            let (adapter_listener, adapter_lease, _) = leased_listener.into_parts();
            let adapter_evidence = match adapter_lease.observation_evidence() {
                Some(evidence) => evidence,
                None => {
                    drop(adapter_listener);
                    let mut result = Err(std::io::Error::other(format!(
                        "{} listener carries crossed active lease evidence",
                        adapter.name()
                    )));
                    if let Err(cleanup_error) = adapter_lease.settle_after_confirmed_local_close() {
                        result = append_cleanup_error(
                            result,
                            "failed to settle the sibling lease after its evidence was crossed",
                            cleanup_error,
                        );
                    }
                    return result;
                }
            };
            let adapter_observation = match physical_listener_observation(
                adapter.name(),
                adapter.protocol(),
                &adapter_evidence,
            ) {
                Ok(observation) => observation,
                Err(error) => {
                    drop(adapter_listener);
                    let mut result = Err(error);
                    if let Err(cleanup_error) = adapter_lease.settle_after_confirmed_local_close() {
                        result = append_cleanup_error(
                            result,
                            "failed to settle the sibling lease after invalid evidence",
                            cleanup_error,
                        );
                    }
                    return result;
                }
            };
            system_connectivity_projection.project_port_listener(adapter_observation);
            listener_group.prepare(
                adapter,
                adapter_listener,
                adapter_lease,
                Arc::clone(&engine),
            )?;
        }

        if let Some(unused_name) = prebound_wire_listeners
            .as_ref()
            .and_then(PreboundServerListeners::first_name)
            .map(str::to_owned)
        {
            return close_prebound_bundle_after_error(
                prebound_wire_listeners
                    .take()
                    .expect("the unmatched listener name came from this bundle"),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("pre-bound listener `{unused_name}` has no matching wire adapter"),
                ),
                "failed to settle unmatched pre-bound listeners",
            );
        }

        listener_group.activate();
        let listener = main_listener
            .take()
            .expect("the main listener must be consumed exactly once");
        listener_group
            .supervise(Box::pin(serve_with_router_config(
                listener,
                config,
                tls_config,
                server_shutdown,
            )))
            .await
    }
    .await;

    settle_unconsumed_prebound_listeners(
        &mut result,
        &mut prebound_wire_listeners,
        "sibling-listener setup failure",
    );
    // A synchronous setup error leaves the main socket here. Dropping it
    // proves local closure before the lease is settled below.
    drop(main_listener.take());
    result = listener_group.shutdown(result).await;
    if let Err(error) = main_lease.settle_after_confirmed_local_close() {
        result = append_cleanup_error(
            result,
            "failed to settle the main listener lease after confirmed local closure",
            error,
        );
    }
    result
}

fn physical_listener_observation(
    adapter: &str,
    application_protocol: &str,
    evidence: &ActiveServerListenerEvidence,
) -> std::io::Result<nimbus_system::SystemPortListenerObservation> {
    let NetworkResourceId::Listener(listener_id) = evidence.request().owner_id() else {
        return Err(std::io::Error::other(format!(
            "{adapter} physical listener lease is owned by a non-listener resource"
        )));
    };
    nimbus_system::SystemPortListenerObservation::new(
        adapter,
        application_protocol,
        listener_id.clone(),
        evidence.request().clone(),
        evidence.bound_endpoint().clone(),
        evidence.provider_id().clone(),
        NetworkResourcePhase::Ready,
        [
            NetworkCondition::new(NetworkConditionKind::Ready, NetworkConditionState::True),
            NetworkCondition::new(
                NetworkConditionKind::Published,
                NetworkConditionState::False,
            ),
            NetworkCondition::new(
                NetworkConditionKind::CleanupPending,
                NetworkConditionState::False,
            ),
        ],
    )
    .map(|observation| observation.with_version(env!("CARGO_PKG_VERSION")))
    .map_err(|error| std::io::Error::other(error.to_string()))
}

fn bind_failure_error(
    result: Result<RecordedListenerBindFailure, std::io::Error>,
) -> std::io::Error {
    match result {
        Ok(recorded) => recorded.into_error(),
        Err(error) => error,
    }
}

fn wire_listener_setup_error(
    adapter_name: &str,
    requested_addr: std::net::SocketAddr,
    error: std::io::Error,
) -> std::io::Error {
    std::io::Error::new(
        error.kind(),
        format!("{adapter_name} listener {requested_addr}: {error}"),
    )
}

fn settle_unconsumed_prebound_listeners(
    result: &mut std::io::Result<()>,
    listeners: &mut Option<PreboundServerListeners>,
    failure_context: &str,
) {
    let Some(listeners) = listeners.take() else {
        return;
    };
    if let Err(cleanup_error) = listeners.close_and_settle() {
        *result = append_cleanup_error(
            std::mem::replace(result, Ok(())),
            &format!("failed to settle unconsumed pre-bound listeners after {failure_context}"),
            cleanup_error,
        );
    }
}

fn close_prebound_after_error(
    listener: PreboundServerListener,
    primary: std::io::Error,
    cleanup_context: &str,
) -> std::io::Result<()> {
    match listener.close_and_settle() {
        Ok(()) => Err(primary),
        Err(cleanup_error) => append_cleanup_error(Err(primary), cleanup_context, cleanup_error),
    }
}

fn close_prebound_bundle_after_error(
    listeners: PreboundServerListeners,
    primary: std::io::Error,
    cleanup_context: &str,
) -> std::io::Result<()> {
    match listeners.close_and_settle() {
        Ok(()) => Err(primary),
        Err(cleanup_error) => append_cleanup_error(Err(primary), cleanup_context, cleanup_error),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::net::SocketAddr;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nimbus_network::{
        LocalNetworkManager, LocalPortLeaseAuthority, NetworkCapabilityRegistry, PortLeasePhase,
    };
    use nimbus_process_harness::PortWindow;
    use nimbus_testing::EngineFixture;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::adapters::wire::WireProtocolTasks;

    const TEST_LISTENER_LIVENESS_TIMEOUT: Duration = Duration::from_secs(5);

    fn direct_options(engine: Arc<Engine>) -> ServeOptions {
        ServeOptions::reconstruct_direct(engine)
            .expect("test server network authority should reconstruct once")
    }

    fn direct_prebound(state_root: &Path) -> PreboundServerListeners {
        PreboundServerListeners::reconstruct_direct(state_root)
            .expect("test prebound network authority should reconstruct once")
    }

    fn filesystem_snapshot(root: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
        fn visit(root: &Path, current: &Path, snapshot: &mut BTreeMap<PathBuf, Option<Vec<u8>>>) {
            let mut entries = std::fs::read_dir(current)
                .unwrap_or_else(|error| panic!("failed to read {}: {error}", current.display()))
                .collect::<Result<Vec<_>, _>>()
                .unwrap_or_else(|error| {
                    panic!("failed to enumerate {}: {error}", current.display())
                });
            entries.sort_by_key(|entry| entry.path());
            for entry in entries {
                let path = entry.path();
                let relative = path
                    .strip_prefix(root)
                    .expect("snapshot entry should stay below its root")
                    .to_path_buf();
                if path.is_dir() {
                    snapshot.insert(relative, None);
                    visit(root, &path, snapshot);
                } else {
                    snapshot.insert(
                        relative,
                        Some(std::fs::read(&path).unwrap_or_else(|error| {
                            panic!("failed to read snapshot file {}: {error}", path.display())
                        })),
                    );
                }
            }
        }

        let mut snapshot = BTreeMap::new();
        if root.is_dir() {
            visit(root, root, &mut snapshot);
        }
        snapshot
    }

    #[test]
    fn protocol_only_with_authority_reuses_engine_and_network_claim_without_effects() {
        let network_root = tempfile::tempdir().expect("network root should build");
        let engine_root = tempfile::tempdir().expect("engine root should build");
        let manager = LocalNetworkManager::bootstrap(network_root.path())
            .expect("network manager should claim once")
            .freeze(NetworkCapabilityRegistry::new([]).expect("empty registry should validate"));
        let authority = manager.authority();
        let prebound = PreboundServerListeners::new(authority.clone());
        let engine = Arc::new(Engine::new(engine_root.path()).expect("engine should initialize"));
        let before = filesystem_snapshot(network_root.path());

        let options =
            ServeOptions::protocol_only_with_authority(Arc::clone(&engine), authority.clone())
                .with_prebound_listener_authority(&prebound)
                .expect("the exact prepared authority should authenticate");

        assert!(Arc::ptr_eq(&options.router_options.engine(), &engine));
        assert_eq!(manager.capability_registry().selections().count(), 0);
        assert_eq!(authority.authority_path(), manager.authority_path());
        assert_eq!(filesystem_snapshot(network_root.path()), before);
        drop(options);
        drop(prebound);
        drop(authority);
        drop(manager);
    }

    struct ProbeAdapter {
        bound_addr: Arc<Mutex<Option<SocketAddr>>>,
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

        fn build_tasks(
            self: Box<Self>,
            _engine: Arc<Engine>,
        ) -> std::io::Result<WireProtocolTasks> {
            Ok(WireProtocolTasks::new("listener", move |listener| {
                Box::pin(async move {
                    if let Ok((mut stream, _)) = listener.accept().await {
                        let _ = stream.write_all(b"still-live").await;
                    }
                    Ok(())
                })
            }))
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

        fn build_tasks(
            self: Box<Self>,
            _engine: Arc<Engine>,
        ) -> std::io::Result<WireProtocolTasks> {
            unreachable!("the occupied address must fail before task construction")
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
            let claim_observed = LocalPortLeaseAuthority::open(&self.state_root)
                .and_then(|authority| authority.list())
                .is_ok_and(|records| {
                    records.iter().any(|record| {
                        record.phase() == PortLeasePhase::Reserved && record.bind_claim().is_some()
                    })
                });
            self.claim_observed.store(claim_observed, Ordering::Release);
            if claim_observed {
                *self
                    .bound_addr
                    .lock()
                    .expect("probe address lock should remain healthy") = Some(addr);
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "sibling bind reached its guard without a claimed port lease",
                ))
            }
        }

        fn build_tasks(
            self: Box<Self>,
            _engine: Arc<Engine>,
        ) -> std::io::Result<WireProtocolTasks> {
            Ok(WireProtocolTasks::new("listener", move |listener| {
                Box::pin(async move {
                    if let Ok((mut stream, _)) = listener.accept().await {
                        let _ = stream.write_all(b"lease-owned").await;
                    }
                    Ok(())
                })
            }))
        }
    }

    struct PreboundAdapter {
        addr: SocketAddr,
        state_root: PathBuf,
        active_observed: Arc<AtomicBool>,
    }

    impl WireProtocolAdapter for PreboundAdapter {
        fn name(&self) -> &'static str {
            "nnc3-7a-prebound"
        }

        fn protocol(&self) -> &'static str {
            "tcp"
        }

        fn bind_addr(&self) -> SocketAddr {
            self.addr
        }

        fn guard(&self, addr: SocketAddr) -> std::io::Result<()> {
            let active = active_lease_matches_addr(&self.state_root, addr);
            self.active_observed.store(active, Ordering::Release);
            if active {
                Ok(())
            } else {
                Err(std::io::Error::other(
                    "pre-bound sibling reached its guard without an Active lease",
                ))
            }
        }

        fn build_tasks(
            self: Box<Self>,
            _engine: Arc<Engine>,
        ) -> std::io::Result<WireProtocolTasks> {
            Ok(WireProtocolTasks::new("listener", move |listener| {
                Box::pin(async move {
                    if let Ok((mut stream, _)) = listener.accept().await {
                        let _ = stream.write_all(b"prebound-owned").await;
                    }
                    Ok(())
                })
            }))
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
        let task = tokio::spawn(serve(listener, direct_options(fixture.engine())));

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
        let preparing_options = direct_options(fixture.engine());
        let prepared = preparing_options
            .prepare_main_listener("127.0.0.1:0".parse().expect("fixture address should parse"))
            .expect("main listener should reserve");
        let raw = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("main listener should bind");
        let leased = prepared.adopt(raw).expect("main listener should activate");
        let serving_options = direct_options(fixture.engine());

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
        let mut options = direct_options(fixture.engine());
        // The reservation stays provider-assigned, so `prepare_main_listener`
        // keeps receiving port zero. The main socket itself takes a port this
        // process holds, because the assertion at the end re-binds that exact
        // address to prove the failed start closed it.
        let window = PortWindow::claim();
        let prepared = options
            .prepare_main_listener("127.0.0.1:0".parse().expect("fixture address should parse"))
            .expect("main listener should reserve");
        let raw = tokio::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], window.port(0))))
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
        let main_addr = main_listener
            .local_addr()
            .expect("main listener address should resolve");
        let bound_addr = Arc::new(Mutex::new(None));
        let claim_observed = Arc::new(AtomicBool::new(false));
        let mut options = direct_options(fixture.engine());
        options.wire_adapters.push(Box::new(LeaseAwareAdapter {
            state_root: fixture.data_dir().to_path_buf(),
            bound_addr: Arc::clone(&bound_addr),
            claim_observed: Arc::clone(&claim_observed),
        }));
        let task = tokio::spawn(serve(main_listener, options));

        let sibling_addr = tokio::time::timeout(TEST_LISTENER_LIVENESS_TIMEOUT, async {
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
            tokio::time::timeout(TEST_LISTENER_LIVENESS_TIMEOUT, async {
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

        assert!(
            claimed_before_guard,
            "NNC3.5: the sibling kernel bind reached its guard without an exact durable claim"
        );
        assert_eq!(
            bytes.as_ref().map(<[u8; 11]>::as_slice),
            Some(b"lease-owned".as_slice()),
            "the lease migration must preserve sibling protocol bytes"
        );

        let system_tenant = nimbus_system::system_tenant_id().expect("system tenant should parse");
        let listeners_table =
            nimbus_core::TableName::new("listeners").expect("listeners table should parse");
        let ports_table = nimbus_core::TableName::new("ports").expect("ports table should parse");
        let (listeners, ports) = tokio::time::timeout(TEST_LISTENER_LIVENESS_TIMEOUT, async {
            loop {
                let listeners = fixture
                    .engine()
                    .list_documents_async(system_tenant.clone(), listeners_table.clone())
                    .await
                    .expect("physical listener projections should list");
                let ports = fixture
                    .engine()
                    .list_documents_async(system_tenant.clone(), ports_table.clone())
                    .await
                    .expect("physical port projections should list");
                if listeners.len() >= 2 && ports.len() >= 2 {
                    break (listeners, ports);
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("main system preparation should retain both physical listener projections");
        assert_eq!(
            listeners.len(),
            2,
            "one main and one sibling socket must produce exactly two physical listener rows"
        );
        assert_eq!(ports.len(), 2);
        let adapters = listeners
            .iter()
            .filter_map(|document| {
                document
                    .fields
                    .get("adapter")
                    .and_then(serde_json::Value::as_str)
            })
            .collect::<BTreeSet<_>>();
        assert_eq!(
            adapters,
            BTreeSet::from(["nimbus-server", "nnc3-5-lease-aware"])
        );
        assert!(
            listeners.iter().all(|document| {
                !matches!(
                    document
                        .fields
                        .get("adapter")
                        .and_then(serde_json::Value::as_str),
                    Some("convex" | "firebase" | "cloud-functions" | "cloudflare")
                )
            }),
            "logical protocol registrations must not create listener authority"
        );

        for listener in &listeners {
            let listener_id = listener
                .fields
                .get("listenerId")
                .and_then(serde_json::Value::as_str)
                .expect("physical listener projection should retain its stable identity")
                .parse::<nimbus_network::ListenerId>()
                .expect("listener projection identity should be canonical");
            let expected_lease_id = nimbus_network::PortLeaseId::for_listener(&listener_id);
            let expected_address = listener
                .fields
                .get("actualAddress")
                .and_then(serde_json::Value::as_str)
                .expect("listener projection should retain its observed address");
            assert!(
                expected_address == main_addr.to_string()
                    || expected_address == sibling_addr.to_string()
            );
            let port = ports
                .iter()
                .find(|document| {
                    document
                        .fields
                        .get("portLeaseId")
                        .and_then(serde_json::Value::as_str)
                        == Some(expected_lease_id.as_str())
                })
                .expect("stable port projection should derive from its listener identity");
            assert_eq!(
                listener
                    .fields
                    .get("portLeaseId")
                    .and_then(serde_json::Value::as_str),
                Some(expected_lease_id.as_str())
            );
            for field in [
                "generation",
                "leaseEpoch",
                "providerId",
                "actualAddress",
                "observedPhase",
                "cleanupState",
            ] {
                assert_eq!(
                    listener.fields.get(field),
                    port.fields.get(field),
                    "listener and port observations must share exact {field} evidence"
                );
            }
            listener
                .fields
                .get("providerId")
                .and_then(serde_json::Value::as_str)
                .expect("listener provider identity should be present")
                .parse::<nimbus_network::NetworkProviderId>()
                .expect("listener provider identity should be canonical");
        }

        task.abort();
        let _ = task.await;
    }

    #[tokio::test]
    async fn nnc3_7a_prebound_sibling_is_adopted_without_rebind() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let mut prebound = direct_prebound(fixture.data_dir());
        let requested_addr = "127.0.0.1:0"
            .parse()
            .expect("provider-assigned address should parse");
        let prepared = prebound
            .prepare("dev-nnc3-7a-prebound", requested_addr)
            .expect("pre-bound listener should reserve before bind");
        let raw = std::net::TcpListener::bind(requested_addr)
            .expect("provider should bind its requested socket");
        let listener = prepared
            .adopt_std(raw)
            .expect("provider socket should activate its lease");
        let sibling_addr = listener
            .local_addr()
            .expect("pre-bound address should resolve");
        prebound
            .insert("nnc3-7a-prebound", listener)
            .expect("pre-bound listener should enter its bundle");

        let competing_bind = std::net::TcpListener::bind(sibling_addr);
        assert!(
            matches!(
                competing_bind,
                Err(ref error) if error.kind() == std::io::ErrorKind::AddrInUse
            ),
            "the provider-assigned socket must remain continuously held before server adoption"
        );

        let main_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("main listener should bind");
        let active_observed = Arc::new(AtomicBool::new(false));
        let mut options = direct_options(fixture.engine())
            .with_prebound_listener_authority(&prebound)
            .expect("matching prebound authority should authenticate");
        options.wire_adapters.push(Box::new(PreboundAdapter {
            addr: sibling_addr,
            state_root: fixture.data_dir().to_path_buf(),
            active_observed: Arc::clone(&active_observed),
        }));
        options = options
            .with_prebound_wire_listeners(prebound)
            .expect("matching prebound bundle should transfer");
        let task = tokio::spawn(serve(main_listener, options));

        let bytes = tokio::time::timeout(TEST_LISTENER_LIVENESS_TIMEOUT, async {
            let mut stream = tokio::net::TcpStream::connect(sibling_addr).await?;
            let mut bytes = [0_u8; 14];
            stream.read_exact(&mut bytes).await?;
            Ok::<_, std::io::Error>(bytes)
        })
        .await
        .expect("the adopted sibling should accept before timeout")
        .expect("the adopted sibling should serve its protocol bytes");

        task.abort();
        let _ = task.await;
        assert!(
            active_observed.load(Ordering::Acquire),
            "the pre-bound sibling guard must observe its durable Active lease"
        );
        assert_eq!(
            bytes.as_slice(),
            b"prebound-owned",
            "adopting the retained socket must preserve the wire behavior"
        );
    }

    #[tokio::test]
    async fn nnc3_7a_mismatched_handoff_settles_every_prebound_listener() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        let mut prebound = direct_prebound(fixture.data_dir());
        // Both reservations stay provider-assigned, so `prepare` keeps
        // receiving port zero. The two provider sockets take held ports —
        // offset 0 for the matching member and offset 1 for the orphan —
        // because the loop below re-binds both to prove the rejected handoff
        // closed them.
        let window = PortWindow::claim();
        let requested_addr = "127.0.0.1:0"
            .parse()
            .expect("provider-assigned address should parse");

        let prepared = prebound
            .prepare("dev-nnc3-7a-prebound", requested_addr)
            .expect("matching listener should reserve");
        let raw = std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], window.port(0))))
            .expect("matching provider socket should bind");
        let listener = prepared
            .adopt_std(raw)
            .expect("matching provider socket should activate");
        let matching_addr = listener
            .local_addr()
            .expect("matching address should resolve");
        prebound
            .insert("nnc3-7a-prebound", listener)
            .expect("matching listener should enter the bundle");

        let prepared = prebound
            .prepare("dev-nnc3-7a-orphan", requested_addr)
            .expect("unconsumed listener should reserve");
        let raw = std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], window.port(1))))
            .expect("unconsumed provider socket should bind");
        let listener = prepared
            .adopt_std(raw)
            .expect("unconsumed provider socket should activate");
        let orphan_addr = listener
            .local_addr()
            .expect("unconsumed address should resolve");
        prebound
            .insert("nnc3-7a-orphan", listener)
            .expect("unconsumed listener should enter the bundle");

        let active_observed = Arc::new(AtomicBool::new(false));
        let mut options = direct_options(fixture.engine());
        options.wire_adapters.push(Box::new(PreboundAdapter {
            addr: matching_addr,
            state_root: fixture.data_dir().to_path_buf(),
            active_observed: Arc::clone(&active_observed),
        }));
        // Deliberately omit `with_prebound_listener_authority`: the server
        // must reject the foreign incarnation and settle both the selected
        // listener and every still-unconsumed bundle member.
        let error = match options.with_prebound_wire_listeners(prebound) {
            Ok(_) => panic!("cross-incarnation pre-bound adoption must fail closed"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("different server authority incarnation"),
            "the failure should identify the authority mismatch: {error}"
        );
        assert!(
            !active_observed.load(Ordering::Acquire),
            "an authority mismatch must fail before the adapter guard or serving effect"
        );
        for addr in [matching_addr, orphan_addr] {
            std::net::TcpListener::bind(addr)
                .expect("every rejected pre-bound socket must be confirmed closed");
        }
        let records = LocalPortLeaseAuthority::open(fixture.data_dir())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        for addr in [matching_addr, orphan_addr] {
            let record = records
                .iter()
                .find(|record| {
                    record
                        .binding()
                        .is_some_and(|binding| binding.actual_port().get() == addr.port())
                })
                .expect("each rejected pre-bound lease should remain inspectable");
            assert_eq!(record.phase(), PortLeasePhase::Released);
        }
    }

    #[test]
    fn nnc3_7a_serve_options_drop_and_replacement_settle_prebound_bundles() {
        let fixture = EngineFixture::new(|path| Engine::new(path));
        // Each bundle's reservation stays provider-assigned, so `prepare`
        // keeps receiving port zero. The caller hands in the held port the
        // provider socket actually binds, because both addresses are re-bound
        // at the end to prove the rejected replacement settled them.
        let window = PortWindow::claim();
        let prepare_bundle = |listener_name: &str, adapter_name: &str, port: u16| {
            let mut listeners = direct_prebound(fixture.data_dir());
            let requested_addr = "127.0.0.1:0"
                .parse()
                .expect("provider-assigned address should parse");
            let prepared = listeners
                .prepare(listener_name, requested_addr)
                .expect("pre-bound listener should reserve");
            let raw = std::net::TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port)))
                .expect("provider should bind its requested socket");
            let listener = prepared
                .adopt_std(raw)
                .expect("pre-bound listener should activate");
            let actual_addr = listener
                .local_addr()
                .expect("pre-bound address should resolve");
            listeners
                .insert(adapter_name, listener)
                .expect("listener should enter the handoff bundle");
            (listeners, actual_addr)
        };

        let (first, first_addr) =
            prepare_bundle("dev-mongodb-provider-assigned", "mongodb", window.port(0));
        let (second, second_addr) =
            prepare_bundle("dev-s3-provider-assigned", "s3", window.port(1));
        let options = direct_options(fixture.engine())
            .with_prebound_listener_authority(&first)
            .expect("first prebound authority should authenticate")
            .with_prebound_wire_listeners(first)
            .expect("first prebound bundle should transfer");
        let error = match options.with_prebound_wire_listeners(second) {
            Ok(_) => panic!("a divergent replacement incarnation must fail closed"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("different server authority incarnation"),
            "the failure should identify the replacement mismatch: {error}"
        );

        std::net::TcpListener::bind(first_addr)
            .expect("rejected replacement must settle the previously retained bundle");
        std::net::TcpListener::bind(second_addr)
            .expect("rejected replacement must settle the divergent bundle");

        let records = LocalPortLeaseAuthority::open(fixture.data_dir())
            .expect("port authority should reopen")
            .list()
            .expect("port records should list");
        for addr in [first_addr, second_addr] {
            let record = records
                .iter()
                .find(|record| {
                    record
                        .binding()
                        .is_some_and(|binding| binding.actual_port().get() == addr.port())
                })
                .expect("each pre-bound lease should remain inspectable");
            assert_eq!(record.phase(), PortLeasePhase::Released);
        }
    }

    #[tokio::test]
    // NNC0.7 captured this as the NNCF17 fail-before. The structured listener
    // group now makes it an ordinary regression test.
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
        let mut options = direct_options(fixture.engine());
        options.wire_adapters.push(Box::new(ProbeAdapter {
            bound_addr: Arc::clone(&bound_addr),
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

        assert!(
            !prior_listener_served,
            "NNCF17: startup returned {error}, but the earlier sibling listener still \
             accepted and served bytes after the listener-group failure"
        );
    }
}
