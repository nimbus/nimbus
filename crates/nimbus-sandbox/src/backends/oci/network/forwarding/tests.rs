use std::io::{Read as _, Write as _};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use std::os::fd::AsRawFd as _;
#[cfg(unix)]
use std::os::unix::net::{UnixListener, UnixStream};

use nimbus_core::TenantId;
use nimbus_network::NetworkResourceGeneration;

use super::{
    MAX_MACHINE_FORWARDER_RESPONSE_BYTES, MachinePortForwardOutcome, MachinePortForwardReceipt,
    OciMachinePortForwarderConfig, expose_machine_ports as expose_machine_ports_with_identity,
    inspect_machine_ports as inspect_machine_ports_with_identity,
    unexpose_machine_ports as unexpose_machine_ports_with_identity,
};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

enum ScriptedResponse {
    Bytes(Vec<u8>),
    BytesAllowDisconnect(Vec<u8>),
    Eof,
    Delay(Duration),
}

#[derive(Clone, Copy)]
enum MutationLossAction {
    Expose,
    Unexpose,
}

struct MutationLossObserver {
    address: SocketAddr,
    action: MutationLossAction,
    server: thread::JoinHandle<(usize, Vec<String>)>,
}

impl MutationLossObserver {
    fn spawn(
        listener: TcpListener,
        action: MutationLossAction,
        binding: SandboxPortBinding,
    ) -> Self {
        let address = listener
            .local_addr()
            .expect("mutation-loss observer address should resolve");
        let server = thread::spawn(move || {
            let mut route_present = matches!(action, MutationLossAction::Unexpose);
            let mut lose_mutation_response = true;
            let mut lose_inspection = true;
            let mut mutation_count = 0;
            let mut requests = Vec::new();
            loop {
                let (mut stream, _) = listener.accept().expect("request should arrive");
                let request = read_complete_request(&mut stream);
                if request.starts_with("POST /__nimbus_nnc5_4a_complete ") {
                    stream
                        .write_all(&http_response("200 OK", &[]))
                        .expect("completion response should write");
                    return (mutation_count, requests);
                }
                let is_inspection = request.starts_with("GET /services/forwarder/all ");
                let is_mutation = match action {
                    MutationLossAction::Expose => {
                        request.starts_with("POST /services/forwarder/expose ")
                    }
                    MutationLossAction::Unexpose => {
                        request.starts_with("POST /services/forwarder/unexpose ")
                    }
                };
                assert!(
                    is_inspection || is_mutation,
                    "unexpected mutation-loss request: {request}"
                );
                requests.push(request);
                if is_mutation {
                    mutation_count += 1;
                    route_present = matches!(action, MutationLossAction::Expose);
                    if lose_mutation_response {
                        lose_mutation_response = false;
                        stream
                            .shutdown(Shutdown::Write)
                            .expect("lost mutation response should close");
                    } else {
                        stream
                            .write_all(&http_response("200 OK", &[]))
                            .expect("mutation response should write");
                    }
                    continue;
                }
                if lose_inspection {
                    lose_inspection = false;
                    stream
                        .shutdown(Shutdown::Write)
                        .expect("lost inspection should close");
                    continue;
                }
                let body = if route_present {
                    native_routes(std::slice::from_ref(&binding))
                } else {
                    b"[]".to_vec()
                };
                stream
                    .write_all(&http_response("200 OK", &body))
                    .expect("inspection response should write");
            }
        });
        Self {
            address,
            action,
            server,
        }
    }

    fn finish(self) -> (usize, Vec<String>) {
        let mut stream =
            TcpStream::connect(self.address).expect("completion request should connect");
        stream
            .write_all(b"POST /__nimbus_nnc5_4a_complete HTTP/1.0\r\nContent-Length: 0\r\n\r\n")
            .expect("completion request should write");
        stream
            .shutdown(Shutdown::Write)
            .expect("completion request should finish");
        let result = self
            .server
            .join()
            .expect("mutation-loss observer should join");
        let expected_action = match self.action {
            MutationLossAction::Expose => "/expose",
            MutationLossAction::Unexpose => "/unexpose",
        };
        assert!(
            result
                .1
                .iter()
                .any(|request| request.contains(expected_action)),
            "fixture must exercise {expected_action}: {:?}",
            result.1
        );
        result
    }
}

fn config_for(
    listener: &TcpListener,
    identity: &str,
    generation: u64,
) -> OciMachinePortForwarderConfig {
    OciMachinePortForwarderConfig::for_provider_instance(
        Ipv4Addr::LOCALHOST.to_string(),
        listener
            .local_addr()
            .expect("test forwarder address should resolve")
            .port(),
        "/services/forwarder",
        identity,
        NetworkResourceGeneration::new(generation),
    )
    .expect("test provider instance should validate")
}

#[cfg(unix)]
#[test]
fn unix_services_socket_is_the_reachable_parent_control_endpoint() {
    let root = tempfile::TempDir::new().expect("Unix services fixture should exist");
    let socket_path = root.path().join("gvproxy-services.sock");
    let listener = UnixListener::bind(&socket_path).expect("Unix services socket should bind");
    let server = thread::spawn(move || {
        let (mut stream, _) = listener
            .accept()
            .expect("parent control probe should connect to the Unix services socket");
        let request = read_complete_unix_request(&mut stream);
        assert!(request.starts_with("GET /services/forwarder/all "));
        stream
            .write_all(&http_response("200 OK", b"[]"))
            .expect("services probe response should write");
    });
    let config = OciMachinePortForwarderConfig::for_unix_services_socket(
        &socket_path,
        "/services/forwarder",
        "unix-parent-control",
        NetworkResourceGeneration::new(7),
    )
    .expect("Unix parent control config should validate");

    assert_eq!(config.unix_socket_path(), Some(socket_path.as_path()));
    config
        .require_reachable()
        .expect("bound Unix services socket should be reachable");
    server.join().expect("Unix control probe should finish");
}

#[cfg(unix)]
#[test]
fn unix_services_capability_requires_valid_forwarder_http_api() {
    let root = tempfile::TempDir::new().expect("Unix services fixture should exist");
    let socket_path = root.path().join("accept-only.sock");
    let listener = UnixListener::bind(&socket_path).expect("accept-only socket should bind");
    let server = thread::spawn(move || {
        let _ = listener
            .accept()
            .expect("services probe should reach accept-only listener");
    });
    let config = OciMachinePortForwarderConfig::for_unix_services_socket(
        &socket_path,
        "/services/forwarder",
        "accept-only-parent-control",
        NetworkResourceGeneration::new(9),
    )
    .expect("Unix parent control config should validate");

    config
        .require_reachable()
        .expect_err("an arbitrary accepting listener is not the gvproxy services API");
    server.join().expect("accept-only listener should finish");
}

#[cfg(unix)]
#[test]
fn unix_services_config_rejects_relative_empty_prefix_and_overlong_paths() {
    use std::os::unix::ffi::OsStringExt as _;

    let generation = NetworkResourceGeneration::new(10);
    assert!(
        OciMachinePortForwarderConfig::for_unix_services_socket(
            "relative.sock",
            "/services/forwarder",
            "relative",
            generation,
        )
        .is_err()
    );
    assert!(
        OciMachinePortForwarderConfig::for_unix_services_socket(
            "/tmp/nimbus.sock",
            "",
            "empty-prefix",
            generation,
        )
        .is_err()
    );
    let overlong = format!("/tmp/{}.sock", "x".repeat(256));
    assert!(
        OciMachinePortForwarderConfig::for_unix_services_socket(
            overlong,
            "/services/forwarder",
            "overlong",
            generation,
        )
        .is_err()
    );
    assert!(
        OciMachinePortForwarderConfig::for_unix_services_socket(
            std::ffi::OsString::from_vec(b"/tmp/nimbus\0crossed.sock".to_vec()),
            "/services/forwarder",
            "nul-path",
            generation,
        )
        .is_err()
    );
}

#[cfg(unix)]
#[test]
fn unix_services_errors_name_socket_not_localhost_zero() {
    let root = tempfile::TempDir::new().expect("Unix services fixture should exist");
    let socket_path = root.path().join("missing.sock");
    let config = OciMachinePortForwarderConfig::for_unix_services_socket(
        &socket_path,
        "/services/forwarder",
        "diagnostic-parent-control",
        NetworkResourceGeneration::new(11),
    )
    .expect("Unix parent control config should validate");

    let error = config
        .require_reachable()
        .expect_err("missing services socket should fail");
    let diagnostic = error.to_string();
    assert!(diagnostic.contains(&socket_path.display().to_string()));
    assert!(!diagnostic.contains("localhost:0"), "{diagnostic}");
}

#[cfg(unix)]
#[test]
fn unix_services_connect_poll_obeys_deadline() {
    let (mut sender, receiver) = UnixStream::pair().expect("Unix socket pair should create");
    sender
        .set_nonblocking(true)
        .expect("test sender should become nonblocking");
    let chunk = [0_u8; 8192];
    loop {
        match sender.write(&chunk) {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => break,
            Err(error) => panic!("test send buffer should fill: {error}"),
        }
    }
    let started = std::time::Instant::now();
    let error = super::wait_for_unix_connect(sender.as_raw_fd(), Duration::from_millis(20))
        .expect_err("a non-writable socket must honor the connect deadline");
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(1));
    drop(receiver);
}

#[cfg(unix)]
#[test]
fn missing_unix_services_socket_is_not_an_available_capability() {
    let root = tempfile::TempDir::new().expect("Unix services fixture should exist");
    let socket_path = root.path().join("missing-gvproxy-services.sock");
    let config = OciMachinePortForwarderConfig::for_unix_services_socket(
        &socket_path,
        "/services/forwarder",
        "missing-unix-parent-control",
        NetworkResourceGeneration::new(8),
    )
    .expect("Unix parent control config should validate");

    let error = config
        .require_reachable()
        .expect_err("missing Unix services socket must keep capability unavailable");
    assert!(
        error
            .to_string()
            .contains(&socket_path.display().to_string())
    );
}

fn binding() -> SandboxPortBinding {
    SandboxPortBinding::tcp("http", 18080, 8080)
}

fn tenant_id() -> TenantId {
    TenantId::new("tenant-forwarding-test").expect("test tenant should validate")
}

fn sandbox_id() -> SandboxId {
    SandboxId::new("machine-api:test-forwarding-plan")
}

fn expose_machine_ports(
    config: &OciMachinePortForwarderConfig,
    bindings: &[SandboxPortBinding],
) -> crate::error::Result<Vec<MachinePortForwardReceipt>> {
    expose_machine_ports_with_identity(config, &tenant_id(), &sandbox_id(), bindings)
}

fn unexpose_machine_ports(
    config: &OciMachinePortForwarderConfig,
    bindings: &[SandboxPortBinding],
) -> crate::error::Result<Vec<MachinePortForwardReceipt>> {
    unexpose_machine_ports_with_identity(config, &tenant_id(), &sandbox_id(), bindings)
}

fn inspect_machine_ports(
    config: &OciMachinePortForwarderConfig,
    bindings: &[SandboxPortBinding],
) -> crate::error::Result<super::CurrentMachinePortForwardingObservation> {
    inspect_machine_ports_with_identity(config, &tenant_id(), &sandbox_id(), bindings)
}

fn http_response(status: &str, body: &[u8]) -> Vec<u8> {
    let mut response = format!(
        "HTTP/1.0 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    response.extend_from_slice(body);
    response
}

fn spawn_scripted_forwarder(
    listener: TcpListener,
    responses: Vec<ScriptedResponse>,
) -> thread::JoinHandle<Vec<String>> {
    thread::spawn(move || {
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept().expect("request should arrive");
            requests.push(read_complete_request(&mut stream));
            match response {
                ScriptedResponse::Bytes(response) => {
                    stream
                        .write_all(&response)
                        .expect("scripted response should write");
                    stream
                        .shutdown(Shutdown::Write)
                        .expect("response EOF should be explicit");
                    let mut trailing = [0_u8; 64];
                    while stream
                        .read(&mut trailing)
                        .expect("client shutdown should be readable")
                        != 0
                    {}
                }
                ScriptedResponse::BytesAllowDisconnect(response) => {
                    let _ = stream.write_all(&response);
                    let _ = stream.shutdown(Shutdown::Write);
                }
                ScriptedResponse::Eof => {
                    stream
                        .shutdown(Shutdown::Write)
                        .expect("empty response EOF should be explicit");
                }
                ScriptedResponse::Delay(delay) => thread::sleep(delay),
            }
        }
        requests
    })
}

fn read_complete_request(stream: &mut std::net::TcpStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("test request timeout should configure");
    read_complete_request_from(stream)
}

#[cfg(unix)]
fn read_complete_unix_request(stream: &mut UnixStream) -> String {
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .expect("test Unix request timeout should configure");
    read_complete_request_from(stream)
}

fn read_complete_request_from(stream: &mut impl std::io::Read) -> String {
    let mut request = Vec::new();
    let mut expected_len = None;
    let mut chunk = [0_u8; 1024];
    loop {
        let read = stream
            .read(&mut chunk)
            .expect("request bytes should be readable");
        assert_ne!(read, 0, "request must not close before its complete body");
        request.extend_from_slice(&chunk[..read]);
        if expected_len.is_none()
            && let Some(header_end) = request.windows(4).position(|window| window == b"\r\n\r\n")
        {
            let headers = std::str::from_utf8(&request[..header_end])
                .expect("test request headers should be UTF-8");
            let content_length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then_some(value.trim())
                })
                .map(|value| {
                    value
                        .parse::<usize>()
                        .expect("test request length should parse")
                })
                .unwrap_or(0);
            expected_len = Some(header_end + 4 + content_length);
        }
        if expected_len.is_some_and(|expected| request.len() >= expected) {
            return String::from_utf8(request).expect("test request should be UTF-8");
        }
    }
}

#[cfg(unix)]
#[test]
fn unix_services_retirement_inspects_and_withdraws_exact_batch() {
    let root = tempfile::TempDir::new().expect("Unix retirement fixture should exist");
    let socket_path = root.path().join("gvproxy-services.sock");
    let listener = UnixListener::bind(&socket_path).expect("Unix services socket should bind");
    let expected_binding = binding();
    let route_body = native_routes(std::slice::from_ref(&expected_binding));
    let server = thread::spawn(move || {
        let responses = [
            http_response("200 OK", &route_body),
            http_response("200 OK", b"[]"),
            http_response("200 OK", b"[]"),
        ];
        let mut requests = Vec::new();
        for response in responses {
            let (mut stream, _) = listener.accept().expect("Unix request should arrive");
            requests.push(read_complete_unix_request(&mut stream));
            stream
                .write_all(&response)
                .expect("Unix services response should write");
        }
        requests
    });
    let config = OciMachinePortForwarderConfig::for_unix_services_socket(
        &socket_path,
        "/services/forwarder",
        "unix-retirement",
        NetworkResourceGeneration::new(12),
    )
    .expect("Unix parent control config should validate");

    let receipts = unexpose_machine_ports(&config, std::slice::from_ref(&expected_binding))
        .expect("Unix services retirement should prove exact absence");
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].outcome, MachinePortForwardOutcome::Withdrawn);
    let requests = server.join().expect("Unix retirement server should join");
    assert_eq!(requests.len(), 3);
    assert!(requests[0].starts_with("GET /services/forwarder/all "));
    assert!(requests[1].starts_with("POST /services/forwarder/unexpose "));
    assert!(requests[2].starts_with("GET /services/forwarder/all "));
}

fn assert_ambiguous(error: crate::error::SandboxError) {
    assert!(
        error.to_string().contains("ambiguous"),
        "the rejection must preserve the provider effect as ambiguous: {error}"
    );
}

fn native_routes(bindings: &[SandboxPortBinding]) -> Vec<u8> {
    serde_json::to_vec(
        &bindings
            .iter()
            .map(|binding| {
                serde_json::json!({
                    "local": format!("{}:{}", binding.host_address, binding.host_port),
                    "remote": format!(":{}", binding.host_port),
                    "protocol": "tcp",
                })
            })
            .collect::<Vec<_>>(),
    )
    .expect("native route list should encode")
}

#[test]
fn nnc5_4a_expose_response_loss_inspects_before_retrying_visible_route() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "nnc5-4a-expose-response-loss", 71);
    let binding = binding();
    let observer =
        MutationLossObserver::spawn(listener, MutationLossAction::Expose, binding.clone());

    let first = expose_machine_ports(&config, std::slice::from_ref(&binding))
        .expect_err("lost mutation response plus lost inspection must remain ambiguous");
    assert_ambiguous(first);
    expose_machine_ports(&config, std::slice::from_ref(&binding))
        .expect("retry should inspect the already-visible route and converge");

    let (mutation_count, requests) = observer.finish();
    assert_eq!(
        mutation_count, 1,
        "NNC5.4a requires inspect-before-retry after an expose effect may have committed; \
             requests: {requests:?}"
    );
}

#[test]
fn nnc5_4a_withdraw_response_loss_inspects_before_retrying_absent_route() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "nnc5-4a-withdraw-response-loss", 72);
    let binding = binding();
    let observer =
        MutationLossObserver::spawn(listener, MutationLossAction::Unexpose, binding.clone());

    let first = unexpose_machine_ports(&config, std::slice::from_ref(&binding))
        .expect_err("lost mutation response plus lost inspection must remain ambiguous");
    assert_ambiguous(first);
    unexpose_machine_ports(&config, std::slice::from_ref(&binding))
        .expect("retry should inspect exact absence and converge");

    let (mutation_count, requests) = observer.finish();
    assert_eq!(
        mutation_count, 1,
        "NNC5.4a requires inspect-before-retry after an unexpose effect may have committed; \
             requests: {requests:?}"
    );
}

#[test]
fn expose_and_unexpose_translate_the_native_protocol_into_fenced_receipts() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "test-native-mutations", 41);
    let server = spawn_scripted_forwarder(
        listener,
        vec![
            ScriptedResponse::Bytes(http_response("200 OK", b"[]")),
            ScriptedResponse::Bytes(http_response("200 OK", &[])),
            ScriptedResponse::Bytes(http_response("200 OK", &native_routes(&[binding()]))),
            ScriptedResponse::Bytes(http_response("200 OK", &native_routes(&[binding()]))),
            ScriptedResponse::Bytes(http_response("200 OK", &[])),
            ScriptedResponse::Bytes(http_response("200 OK", b"[]")),
        ],
    );

    let exposed = expose_machine_ports(&config, &[binding()])
        .expect("native expose plus exact list should authenticate");
    let withdrawn = unexpose_machine_ports(&config, &[binding()])
        .expect("native unexpose plus exact absence should authenticate");
    let requests = server.join().expect("test forwarder should join");

    assert_eq!(
        exposed,
        vec![MachinePortForwardReceipt {
            outcome: MachinePortForwardOutcome::Exposed,
            tenant_id: tenant_id(),
            sandbox_id: sandbox_id(),
            binding: binding(),
            provider_instance: config.provider_instance().clone(),
            provider_generation: config.provider_generation(),
        }]
    );
    assert_eq!(withdrawn[0].outcome, MachinePortForwardOutcome::Withdrawn);
    assert_eq!(requests.len(), 6);
    assert!(requests[0].starts_with("GET /services/forwarder/all "));
    assert!(requests[1].starts_with("POST /services/forwarder/expose "));
    assert!(requests[2].starts_with("GET /services/forwarder/all "));
    assert!(requests[3].starts_with("GET /services/forwarder/all "));
    assert!(requests[4].starts_with("POST /services/forwarder/unexpose "));
    assert!(requests[5].starts_with("GET /services/forwarder/all "));
    for (index, request) in [requests[1].as_str(), requests[4].as_str()]
        .into_iter()
        .enumerate()
    {
        let (_, body) = request
            .split_once("\r\n\r\n")
            .expect("native mutation should contain a body");
        let body: serde_json::Value =
            serde_json::from_str(body).expect("native mutation body should decode");
        assert_eq!(body["local"], "127.0.0.1:18080");
        assert_eq!(body["protocol"], "tcp");
        assert!(
            body.get("provider_instance").is_none() && body.get("provider_generation").is_none(),
            "adapter-only fencing fields must not be sent to gvproxy"
        );
        if index == 0 {
            assert_eq!(body["remote"], ":18080");
        } else {
            assert!(body.get("remote").is_none());
        }
    }
}

#[test]
fn current_inspection_uses_the_gvproxy_native_batch_list_contract() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "test-native-current-inspection", 43);
    let server = spawn_scripted_forwarder(
        listener,
        vec![ScriptedResponse::Bytes(http_response(
            "200 OK",
            &native_routes(&[binding()]),
        ))],
    );

    let observation = inspect_machine_ports(&config, &[binding()])
        .expect("the exact native gvproxy route list should authenticate");
    let requests = server.join().expect("test forwarder should join");

    assert_eq!(observation.provider_instance(), config.provider_instance());
    assert_eq!(
        observation.provider_generation(),
        config.provider_generation()
    );
    assert_eq!(observation.receipts().len(), 1);
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].starts_with("GET /services/forwarder/all HTTP/1.0\r\n"),
        "current observation must use gvproxy's one supported read-only batch route: \
             {requests:?}"
    );
    assert!(
        !requests[0].contains("/expose ") && !requests[0].contains("/inspect "),
        "current observation must neither mutate nor invent a provider route: {requests:?}"
    );
}

#[test]
fn unavailable_current_list_never_replays_expose() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "unsupported", 42);
    let server = spawn_scripted_forwarder(
        listener,
        vec![ScriptedResponse::Bytes(http_response(
            "404 Not Found",
            b"unsupported",
        ))],
    );

    let error = inspect_machine_ports(&config, &[binding()])
        .expect_err("unsupported inspection must remain provider-unknown");
    let requests = server.join().expect("test forwarder should join");

    assert_ambiguous(error);
    assert_eq!(requests.len(), 1);
    assert!(
        requests[0].contains("/all") && !requests[0].contains("/expose "),
        "inspection failure must not invoke a mutating fallback: {requests:?}"
    );
}

#[test]
fn unrelated_native_route_authenticates_exact_desired_absence() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "unrelated-route", 42);
    let body = serde_json::to_vec(&vec![serde_json::json!({
        "local": "127.0.0.1:18081",
        "remote": ":18081",
        "protocol": "tcp",
    })])
    .expect("unrelated route should encode");
    let server = spawn_scripted_forwarder(
        listener,
        vec![ScriptedResponse::Bytes(http_response("200 OK", &body))],
    );

    let observation = inspect_machine_ports(&config, &[binding()])
        .expect("a complete native list proves the desired route absent");
    let requests = server.join().expect("test forwarder should join");

    assert!(observation.receipts().is_empty());
    assert!(
        observation.slots()[0].absent_receipt().is_some(),
        "an unrelated route must not conflict with the desired listener"
    );
    assert_eq!(requests.len(), 1);
    assert!(requests[0].contains("/all") && !requests[0].contains("/expose "));
}

#[test]
fn invalid_current_inspection_returns_no_observation_or_mutating_fallback() {
    for label in [
        "generic-success",
        "unsupported",
        "malformed",
        "truncated",
        "eof",
        "oversized",
    ] {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, label, 62);
        let response = match label {
            "generic-success" => ScriptedResponse::Bytes(http_response("200 OK", b"{}")),
            "unsupported" => {
                ScriptedResponse::Bytes(http_response("404 Not Found", b"unsupported"))
            }
            "malformed" => ScriptedResponse::Bytes(http_response("200 OK", br#"[{"local":"#)),
            "truncated" => ScriptedResponse::Bytes({
                let body = br#"[{"local":"127.0.0.1:18080"}]"#;
                let mut response = format!(
                    "HTTP/1.0 200 OK\r\nContent-Type: application/json\r\nContent-Length: \
                     {}\r\n\r\n",
                    body.len() + 17
                )
                .into_bytes();
                response.extend_from_slice(body);
                response
            }),
            "eof" => ScriptedResponse::Eof,
            "oversized" => ScriptedResponse::BytesAllowDisconnect(http_response(
                "200 OK",
                &vec![b'x'; MAX_MACHINE_FORWARDER_RESPONSE_BYTES + 1],
            )),
            _ => unreachable!("the substitution labels are closed above"),
        };
        let server = spawn_scripted_forwarder(listener, vec![response]);

        let error = inspect_machine_ports(&config, &[binding()])
            .expect_err("substituted current evidence must return no observation");
        let requests = server.join().expect("test forwarder should join");

        assert_ambiguous(error);
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0].contains("/all") && !requests[0].contains("/expose "),
            "{label}: current inspection must have no mutating fallback: {requests:?}"
        );
    }
}

#[test]
fn current_inspection_classifies_complete_conflicts_without_mutating_fallback() {
    for label in ["wrong-remote", "duplicate"] {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, label, 62);
        let exact = serde_json::json!({
            "local": "127.0.0.1:18080",
            "remote": ":18080",
            "protocol": "tcp",
        });
        let routes = match label {
            "wrong-remote" => {
                let mut value = exact.clone();
                value["remote"] = serde_json::json!(":18081");
                vec![value]
            }
            "duplicate" => vec![exact.clone(), exact],
            _ => unreachable!("the conflict labels are closed above"),
        };
        let body = serde_json::to_vec(&routes).expect("response should encode");
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response("200 OK", &body))],
        );

        let observation = inspect_machine_ports(&config, &[binding()])
            .expect("a complete native list should classify its desired slot");
        let requests = server.join().expect("test forwarder should join");

        assert!(
            observation.slots()[0].conflict_detail().is_some(),
            "{label}: a same-listener substitution must be conflicting"
        );
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0].contains("/all") && !requests[0].contains("/expose "),
            "{label}: classification must have no mutating fallback: {requests:?}"
        );
    }
}

#[test]
fn current_inspection_fences_overlapping_wildcard_routes() {
    let exact = serde_json::json!({
        "local": "127.0.0.1:18080",
        "remote": ":18080",
        "protocol": "tcp",
    });
    let wildcard = serde_json::json!({
        "local": "0.0.0.0:18080",
        "remote": ":18080",
        "protocol": "tcp",
    });
    let native_wildcard = serde_json::json!({
        "local": ":18080",
        "remote": ":18080",
        "protocol": "tcp",
    });
    let mapped_exact = serde_json::json!({
        "local": "[::ffff:127.0.0.1]:18080",
        "remote": ":18080",
        "protocol": "tcp",
    });
    let mapped_wildcard = serde_json::json!({
        "local": "[::ffff:0.0.0.0]:18080",
        "remote": ":18080",
        "protocol": "tcp",
    });
    for (label, routes) in [
        ("wildcard-only", vec![wildcard.clone()]),
        ("native-wildcard-only", vec![native_wildcard]),
        ("mapped-exact-only", vec![mapped_exact]),
        ("mapped-wildcard-only", vec![mapped_wildcard]),
        ("exact-plus-wildcard", vec![exact.clone(), wildcard.clone()]),
    ] {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, label, 63);
        let body = serde_json::to_vec(&routes).expect("response should encode");
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response("200 OK", &body))],
        );

        let observation = inspect_machine_ports(&config, &[binding()])
            .expect("a complete native list should classify its desired slot");
        let requests = server.join().expect("test forwarder should join");

        assert!(
            observation.slots()[0].conflict_detail().is_some(),
            "{label}: an overlapping wildcard publication must fence exact absence"
        );
        assert_eq!(requests.len(), 1);
        assert!(
            requests[0].contains("/all") && !requests[0].contains("/expose "),
            "{label}: classification must have no mutating fallback: {requests:?}"
        );
    }
}

#[test]
fn current_inspection_fences_specific_routes_for_a_desired_wildcard() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "desired-wildcard", 63);
    let desired = binding().with_host_address(Ipv4Addr::UNSPECIFIED.into());
    let routes = serde_json::to_vec(&[serde_json::json!({
        "local": "127.0.0.1:18080",
        "remote": ":18080",
        "protocol": "tcp",
    })])
    .expect("response should encode");
    let server = spawn_scripted_forwarder(
        listener,
        vec![ScriptedResponse::Bytes(http_response("200 OK", &routes))],
    );

    let observation = inspect_machine_ports(&config, &[desired])
        .expect("complete specific current state should authenticate as a conflict");
    let requests = server.join().expect("test forwarder should join");

    assert!(observation.slots()[0].conflict_detail().is_some());
    assert_eq!(requests.len(), 1);
}

#[test]
fn current_inspection_ignores_known_disjoint_non_tcp_routes() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "disjoint-non-tcp", 63);
    let routes = serde_json::to_vec(&[
        serde_json::json!({
            "local": "127.0.0.1:18080",
            "remote": ":18080",
            "protocol": "udp",
        }),
        serde_json::json!({
            "local": "/tmp/nimbus.sock",
            "remote": "/tmp/guest.sock",
            "protocol": "unix",
        }),
        serde_json::json!({
            "local": r"\\.\pipe\nimbus",
            "remote": r"\\.\pipe\guest",
            "protocol": "npipe",
        }),
    ])
    .expect("response should encode");
    let server = spawn_scripted_forwarder(
        listener,
        vec![ScriptedResponse::Bytes(http_response("200 OK", &routes))],
    );

    let observation = inspect_machine_ports(&config, &[binding()])
        .expect("known non-TCP routes must be disjoint from the desired TCP slot");
    let requests = server.join().expect("test forwarder should join");

    assert!(observation.slots()[0].absent_receipt().is_some());
    assert_eq!(requests.len(), 1);
}

#[test]
fn current_inspection_rejects_unknown_protocol_or_malformed_tcp_local() {
    for (label, route) in [
        (
            "unknown-protocol",
            serde_json::json!({
                "local": "127.0.0.1:18080",
                "remote": ":18080",
                "protocol": "sctp",
            }),
        ),
        (
            "malformed-tcp-local",
            serde_json::json!({
                "local": "not-a-socket",
                "remote": ":18080",
                "protocol": "tcp",
            }),
        ),
    ] {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, label, 63);
        let body = serde_json::to_vec(&[route]).expect("response should encode");
        let server = spawn_scripted_forwarder(
            listener,
            vec![ScriptedResponse::Bytes(http_response("200 OK", &body))],
        );

        let error = inspect_machine_ports(&config, &[binding()])
            .expect_err("unknown TCP-relevant provider state must fail closed");
        let requests = server.join().expect("test forwarder should join");

        assert_ambiguous(error);
        assert_eq!(requests.len(), 1);
    }
}

#[test]
fn current_inspection_mixed_timeout_and_refusal_have_precise_outcomes() {
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("mixed forwarder should bind");
    let config = config_for(&listener, "mixed-current-inspection", 63);
    let second_binding = SandboxPortBinding::tcp("metrics", 19090, 9090);
    let server = spawn_scripted_forwarder(
        listener,
        vec![ScriptedResponse::Bytes(http_response(
            "200 OK",
            &native_routes(&[binding()]),
        ))],
    );

    let mixed = inspect_machine_ports(&config, &[binding(), second_binding])
        .expect("a complete native list must classify every desired slot");
    let mixed_requests = server.join().expect("mixed forwarder should join");
    assert_eq!(mixed.receipts().len(), 1);
    assert!(mixed.slots()[0].exposed_receipt().is_some());
    assert!(mixed.slots()[1].absent_receipt().is_some());
    assert_eq!(mixed_requests.len(), 1);
    assert!(
        mixed_requests
            .iter()
            .all(|request| request.contains("/all") && !request.contains("/expose "))
    );

    let timeout_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("timeout forwarder should bind");
    let timeout_config = config_for(&timeout_listener, "timeout-current-inspection", 64);
    let timeout_server = spawn_scripted_forwarder(
        timeout_listener,
        vec![ScriptedResponse::Delay(Duration::from_millis(2_100))],
    );
    let timeout_error = inspect_machine_ports(&timeout_config, &[binding()])
        .expect_err("a provider timeout must remain unknown");
    let timeout_requests = timeout_server
        .join()
        .expect("timeout forwarder should join");
    assert_ambiguous(timeout_error);
    assert_eq!(timeout_requests.len(), 1);
    assert!(timeout_requests[0].contains("/all") && !timeout_requests[0].contains("/expose "));

    let refusal_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("refusal port should bind");
    let refusal_config = config_for(&refusal_listener, "refused-current-inspection", 65);
    drop(refusal_listener);
    assert_ambiguous(
        inspect_machine_ports(&refusal_config, &[binding()])
            .expect_err("connection refusal must remain unknown"),
    );
}

#[test]
fn empty_current_inspection_has_no_provider_io_or_forwarding_claim() {
    let listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("unused provider should bind");
    let config = config_for(&listener, "test-empty-current-inspection", 43);

    let observation =
        inspect_machine_ports(&config, &[]).expect("empty desired forwarding should inspect");

    assert_eq!(observation.provider_instance(), config.provider_instance());
    assert_eq!(
        observation.provider_generation(),
        config.provider_generation()
    );
    assert!(
        observation.receipts().is_empty(),
        "empty desired forwarding must not fabricate a route receipt"
    );
    listener
        .set_nonblocking(true)
        .expect("provider fixture should become nonblocking");
    assert!(
        matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ),
        "empty desired forwarding must perform no provider I/O"
    );
}

#[test]
fn partial_native_observation_returns_no_mutation_success_evidence() {
    let second_binding = SandboxPortBinding::tcp("metrics", 19090, 9090);
    let bindings = vec![binding(), second_binding.clone()];

    for action in ["expose", "unexpose"] {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, action, 51);
        let observed = if action == "expose" {
            native_routes(&[binding()])
        } else {
            native_routes(std::slice::from_ref(&second_binding))
        };
        let initial = if action == "expose" {
            b"[]".to_vec()
        } else {
            native_routes(&bindings)
        };
        let after_first = if action == "expose" {
            native_routes(&[binding()])
        } else {
            native_routes(std::slice::from_ref(&second_binding))
        };
        let server = spawn_scripted_forwarder(
            listener,
            vec![
                ScriptedResponse::Bytes(http_response("200 OK", &initial)),
                ScriptedResponse::Bytes(http_response("200 OK", &[])),
                ScriptedResponse::Bytes(http_response("200 OK", &after_first)),
                ScriptedResponse::Bytes(http_response("200 OK", &after_first)),
                ScriptedResponse::Bytes(http_response("200 OK", &[])),
                ScriptedResponse::Bytes(http_response("200 OK", &observed)),
            ],
        );

        let error = if action == "expose" {
            expose_machine_ports(&config, &bindings)
                .expect_err("a partial exposed batch must return no success evidence")
        } else {
            unexpose_machine_ports(&config, &bindings)
                .expect_err("a partial absent batch must return no success evidence")
        };
        let requests = server.join().expect("test forwarder should join");

        assert_eq!(
            requests.len(),
            6,
            "each native mutation must be fenced by complete batch observations"
        );
        assert!(requests[0].contains("/all"));
        assert!(requests[1].contains(if action == "expose" {
            "/expose"
        } else {
            "/unexpose"
        }));
        assert!(requests[2].contains("/all") && requests[3].contains("/all"));
        assert!(requests[4].contains(if action == "expose" {
            "/expose"
        } else {
            "/unexpose"
        }));
        assert!(requests[5].contains("/all"));
        assert_ambiguous(error);
    }
}

#[test]
fn generic_http_success_is_not_machine_forwarder_evidence() {
    let expose_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let expose_config = config_for(&expose_listener, "test-generic-expose-status", 6);
    let expose_server = spawn_scripted_forwarder(
        expose_listener,
        vec![
            ScriptedResponse::Bytes(http_response("200 OK", b"[]")),
            ScriptedResponse::Bytes(http_response("200 OK", &[])),
            ScriptedResponse::Bytes(http_response("200 OK", b"{}")),
        ],
    );
    let expose_error = expose_machine_ports(&expose_config, &[binding()])
        .expect_err("a generic mutation status without exact list cannot prove exposure");
    let expose_requests = expose_server.join().expect("test forwarder should join");
    assert!(
        expose_requests.len() == 3
            && expose_requests[0].contains("/all")
            && expose_requests[1].contains("/expose")
            && expose_requests[2].contains("/all"),
        "generic expose status must be surrounded by exact observations: {expose_requests:?}"
    );
    assert_ambiguous(expose_error);

    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "test-generic-status", 7);
    let server = spawn_scripted_forwarder(
        listener,
        vec![
            ScriptedResponse::Bytes(http_response("200 OK", &native_routes(&[binding()]))),
            ScriptedResponse::Bytes(http_response("200 OK", &[])),
            ScriptedResponse::Bytes(http_response("200 OK", &native_routes(&[binding()]))),
        ],
    );

    let error = unexpose_machine_ports(&config, &[binding()])
        .expect_err("a generic status cannot replace observed provider absence");
    let requests = server.join().expect("test forwarder should join");

    assert!(
        requests.len() == 3
            && requests[0].contains("/all")
            && requests[1].contains("/unexpose")
            && requests[2].contains("/all"),
        "withdrawal must remain fenced while the native list still contains the route: \
             {requests:?}"
    );
    assert_ambiguous(error);
}

#[test]
fn failed_unexpose_with_exact_native_absence_is_idempotently_already_absent() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "test-exact-absence", 8);
    let server = spawn_scripted_forwarder(
        listener,
        vec![
            ScriptedResponse::Bytes(http_response("200 OK", &native_routes(&[binding()]))),
            ScriptedResponse::Bytes(http_response("500 Internal Server Error", b"missing")),
            ScriptedResponse::Bytes(http_response("200 OK", b"[]")),
        ],
    );

    let receipts = unexpose_machine_ports(&config, &[binding()])
        .expect("exact native absence may settle an ambiguous idempotent withdrawal");
    let requests = server.join().expect("test forwarder should join");

    assert_eq!(
        receipts[0].outcome,
        MachinePortForwardOutcome::ExactAlreadyAbsent
    );
    assert_eq!(requests.len(), 3);
    assert!(
        requests[0].contains("/all")
            && requests[1].contains("/unexpose")
            && requests[2].contains("/all")
    );
}

#[test]
fn successful_unexpose_plus_native_absence_emits_withdrawn_receipt() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "test-native-withdrawal", 9);
    let server = spawn_scripted_forwarder(
        listener,
        vec![
            ScriptedResponse::Bytes(http_response("200 OK", &native_routes(&[binding()]))),
            ScriptedResponse::Bytes(http_response("200 OK", &[])),
            ScriptedResponse::Bytes(http_response("200 OK", b"[]")),
        ],
    );

    let receipts = unexpose_machine_ports(&config, &[binding()])
        .expect("native success and exact absence may authorize withdrawal");
    let requests = server.join().expect("test forwarder should join");

    assert_eq!(receipts[0].outcome, MachinePortForwardOutcome::Withdrawn);
    assert_eq!(receipts[0].provider_instance, *config.provider_instance());
    assert_eq!(
        receipts[0].provider_generation,
        config.provider_generation()
    );
    assert_eq!(requests.len(), 3);
}

#[test]
fn native_route_list_cannot_substitute_configured_provider_authority() {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
    let config = config_for(&listener, "test-configured-authority", 10);
    let server = spawn_scripted_forwarder(
        listener,
        vec![ScriptedResponse::Bytes(http_response(
            "200 OK",
            &native_routes(&[binding()]),
        ))],
    );

    let observation = inspect_machine_ports(&config, &[binding()])
        .expect("native route should be translated under configured lifecycle authority");
    let requests = server.join().expect("test forwarder should join");

    assert_eq!(observation.provider_instance(), config.provider_instance());
    assert_eq!(
        observation.provider_generation(),
        config.provider_generation()
    );
    assert_eq!(requests.len(), 1);
}

#[test]
fn status_eof_timeout_refusal_and_arbitrary_text_are_provider_unknown() {
    let binding = binding();

    for (label, first_response) in [
        (
            "status",
            ScriptedResponse::Bytes(http_response("204 No Content", &[])),
        ),
        ("eof", ScriptedResponse::Eof),
        (
            "text",
            ScriptedResponse::Bytes(http_response("200 OK", b"withdrawn")),
        ),
    ] {
        let listener =
            TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("test forwarder should bind");
        let config = config_for(&listener, label, 11);
        let server = spawn_scripted_forwarder(listener, vec![first_response]);
        let error = inspect_machine_ports(&config, std::slice::from_ref(&binding))
            .expect_err("non-evidence must remain provider-unknown");
        server.join().expect("test forwarder should join");
        assert_ambiguous(error);
    }

    let timeout_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("timeout forwarder should bind");
    let timeout_config = config_for(&timeout_listener, "timeout", 12);
    let timeout_server = spawn_scripted_forwarder(
        timeout_listener,
        vec![ScriptedResponse::Delay(Duration::from_secs(3))],
    );
    let timeout_error = inspect_machine_ports(&timeout_config, std::slice::from_ref(&binding))
        .expect_err("timeout must not authorize withdrawal");
    timeout_server
        .join()
        .expect("timeout forwarder should join");
    assert_ambiguous(timeout_error);

    let refused_listener =
        TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("refusal port should bind");
    let refused_config = config_for(&refused_listener, "refused", 13);
    drop(refused_listener);
    let refused_error = inspect_machine_ports(&refused_config, std::slice::from_ref(&binding))
        .expect_err("connection refusal must not authorize withdrawal");
    assert_ambiguous(refused_error);
}
