use super::*;

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
fn egress_proxy_rejects_transfer_encoding_request() {
    // CL.TE: a request carrying both Transfer-Encoding and Content-Length lets the
    // proxy's substring DLP read the CL bytes while a downstream dechunks them — a
    // DLP bypass / request-smuggling vector. The proxy forwards only CL-framed
    // bodies, so any Transfer-Encoding is rejected outright, before dial.
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let proxy = start_test_proxy(allow_policy([EgressRule::new(
        "allowed",
        EgressProtocol::Http,
        "allowed.test",
        upstream.addr.port(),
    )
    .with_methods(["POST"])
    .with_path_prefixes(["/ok"])
    .allow_internal_ips(true)]));

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "POST http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\nTransfer-Encoding: chunked\r\nContent-Length: 11\r\n\r\n3\r\nsec\r\n3\r\nret\r\n0\r\n\r\n",
            upstream.addr.port()
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "Transfer-Encoding requests must be rejected, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "a rejected TE+CL request must never reach upstream"
    );
}

#[test]
fn egress_proxy_rejects_bare_lf_header_smuggling() {
    // A bare LF inside a header value survives `split("\r\n")` and re-emits an
    // embedded `Authorization` the per-line credential guard never sees — a
    // credential-control bypass. Reject any bare CR/LF in the header block.
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
            "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\nX-Smuggle: a\nAuthorization: Bearer stolen\r\n\r\n",
            upstream.addr.port()
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request"),
        "bare-LF header smuggling must be rejected, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "a smuggled Authorization must never reach upstream"
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
fn egress_proxy_streams_large_body_without_dlp_inspection_cap_coupling() {
    let body_len = 128 * 1024;
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
        "large non-DLP body should stream through Pingora, got: {response}"
    );
    let received = upstream
        .body_len
        .recv_timeout(Duration::from_secs(2))
        .expect("upstream should receive the streamed request body");
    assert_eq!(received, body_len);
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

// Host normalization (L1): the caller's Host header is advisory and must never
// be forwarded verbatim. The proxy overwrites it with the authorized authority
// (`upstream_host:upstream_port`). This test fails if the spoofed Host survives
// or the authorized Host is missing.
#[test]
fn egress_proxy_overwrites_caller_host_with_authorized_authority() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let proxy = start_test_proxy(allow_policy([EgressRule::new(
        "allowed",
        EgressProtocol::Http,
        "allowed.test",
        upstream.addr.port(),
    )
    .allow_internal_ips(true)]));

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: spoofed.evil:1234\r\n\r\n",
            upstream.addr.port()
        ),
    );
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "the authorized request should reach upstream, got: {response}"
    );

    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("upstream should receive the forwarded request");
    let authorized_host = format!("Host: allowed.test:{}", upstream.addr.port());
    assert!(
        upstream_request.contains(&authorized_host),
        "proxy must forward the authorized Host `{authorized_host}`, got: {upstream_request}"
    );
    assert!(
        !upstream_request.contains("spoofed.evil"),
        "the caller-supplied Host must not be forwarded verbatim, got: {upstream_request}"
    );
}

// Dial-failure UX (L3): a failed upstream dial must surface a 502 rather than
// dropping the client with no response. This test fails if the dial error
// propagates and the client receives an empty response.
#[test]
fn egress_proxy_maps_upstream_dial_failure_to_bad_gateway() {
    // The claim is what keeps this endpoint dead: the window reserves the port
    // for this process and binds nothing on it, so the upstream dial below is
    // refused. The probe it replaces released the port before the dial, which
    // let any other process answer in its place.
    let port_window = PortWindow::claim();
    let dead_port = port_window.port(0);

    let resolver = Arc::new(move |_host: &str, _port: u16| {
        Ok(vec![SocketAddr::from(([127, 0, 0, 1], dead_port))])
    });
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            dead_port,
        )
        .allow_internal_ips(true)]))
        .with_timeouts(Duration::from_millis(500), Duration::from_secs(2))
        .with_resolver(resolver),
    )
    .expect("proxy should start");

    let response = proxy_request(
        proxy.local_addr(),
        format!("GET http://allowed.test:{dead_port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"),
    );

    assert!(
        response.starts_with("HTTP/1.1 502 Bad Gateway") && response.contains("failed to dial"),
        "an upstream dial failure must surface a 502 to the client, got: {response}"
    );
}

#[test]
fn egress_proxy_returns_503_when_connection_limit_is_exhausted() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
    let upstream_addr = listener
        .local_addr()
        .expect("upstream address should resolve");
    let (request_tx, request_rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut request = [0_u8; 1024];
        let read = stream.read(&mut request).unwrap_or_default();
        let _ = request_tx.send(String::from_utf8_lossy(&request[..read]).to_string());
        thread::sleep(Duration::from_secs(2));
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    });
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream_addr.port(),
        )
        .allow_internal_ips(true)]))
        .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
        .with_max_connections(1)
        .with_resolver(Arc::new(move |host: &str, port: u16| {
            assert_eq!(host, "allowed.test");
            Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
        })),
    )
    .expect("proxy should start");
    let mut held = TcpStream::connect(proxy.local_addr()).expect("first client should connect");
    held.write_all(
        format!(
            "GET http://allowed.test:{}/hold HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream_addr.port()
        )
        .as_bytes(),
    )
    .expect("held request should write");
    request_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("held request should reach the delayed upstream");

    let response = proxy_request(
        proxy.local_addr(),
        "GET http://blocked.test:80/ HTTP/1.1\r\nHost: blocked.test\r\n\r\n".to_owned(),
    );

    assert!(
        response.starts_with("HTTP/1.1 503 Service Unavailable")
            && response.contains("connection limit exceeded"),
        "exhausted connection limit should return the existing 503 response, got: {response}"
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
fn egress_proxy_rejects_unbounded_http_body_before_dial() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let proxy = start_test_proxy(allow_policy([EgressRule::new(
        "allowed",
        EgressProtocol::Http,
        "allowed.test",
        upstream.addr.port(),
    )
    .with_methods(["POST"])
    .with_path_prefixes(["/upload"])
    .allow_internal_ips(true)]));

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "POST http://allowed.test:{}/upload HTTP/1.1\r\nHost: allowed.test\r\n\r\nbody-without-length",
            upstream.addr.port()
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request")
            && response.contains("request bodies require Content-Length"),
        "unbounded request bodies (no Content-Length) should fail closed before dial, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "unbounded request bodies must not contact upstream"
    );
}

#[test]
fn egress_proxy_redirect_to_non_allowlisted_host_strips_injected_credentials() {
    let redirect_location = "http://redirect.test/landing";
    let upstream = TestHttpServer::start(
        "HTTP/1.1 302 Found\r\nLocation: http://redirect.test/landing\r\nContent-Length: 0\r\n\r\n",
    );
    let redirect_upstream =
        TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nredirect");
    let proxy = start_test_proxy_with_store(
        allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]),
    );

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/redirect HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream.addr.port()
        ),
    );
    assert!(
        response.starts_with("HTTP/1.1 302 Found"),
        "allowed redirect response should pass through, got: {response}"
    );
    assert!(
        response.contains(redirect_location),
        "redirect location should be visible to the caller, got: {response}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("original upstream should receive allowed request");
    assert!(upstream_request.contains("Authorization: Bearer secret-token"));

    let redirected_with_stale_credential = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://redirect.test:{}/landing HTTP/1.1\r\nHost: redirect.test\r\nAuthorization: Bearer secret-token\r\n\r\n",
            redirect_upstream.addr.port()
        ),
    );
    assert!(
        redirected_with_stale_credential.starts_with("HTTP/1.1 403 Forbidden"),
        "redirect follow to non-allowlisted host must strip or deny stale credentials, got: {redirected_with_stale_credential}"
    );
    assert!(
        redirect_upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "redirect target must not receive stale injected credentials"
    );
}

// Request-line arity: the request line must be exactly `METHOD absolute-uri
// HTTP-version`. Missing method/target/version and an extra trailing token must
// all fail closed with `400 Bad Request` before any resolver/upstream contact.
// A mutation that drops the arity check would forward the extra-token and
// missing-version cases to upstream, and would reject the empty-method and
// missing-target cases with a different (`must be an absolute URI`) message;
// asserting the specific request-line message plus zero upstream contact across
// all four cases kills every variant. The proxy authorizes `allowed.test` with a
// live upstream so any wrongly-forwarded request would visibly reach it.
#[test]
fn egress_proxy_rejects_malformed_request_line_arity_before_upstream() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let upstream_port = upstream.addr.port();
    let proxy = start_test_proxy(allow_policy([EgressRule::new(
        "allowed",
        EgressProtocol::Http,
        "allowed.test",
        upstream_port,
    )
    .allow_internal_ips(true)]));

    let cases = [
        (
            "extra trailing request-line token",
            format!(
                "GET http://allowed.test:{upstream_port}/ok HTTP/1.1 extra\r\nHost: allowed.test\r\n\r\n"
            ),
        ),
        (
            "missing HTTP version",
            format!("GET http://allowed.test:{upstream_port}/ok\r\nHost: allowed.test\r\n\r\n"),
        ),
        (
            "missing request target",
            "GET\r\nHost: allowed.test\r\n\r\n".to_string(),
        ),
        (
            "empty method (blank request line)",
            "\r\nHost: allowed.test\r\n\r\n".to_string(),
        ),
    ];

    for (label, request) in cases {
        let response = proxy_request(proxy.local_addr(), request);
        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request")
                && response.contains("request line must be METHOD absolute-uri HTTP-version"),
            "{label} must fail closed as a malformed request line, got: {response}"
        );
    }

    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "malformed request lines must be rejected before any resolver/upstream contact"
    );
}

// Bracketed IPv6 in the forward-HTTP absolute-URI path. IPv6 egress goes through
// CONNECT; the forward path rejects every bracketed IPv6 literal at URL-host
// canonicalization (which forbids brackets). A *valid* `[::1]:8080` authority
// must first pass the raw-authority bracket/suffix parser (extract `::1`, accept
// the numeric `:port`) and only then be rejected with the bracket-canonicalization
// message. The mutation that rejects a valid `:port` suffix inside the raw parser
// would instead surface the "parser-differential host" message here — so asserting
// the bracket message (and the absence of "parser-differential") kills it.
#[test]
fn egress_proxy_forward_http_valid_port_bracketed_ipv6_rejected_at_bracket_canonicalization() {
    let proxy = start_test_proxy(CompiledEgressPolicy::deny_all());

    let response = proxy_request(
        proxy.local_addr(),
        "GET http://[::1]:8080/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_string(),
    );

    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request")
            && response.contains("host authority must not include brackets or ports"),
        "a valid-port bracketed IPv6 forward-HTTP authority must be rejected at bracket canonicalization, got: {response}"
    );
    assert!(
        !response.contains("parser-differential"),
        "a valid `:port` suffix must pass the raw bracket/suffix parser; a parser-differential rejection means the suffix validation wrongly rejected a valid port, got: {response}"
    );
}

// Malformed bracketed IPv6 suffixes in the forward-HTTP path must be caught by the
// raw-authority suffix parser as a parser-differential host, before URL parsing.
// The mutation that accepts a malformed suffix would let `Url::parse` reject it
// later with a different ("must be an absolute URI") message; the missing-closing-
// bracket case is caught earlier with the bracket message. Asserting each specific
// message pins the exact rejection path.
#[test]
fn egress_proxy_forward_http_rejects_malformed_bracketed_ipv6_suffix() {
    let proxy = start_test_proxy(CompiledEgressPolicy::deny_all());

    for authority in ["[::1]:", "[::1]:8080junk", "[::1]:80x"] {
        let response = proxy_request(
            proxy.local_addr(),
            format!("GET http://{authority}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"),
        );
        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request")
                && response.contains("parser-differential host"),
            "malformed bracketed IPv6 suffix {authority} must be rejected as a parser-differential host by the raw authority parser, got: {response}"
        );
    }

    let missing_bracket = proxy_request(
        proxy.local_addr(),
        "GET http://[::1:8080/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_string(),
    );
    assert!(
        missing_bracket.starts_with("HTTP/1.1 400 Bad Request")
            && missing_bracket.contains("host authority must not include brackets or ports"),
        "a bracketed IPv6 authority with no closing bracket must be rejected, got: {missing_bracket}"
    );
}
