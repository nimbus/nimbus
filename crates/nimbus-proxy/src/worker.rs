use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nimbus_egress::{CompiledEgressPolicy, EgressPolicy};

use crate::credentials::CredentialSecretStore;
use crate::decision_log::{DecisionLogger, EgressDecisionLog, noop_decision_logger};
use crate::dns::{DnsCacheConfig, Resolver, resolve_dns, resolve_socket_addrs};
use crate::enforcement::prepare_proxy_request_enforcement;
use crate::error::{EgressProxyError, Result};
use crate::policy_state::{EgressProxyPolicyState, EgressProxyReadiness, PolicyGeneration};
use crate::request::{
    ParsedProxyRequest, ProxyRequestMode, find_header_end, parse_proxy_request,
    render_upstream_request,
};
use crate::response::{HttpProxyResponse, write_http_response};
use crate::{
    DEFAULT_CONNECT_TIMEOUT, DEFAULT_IO_TIMEOUT, DEFAULT_MAX_CONNECTIONS, MAX_HTTP_HEADER_BYTES,
};

pub struct EgressProxyConfig {
    pub bind_addr: SocketAddr,
    pub policy: Option<CompiledEgressPolicy>,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    pub max_connections: usize,
    pub dns_cache: DnsCacheConfig,
    pub credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
    resolver: Resolver,
}

impl EgressProxyConfig {
    pub fn new(policy: CompiledEgressPolicy) -> Self {
        // Delegate to the policy-less constructor so the full default field set
        // lives in exactly one place; only the active policy differs. (egress
        // audit L13.)
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
            decision_logger: noop_decision_logger(),
            resolver: Arc::new(resolve_socket_addrs),
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
        self
    }

    pub fn with_decision_logger(mut self, decision_logger: DecisionLogger) -> Self {
        self.decision_logger = decision_logger;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_resolver(mut self, resolver: Resolver) -> Self {
        self.resolver = resolver;
        self
    }
}

pub struct EgressProxy {
    local_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    policy_state: Arc<RwLock<EgressProxyPolicyState>>,
}

impl EgressProxy {
    pub fn start(config: EgressProxyConfig) -> Result<Self> {
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

        let shutdown = Arc::new(AtomicBool::new(false));
        let policy_state = Arc::new(RwLock::new(
            config
                .policy
                .map(EgressProxyPolicyState::with_policy)
                .unwrap_or_default(),
        ));
        let worker = ProxyWorker {
            listener,
            shutdown: Arc::clone(&shutdown),
            policy_state: Arc::clone(&policy_state),
            resolver: config.resolver,
            dns_cache: config.dns_cache,
            credential_store: config.credential_store,
            decision_logger: config.decision_logger,
            connect_timeout: config.connect_timeout,
            io_timeout: config.io_timeout,
            connection_limiter: ConnectionLimiter::new(config.max_connections),
        };
        let join = thread::Builder::new()
            .name("nimbus-egress-proxy".to_owned())
            .spawn(move || worker.run())
            .map_err(|error| EgressProxyError::OperationFailed {
                message: format!("failed to spawn egress proxy: {error}"),
            })?;

        Ok(Self {
            local_addr,
            shutdown,
            join: Some(join),
            policy_state,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn readiness(&self) -> Result<EgressProxyReadiness> {
        let guard = self
            .policy_state
            .read()
            .map_err(|_| EgressProxyError::OperationFailed {
                message: "egress proxy policy lock is poisoned".to_owned(),
            })?;
        Ok(guard.readiness())
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

impl Drop for EgressProxy {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(&self.local_addr, Duration::from_millis(100));
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

struct ProxyWorker {
    listener: TcpListener,
    shutdown: Arc<AtomicBool>,
    policy_state: Arc<RwLock<EgressProxyPolicyState>>,
    resolver: Resolver,
    dns_cache: DnsCacheConfig,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
    connect_timeout: Duration,
    io_timeout: Duration,
    connection_limiter: ConnectionLimiter,
}

impl ProxyWorker {
    fn handler_context(&self) -> ClientHandlerContext {
        ClientHandlerContext {
            policy_state: Arc::clone(&self.policy_state),
            resolver: Arc::clone(&self.resolver),
            dns_cache: self.dns_cache.clone(),
            credential_store: self.credential_store.clone(),
            decision_logger: Arc::clone(&self.decision_logger),
            connect_timeout: self.connect_timeout,
            io_timeout: self.io_timeout,
        }
    }

    fn run(self) {
        while !self.shutdown.load(Ordering::SeqCst) {
            match self.listener.accept() {
                Ok((mut client, _)) => {
                    let Some(connection_permit) = self.connection_limiter.try_acquire() else {
                        let _ = client.set_write_timeout(Some(self.io_timeout));
                        let _ = write_http_response(
                            &mut client,
                            HttpProxyResponse::service_unavailable(
                                "egress proxy connection limit exceeded",
                            ),
                        );
                        continue;
                    };
                    let handler_context = self.handler_context();
                    thread::spawn(move || {
                        let _connection_permit = connection_permit;
                        let _ = handle_client(client, handler_context);
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    }
}

#[derive(Clone)]
struct ClientHandlerContext {
    policy_state: Arc<RwLock<EgressProxyPolicyState>>,
    resolver: Resolver,
    dns_cache: DnsCacheConfig,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
    connect_timeout: Duration,
    io_timeout: Duration,
}

#[derive(Clone)]
pub(crate) struct ConnectionLimiter {
    active: Arc<AtomicUsize>,
    max: usize,
}

impl ConnectionLimiter {
    pub(crate) fn new(max: usize) -> Self {
        Self {
            active: Arc::new(AtomicUsize::new(0)),
            max,
        }
    }

    pub(crate) fn try_acquire(&self) -> Option<ConnectionPermit> {
        let mut current = self.active.load(Ordering::Acquire);
        loop {
            if current >= self.max {
                return None;
            }
            match self.active.compare_exchange_weak(
                current,
                current + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    return Some(ConnectionPermit {
                        active: Arc::clone(&self.active),
                    });
                }
                Err(observed) => current = observed,
            }
        }
    }
}

pub(crate) struct ConnectionPermit {
    active: Arc<AtomicUsize>,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Audits a terminal deny and writes the matching fail-closed response. Every
/// post-parse deny path funnels through here so a blocked request emits exactly
/// one decision-log record (`allowed = false`) carrying the reason and matched
/// rule, mirroring the response body without leaking secret material.
fn deny_terminal(
    client: &mut TcpStream,
    decision_logger: &DecisionLogger,
    parsed: &ParsedProxyRequest,
    matched_rule: Option<String>,
    policy_generation: Option<PolicyGeneration>,
    response: HttpProxyResponse,
) -> io::Result<()> {
    let mut decision_log =
        EgressDecisionLog::denied(parsed, response.body().to_owned(), matched_rule);
    if let Some(policy_generation) = policy_generation {
        decision_log = decision_log.with_policy_generation(policy_generation);
    }
    decision_logger(decision_log);
    write_http_response(client, response)
}

fn handle_client(mut client: TcpStream, context: ClientHandlerContext) -> io::Result<()> {
    client.set_read_timeout(Some(context.io_timeout))?;
    client.set_write_timeout(Some(context.io_timeout))?;

    let mut buffer = Vec::new();
    read_http_headers(&mut client, &mut buffer)?;
    let parsed = match parse_proxy_request(&buffer) {
        Ok(parsed) => parsed,
        Err(response) => return write_http_response(&mut client, response),
    };

    let active_policy = context
        .policy_state
        .read()
        .map_err(|_| io::Error::other("egress proxy policy lock is poisoned"))?
        .active()
        .cloned();
    let Some(active_policy) = active_policy else {
        return deny_terminal(
            &mut client,
            &context.decision_logger,
            &parsed,
            None,
            None,
            HttpProxyResponse::forbidden(
                "egress proxy default deny: no active policy generation is ready",
            ),
        );
    };

    let pre_dns_authorization = authorize_hostname_before_dns(&active_policy.policy, &parsed);
    if !pre_dns_authorization.is_allowed() {
        return deny_terminal(
            &mut client,
            &context.decision_logger,
            &parsed,
            pre_dns_authorization.matched_rule().map(ToOwned::to_owned),
            Some(active_policy.policy_generation),
            HttpProxyResponse::forbidden(pre_dns_authorization.reason()),
        );
    }

    let dns_resolution = match resolve_dns(
        &context.resolver,
        &context.dns_cache,
        &parsed.upstream_host,
        parsed.upstream_port,
    ) {
        Ok(resolution) if !resolution.addresses.is_empty() => resolution,
        Ok(_) => {
            return deny_terminal(
                &mut client,
                &context.decision_logger,
                &parsed,
                None,
                Some(active_policy.policy_generation),
                HttpProxyResponse::bad_gateway("egress proxy DNS resolution returned no addresses"),
            );
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return deny_terminal(
                &mut client,
                &context.decision_logger,
                &parsed,
                None,
                Some(active_policy.policy_generation),
                HttpProxyResponse::forbidden(&format!(
                    "egress proxy DNS cache overflow default deny: {error}"
                )),
            );
        }
        Err(error) => {
            return deny_terminal(
                &mut client,
                &context.decision_logger,
                &parsed,
                None,
                Some(active_policy.policy_generation),
                HttpProxyResponse::bad_gateway(&format!(
                    "egress proxy DNS resolution failed: {error}"
                )),
            );
        }
    };
    let upstream_addr = dns_resolution.addresses[0];
    let egress_request = parsed
        .egress_request
        .clone()
        .with_resolved_ip(upstream_addr.ip());
    let authorization = active_policy.policy.authorize(&egress_request);
    if !authorization.is_allowed() {
        return deny_terminal(
            &mut client,
            &context.decision_logger,
            &parsed,
            None,
            Some(active_policy.policy_generation),
            HttpProxyResponse::forbidden(authorization.reason()),
        );
    }
    let matched_rule = authorization.matched_rule().map(ToOwned::to_owned);
    let prepared = match prepare_proxy_request_enforcement(
        &mut client,
        &mut buffer,
        &parsed,
        &active_policy.policy,
        authorization.matched_rule(),
        authorization.reason(),
        &context.credential_store,
    ) {
        Ok(prepared) => prepared,
        // DLP block or credential deny: audit the terminal deny before
        // responding so blocked exfiltration attempts are never a blind spot.
        Err(response) => {
            return deny_terminal(
                &mut client,
                &context.decision_logger,
                &parsed,
                matched_rule,
                Some(active_policy.policy_generation),
                response,
            );
        }
    };

    if matches!(parsed.mode, ProxyRequestMode::ForwardHttp { .. })
        && prepared.inspected_body.is_none()
        && parsed.content_length.is_none()
        && buffer.len() > parsed.body_offset
    {
        return deny_terminal(
            &mut client,
            &context.decision_logger,
            &parsed,
            matched_rule,
            Some(active_policy.policy_generation),
            HttpProxyResponse::bad_request(
                "egress proxy HTTP request bodies require Content-Length",
            ),
        );
    }

    // The policy decision is final and allowed; record it exactly once before
    // dialing so every request emits a single terminal decision log.
    (context.decision_logger)(
        prepared
            .decision_log
            .clone()
            .with_policy_generation(active_policy.policy_generation),
    );

    let mut upstream = match TcpStream::connect_timeout(&upstream_addr, context.connect_timeout) {
        Ok(upstream) => upstream,
        // A dial failure is an operational fault, not a policy deny: surface a
        // 502 to the client instead of dropping the connection silently.
        Err(_) => {
            return write_http_response(
                &mut client,
                HttpProxyResponse::bad_gateway("egress proxy failed to dial the upstream"),
            );
        }
    };
    upstream.set_nonblocking(false)?;
    upstream.set_read_timeout(Some(context.io_timeout))?;
    upstream.set_write_timeout(Some(context.io_timeout))?;
    match &parsed.mode {
        ProxyRequestMode::ForwardHttp { .. } => {
            let request = render_upstream_request(&parsed, &prepared.header_lines);
            upstream.write_all(request.as_bytes())?;
            if let Some(body) = prepared.inspected_body {
                upstream.write_all(&body)?;
                let _ = upstream.shutdown(Shutdown::Write);
                io::copy(&mut upstream, &mut client)?;
                return Ok(());
            }
            if let Some(content_length) = parsed.content_length {
                write_known_request_body(
                    &mut client,
                    &mut upstream,
                    &buffer[parsed.body_offset..],
                    content_length,
                )?;
            }
            let _ = upstream.shutdown(Shutdown::Write);
            io::copy(&mut upstream, &mut client)?;
            Ok(())
        }
        ProxyRequestMode::ConnectTunnel => {
            tunnel_connect(client, upstream, &buffer[parsed.body_offset..])
        }
    }
}

fn authorize_hostname_before_dns(
    policy: &CompiledEgressPolicy,
    parsed: &ParsedProxyRequest,
) -> nimbus_egress::EgressAuthorization {
    policy.authorize_hostname_without_resolved_ip(&parsed.egress_request)
}

fn read_http_headers(client: &mut TcpStream, buffer: &mut Vec<u8>) -> io::Result<()> {
    let mut chunk = [0_u8; 1024];
    loop {
        let read = client.read(&mut chunk)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "client closed before sending HTTP headers",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
        if find_header_end(buffer).is_some() {
            return Ok(());
        }
        if buffer.len() > MAX_HTTP_HEADER_BYTES {
            let _ = write_http_response(
                client,
                HttpProxyResponse::request_header_fields_too_large(
                    "egress proxy request headers are too large",
                ),
            );
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP headers exceed maximum size",
            ));
        }
    }
}

fn tunnel_connect(
    mut client: TcpStream,
    mut upstream: TcpStream,
    buffered_client_bytes: &[u8],
) -> io::Result<()> {
    client.write_all(b"HTTP/1.1 200 Connection Established\r\nConnection: close\r\n\r\n")?;
    if !buffered_client_bytes.is_empty() {
        upstream.write_all(buffered_client_bytes)?;
    }
    relay_bidirectional(client, upstream)
}

fn write_known_request_body(
    client: &mut TcpStream,
    upstream: &mut TcpStream,
    buffered_client_bytes: &[u8],
    content_length: usize,
) -> io::Result<()> {
    let buffered_len = buffered_client_bytes.len().min(content_length);
    upstream.write_all(&buffered_client_bytes[..buffered_len])?;

    let mut remaining = content_length - buffered_len;
    let mut chunk = [0_u8; 8192];
    while remaining > 0 {
        let read_len = remaining.min(chunk.len());
        let read = client.read(&mut chunk[..read_len])?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "client closed before sending declared request body",
            ));
        }
        upstream.write_all(&chunk[..read])?;
        remaining -= read;
    }
    Ok(())
}

/// Relays bytes in both directions between an already-prepared client and
/// upstream socket until each side closes its write half. Both the CONNECT
/// tunnel and other full-duplex transports use this so payload bytes are not
/// truncated when both sides may continue writing after the initial headers.
fn relay_bidirectional(mut client: TcpStream, mut upstream: TcpStream) -> io::Result<()> {
    let mut upstream_reader = upstream.try_clone()?;
    let mut client_writer = client.try_clone()?;
    let upstream_to_client = thread::spawn(move || {
        let _ = io::copy(&mut upstream_reader, &mut client_writer);
        let _ = client_writer.shutdown(Shutdown::Write);
    });
    let _ = io::copy(&mut client, &mut upstream);
    let _ = upstream.shutdown(Shutdown::Write);
    let _ = upstream_to_client.join();
    Ok(())
}
