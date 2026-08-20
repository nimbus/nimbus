use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::credentials::{CredentialSecretProvider, CredentialSecretProviderRef};
use nimbus_core::TenantId;
use nimbus_egress::{
    CompiledEgressPolicy, EgressCredentialInjection, EgressDlpRule, EgressPolicy, EgressProtocol,
    EgressRule,
};
use nimbus_process_harness::PortWindow;

mod connect_tunnel;
mod credentials_dlp;
mod decision_log_phase;
mod dns_resolution;
mod forward_http;
mod https_intercept;
mod policy_lifecycle;
mod reachability_lint;
mod substrate_shutdown;

fn start_test_proxy(policy: CompiledEgressPolicy) -> WorkloadPep {
    start_test_proxy_with_store(policy, CredentialSecretStore::empty())
}

fn start_test_proxy_on_substrate(
    policy: CompiledEgressPolicy,
    substrate: ProxySubstrate,
) -> WorkloadPep {
    WorkloadPep::start(
        WorkloadPepConfig::new(policy)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
            .with_substrate(substrate)
            .with_resolver(loopback_test_resolver()),
    )
    .expect("proxy should start")
}

fn start_test_proxy_with_store(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
) -> WorkloadPep {
    start_test_proxy_with_store_and_logger(policy, credential_store, Arc::new(|_| {}))
}

fn start_test_proxy_with_store_and_logger(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
) -> WorkloadPep {
    start_test_proxy_with_store_logger_durable_sink_and_phase_observer(
        policy,
        credential_store,
        decision_logger,
        noop_durable_sink_for_test(),
        Arc::new(|_| {}),
    )
}

fn start_test_proxy_with_store_logger_and_durable_sink(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
    durable_decision_sink: DurableDecisionSink,
) -> WorkloadPep {
    start_test_proxy_with_store_logger_durable_sink_and_phase_observer(
        policy,
        credential_store,
        decision_logger,
        durable_decision_sink,
        Arc::new(|_| {}),
    )
}

fn start_test_proxy_with_store_logger_and_phase_observer(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
    phase_observer: crate::phase::PhaseObserver,
) -> WorkloadPep {
    start_test_proxy_with_store_logger_durable_sink_and_phase_observer(
        policy,
        credential_store,
        decision_logger,
        noop_durable_sink_for_test(),
        phase_observer,
    )
}

fn start_test_proxy_with_store_logger_durable_sink_and_phase_observer(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
    durable_decision_sink: DurableDecisionSink,
    phase_observer: crate::phase::PhaseObserver,
) -> WorkloadPep {
    WorkloadPep::start(
        WorkloadPepConfig::new(policy)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
            .with_credential_store(credential_store)
            .with_durable_decision_sink(durable_decision_sink)
            .with_decision_logger(decision_logger)
            .with_phase_observer(phase_observer)
            .with_resolver(loopback_test_resolver()),
    )
    .expect("proxy should start")
}

fn noop_durable_sink_for_test() -> DurableDecisionSink {
    Arc::new(|_| Ok(()))
}

fn failing_durable_sink_for_test() -> DurableDecisionSink {
    Arc::new(|_| Err(io::Error::other("test durable sink failure")))
}

fn capturing_durable_sink_for_test(
    captured: Arc<Mutex<Vec<EgressDecisionLog>>>,
) -> DurableDecisionSink {
    Arc::new(move |log| {
        captured
            .lock()
            .expect("durable log capture lock should hold")
            .push(log.clone());
        Ok(())
    })
}

fn blocking_second_durable_sink_for_test(
    captured: Arc<Mutex<Vec<EgressDecisionLog>>>,
) -> (DurableDecisionSink, mpsc::Receiver<()>, mpsc::Sender<()>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let (started_tx, started_rx) = mpsc::channel();
    let started_tx = Arc::new(Mutex::new(Some(started_tx)));
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Arc::new(Mutex::new(release_rx));
    let sink = {
        let calls = Arc::clone(&calls);
        let started_tx = Arc::clone(&started_tx);
        let release_rx = Arc::clone(&release_rx);
        Arc::new(move |log: &EgressDecisionLog| {
            let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
            captured
                .lock()
                .expect("durable log capture lock should hold")
                .push(log.clone());
            if call == 2 {
                if let Some(started_tx) = started_tx
                    .lock()
                    .expect("terminal append signal lock should hold")
                    .take()
                {
                    let _ = started_tx.send(());
                }
                release_rx
                    .lock()
                    .expect("terminal append release lock should hold")
                    .recv_timeout(Duration::from_secs(2))
                    .expect("test should release blocked terminal durable append");
            }
            Ok(())
        }) as DurableDecisionSink
    };
    (sink, started_rx, release_tx)
}

fn failing_second_durable_sink_for_test(
    captured: Arc<Mutex<Vec<EgressDecisionLog>>>,
) -> DurableDecisionSink {
    let calls = Arc::new(AtomicUsize::new(0));
    Arc::new(move |log| {
        let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
        if call == 2 {
            return Err(io::Error::other("test terminal durable sink failure"));
        }
        captured
            .lock()
            .expect("durable log capture lock should hold")
            .push(log.clone());
        Ok(())
    })
}

fn snapshot_durable_logs(captured: &Arc<Mutex<Vec<EgressDecisionLog>>>) -> Vec<EgressDecisionLog> {
    captured
        .lock()
        .expect("durable log capture lock should hold")
        .clone()
}

fn start_test_proxy_with_store_logger_tls_and_phase_observer(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
    tls_authority: WorkloadPepTlsAuthority,
    phase_observer: crate::phase::PhaseObserver,
) -> WorkloadPep {
    start_test_proxy_with_provider_logger_tls_and_phase_observer(
        policy,
        credential_store.into_provider(),
        decision_logger,
        tls_authority,
        phase_observer,
    )
}

fn start_test_proxy_with_provider_logger_tls_and_phase_observer(
    policy: CompiledEgressPolicy,
    credential_provider: CredentialSecretProviderRef,
    decision_logger: DecisionLogger,
    tls_authority: WorkloadPepTlsAuthority,
    phase_observer: crate::phase::PhaseObserver,
) -> WorkloadPep {
    start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
        policy,
        credential_provider,
        decision_logger,
        noop_durable_sink_for_test(),
        tls_authority,
        phase_observer,
    )
}

fn start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
    policy: CompiledEgressPolicy,
    credential_provider: CredentialSecretProviderRef,
    decision_logger: DecisionLogger,
    durable_decision_sink: DurableDecisionSink,
    tls_authority: WorkloadPepTlsAuthority,
    phase_observer: crate::phase::PhaseObserver,
) -> WorkloadPep {
    WorkloadPep::start(
        WorkloadPepConfig::new(policy)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
            .with_credential_provider(credential_provider)
            .with_decision_logger(decision_logger)
            .with_durable_decision_sink(durable_decision_sink)
            .with_tls_authority(tls_authority)
            .with_phase_observer(phase_observer)
            .with_resolver(loopback_test_resolver()),
    )
    .expect("proxy should start")
}

fn loopback_test_resolver() -> crate::dns::Resolver {
    Arc::new(|host: &str, port: u16| {
        let ip = match host {
            "allowed.test" | "denied.test" | "first.test" | "second.test" | "metadata.test"
            | "redirect.test" => [127, 0, 0, 1].into(),
            _ => return Err(io::Error::other(format!("unexpected host {host}"))),
        };
        Ok(vec![SocketAddr::new(ip, port)])
    })
}

struct MockCredentialProvider {
    entries: Vec<(String, String)>,
}

impl CredentialSecretProvider for MockCredentialProvider {
    fn resolve_credential_secret(&self, credential_ref: &str) -> Option<String> {
        self.entries
            .iter()
            .find_map(|(key, value)| (key == credential_ref).then(|| value.clone()))
    }
}

fn mock_credential_provider(
    entries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
) -> CredentialSecretProviderRef {
    Arc::new(MockCredentialProvider {
        entries: entries
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect(),
    })
}

fn recorded_phases() -> Arc<Mutex<Vec<EgressProxyRequestPhase>>> {
    Arc::new(Mutex::new(Vec::new()))
}

fn phase_observer(
    phases: &Arc<Mutex<Vec<EgressProxyRequestPhase>>>,
) -> crate::phase::PhaseObserver {
    let phases = Arc::clone(phases);
    Arc::new(move |phase| {
        phases
            .lock()
            .expect("phase trace lock should not be poisoned")
            .push(phase);
    })
}

fn snapshot_phases(
    phases: &Arc<Mutex<Vec<EgressProxyRequestPhase>>>,
) -> Vec<EgressProxyRequestPhase> {
    phases
        .lock()
        .expect("phase trace lock should not be poisoned")
        .clone()
}

fn allow_policy<const N: usize>(rules: [EgressRule; N]) -> CompiledEgressPolicy {
    EgressPolicy::new(rules)
        .compile()
        .expect("policy should compile")
}

fn proxy_request(proxy_addr: SocketAddr, request: String) -> String {
    proxy_request_until_close(proxy_addr, request).expect("client should read response")
}

fn proxy_request_until_close(proxy_addr: SocketAddr, request: String) -> io::Result<String> {
    let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    stream
        .write_all(request.as_bytes())
        .expect("client should write request");
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
}

fn proxy_request_bytes_until_close(proxy_addr: SocketAddr, request: String) -> io::Result<Vec<u8>> {
    let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    stream
        .write_all(request.as_bytes())
        .expect("client should write request");
    let mut response = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => response.extend_from_slice(&chunk[..read]),
            Err(error) if error.kind() == io::ErrorKind::ConnectionReset => break,
            Err(error) => return Err(error),
        }
    }
    Ok(response)
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

struct TestStallingHttpServer {
    addr: SocketAddr,
    request: mpsc::Receiver<String>,
    release: mpsc::Sender<()>,
}

impl TestStallingHttpServer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        let (request_tx, request_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        request.extend_from_slice(&chunk[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = request_tx.send(String::from_utf8_lossy(&request).to_string());
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        Self {
            addr,
            request: request_rx,
            release: release_tx,
        }
    }

    fn release(&self) {
        let _ = self.release.send(());
    }
}

struct TestStallingHttpBodyServer {
    addr: SocketAddr,
    request: mpsc::Receiver<String>,
    release: mpsc::Sender<()>,
}

impl TestStallingHttpBodyServer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        let (request_tx, request_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let Ok(request) = read_h1_request_from_stream(&mut stream) else {
                return;
            };
            let _ = request_tx.send(request);
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n");
            let _ = stream.flush();
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        Self {
            addr,
            request: request_rx,
            release: release_tx,
        }
    }

    fn release(&self) {
        let _ = self.release.send(());
    }
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

struct TestStallingTcpTunnelServer {
    addr: SocketAddr,
    accepted: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}

impl TestStallingTcpTunnelServer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        let (accepted_tx, accepted_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let _stream = stream;
            let _ = accepted_tx.send(());
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        Self {
            addr,
            accepted: accepted_rx,
            release: release_tx,
        }
    }

    fn release(&self) {
        let _ = self.release.send(());
    }
}

struct TestHttpsServer {
    addr: SocketAddr,
    request: mpsc::Receiver<String>,
}

impl TestHttpsServer {
    fn start(
        authority: &WorkloadPepTlsAuthority,
        hostname: &'static str,
        response: impl Into<String>,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        let server_config = authority
            .server_config_for_host(hostname)
            .expect("upstream TLS server config should build");
        let (request_tx, request_rx) = mpsc::channel();
        let response = response.into();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let Ok(server_connection) = rustls::ServerConnection::new(server_config) else {
                return;
            };
            let mut tls = rustls::StreamOwned::new(server_connection, stream);
            let Ok(request) = read_h1_request_from_stream(&mut tls) else {
                return;
            };
            let _ = request_tx.send(request);
            let _ = tls.write_all(response.as_bytes());
        });
        Self {
            addr,
            request: request_rx,
        }
    }
}

struct TestStallingHttpsBodyServer {
    addr: SocketAddr,
    request: mpsc::Receiver<String>,
    release: mpsc::Sender<()>,
}

impl TestStallingHttpsBodyServer {
    fn start(authority: &WorkloadPepTlsAuthority, hostname: &'static str) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        let server_config = authority
            .server_config_for_host(hostname)
            .expect("upstream TLS server config should build");
        let (request_tx, request_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let Ok(server_connection) = rustls::ServerConnection::new(server_config) else {
                return;
            };
            let mut tls = rustls::StreamOwned::new(server_connection, stream);
            let Ok(request) = read_h1_request_from_stream(&mut tls) else {
                return;
            };
            let _ = request_tx.send(request);
            let _ = tls
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\n");
            let _ = tls.flush();
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        Self {
            addr,
            request: request_rx,
            release: release_tx,
        }
    }

    fn release(&self) {
        let _ = self.release.send(());
    }
}

struct TestHttpsCloseServer {
    addr: SocketAddr,
    request: mpsc::Receiver<String>,
}

impl TestHttpsCloseServer {
    fn start_after_request(authority: &WorkloadPepTlsAuthority, hostname: &'static str) -> Self {
        Self::start(authority, hostname, true)
    }

    fn start_after_handshake(authority: &WorkloadPepTlsAuthority, hostname: &'static str) -> Self {
        Self::start(authority, hostname, false)
    }

    fn start(
        authority: &WorkloadPepTlsAuthority,
        hostname: &'static str,
        read_request: bool,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        let server_config = authority
            .server_config_for_host(hostname)
            .expect("upstream TLS server config should build");
        let (request_tx, request_rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let Ok(server_connection) = rustls::ServerConnection::new(server_config) else {
                return;
            };
            let mut tls = rustls::StreamOwned::new(server_connection, stream);
            if read_request {
                if let Ok(request) = read_h1_request_from_stream(&mut tls) {
                    let _ = request_tx.send(request);
                }
            } else {
                let _ = tls.conn.complete_io(&mut tls.sock);
            }
        });
        Self {
            addr,
            request: request_rx,
        }
    }
}

fn read_h1_request_from_stream(stream: &mut impl Read) -> io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(String::from_utf8_lossy(&buffer).to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break header_end;
        }
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
    while buffer.len().saturating_sub(header_end + 4) < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn connect_tls_through_proxy(
    proxy_addr: SocketAddr,
    authority: &WorkloadPepTlsAuthority,
    host: &str,
    port: u16,
) -> rustls::StreamOwned<rustls::ClientConnection, TcpStream> {
    let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .expect("write timeout should set");
    stream
        .write_all(
            format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n").as_bytes(),
        )
        .expect("CONNECT request should write");
    let connect_response = read_http_headers_from_raw_stream(&mut stream);
    assert!(
        connect_response.starts_with("HTTP/1.1 200 Connection Established"),
        "CONNECT should establish before TLS handshake, got: {connect_response}"
    );

    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(authority.trust_anchor_der())
        .expect("proxy CA trust anchor should parse");
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
    .expect("client protocol versions should configure")
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let server_name =
        rustls::pki_types::ServerName::try_from(host.to_owned()).expect("SNI should be valid");
    let connection = rustls::ClientConnection::new(Arc::new(config), server_name)
        .expect("client TLS connection should build");
    let mut tls = rustls::StreamOwned::new(connection, stream);
    tls.conn
        .complete_io(&mut tls.sock)
        .expect("client TLS handshake should complete");
    tls
}

fn read_http_headers_from_raw_stream(stream: &mut TcpStream) -> String {
    let mut response = Vec::new();
    let mut chunk = [0_u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        let read = stream
            .read(&mut chunk)
            .expect("client should read HTTP response headers");
        if read == 0 {
            break;
        }
        response.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8_lossy(&response).to_string()
}

fn read_tls_headers_to_string(
    stream: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
) -> io::Result<String> {
    let mut response = Vec::new();
    let mut chunk = [0_u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        response.extend_from_slice(&chunk[..read]);
    }
    Ok(String::from_utf8_lossy(&response).to_string())
}

fn read_tls_to_string(
    stream: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
    output: &mut String,
) -> io::Result<()> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error)
                if error.kind() == io::ErrorKind::UnexpectedEof
                    && error
                        .to_string()
                        .contains("without sending TLS close_notify") =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    output.push_str(&String::from_utf8_lossy(&bytes));
    Ok(())
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
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        });
        Self {
            addr,
            body_len: body_rx,
        }
    }
}
