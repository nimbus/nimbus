use super::*;

#[test]
fn egress_proxy_rejects_numeric_ip_connect_authority() {
    // CONNECT authorities skip the WHATWG IPv4 normalization ForwardHttp targets
    // get from `Url::parse`, so dword/hex/octal obfuscation must be rejected rather
    // than handed to the resolver. `2130706433` == `0x7f000001` == 127.0.0.1.
    let proxy = start_test_proxy(CompiledEgressPolicy::deny_all());

    for authority in ["2130706433:443", "0x7f000001:443", "0177.0.0.1:443"] {
        let response = proxy_request(
            proxy.local_addr(),
            format!("CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n\r\n"),
        );
        assert!(
            response.starts_with("HTTP/1.1 400 Bad Request"),
            "numeric-IP CONNECT authority {authority} must be rejected, got: {response}"
        );
    }
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
fn egress_proxy_canonicalizes_connect_authority_case_and_trailing_dot() {
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
                "CONNECT Allowed.TEST.:{} HTTP/1.1\r\nHost: allowed.test:{}\r\n\r\nping",
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
        response.starts_with("HTTP/1.1 200 Connection Established") && response.contains("pong"),
        "CONNECT canonicalization should match HTTP authority normalization, got: {response}"
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
