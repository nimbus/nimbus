use std::error::Error as StdError;
use std::fmt;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use nimbus_core::TenantId;
use nimbus_egress::{CompiledEgressPolicy, EgressPolicy, EgressProtocol, EgressRequest};
use url::Url;

const MAX_HTTP_HEADER_BYTES: usize = 16 * 1024;
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_IO_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_MAX_CONNECTIONS: usize = 128;

type Resolver = Arc<dyn Fn(&str, u16) -> io::Result<Vec<SocketAddr>> + Send + Sync + 'static>;

pub type Result<T> = std::result::Result<T, EgressProxyError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressProxyError {
    OperationFailed { message: String },
}

impl fmt::Display for EgressProxyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationFailed { message } => formatter.write_str(message),
        }
    }
}

impl StdError for EgressProxyError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PolicyGeneration(u64);

impl PolicyGeneration {
    fn initial() -> Self {
        Self(1)
    }

    fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressProxyReadiness {
    pub ready: bool,
    pub policy_generation: Option<PolicyGeneration>,
}

#[derive(Debug, Clone)]
struct LastKnownGoodPolicy {
    policy_generation: PolicyGeneration,
    policy: CompiledEgressPolicy,
}

#[derive(Debug, Clone, Default)]
struct EgressProxyPolicyState {
    last_known_good: Option<LastKnownGoodPolicy>,
}

impl EgressProxyPolicyState {
    fn with_policy(policy: CompiledEgressPolicy) -> Self {
        Self {
            last_known_good: Some(LastKnownGoodPolicy {
                policy_generation: PolicyGeneration::initial(),
                policy,
            }),
        }
    }

    fn active(&self) -> Option<&LastKnownGoodPolicy> {
        self.last_known_good.as_ref()
    }

    fn reload(&mut self, policy: CompiledEgressPolicy) -> PolicyGeneration {
        let next_generation = self
            .last_known_good
            .as_ref()
            .map(|current| current.policy_generation.next())
            .unwrap_or_else(PolicyGeneration::initial);
        self.last_known_good = Some(LastKnownGoodPolicy {
            policy_generation: next_generation,
            policy,
        });
        next_generation
    }

    fn readiness(&self) -> EgressProxyReadiness {
        EgressProxyReadiness {
            ready: self.last_known_good.is_some(),
            policy_generation: self
                .last_known_good
                .as_ref()
                .map(|policy| policy.policy_generation),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsCacheConfig {
    pub max_hosts: usize,
    pub max_addresses_per_host: usize,
    pub min_ttl: Duration,
    pub max_ttl: Duration,
}

impl Default for DnsCacheConfig {
    fn default() -> Self {
        Self {
            max_hosts: 1024,
            max_addresses_per_host: 16,
            min_ttl: Duration::from_secs(1),
            max_ttl: Duration::from_secs(300),
        }
    }
}

impl DnsCacheConfig {
    pub fn with_max_addresses_per_host(mut self, max_addresses_per_host: usize) -> Self {
        self.max_addresses_per_host = max_addresses_per_host;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DnsResolution {
    pub canonical_host: String,
    pub alias_chain: Vec<String>,
    pub addresses: Vec<SocketAddr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EgressProxySubstrate {
    Container,
    Isolate,
    Wasm,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TlsVerificationMode {
    WebPki,
    Disabled,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EgressProxyPoolKey {
    pub tenant_id: TenantId,
    pub substrate: EgressProxySubstrate,
    pub policy_generation: PolicyGeneration,
    pub credential_identity: Option<String>,
    pub destination: String,
    pub resolved_peer: SocketAddr,
    pub sni: Option<String>,
    pub tls_verification: TlsVerificationMode,
    pub client_cert_identity: Option<String>,
    pub alpn: Vec<String>,
    pub proxy_settings: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressProxyRequestPhase {
    CanonicalizeAuthority,
    ResolveDns,
    AuthorizeResolvedPeer,
    SelectPoolKey,
    Dial,
    Relay,
}

pub const REQUEST_PHASE_ORDER: [EgressProxyRequestPhase; 6] = [
    EgressProxyRequestPhase::CanonicalizeAuthority,
    EgressProxyRequestPhase::ResolveDns,
    EgressProxyRequestPhase::AuthorizeResolvedPeer,
    EgressProxyRequestPhase::SelectPoolKey,
    EgressProxyRequestPhase::Dial,
    EgressProxyRequestPhase::Relay,
];

#[derive(Clone)]
pub struct EgressProxyConfig {
    pub bind_addr: SocketAddr,
    pub policy: Option<CompiledEgressPolicy>,
    pub connect_timeout: Duration,
    pub io_timeout: Duration,
    pub max_connections: usize,
    pub dns_cache: DnsCacheConfig,
    resolver: Resolver,
}

impl EgressProxyConfig {
    pub fn new(policy: CompiledEgressPolicy) -> Self {
        Self {
            bind_addr: SocketAddr::from(([127, 0, 0, 1], 0)),
            policy: Some(policy),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            io_timeout: DEFAULT_IO_TIMEOUT,
            max_connections: DEFAULT_MAX_CONNECTIONS,
            dns_cache: DnsCacheConfig::default(),
            resolver: Arc::new(resolve_socket_addrs),
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

    #[cfg(test)]
    fn with_resolver(mut self, resolver: Resolver) -> Self {
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
    connect_timeout: Duration,
    io_timeout: Duration,
    connection_limiter: ConnectionLimiter,
}

impl ProxyWorker {
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
                    let policy_state = Arc::clone(&self.policy_state);
                    let resolver = Arc::clone(&self.resolver);
                    let dns_cache = self.dns_cache.clone();
                    let connect_timeout = self.connect_timeout;
                    let io_timeout = self.io_timeout;
                    thread::spawn(move || {
                        let _connection_permit = connection_permit;
                        let _ = handle_client(
                            client,
                            policy_state,
                            resolver,
                            dns_cache,
                            connect_timeout,
                            io_timeout,
                        );
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
struct ConnectionLimiter {
    active: Arc<AtomicUsize>,
    max: usize,
}

impl ConnectionLimiter {
    fn new(max: usize) -> Self {
        Self {
            active: Arc::new(AtomicUsize::new(0)),
            max,
        }
    }

    fn try_acquire(&self) -> Option<ConnectionPermit> {
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

struct ConnectionPermit {
    active: Arc<AtomicUsize>,
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

struct ParsedProxyRequest {
    egress_request: EgressRequest,
    upstream_host: String,
    upstream_port: u16,
    mode: ProxyRequestMode,
    method: String,
    version: String,
    header_lines: Vec<String>,
    body_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ProxyRequestMode {
    ForwardHttp { origin_form: String },
    ConnectTunnel,
}

fn handle_client(
    mut client: TcpStream,
    policy_state: Arc<RwLock<EgressProxyPolicyState>>,
    resolver: Resolver,
    dns_cache: DnsCacheConfig,
    connect_timeout: Duration,
    io_timeout: Duration,
) -> io::Result<()> {
    client.set_read_timeout(Some(io_timeout))?;
    client.set_write_timeout(Some(io_timeout))?;

    let mut buffer = Vec::new();
    read_http_headers(&mut client, &mut buffer)?;
    let parsed = match parse_proxy_request(&buffer) {
        Ok(parsed) => parsed,
        Err(response) => return write_http_response(&mut client, response),
    };

    let active_policy = policy_state
        .read()
        .map_err(|_| io::Error::other("egress proxy policy lock is poisoned"))?
        .active()
        .cloned();
    let Some(active_policy) = active_policy else {
        return write_http_response(
            &mut client,
            HttpProxyResponse::forbidden(
                "egress proxy default deny: no active policy generation is ready",
            ),
        );
    };

    let dns_resolution = match resolve_dns(
        &resolver,
        &dns_cache,
        &parsed.upstream_host,
        parsed.upstream_port,
    ) {
        Ok(resolution) if !resolution.addresses.is_empty() => resolution,
        Ok(_) => {
            return write_http_response(
                &mut client,
                HttpProxyResponse::bad_gateway("egress proxy DNS resolution returned no addresses"),
            );
        }
        Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            return write_http_response(
                &mut client,
                HttpProxyResponse::forbidden(&format!(
                    "egress proxy DNS cache overflow default deny: {error}"
                )),
            );
        }
        Err(error) => {
            return write_http_response(
                &mut client,
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
    let _policy_generation = active_policy.policy_generation;
    let authorization = active_policy.policy.authorize(&egress_request);
    if !authorization.is_allowed() {
        return write_http_response(
            &mut client,
            HttpProxyResponse::forbidden(authorization.reason()),
        );
    }

    let mut upstream = TcpStream::connect_timeout(&upstream_addr, connect_timeout)?;
    upstream.set_nonblocking(false)?;
    upstream.set_read_timeout(Some(io_timeout))?;
    upstream.set_write_timeout(Some(io_timeout))?;
    match &parsed.mode {
        ProxyRequestMode::ForwardHttp { .. } => {
            let request = render_upstream_request(&parsed);
            upstream.write_all(request.as_bytes())?;
            // Flush the body bytes that arrived co-buffered with the headers,
            // then relay both directions: the rest of the client request body
            // streams to upstream while the response streams back. Without the
            // bidirectional relay, a request body larger than the initial header
            // read was silently truncated and the request stalled until timeout
            // (M3). CONNECT already did this via `tunnel_connect`; the forward
            // path was the asymmetric exception.
            upstream.write_all(&buffer[parsed.body_offset..])?;
            relay_bidirectional(client, upstream)
        }
        ProxyRequestMode::ConnectTunnel => {
            tunnel_connect(client, upstream, &buffer[parsed.body_offset..])
        }
    }
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

fn parse_proxy_request(
    buffer: &[u8],
) -> std::result::Result<ParsedProxyRequest, HttpProxyResponse> {
    let Some(header_end) = find_header_end(buffer) else {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy request is missing HTTP headers",
        ));
    };
    let body_offset = header_end + 4;
    let headers = std::str::from_utf8(&buffer[..header_end]).map_err(|_| {
        HttpProxyResponse::bad_request("egress proxy request headers must be UTF-8")
    })?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if method.is_empty() || target.is_empty() || version.is_empty() || parts.next().is_some() {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy request line must be METHOD absolute-uri HTTP-version",
        ));
    }
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = parse_connect_authority(target)?;
        let egress_request =
            EgressRequest::new(EgressProtocol::Https, host.clone(), port).with_http("CONNECT", "");
        return Ok(ParsedProxyRequest {
            egress_request,
            upstream_host: host,
            upstream_port: port,
            mode: ProxyRequestMode::ConnectTunnel,
            method: method.to_owned(),
            version: version.to_owned(),
            header_lines: lines.map(ToOwned::to_owned).collect(),
            body_offset,
        });
    }
    let url = Url::parse(target).map_err(|_| {
        HttpProxyResponse::bad_request("egress proxy target must be an absolute URI")
    })?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy canonical authority must not include userinfo",
        ));
    }
    let protocol = match url.scheme() {
        "http" => EgressProtocol::Http,
        "https" => {
            return Err(HttpProxyResponse::not_implemented(
                "egress proxy HTTPS requests must use CONNECT",
            ));
        }
        _ => {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy only supports http and https targets",
            ));
        }
    };
    let host = url
        .host_str()
        .ok_or_else(|| HttpProxyResponse::bad_request("egress proxy target needs a host"))?
        .to_owned();
    let host = canonicalize_proxy_host(&host)?;
    let port = url.port_or_known_default().ok_or_else(|| {
        HttpProxyResponse::bad_request("egress proxy target needs an explicit port")
    })?;
    let origin_form = origin_form(&url);
    let egress_request =
        EgressRequest::new(protocol, host.clone(), port).with_http(method, url.path());
    let header_lines = lines
        .filter(|line| {
            let header = line
                .split_once(':')
                .map(|(name, _)| name.trim())
                .unwrap_or_default();
            !header.eq_ignore_ascii_case("connection")
                && !header.eq_ignore_ascii_case("proxy-connection")
        })
        .map(ToOwned::to_owned)
        .collect();

    Ok(ParsedProxyRequest {
        egress_request,
        upstream_host: host,
        upstream_port: port,
        mode: ProxyRequestMode::ForwardHttp { origin_form },
        method: method.to_owned(),
        version: version.to_owned(),
        header_lines,
        body_offset,
    })
}

fn render_upstream_request(parsed: &ParsedProxyRequest) -> String {
    let ProxyRequestMode::ForwardHttp { origin_form } = &parsed.mode else {
        return String::new();
    };
    let mut rendered = format!("{} {} {}\r\n", parsed.method, origin_form, parsed.version);
    for line in &parsed.header_lines {
        rendered.push_str(line);
        rendered.push_str("\r\n");
    }
    rendered.push_str("Connection: close\r\n\r\n");
    rendered
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

/// Relays bytes in both directions between an already-prepared client and
/// upstream socket until each side closes its write half. Both the CONNECT
/// tunnel and the plain-HTTP forward path use this so a request body larger
/// than the initial header read is fully delivered upstream (and the full
/// response streamed back), instead of being truncated.
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

fn parse_connect_authority(target: &str) -> std::result::Result<(String, u16), HttpProxyResponse> {
    if target.contains("://") || target.contains('/') || target.contains('@') {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy CONNECT target must be host:port",
        ));
    }
    let (host, port) = if let Some(rest) = target.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy CONNECT IPv6 target needs closing bracket",
            ));
        };
        let Some(port) = suffix.strip_prefix(':') else {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy CONNECT target needs a port",
            ));
        };
        (host.to_owned(), port)
    } else {
        let Some((host, port)) = target.rsplit_once(':') else {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy CONNECT target needs a port",
            ));
        };
        if host.contains(':') {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy CONNECT IPv6 target must use brackets",
            ));
        }
        (host.to_owned(), port)
    };
    if host.is_empty() {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy CONNECT target needs a host",
        ));
    }
    let port = port.parse::<u16>().map_err(|_| {
        HttpProxyResponse::bad_request("egress proxy CONNECT port must be a number")
    })?;
    if port == 0 {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy CONNECT port must not be 0",
        ));
    }
    Ok((canonicalize_proxy_host(&host)?, port))
}

fn origin_form(url: &Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

fn resolve_socket_addrs(host: &str, port: u16) -> io::Result<Vec<SocketAddr>> {
    (host, port).to_socket_addrs().map(|addrs| addrs.collect())
}

fn resolve_dns(
    resolver: &Resolver,
    dns_cache: &DnsCacheConfig,
    host: &str,
    port: u16,
) -> io::Result<DnsResolution> {
    if dns_cache.max_hosts == 0 || dns_cache.max_addresses_per_host == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "DNS cache caps must be nonzero",
        ));
    }
    let canonical_host = host.to_ascii_lowercase();
    let addresses = resolver(&canonical_host, port)?;
    if addresses.len() > dns_cache.max_addresses_per_host {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} addresses exceeds max_addresses_per_host {}",
                addresses.len(),
                dns_cache.max_addresses_per_host
            ),
        ));
    }
    Ok(DnsResolution {
        canonical_host: canonical_host.clone(),
        alias_chain: vec![canonical_host],
        addresses,
    })
}

fn canonicalize_proxy_host(host: &str) -> std::result::Result<String, HttpProxyResponse> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy target needs a host",
        ));
    }
    if trimmed.contains('%')
        || trimmed.contains('@')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(char::is_whitespace)
    {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy canonical authority rejected ambiguous host",
        ));
    }
    Ok(trimmed.trim_end_matches('.').to_ascii_lowercase())
}

struct HttpProxyResponse {
    status: &'static str,
    body: String,
}

impl HttpProxyResponse {
    fn bad_request(body: &str) -> Self {
        Self {
            status: "400 Bad Request",
            body: body.to_owned(),
        }
    }

    fn forbidden(body: &str) -> Self {
        Self {
            status: "403 Forbidden",
            body: body.to_owned(),
        }
    }

    fn not_implemented(body: &str) -> Self {
        Self {
            status: "501 Not Implemented",
            body: body.to_owned(),
        }
    }

    fn bad_gateway(body: &str) -> Self {
        Self {
            status: "502 Bad Gateway",
            body: body.to_owned(),
        }
    }

    fn service_unavailable(body: &str) -> Self {
        Self {
            status: "503 Service Unavailable",
            body: body.to_owned(),
        }
    }

    fn request_header_fields_too_large(body: &str) -> Self {
        Self {
            status: "431 Request Header Fields Too Large",
            body: body.to_owned(),
        }
    }
}

fn write_http_response(client: &mut TcpStream, response: HttpProxyResponse) -> io::Result<()> {
    let rendered = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        response.body.len(),
        response.body
    );
    client.write_all(rendered.as_bytes())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Instant;

    use super::*;
    use nimbus_egress::{EgressPolicy, EgressProtocol, EgressRule};

    #[test]
    fn egress_proxy_allows_matching_http_request() {
        let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        let proxy = start_test_proxy(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .with_methods(["GET"])
        .with_path_prefixes(["/ok"])
        .allow_internal_ips(true)]));

        let response = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
                upstream.addr.port()
            ),
        );

        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "expected upstream response through proxy, got: {response}"
        );
        let upstream_request = upstream
            .request
            .recv_timeout(Duration::from_secs(1))
            .expect("upstream should receive the rewritten origin-form request");
        assert!(
            upstream_request.starts_with("GET /ok HTTP/1.1"),
            "proxy should forward origin-form request, got: {upstream_request}"
        );
    }

    #[test]
    fn egress_proxy_forwards_full_request_body_larger_than_header_buffer() {
        // The proxy reads headers in 1024-byte chunks and stops at the header
        // terminator, so only a small body prefix is co-buffered. A 16 KiB body
        // must still reach upstream in full via the bidirectional relay; before
        // M3 the forward path truncated it to the co-buffered prefix.
        let body_len = 16 * 1024;
        let upstream = TestHttpBodyEchoServer::start();
        let proxy = start_test_proxy(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .with_methods(["POST"])
        .with_path_prefixes(["/upload"])
        .allow_internal_ips(true)]));

        let body = "x".repeat(body_len);
        let response = proxy_request(
            proxy.local_addr(),
            format!(
                "POST http://allowed.test:{}/upload HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: {}\r\n\r\n{}",
                upstream.addr.port(),
                body_len,
                body
            ),
        );

        assert!(
            response.starts_with("HTTP/1.1 200 OK"),
            "proxy should relay the upstream response after the full body, got: {response}"
        );
        let received = upstream
            .body_len
            .recv_timeout(Duration::from_secs(2))
            .expect("upstream should receive the request body");
        assert_eq!(
            received, body_len,
            "proxy must forward the entire request body, not just the co-buffered prefix"
        );
    }

    #[test]
    fn egress_proxy_denies_default_policy_without_contacting_upstream() {
        let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        let proxy = start_test_proxy(CompiledEgressPolicy::deny_all());

        let response = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
                upstream.addr.port()
            ),
        );

        assert!(
            response.starts_with("HTTP/1.1 403 Forbidden"),
            "default deny should reject the request, got: {response}"
        );
        assert!(
            upstream
                .request
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "default-denied requests must not contact upstream"
        );
    }

    #[test]
    fn egress_proxy_without_active_policy_denies_before_dns() {
        let resolver_calls = Arc::new(AtomicUsize::new(0));
        let resolver_call_counter = Arc::clone(&resolver_calls);
        let resolver = Arc::new(move |_host: &str, _port: u16| {
            resolver_call_counter.fetch_add(1, Ordering::SeqCst);
            Err(io::Error::other(
                "resolver must not be called without policy",
            ))
        });
        let proxy = EgressProxy::start(
            EgressProxyConfig::without_active_policy()
                .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
                .with_resolver(resolver),
        )
        .expect("proxy should start without active policy");

        let readiness = proxy.readiness().expect("readiness should be observable");
        assert!(!readiness.ready);
        assert_eq!(readiness.policy_generation, None);

        let response = proxy_request(
            proxy.local_addr(),
            "GET http://blocked.test:443/ HTTP/1.1\r\nHost: blocked.test\r\n\r\n".to_string(),
        );

        assert!(
            response.starts_with("HTTP/1.1 403 Forbidden")
                && response.contains("no active policy generation"),
            "missing policy generation must fail closed, got: {response}"
        );
        assert_eq!(
            resolver_calls.load(Ordering::SeqCst),
            0,
            "missing active policy must deny before DNS resolution"
        );
    }

    #[test]
    fn egress_proxy_denies_dns_resolved_internal_targets() {
        let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        let proxy = start_test_proxy(allow_policy([EgressRule::new(
            "metadata-by-name",
            EgressProtocol::Http,
            "metadata.test",
            upstream.addr.port(),
        )]));

        let response = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://metadata.test:{}/latest HTTP/1.1\r\nHost: metadata.test\r\n\r\n",
                upstream.addr.port()
            ),
        );

        assert!(
            response.starts_with("HTTP/1.1 403 Forbidden")
                && response.contains("internal/non-global targets"),
            "resolved loopback target should be denied as SSRF/internal egress, got: {response}"
        );
        assert!(
            upstream
                .request
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "resolved-internal denied requests must not contact upstream"
        );
    }

    #[test]
    fn egress_proxy_denies_l7_method_and_path_mismatches() {
        let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        let proxy = start_test_proxy(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .with_methods(["GET"])
        .with_path_prefixes(["/ok"])
        .allow_internal_ips(true)]));

        let denied_method = proxy_request(
            proxy.local_addr(),
            format!(
                "POST http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: 0\r\n\r\n",
                upstream.addr.port()
            ),
        );
        let denied_path = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://allowed.test:{}/blocked HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
                upstream.addr.port()
            ),
        );

        assert!(
            denied_method.starts_with("HTTP/1.1 403 Forbidden"),
            "method mismatch should be denied, got: {denied_method}"
        );
        assert!(
            denied_path.starts_with("HTTP/1.1 403 Forbidden"),
            "path mismatch should be denied, got: {denied_path}"
        );
        assert!(
            upstream
                .request
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "L7-denied requests must not contact upstream"
        );
    }

    #[test]
    fn egress_proxy_reload_updates_policy_without_restart() {
        let first = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst");
        let second = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecond");
        let proxy = start_test_proxy(CompiledEgressPolicy::deny_all());

        let denied = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://first.test:{}/ok HTTP/1.1\r\nHost: first.test\r\n\r\n",
                first.addr.port()
            ),
        );
        assert!(
            denied.starts_with("HTTP/1.1 403 Forbidden"),
            "initial deny-all policy should deny, got: {denied}"
        );

        proxy
            .reload_policy(allow_policy([EgressRule::new(
                "first",
                EgressProtocol::Http,
                "first.test",
                first.addr.port(),
            )
            .allow_internal_ips(true)]))
            .expect("proxy policy reload should succeed");
        let allowed = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://first.test:{}/ok HTTP/1.1\r\nHost: first.test\r\n\r\n",
                first.addr.port()
            ),
        );
        assert!(
            allowed.starts_with("HTTP/1.1 200 OK") && allowed.contains("first"),
            "reloaded policy should allow first upstream, got: {allowed}"
        );

        proxy
            .reload_policy(allow_policy([EgressRule::new(
                "second",
                EgressProtocol::Http,
                "second.test",
                second.addr.port(),
            )
            .allow_internal_ips(true)]))
            .expect("second proxy policy reload should succeed");
        let old_target = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://first.test:{}/ok HTTP/1.1\r\nHost: first.test\r\n\r\n",
                first.addr.port()
            ),
        );
        let new_target = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://second.test:{}/ok HTTP/1.1\r\nHost: second.test\r\n\r\n",
                second.addr.port()
            ),
        );
        assert!(
            old_target.starts_with("HTTP/1.1 403 Forbidden"),
            "old target should be denied after reload, got: {old_target}"
        );
        assert!(
            new_target.starts_with("HTTP/1.1 200 OK") && new_target.contains("second"),
            "new target should be allowed after reload, got: {new_target}"
        );
    }

    #[test]
    fn egress_proxy_invalid_reload_preserves_last_known_good_generation() {
        let first = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst");
        let proxy = start_test_proxy(allow_policy([EgressRule::new(
            "first",
            EgressProtocol::Http,
            "first.test",
            first.addr.port(),
        )
        .allow_internal_ips(true)]));
        let initial = proxy.readiness().expect("readiness should be observable");
        assert_eq!(initial.policy_generation, Some(PolicyGeneration::initial()));

        let invalid =
            EgressPolicy::new([EgressRule::new("wildcard", EgressProtocol::Http, "*", 80)]);
        let error = proxy
            .reload_uncompiled_policy(invalid)
            .expect_err("invalid reload should fail closed");
        assert!(
            error
                .to_string()
                .contains("invalid egress proxy policy reload"),
            "reload error should explain invalid policy: {error}"
        );
        let after_error = proxy.readiness().expect("readiness should remain readable");
        assert_eq!(
            after_error.policy_generation,
            Some(PolicyGeneration::initial()),
            "invalid reload must keep the last-known-good generation"
        );

        let allowed = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://first.test:{}/ok HTTP/1.1\r\nHost: first.test\r\n\r\n",
                first.addr.port()
            ),
        );
        assert!(
            allowed.starts_with("HTTP/1.1 200 OK"),
            "last-known-good policy should still authorize first target, got: {allowed}"
        );
    }

    #[test]
    fn egress_proxy_dns_overflow_defaults_to_deny_before_dial() {
        let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        let resolver = Arc::new(move |_host: &str, port: u16| {
            Ok(vec![
                SocketAddr::from(([127, 0, 0, 1], port)),
                SocketAddr::from(([127, 0, 0, 2], port)),
            ])
        });
        let proxy = EgressProxy::start(
            EgressProxyConfig::new(allow_policy([EgressRule::new(
                "allowed",
                EgressProtocol::Http,
                "allowed.test",
                upstream.addr.port(),
            )
            .allow_internal_ips(true)]))
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
            .with_dns_cache_config(DnsCacheConfig::default().with_max_addresses_per_host(1))
            .with_resolver(resolver),
        )
        .expect("proxy should start");

        let response = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
                upstream.addr.port()
            ),
        );

        assert!(
            response.starts_with("HTTP/1.1 403 Forbidden")
                && response.contains("DNS cache overflow default deny"),
            "DNS overflow must fail closed, got: {response}"
        );
        assert!(
            upstream
                .request
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "DNS-overflow denied requests must not contact upstream"
        );
    }

    #[test]
    fn egress_proxy_rejects_ambiguous_canonical_authorities() {
        let resolver_calls = Arc::new(AtomicUsize::new(0));
        let resolver_call_counter = Arc::clone(&resolver_calls);
        let resolver = Arc::new(move |_host: &str, _port: u16| {
            resolver_call_counter.fetch_add(1, Ordering::SeqCst);
            Ok(vec![SocketAddr::from(([127, 0, 0, 1], 80))])
        });
        let proxy = EgressProxy::start(
            EgressProxyConfig::new(allow_policy([EgressRule::new(
                "allowed",
                EgressProtocol::Http,
                "allowed.test",
                80,
            )
            .allow_internal_ips(true)]))
            .with_resolver(resolver),
        )
        .expect("proxy should start");

        let userinfo = proxy_request(
            proxy.local_addr(),
            "GET http://allowed.test@127.0.0.1/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
                .to_string(),
        );
        let encoded = proxy_request(
            proxy.local_addr(),
            "CONNECT allowed.test%2eexample:443 HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_string(),
        );

        assert!(
            userinfo.starts_with("HTTP/1.1 400 Bad Request")
                && userinfo.contains("canonical authority"),
            "userinfo authority smuggling should reject, got: {userinfo}"
        );
        assert!(
            encoded.starts_with("HTTP/1.1 400 Bad Request")
                && encoded.contains("canonical authority"),
            "encoded authority should reject, got: {encoded}"
        );
        assert_eq!(
            resolver_calls.load(Ordering::SeqCst),
            0,
            "canonicalization failures must happen before DNS resolution"
        );
    }

    #[test]
    fn egress_proxy_request_phase_order_is_explicit() {
        assert_eq!(
            REQUEST_PHASE_ORDER,
            [
                EgressProxyRequestPhase::CanonicalizeAuthority,
                EgressProxyRequestPhase::ResolveDns,
                EgressProxyRequestPhase::AuthorizeResolvedPeer,
                EgressProxyRequestPhase::SelectPoolKey,
                EgressProxyRequestPhase::Dial,
                EgressProxyRequestPhase::Relay,
            ]
        );
    }

    #[test]
    fn egress_proxy_pool_key_covers_security_relevant_identity() {
        let tenant_id = TenantId::new("tenant-a").expect("tenant id should be valid");
        let base = EgressProxyPoolKey {
            tenant_id,
            substrate: EgressProxySubstrate::Container,
            policy_generation: PolicyGeneration::initial(),
            credential_identity: Some("secret:stripe".to_string()),
            destination: "https://api.stripe.com:443".to_string(),
            resolved_peer: SocketAddr::from(([203, 0, 113, 10], 443)),
            sni: Some("api.stripe.com".to_string()),
            tls_verification: TlsVerificationMode::WebPki,
            client_cert_identity: Some("client-cert:payments".to_string()),
            alpn: vec!["h2".to_string()],
            proxy_settings: Some("direct".to_string()),
        };

        let mut changed = base.clone();
        changed.policy_generation = base.policy_generation.next();
        assert_ne!(base, changed);
        let mut changed = base.clone();
        changed.credential_identity = Some("secret:github".to_string());
        assert_ne!(base, changed);
        let mut changed = base.clone();
        changed.resolved_peer = SocketAddr::from(([203, 0, 113, 11], 443));
        assert_ne!(base, changed);
        let mut changed = base.clone();
        changed.sni = Some("uploads.stripe.com".to_string());
        assert_ne!(base, changed);
        let mut changed = base.clone();
        changed.tls_verification = TlsVerificationMode::Disabled;
        assert_ne!(base, changed);
        let mut changed = base.clone();
        changed.client_cert_identity = None;
        assert_ne!(base, changed);
        let mut changed = base.clone();
        changed.alpn = vec!["http/1.1".to_string()];
        assert_ne!(base, changed);
        let mut changed = base.clone();
        changed.proxy_settings = Some("upstream-proxy-a".to_string());
        assert_ne!(base, changed);
    }

    fn start_test_proxy(policy: CompiledEgressPolicy) -> EgressProxy {
        let resolver = Arc::new(|host: &str, port: u16| {
            let ip = match host {
                "allowed.test" | "first.test" | "second.test" | "metadata.test" => {
                    [127, 0, 0, 1].into()
                }
                _ => return Err(io::Error::other(format!("unexpected host {host}"))),
            };
            Ok(vec![SocketAddr::new(ip, port)])
        });
        EgressProxy::start(
            EgressProxyConfig::new(policy)
                .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
                .with_resolver(resolver),
        )
        .expect("proxy should start")
    }

    fn allow_policy<const N: usize>(rules: [EgressRule; N]) -> CompiledEgressPolicy {
        EgressPolicy::new(rules)
            .compile()
            .expect("policy should compile")
    }

    fn proxy_request(proxy_addr: SocketAddr, request: String) -> String {
        let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");
        stream
            .write_all(request.as_bytes())
            .expect("client should write request");
        let mut response = String::new();
        stream
            .read_to_string(&mut response)
            .expect("client should read response");
        response
    }

    fn read_until_contains(stream: &mut TcpStream, expected: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut response = Vec::new();
        while Instant::now() < deadline {
            let mut chunk = [0_u8; 128];
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    response.extend_from_slice(&chunk[..read]);
                    let rendered = String::from_utf8_lossy(&response);
                    if rendered.contains(expected) {
                        return rendered.to_string();
                    }
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("client should read CONNECT tunnel response: {error}"),
            }
        }
        String::from_utf8_lossy(&response).to_string()
    }

    struct TestHttpServer {
        addr: SocketAddr,
        request: mpsc::Receiver<String>,
    }

    impl TestHttpServer {
        fn start(response: &'static str) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
            let addr = listener
                .local_addr()
                .expect("upstream address should resolve");
            let (request_tx, request_rx) = mpsc::channel();
            thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut request = [0_u8; 1024];
                    let read = stream.read(&mut request).unwrap_or_default();
                    let _ = request_tx.send(String::from_utf8_lossy(&request[..read]).to_string());
                    let _ = stream.write_all(response.as_bytes());
                }
            });
            Self {
                addr,
                request: request_rx,
            }
        }
    }

    #[test]
    fn egress_proxy_config_defaults_to_loopback_ephemeral_bind() {
        let config = EgressProxyConfig::new(CompiledEgressPolicy::deny_all());

        assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 0)));
        assert_eq!(config.max_connections, DEFAULT_MAX_CONNECTIONS);
    }

    #[test]
    fn egress_proxy_rejects_zero_connection_limit() {
        let error = match EgressProxy::start(
            EgressProxyConfig::new(CompiledEgressPolicy::deny_all()).with_max_connections(0),
        ) {
            Ok(_) => panic!("zero connection limit should be rejected"),
            Err(error) => error,
        };

        assert!(
            error.to_string().contains("max_connections"),
            "error should identify the invalid connection limit: {error}"
        );
    }

    #[test]
    fn connection_limiter_caps_active_permits() {
        let limiter = ConnectionLimiter::new(1);
        let permit = limiter
            .try_acquire()
            .expect("first connection should acquire the only permit");

        assert!(
            limiter.try_acquire().is_none(),
            "second concurrent connection should be rejected"
        );

        drop(permit);
        assert!(
            limiter.try_acquire().is_some(),
            "released permits should become available again"
        );
    }

    #[test]
    fn egress_proxy_strips_hop_by_hop_proxy_headers() {
        let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        let proxy = start_test_proxy(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]));

        let _ = proxy_request(
            proxy.local_addr(),
            format!(
                "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\nConnection: keep-alive\r\nProxy-Connection: keep-alive\r\n\r\n",
                upstream.addr.port()
            ),
        );
        let upstream_request = upstream
            .request
            .recv_timeout(Duration::from_secs(1))
            .expect("upstream should receive request");

        assert!(!upstream_request.contains("Proxy-Connection"));
        assert!(!upstream_request.contains("Connection: keep-alive"));
        assert!(upstream_request.contains("Connection: close"));
    }

    #[test]
    fn egress_proxy_allows_https_connect_tunnel() {
        let upstream = TestTcpServer::start(b"pong");
        let proxy = start_test_proxy(allow_policy([EgressRule::new(
            "allowed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]));

        let mut stream =
            TcpStream::connect(proxy.local_addr()).expect("client should connect to proxy");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");
        stream
            .write_all(
                format!(
                    "CONNECT allowed.test:{} HTTP/1.1\r\nHost: allowed.test:{}\r\n\r\nping",
                    upstream.addr.port(),
                    upstream.addr.port()
                )
                .as_bytes(),
            )
            .expect("CONNECT request should write");
        let upstream_payload = upstream
            .request
            .recv_timeout(Duration::from_secs(1))
            .expect("upstream should receive tunneled bytes");
        assert_eq!(upstream_payload, "ping");
        let response = read_until_contains(&mut stream, "pong");
        assert!(
            response.starts_with("HTTP/1.1 200 Connection Established"),
            "CONNECT should establish a tunnel, got: {response}"
        );
        assert!(
            response.contains("pong"),
            "CONNECT tunnel should relay upstream payload, got: {response}"
        );
    }

    #[test]
    fn egress_proxy_rejects_https_absolute_uri_without_connect() {
        let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
        let proxy = start_test_proxy(allow_policy([EgressRule::new(
            "allowed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]));

        let response = proxy_request(
            proxy.local_addr(),
            format!(
                "GET https://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
                upstream.addr.port()
            ),
        );

        assert!(
            response.starts_with("HTTP/1.1 501 Not Implemented")
                && response.contains("must use CONNECT"),
            "HTTPS without CONNECT should fail closed, got: {response}"
        );
        assert!(
            upstream
                .request
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "unsupported HTTPS requests must not contact upstream"
        );
    }

    struct TestTcpServer {
        addr: SocketAddr,
        request: mpsc::Receiver<String>,
    }

    impl TestTcpServer {
        fn start(response: &'static [u8]) -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
            let addr = listener
                .local_addr()
                .expect("upstream address should resolve");
            let (request_tx, request_rx) = mpsc::channel();
            thread::spawn(move || {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut request = [0_u8; 4];
                    if stream.read_exact(&mut request).is_ok() {
                        let _ = request_tx.send(String::from_utf8_lossy(&request).to_string());
                        let _ = stream.write_all(response);
                    }
                }
            });
            Self {
                addr,
                request: request_rx,
            }
        }
    }

    /// Upstream that reads a full Content-Length request body and reports how
    /// many body bytes it actually received, so a test can prove the proxy
    /// relayed the entire body rather than a truncated prefix.
    struct TestHttpBodyEchoServer {
        addr: SocketAddr,
        body_len: mpsc::Receiver<usize>,
    }

    impl TestHttpBodyEchoServer {
        fn start() -> Self {
            let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
            let addr = listener
                .local_addr()
                .expect("upstream address should resolve");
            let (body_tx, body_rx) = mpsc::channel();
            thread::spawn(move || {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let mut buffer = Vec::new();
                let mut chunk = [0_u8; 1024];
                let header_end = loop {
                    match stream.read(&mut chunk) {
                        Ok(0) => break None,
                        Ok(read) => {
                            buffer.extend_from_slice(&chunk[..read]);
                            if let Some(pos) =
                                buffer.windows(4).position(|window| window == b"\r\n\r\n")
                            {
                                break Some(pos);
                            }
                        }
                        Err(_) => break None,
                    }
                };
                let Some(header_end) = header_end else {
                    return;
                };
                let content_length = String::from_utf8_lossy(&buffer[..header_end])
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.trim()
                            .eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                let mut received = buffer.len() - (header_end + 4);
                while received < content_length {
                    match stream.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(read) => received += read,
                        Err(_) => break,
                    }
                }
                let _ = body_tx.send(received);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
            });
            Self {
                addr,
                body_len: body_rx,
            }
        }
    }
}
