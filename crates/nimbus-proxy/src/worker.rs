use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use nimbus_egress::{
    CompiledEgressPolicy, EgressPolicy, EgressProtocol, EgressRule, LayeredEgressPolicy,
};
use pingora_core::apps::HttpServerApp;
use pingora_core::protocols::http::ServerSession as HttpSession;
use pingora_core::server::configuration::ServerConf;
use pingora_proxy::http_proxy;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Semaphore, watch};
use tokio::task::JoinSet;

use crate::body::read_exact_body_into_buffer;
use crate::connect::{connect_upstream, splice_connect};
use crate::credentials::{CredentialSecretProviderRef, CredentialSecretStore};
use crate::decision_log::{
    ABORT_AFTER_RESPONSE_REASON, DecisionLogger, DurableDecisionSink, EgressDecisionLog,
    UPSTREAM_FAILURE_AFTER_RESPONSE_REASON, noop_decision_logger, noop_durable_decision_sink,
};
use crate::dns::{DnsCacheConfig, Resolver, resolve_dns, resolve_socket_addrs};
use crate::enforcement::{
    ProxyRequestEnforcementContext, prepare_proxy_request_enforcement,
    reject_unapproved_caller_credentials_for_rule,
};
use crate::error::{EgressProxyError, Result};
use crate::fairness::TenantFairness;
use crate::https_intercept::{
    ConnectInterceptAction, HttpsInterceptContext, classify_connect, intercept_connect_h1,
};
use crate::phase::{
    EgressProxyRequestPhase, PhaseObserver, RequestPhaseRecorder, noop_phase_observer,
};
use crate::pingora_app::{ForwardRequestPlan, NimbusForwardApp, downstream_target};
use crate::pingora_identity::PingoraPeerPlan;
use crate::pingora_io::{FinalResponseWriteGate, PrereadStream};
use crate::policy_state::{EgressProxyPolicyState, PolicyGeneration, WorkloadPepReadiness};
use crate::pool::{
    EgressProxyCredentialDlpMode, EgressProxyPoolIdentity, EgressProxyPoolKey, TlsVerificationMode,
};
use crate::request::{ParsedProxyRequest, ProxyRequestMode, find_header_end, parse_proxy_request};
use crate::response::{HttpProxyResponse, write_http_response_async};
use crate::substrate::ProxySubstrate;
use crate::terminal::{
    AbortTerminalGuard, ParsedRequestLogContext, RequestIdGenerator, ResponseStartedSignal,
    TerminalSinks, audit_failure_terminal, audit_unhealthy_terminal, deny_terminal,
    emit_terminal_log, malformed_terminal, record_durable_decision, upstream_error_terminal,
};
use crate::tls_authority::WorkloadPepTlsAuthority;
use crate::{
    DEFAULT_CONNECT_TIMEOUT, DEFAULT_IO_TIMEOUT, DEFAULT_MAX_CONNECTIONS, MAX_HTTP_HEADER_BYTES,
};

const SHUTDOWN_ACK_TIMEOUT: Duration = Duration::from_secs(2);

pub struct WorkloadPepConfig {
    pub bind_addr: SocketAddr,
    pub policy: Option<CompiledEgressPolicy>,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    pub max_connections: usize,
    pub dns_cache: DnsCacheConfig,
    pub credential_store: CredentialSecretStore,
    pub pool_identity: EgressProxyPoolIdentity,
    pub tls_authority: Option<WorkloadPepTlsAuthority>,
    pub substrate: ProxySubstrate,
    credential_provider: Option<CredentialSecretProviderRef>,
    decision_logger: DecisionLogger,
    durable_decision_sink: DurableDecisionSink,
    phase_observer: PhaseObserver,
    resolver: Resolver,
    tenant_fairness: Option<Arc<TenantFairness>>,
    global_ceiling: Option<CompiledEgressPolicy>,
    #[cfg(test)]
    audit_healthy: Option<Arc<AtomicBool>>,
}

impl WorkloadPepConfig {
    pub fn new(policy: CompiledEgressPolicy) -> Self {
        Self {
            policy: Some(policy),
            ..Self::without_active_policy()
        }
    }

    pub fn without_active_policy() -> Self {
        Self {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            policy: None,
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            io_timeout: DEFAULT_IO_TIMEOUT,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            dns_cache: DnsCacheConfig::default(),
            credential_store: CredentialSecretStore::empty(),
            pool_identity: EgressProxyPoolIdentity::default(),
            tls_authority: None,
            substrate: ProxySubstrate::shared(),
            credential_provider: None,
            decision_logger: noop_decision_logger(),
            durable_decision_sink: noop_durable_decision_sink(),
            phase_observer: noop_phase_observer(),
            resolver: Arc::new(resolve_socket_addrs),
            tenant_fairness: None,
            global_ceiling: None,
            #[cfg(test)]
            audit_healthy: None,
        }
    }

    pub fn with_bind_addr(mut self, bind_addr: SocketAddr) -> Self {
        self.bind_addr = bind_addr;
        self
    }

    pub fn with_timeouts(mut self, connect_timeout: Duration, io_timeout: Duration) -> Self {
        self.connect_timeout = connect_timeout;
        self.io_timeout = io_timeout;
        self
    }

    pub fn with_max_connections(mut self, max_connections: usize) -> Self {
        self.max_connections = max_connections;
        self
    }

    pub fn with_dns_cache_config(mut self, dns_cache: DnsCacheConfig) -> Self {
        self.dns_cache = dns_cache;
        self
    }

    pub fn with_credential_store(mut self, credential_store: CredentialSecretStore) -> Self {
        self.credential_store = credential_store;
        self.credential_provider = None;
        self
    }

    pub fn with_credential_provider(
        mut self,
        credential_provider: CredentialSecretProviderRef,
    ) -> Self {
        self.credential_provider = Some(credential_provider);
        self
    }

    pub fn with_pool_identity(mut self, pool_identity: EgressProxyPoolIdentity) -> Self {
        self.pool_identity = pool_identity;
        self
    }

    pub fn with_tls_authority(mut self, tls_authority: WorkloadPepTlsAuthority) -> Self {
        self.tls_authority = Some(tls_authority);
        self
    }

    pub fn with_substrate(mut self, substrate: ProxySubstrate) -> Self {
        self.substrate = substrate;
        self
    }

    pub fn with_decision_logger(mut self, decision_logger: DecisionLogger) -> Self {
        self.decision_logger = decision_logger;
        self
    }

    pub fn with_durable_decision_sink(
        mut self,
        durable_decision_sink: DurableDecisionSink,
    ) -> Self {
        self.durable_decision_sink = durable_decision_sink;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_phase_observer(mut self, phase_observer: PhaseObserver) -> Self {
        self.phase_observer = phase_observer;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_audit_health_probe(mut self, audit_healthy: Arc<AtomicBool>) -> Self {
        self.audit_healthy = Some(audit_healthy);
        self
    }

    /// Narrow this PEP under a node-global allow-ceiling (EE2 knob: the
    /// ceiling CONTENT is policy-hardening's; both allow-lists must permit).
    pub fn with_global_ceiling(mut self, ceiling: CompiledEgressPolicy) -> Self {
        self.global_ceiling = Some(ceiling);
        self
    }

    /// Attach the tenant's fairness handle (EE3). Captured at registration —
    /// the request path never performs a per-request tenant lookup.
    pub fn with_tenant_fairness(mut self, tenant_fairness: Arc<TenantFairness>) -> Self {
        self.tenant_fairness = Some(tenant_fairness);
        self
    }

    #[cfg(test)]
    pub(crate) fn with_resolver(mut self, resolver: Resolver) -> Self {
        self.resolver = resolver;
        self
    }
}

pub struct WorkloadPep {
    local_addr: SocketAddr,
    shutdown: watch::Sender<bool>,
    shutdown_ack: Option<mpsc::Receiver<()>>,
    _substrate: ProxySubstrate,
    policy_state: Arc<RwLock<EgressProxyPolicyState>>,
    audit_healthy: Arc<AtomicBool>,
}

impl WorkloadPep {
    pub fn start(config: WorkloadPepConfig) -> Result<Self> {
        if config.max_connections == 0 {
            return Err(EgressProxyError::OperationFailed {
                message: "egress proxy max_connections must be greater than 0".to_owned(),
            });
        }
        let listener = TcpListener::bind(config.bind_addr).map_err(|error| {
            EgressProxyError::OperationFailed {
                message: format!(
                    "failed to bind egress proxy on {}: {error}",
                    config.bind_addr
                ),
            }
        })?;
        let local_addr =
            listener
                .local_addr()
                .map_err(|error| EgressProxyError::OperationFailed {
                    message: format!("failed to read egress proxy listen address: {error}"),
                })?;
        listener
            .set_nonblocking(true)
            .map_err(|error| EgressProxyError::OperationFailed {
                message: format!("failed to configure egress proxy listener: {error}"),
            })?;

        let policy_state = Arc::new(RwLock::new({
            let mut state = config
                .policy
                .map(EgressProxyPolicyState::with_policy)
                .unwrap_or_default();
            state.set_global_ceiling(config.global_ceiling);
            state
        }));
        #[cfg(test)]
        let audit_healthy = config
            .audit_healthy
            .clone()
            .unwrap_or_else(|| Arc::new(AtomicBool::new(true)));
        #[cfg(not(test))]
        let audit_healthy = Arc::new(AtomicBool::new(true));
        let request_ids = Arc::new(RequestIdGenerator::new());
        let (shutdown, shutdown_rx) = watch::channel(false);
        let (ack_tx, ack_rx) = mpsc::channel();
        let substrate = config.substrate;
        let worker = ProxyWorker {
            listener,
            policy_state: Arc::clone(&policy_state),
            audit_healthy: Arc::clone(&audit_healthy),
            resolver: config.resolver,
            dns_cache: config.dns_cache,
            credential_provider: config
                .credential_provider
                .unwrap_or_else(|| config.credential_store.into_provider()),
            pool_identity: config.pool_identity,
            tls_authority: config.tls_authority,
            decision_logger: config.decision_logger,
            durable_decision_sink: config.durable_decision_sink,
            phase_observer: config.phase_observer,
            request_ids,
            connect_timeout: config.connect_timeout,
            io_timeout: config.io_timeout,
            max_connections: config.max_connections,
            server_conf: substrate.server_conf(),
            dns_limiter: substrate.dns_limiter(),
            tenant_fairness: config.tenant_fairness,
        };
        substrate.handle().spawn(worker.run(shutdown_rx, ack_tx));

        Ok(Self {
            local_addr,
            shutdown,
            shutdown_ack: Some(ack_rx),
            _substrate: substrate,
            policy_state,
            audit_healthy,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn readiness(&self) -> Result<WorkloadPepReadiness> {
        let guard = self
            .policy_state
            .read()
            .map_err(|_| EgressProxyError::OperationFailed {
                message: "egress proxy policy lock is poisoned".to_owned(),
            })?;
        let audit_healthy = self.audit_healthy.load(Ordering::SeqCst);
        Ok(guard.readiness(audit_healthy))
    }

    pub fn reload_policy(&self, policy: CompiledEgressPolicy) -> Result<PolicyGeneration> {
        let mut guard =
            self.policy_state
                .write()
                .map_err(|_| EgressProxyError::OperationFailed {
                    message: "egress proxy policy lock is poisoned".to_owned(),
                })?;
        Ok(guard.reload(policy))
    }

    pub fn reload_uncompiled_policy(&self, policy: EgressPolicy) -> Result<PolicyGeneration> {
        let compiled = policy
            .compile()
            .map_err(|message| EgressProxyError::OperationFailed {
                message: format!("invalid egress proxy policy reload: {message}"),
            })?;
        self.reload_policy(compiled)
    }
}

impl Drop for WorkloadPep {
    fn drop(&mut self) {
        let _ = self.shutdown.send(true);
        if let Some(shutdown_ack) = self.shutdown_ack.take() {
            let _ = shutdown_ack.recv_timeout(SHUTDOWN_ACK_TIMEOUT);
        }
    }
}

struct ProxyWorker {
    listener: TcpListener,
    policy_state: Arc<RwLock<EgressProxyPolicyState>>,
    audit_healthy: Arc<AtomicBool>,
    resolver: Resolver,
    dns_cache: DnsCacheConfig,
    credential_provider: CredentialSecretProviderRef,
    pool_identity: EgressProxyPoolIdentity,
    tls_authority: Option<WorkloadPepTlsAuthority>,
    decision_logger: DecisionLogger,
    durable_decision_sink: DurableDecisionSink,
    phase_observer: PhaseObserver,
    request_ids: Arc<RequestIdGenerator>,
    connect_timeout: Duration,
    io_timeout: Duration,
    max_connections: usize,
    server_conf: Arc<ServerConf>,
    dns_limiter: Arc<Semaphore>,
    tenant_fairness: Option<Arc<TenantFairness>>,
}

impl ProxyWorker {
    async fn run(self, shutdown: watch::Receiver<bool>, shutdown_ack: mpsc::Sender<()>) {
        let listener = match tokio::net::TcpListener::from_std(self.listener) {
            Ok(listener) => listener,
            Err(_) => {
                let _ = shutdown_ack.send(());
                return;
            }
        };
        let limiter = Arc::new(Semaphore::new(self.max_connections));
        let context = Arc::new(ClientHandlerContext {
            policy_state: self.policy_state,
            audit_healthy: self.audit_healthy,
            resolver: self.resolver,
            dns_cache: self.dns_cache,
            credential_provider: self.credential_provider,
            pool_identity: self.pool_identity,
            tls_authority: self.tls_authority,
            decision_logger: self.decision_logger,
            durable_decision_sink: self.durable_decision_sink,
            phase_observer: self.phase_observer,
            request_ids: self.request_ids,
            connect_timeout: self.connect_timeout,
            io_timeout: self.io_timeout,
            server_conf: self.server_conf,
            dns_limiter: self.dns_limiter,
            tenant_fairness: self.tenant_fairness,
        });
        accept_loop(listener, context, limiter, shutdown).await;
        let _ = shutdown_ack.send(());
    }
}

async fn accept_loop(
    listener: tokio::net::TcpListener,
    context: Arc<ClientHandlerContext>,
    limiter: Arc<Semaphore>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
            joined = connections.join_next(), if !connections.is_empty() => {
                let _ = joined;
            }
            accepted = listener.accept() => {
                let Ok((mut client, _)) = accepted else {
                    break;
                };
                let Ok(permit) = Arc::clone(&limiter).try_acquire_owned() else {
                    let _ = write_http_response_async(
                        &mut client,
                        HttpProxyResponse::service_unavailable(
                            "egress proxy connection limit exceeded",
                        ),
                    )
                    .await;
                    let _ = client.flush().await;
                    let _ = client.shutdown().await;
                    drain_over_limit_client(&mut client).await;
                    continue;
                };
                let handler_context = Arc::clone(&context);
                let connection_shutdown = shutdown.clone();
                connections.spawn(async move {
                    let _permit = permit;
                    let _ = handle_client(client, handler_context, connection_shutdown).await;
                });
            }
        }
    }
    connections.abort_all();
    while connections.join_next().await.is_some() {}
}

async fn drain_over_limit_client(client: &mut TcpStream) {
    let mut buffer = [0_u8; 1024];
    loop {
        match tokio::time::timeout(Duration::from_millis(10), client.read(&mut buffer)).await {
            Ok(Ok(0)) | Ok(Err(_)) | Err(_) => break,
            Ok(Ok(_)) => {}
        }
    }
}

#[derive(Clone)]
struct ClientHandlerContext {
    policy_state: Arc<RwLock<EgressProxyPolicyState>>,
    audit_healthy: Arc<AtomicBool>,
    resolver: Resolver,
    dns_cache: DnsCacheConfig,
    credential_provider: CredentialSecretProviderRef,
    pool_identity: EgressProxyPoolIdentity,
    tls_authority: Option<WorkloadPepTlsAuthority>,
    decision_logger: DecisionLogger,
    durable_decision_sink: DurableDecisionSink,
    phase_observer: PhaseObserver,
    request_ids: Arc<RequestIdGenerator>,
    connect_timeout: Duration,
    io_timeout: Duration,
    server_conf: Arc<ServerConf>,
    dns_limiter: Arc<Semaphore>,
    tenant_fairness: Option<Arc<TenantFairness>>,
}

impl ClientHandlerContext {
    fn terminal_sinks(&self) -> TerminalSinks<'_> {
        TerminalSinks::new(
            &self.durable_decision_sink,
            self.audit_healthy.as_ref(),
            &self.decision_logger,
        )
    }
}

async fn handle_client(
    mut client: TcpStream,
    context: Arc<ClientHandlerContext>,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let phase_recorder = RequestPhaseRecorder::new(Arc::clone(&context.phase_observer));
    let request_id = context.request_ids.next();
    // EE3: wall-clock task-occupancy accounting for the whole request task
    // (records on drop, including error paths). Occupancy, not CPU-seconds.
    let _task_time_span = context
        .tenant_fairness
        .as_ref()
        .map(|fairness| fairness.task_time_span());
    let mut buffer = Vec::new();
    match read_http_headers(&mut client, &mut buffer, context.io_timeout).await {
        Ok(()) => {}
        Err(ReadHeaderError::Response(response)) => {
            return malformed_terminal(
                &mut client,
                &phase_recorder,
                context.terminal_sinks(),
                &request_id,
                response,
            )
            .await;
        }
        Err(ReadHeaderError::Io(error)) => return Err(error),
    }
    phase_recorder.record(EgressProxyRequestPhase::CanonicalizeAuthority);
    let parsed = match parse_proxy_request(&buffer) {
        Ok(parsed) => parsed,
        Err(response) => {
            // Strict-parser rejects are the smuggling guards (bare CR/LF,
            // Transfer-Encoding, parser-differential authorities); blocked
            // attempts must reach the audit log even without a parsed
            // authority.
            return malformed_terminal(
                &mut client,
                &phase_recorder,
                context.terminal_sinks(),
                &request_id,
                response,
            )
            .await;
        }
    };
    phase_recorder.record(EgressProxyRequestPhase::RejectMalformedOrCallerCredentials);

    if !context.audit_healthy.load(Ordering::SeqCst) {
        return audit_unhealthy_terminal(
            &mut client,
            &phase_recorder,
            context.terminal_sinks(),
            ParsedRequestLogContext {
                request_id: &request_id,
                parsed: &parsed,
            },
        )
        .await;
    }

    // From here the request has a parsed authority. If the PEP is torn down
    // mid-request (drop aborts this task), the guard emits the terminal event
    // the aborted path never reached, so an already-authorized request that
    // touched upstream can never vanish from the audit log.
    let abort_guard = AbortTerminalGuard::new(
        phase_recorder.clone(),
        Arc::clone(&context.decision_logger),
        Arc::clone(&context.durable_decision_sink),
        Arc::clone(&context.audit_healthy),
        EgressDecisionLog::denied(
            &request_id,
            &parsed,
            "egress proxy terminated the request before a decision was recorded".to_owned(),
            None,
        ),
    );
    let response_started_signal = abort_guard.response_started_signal();

    let active_policy = context
        .policy_state
        .read()
        .map_err(|_| io::Error::other("egress proxy policy lock is poisoned"))?
        .active()
        .cloned();
    let Some(active_policy) = active_policy else {
        return deny_terminal(
            &mut client,
            &phase_recorder,
            context.terminal_sinks(),
            ParsedRequestLogContext {
                request_id: &request_id,
                parsed: &parsed,
            },
            None,
            None,
            HttpProxyResponse::forbidden(
                "egress proxy default deny: no active policy generation is ready",
            ),
        )
        .await;
    };

    phase_recorder.record(EgressProxyRequestPhase::PreDnsAuthorize);
    let pre_dns_authorization = authorize_hostname_before_dns(&active_policy.policy, &parsed);
    if !pre_dns_authorization.is_allowed() {
        return deny_terminal(
            &mut client,
            &phase_recorder,
            context.terminal_sinks(),
            ParsedRequestLogContext {
                request_id: &request_id,
                parsed: &parsed,
            },
            pre_dns_authorization.matched_rule().map(ToOwned::to_owned),
            Some(active_policy.policy_generation),
            HttpProxyResponse::forbidden(pre_dns_authorization.reason()),
        )
        .await;
    }

    if let Err(response) = reject_unapproved_caller_credentials_for_rule(
        active_policy.policy.sandbox(),
        pre_dns_authorization.matched_rule(),
        &parsed.header_lines,
    ) {
        return deny_terminal(
            &mut client,
            &phase_recorder,
            context.terminal_sinks(),
            ParsedRequestLogContext {
                request_id: &request_id,
                parsed: &parsed,
            },
            pre_dns_authorization.matched_rule().map(ToOwned::to_owned),
            Some(active_policy.policy_generation),
            response,
        )
        .await;
    }

    phase_recorder.record(EgressProxyRequestPhase::ResolveDns);
    let dns_resolution = match resolve_dns_async(
        Arc::clone(&context.dns_limiter),
        context.tenant_fairness.clone(),
        Arc::clone(&context.resolver),
        context.dns_cache.clone(),
        parsed.upstream_host.clone(),
        parsed.upstream_port,
        context.connect_timeout,
    )
    .await
    {
        Ok(resolution) if !resolution.addresses.is_empty() => resolution,
        Ok(_) => {
            return deny_terminal(
                &mut client,
                &phase_recorder,
                context.terminal_sinks(),
                ParsedRequestLogContext {
                    request_id: &request_id,
                    parsed: &parsed,
                },
                None,
                Some(active_policy.policy_generation),
                HttpProxyResponse::bad_gateway("egress proxy DNS resolution returned no addresses"),
            )
            .await;
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return deny_terminal(
                &mut client,
                &phase_recorder,
                context.terminal_sinks(),
                ParsedRequestLogContext {
                    request_id: &request_id,
                    parsed: &parsed,
                },
                None,
                Some(active_policy.policy_generation),
                HttpProxyResponse::forbidden(&format!(
                    "egress proxy DNS cache overflow default deny: {error}"
                )),
            )
            .await;
        }
        Err(error) => {
            return deny_terminal(
                &mut client,
                &phase_recorder,
                context.terminal_sinks(),
                ParsedRequestLogContext {
                    request_id: &request_id,
                    parsed: &parsed,
                },
                None,
                Some(active_policy.policy_generation),
                HttpProxyResponse::bad_gateway(&format!(
                    "egress proxy DNS resolution failed: {error}"
                )),
            )
            .await;
        }
    };
    let upstream_addr = dns_resolution.addresses[0];
    let egress_request = parsed
        .egress_request
        .clone()
        .with_resolved_ip(upstream_addr.ip());
    phase_recorder.record(EgressProxyRequestPhase::AuthorizeResolvedIp);
    let authorization = active_policy.policy.authorize(&egress_request);
    if !authorization.is_allowed() {
        return deny_terminal(
            &mut client,
            &phase_recorder,
            context.terminal_sinks(),
            ParsedRequestLogContext {
                request_id: &request_id,
                parsed: &parsed,
            },
            None,
            Some(active_policy.policy_generation),
            HttpProxyResponse::forbidden(authorization.reason()),
        )
        .await;
    }
    let matched_rule_ref =
        find_matched_rule(active_policy.policy.sandbox(), authorization.matched_rule());
    let matched_rule = authorization.matched_rule().map(ToOwned::to_owned);
    let pool_key = build_pool_key(
        &context.pool_identity,
        active_policy.policy_generation,
        &parsed,
        upstream_addr,
        matched_rule_ref,
    );
    phase_recorder.record(EgressProxyRequestPhase::SelectPoolKey);
    let peer_plan = PingoraPeerPlan::from_pool_key(&pool_key);
    phase_recorder.record(EgressProxyRequestPhase::BuildUpstreamPeer);

    match parsed.mode.clone() {
        ProxyRequestMode::ForwardHttp { origin_form } => {
            handle_forward_http(
                client,
                buffer,
                &request_id,
                parsed,
                origin_form.clone(),
                peer_plan,
                matched_rule,
                active_policy.policy_generation,
                authorization.matched_rule(),
                authorization.reason(),
                active_policy.policy.sandbox(),
                &context,
                phase_recorder,
                response_started_signal.clone(),
                shutdown,
            )
            .await
        }
        ProxyRequestMode::ConnectTunnel => match classify_connect(matched_rule_ref) {
            ConnectInterceptAction::Splice => {
                let allowed_decision_log = EgressDecisionLog::allowed(
                    &request_id,
                    &parsed,
                    None,
                    authorization.reason().to_owned(),
                    matched_rule.clone(),
                )
                .with_policy_generation(active_policy.policy_generation);
                if let Err(error) = record_durable_decision(
                    &phase_recorder,
                    &context.durable_decision_sink,
                    context.audit_healthy.as_ref(),
                    &allowed_decision_log,
                ) {
                    let mut client = client;
                    return audit_failure_terminal(
                        &mut client,
                        &phase_recorder,
                        context.terminal_sinks(),
                        ParsedRequestLogContext {
                            request_id: &request_id,
                            parsed: &parsed,
                        },
                        matched_rule,
                        active_policy.policy_generation,
                        error,
                    )
                    .await;
                }
                phase_recorder.record(EgressProxyRequestPhase::Forward);
                let upstream = match connect_upstream(upstream_addr, context.connect_timeout).await
                {
                    Ok(upstream) => upstream,
                    Err(_) => {
                        let mut client = client;
                        return upstream_error_terminal(
                            &mut client,
                            &phase_recorder,
                            context.terminal_sinks(),
                            &request_id,
                            &parsed,
                            matched_rule,
                            active_policy.policy_generation,
                        )
                        .await;
                    }
                };
                let result = splice_connect(
                    client,
                    upstream,
                    &buffer[parsed.body_offset..],
                    context.io_timeout,
                    response_started_signal.clone(),
                    allowed_decision_log
                        .clone()
                        .into_terminal_after_response(ABORT_AFTER_RESPONSE_REASON),
                )
                .await
                .map(|(to_upstream, to_workload)| {
                    // EE3: relay copy-loop byte metering, per tenant.
                    if let Some(fairness) = &context.tenant_fairness {
                        fairness.record_bytes_to_upstream(to_upstream);
                        fairness.record_bytes_to_workload(to_workload);
                    }
                });
                phase_recorder.record(EgressProxyRequestPhase::ResponseFilters);
                match result {
                    Ok(()) => {
                        emit_terminal_log(
                            &phase_recorder,
                            &context.decision_logger,
                            allowed_decision_log.into_terminal(),
                        );
                        response_started_signal.disarm();
                        Ok(())
                    }
                    Err(error) if response_started_signal.response_started() => {
                        let decision_log = allowed_decision_log
                            .into_terminal_after_response(UPSTREAM_FAILURE_AFTER_RESPONSE_REASON);
                        let _ = record_durable_decision(
                            &phase_recorder,
                            &context.durable_decision_sink,
                            context.audit_healthy.as_ref(),
                            &decision_log,
                        );
                        emit_terminal_log(&phase_recorder, &context.decision_logger, decision_log);
                        response_started_signal.disarm();
                        Err(error)
                    }
                    Err(error) => {
                        emit_terminal_log(
                            &phase_recorder,
                            &context.decision_logger,
                            allowed_decision_log.into_terminal(),
                        );
                        response_started_signal.disarm();
                        Err(error)
                    }
                }
            }
            ConnectInterceptAction::Intercept(_) => {
                let Some(tls_authority) = context.tls_authority.as_ref() else {
                    return deny_terminal(
                        &mut client,
                        &phase_recorder,
                        context.terminal_sinks(),
                        ParsedRequestLogContext {
                            request_id: &request_id,
                            parsed: &parsed,
                        },
                        matched_rule,
                        Some(active_policy.policy_generation),
                        HttpProxyResponse::forbidden(
                            "HTTPS interception failed closed: TLS authority is unavailable",
                        ),
                    )
                    .await;
                };
                let outer_allowed_decision_log = EgressDecisionLog::allowed(
                    &request_id,
                    &parsed,
                    None,
                    authorization.reason().to_owned(),
                    matched_rule.clone(),
                )
                .with_policy_generation(active_policy.policy_generation);
                if let Err(error) = record_durable_decision(
                    &phase_recorder,
                    &context.durable_decision_sink,
                    context.audit_healthy.as_ref(),
                    &outer_allowed_decision_log,
                ) {
                    return audit_failure_terminal(
                        &mut client,
                        &phase_recorder,
                        context.terminal_sinks(),
                        ParsedRequestLogContext {
                            request_id: &request_id,
                            parsed: &parsed,
                        },
                        matched_rule.clone(),
                        active_policy.policy_generation,
                        error,
                    )
                    .await;
                }
                // `Forward` is recorded inside the intercept path immediately
                // before upstream contact: the decrypted inner request still
                // has credential mutation and bounded DLP ahead of it, and the
                // phase trace must keep those before `Forward`.
                let intercept_result = intercept_connect_h1(
                    client,
                    &buffer[parsed.body_offset..],
                    HttpsInterceptContext {
                        parsed_connect: &parsed,
                        upstream_addr,
                        policy: &active_policy.policy,
                        outer_matched_rule: matched_rule.clone(),
                        credential_provider: context.credential_provider.as_ref(),
                        tls_authority,
                        phase_recorder: &phase_recorder,
                        decision_logger: &context.decision_logger,
                        durable_decision_sink: &context.durable_decision_sink,
                        audit_healthy: context.audit_healthy.as_ref(),
                        response_started_signal: response_started_signal.clone(),
                        request_id: &request_id,
                        policy_generation: active_policy.policy_generation,
                        tenant_fairness: context.tenant_fairness.clone(),
                        connect_timeout: context.connect_timeout,
                        io_timeout: context.io_timeout,
                    },
                )
                .await;
                phase_recorder.record(EgressProxyRequestPhase::ResponseFilters);
                let (decision_log, relay_failed_after_response) = match intercept_result {
                    Ok(completion) => (
                        completion
                            .decision_log
                            .with_policy_generation(active_policy.policy_generation),
                        completion.relay_failed_after_response,
                    ),
                    Err(error) => (
                        EgressDecisionLog::denied(
                            &request_id,
                            &parsed,
                            format!("HTTPS interception failed closed: {error}"),
                            matched_rule,
                        )
                        .with_policy_generation(active_policy.policy_generation),
                        false,
                    ),
                };
                // Client-visible denies inside the intercept path emit their
                // terminal log BEFORE writing the response (log-before-respond,
                // matching `deny_terminal`); skip re-emission for those.
                if !phase_recorder.terminal_recorded() {
                    if relay_failed_after_response {
                        debug_assert!(decision_log.is_allowed());
                        let decision_log = decision_log
                            .into_terminal_after_response(UPSTREAM_FAILURE_AFTER_RESPONSE_REASON);
                        let _ = record_durable_decision(
                            &phase_recorder,
                            &context.durable_decision_sink,
                            context.audit_healthy.as_ref(),
                            &decision_log,
                        );
                        emit_terminal_log(&phase_recorder, &context.decision_logger, decision_log);
                    } else {
                        if !decision_log.is_allowed()
                            && record_durable_decision(
                                &phase_recorder,
                                &context.durable_decision_sink,
                                context.audit_healthy.as_ref(),
                                &decision_log,
                            )
                            .is_err()
                        {
                            emit_terminal_log(
                                &phase_recorder,
                                &context.decision_logger,
                                decision_log,
                            );
                            response_started_signal.disarm();
                            return Ok(());
                        }
                        let decision_log = if decision_log.is_allowed() {
                            decision_log.into_terminal()
                        } else {
                            decision_log
                        };
                        if decision_log.is_allowed() {
                            let _ = record_durable_decision(
                                &phase_recorder,
                                &context.durable_decision_sink,
                                context.audit_healthy.as_ref(),
                                &decision_log,
                            );
                        }
                        emit_terminal_log(&phase_recorder, &context.decision_logger, decision_log);
                    }
                    response_started_signal.disarm();
                }
                Ok(())
            }
        },
    }
}

#[allow(clippy::too_many_arguments)]
async fn handle_forward_http(
    client: TcpStream,
    mut buffer: Vec<u8>,
    request_id: &str,
    parsed: ParsedProxyRequest,
    origin_form: String,
    peer_plan: PingoraPeerPlan,
    matched_rule: Option<String>,
    policy_generation: PolicyGeneration,
    matched_rule_name: Option<&str>,
    authorization_reason: &str,
    policy: &CompiledEgressPolicy,
    context: &ClientHandlerContext,
    phase_recorder: RequestPhaseRecorder,
    response_started_signal: ResponseStartedSignal,
    shutdown: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut client = client;
    let enforcement = match prepare_proxy_request_enforcement(
        &parsed,
        ProxyRequestEnforcementContext {
            policy,
            matched_rule: matched_rule_name,
            reason: authorization_reason,
            credential_provider: context.credential_provider.as_ref(),
            phase_recorder: &phase_recorder,
            request_id,
        },
    ) {
        Ok(enforcement) => enforcement,
        Err(response) => {
            return deny_terminal(
                &mut client,
                &phase_recorder,
                context.terminal_sinks(),
                ParsedRequestLogContext {
                    request_id,
                    parsed: &parsed,
                },
                matched_rule,
                Some(policy_generation),
                response,
            )
            .await;
        }
    };
    let inspected_body = if enforcement.requires_dlp() {
        if let Some(content_length) = parsed.content_length {
            if enforcement
                .dlp_max_inspection_bytes()
                .is_some_and(|max| content_length <= max)
            {
                match read_exact_body_into_buffer(
                    &mut client,
                    &mut buffer,
                    parsed.body_offset,
                    content_length,
                    context.io_timeout,
                )
                .await
                {
                    Ok(body) => Some(body),
                    Err(response) => {
                        return deny_terminal(
                            &mut client,
                            &phase_recorder,
                            context.terminal_sinks(),
                            ParsedRequestLogContext {
                                request_id,
                                parsed: &parsed,
                            },
                            matched_rule,
                            Some(policy_generation),
                            response,
                        )
                        .await;
                    }
                }
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };
    let prepared = match enforcement.finish(inspected_body) {
        Ok(prepared) => prepared,
        Err(response) => {
            return deny_terminal(
                &mut client,
                &phase_recorder,
                context.terminal_sinks(),
                ParsedRequestLogContext {
                    request_id,
                    parsed: &parsed,
                },
                matched_rule,
                Some(policy_generation),
                response,
            )
            .await;
        }
    };

    if prepared.inspected_body.is_none()
        && parsed.content_length.is_none()
        && buffer.len() > parsed.body_offset
    {
        return deny_terminal(
            &mut client,
            &phase_recorder,
            context.terminal_sinks(),
            ParsedRequestLogContext {
                request_id,
                parsed: &parsed,
            },
            matched_rule,
            Some(policy_generation),
            HttpProxyResponse::bad_request(
                "egress proxy HTTP request bodies require Content-Length",
            ),
        )
        .await;
    }

    let allowed_decision_log = prepared
        .decision_log
        .clone()
        .with_policy_generation(policy_generation);
    let abort_after_response_log = allowed_decision_log
        .clone()
        .into_terminal_after_response(ABORT_AFTER_RESPONSE_REASON);
    if let Err(error) = record_durable_decision(
        &phase_recorder,
        &context.durable_decision_sink,
        context.audit_healthy.as_ref(),
        &allowed_decision_log,
    ) {
        return audit_failure_terminal(
            &mut client,
            &phase_recorder,
            context.terminal_sinks(),
            ParsedRequestLogContext {
                request_id,
                parsed: &parsed,
            },
            matched_rule.clone(),
            policy_generation,
            error,
        )
        .await;
    }
    let pingora_buffer =
        render_pingora_downstream_request(&parsed, &prepared.header_lines, &buffer);
    let final_response_write_gate = FinalResponseWriteGate::new();
    let plan = ForwardRequestPlan {
        parsed: parsed.clone(),
        downstream_target: downstream_target(&parsed),
        peer_plan,
        prepared_header_lines: prepared.header_lines,
        origin_form,
        allowed_decision_log,
        matched_rule,
        policy_generation,
        connect_timeout: context.connect_timeout,
        io_timeout: context.io_timeout,
        phase_recorder,
        decision_logger: Arc::clone(&context.decision_logger),
        durable_decision_sink: Arc::clone(&context.durable_decision_sink),
        audit_healthy: Arc::clone(&context.audit_healthy),
        response_started_signal: response_started_signal.clone(),
        final_response_write_gate: final_response_write_gate.clone(),
    };
    // Pingora 0.8 runs `ProxyHttp::response_filter` while constructing an
    // `HttpTask::Header`; `Session::write_response_tasks()` later calls
    // `HttpSession::write_response_header()` to write downstream. There is no
    // post-write `ProxyHttp` hook, so the first successful write on our
    // downstream stream wrapper is the earliest confirmed response-start point.
    let stream = PrereadStream::new(client, pingora_buffer).with_response_started_signal(
        response_started_signal,
        abort_after_response_log,
        final_response_write_gate,
    );
    let session = HttpSession::new_http1(Box::new(stream));
    let app = Arc::new(http_proxy(
        &context.server_conf,
        NimbusForwardApp::new(plan),
    ));
    let _ = app.process_new_http(session, &shutdown).await;
    Ok(())
}

enum ReadHeaderError {
    Response(HttpProxyResponse),
    Io(io::Error),
}

async fn read_http_headers(
    client: &mut TcpStream,
    buffer: &mut Vec<u8>,
    io_timeout: Duration,
) -> std::result::Result<(), ReadHeaderError> {
    let mut chunk = [0_u8; 1024];
    loop {
        let read = match tokio::time::timeout(io_timeout, client.read(&mut chunk)).await {
            Ok(Ok(read)) => read,
            Ok(Err(error)) => return Err(ReadHeaderError::Io(error)),
            Err(_) => {
                return Err(ReadHeaderError::Io(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "client timed out before sending HTTP headers",
                )));
            }
        };
        if read == 0 {
            return Err(ReadHeaderError::Io(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "client closed before sending HTTP headers",
            )));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if find_header_end(buffer).is_some() {
            return Ok(());
        }
        if buffer.len() > MAX_HTTP_HEADER_BYTES {
            return Err(ReadHeaderError::Response(
                HttpProxyResponse::request_header_fields_too_large(
                    "egress proxy request headers are too large",
                ),
            ));
        }
    }
}

/// Resolves DNS on the blocking pool under the substrate's node-wide budget.
/// The permit travels INTO the blocking closure so the budget counts
/// actually-running resolver threads: a resolution that outlives its request
/// (blocking work is uncancellable) keeps holding its permit instead of
/// silently eating shared blocking-pool capacity, and both the permit wait and
/// the result wait are bounded so a wedged resolver fails this request closed
/// instead of stalling it.
async fn resolve_dns_async(
    dns_limiter: Arc<Semaphore>,
    tenant_fairness: Option<Arc<TenantFairness>>,
    resolver: Resolver,
    dns_cache: DnsCacheConfig,
    host: String,
    port: u16,
    wait_timeout: Duration,
) -> io::Result<crate::dns::DnsResolution> {
    // EE3: the per-tenant budget is acquired BEFORE the node-wide guard, so a
    // tenant over its budget fails ITS request closed without consuming
    // shared resolver capacity (it cannot starve other tenants).
    let tenant_permit = match &tenant_fairness {
        Some(fairness) => fairness.acquire_dns(wait_timeout).await?,
        None => None,
    };
    let permit = match tokio::time::timeout(wait_timeout, dns_limiter.acquire_owned()).await {
        Ok(Ok(permit)) => permit,
        Ok(Err(_)) => {
            return Err(io::Error::other("egress proxy DNS resolver budget closed"));
        }
        Err(_) => {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "egress proxy DNS resolver capacity exhausted",
            ));
        }
    };
    let resolution = tokio::task::spawn_blocking(move || {
        let _tenant_permit = tenant_permit;
        let _permit = permit;
        resolve_dns(&resolver, &dns_cache, &host, port)
    });
    match tokio::time::timeout(wait_timeout, resolution).await {
        Ok(joined) => joined
            .map_err(|error| io::Error::other(format!("DNS resolver task failed: {error}")))?,
        Err(_) => Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "egress proxy DNS resolution timed out",
        )),
    }
}

fn authorize_hostname_before_dns(
    policy: &LayeredEgressPolicy,
    parsed: &ParsedProxyRequest,
) -> nimbus_egress::EgressAuthorization {
    policy.authorize_hostname_without_resolved_ip(&parsed.egress_request)
}

fn find_matched_rule<'a>(
    policy: &'a CompiledEgressPolicy,
    matched_rule: Option<&str>,
) -> Option<&'a EgressRule> {
    let matched_rule = matched_rule?;
    policy
        .policy()
        .rules()
        .iter()
        .find(|rule| rule.name == matched_rule)
}

fn build_pool_key(
    identity: &EgressProxyPoolIdentity,
    policy_generation: PolicyGeneration,
    parsed: &ParsedProxyRequest,
    resolved_peer: SocketAddr,
    matched_rule: Option<&EgressRule>,
) -> EgressProxyPoolKey {
    let credential_identity = matched_rule
        .and_then(|rule| rule.credential.as_ref())
        .map(|credential| credential.credential_ref.clone());
    let has_dlp = matched_rule.is_some_and(|rule| !rule.dlp.is_empty());
    let sni = matches!(parsed.egress_request.protocol, EgressProtocol::Https)
        .then(|| parsed.upstream_host.clone());
    EgressProxyPoolKey {
        tenant_id: identity.tenant_id.clone(),
        workload_id: identity.workload_id.clone(),
        substrate: identity.substrate,
        policy_generation,
        credential_dlp_mode: EgressProxyCredentialDlpMode::from_rule_requirements(
            credential_identity.is_some(),
            has_dlp,
        ),
        credential_identity,
        destination: format!(
            "{}://{}:{}",
            egress_protocol_scheme(parsed.egress_request.protocol),
            parsed.upstream_host,
            parsed.upstream_port
        ),
        resolved_peer,
        sni,
        tls_verification: TlsVerificationMode::WebPki,
        client_cert_identity: None,
        alpn: vec!["http/1.1".to_owned()],
        proxy_settings: None,
    }
}

fn egress_protocol_scheme(protocol: EgressProtocol) -> &'static str {
    match protocol {
        EgressProtocol::Tcp => "tcp",
        EgressProtocol::Http => "http",
        EgressProtocol::Https => "https",
    }
}

fn render_pingora_downstream_request(
    parsed: &ParsedProxyRequest,
    header_lines: &[String],
    buffer: &[u8],
) -> Vec<u8> {
    let ProxyRequestMode::ForwardHttp { origin_form } = &parsed.mode else {
        return buffer.to_vec();
    };
    let mut rendered = format!("{} {} {}\r\n", parsed.method, origin_form, parsed.version);
    for line in header_lines {
        rendered.push_str(line);
        rendered.push_str("\r\n");
    }
    rendered.push_str("\r\n");
    let mut bytes = rendered.into_bytes();
    bytes.extend_from_slice(&buffer[parsed.body_offset..]);
    bytes
}
