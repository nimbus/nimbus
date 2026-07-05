use super::*;

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
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::without_active_policy()
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
fn egress_proxy_policy_denied_hostname_does_not_resolve() {
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let resolver_call_counter = Arc::clone(&resolver_calls);
    let resolver = Arc::new(move |_host: &str, _port: u16| {
        resolver_call_counter.fetch_add(1, Ordering::SeqCst);
        Ok(vec![SocketAddr::from(([127, 0, 0, 1], 80))])
    });
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            80,
        )
        .allow_internal_ips(true)]))
        .with_resolver(resolver),
    )
    .expect("proxy should start");

    let response = proxy_request(
        proxy.local_addr(),
        "GET http://denied.test:80/ok HTTP/1.1\r\nHost: denied.test\r\n\r\n".to_string(),
    );

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden") && response.contains("default deny"),
        "policy-denied hostnames must fail closed before DNS, got: {response}"
    );
    assert_eq!(
        resolver_calls.load(Ordering::SeqCst),
        0,
        "policy-denied hostnames must not invoke the resolver"
    );
}

#[test]
fn egress_proxy_malformed_authority_denies_before_dns() {
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let resolver_call_counter = Arc::clone(&resolver_calls);
    let resolver = Arc::new(move |_host: &str, _port: u16| {
        resolver_call_counter.fetch_add(1, Ordering::SeqCst);
        Ok(vec![SocketAddr::from(([127, 0, 0, 1], 443))])
    });
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Https,
            "allowed.test",
            443,
        )
        .allow_internal_ips(true)]))
        .with_resolver(resolver),
    )
    .expect("proxy should start");

    let response = proxy_request(
        proxy.local_addr(),
        "CONNECT allowed.test%2fmetadata:443 HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_string(),
    );

    assert!(
        response.starts_with("HTTP/1.1 400 Bad Request")
            && response.contains("canonical authority"),
        "malformed authorities must fail before DNS, got: {response}"
    );
    assert_eq!(
        resolver_calls.load(Ordering::SeqCst),
        0,
        "malformed authorities must not invoke the resolver"
    );
}

#[test]
fn egress_proxy_allowed_hostname_invokes_resolver_just_in_time() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let upstream_port = upstream.addr.port();
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let resolver_call_counter = Arc::clone(&resolver_calls);
    let resolver = Arc::new(move |host: &str, port: u16| {
        assert_eq!(host, "allowed.test");
        assert_eq!(port, upstream_port);
        resolver_call_counter.fetch_add(1, Ordering::SeqCst);
        Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
    });
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream_port,
        )
        .allow_internal_ips(true)]))
        .with_resolver(resolver),
    )
    .expect("proxy should start");

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://Allowed.TEST.:{upstream_port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "case/trailing-dot canonical hostnames should still resolve and forward just in time, got: {response}"
    );
    assert_eq!(
        resolver_calls.load(Ordering::SeqCst),
        1,
        "allowed hostnames should invoke the resolver exactly when forwarding"
    );
}

#[test]
fn egress_proxy_resolved_internal_ip_still_denies_before_dial() {
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
fn egress_proxy_dns_overflow_defaults_to_deny_before_dial() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let resolver = Arc::new(move |_host: &str, port: u16| {
        Ok(vec![
            SocketAddr::from(([127, 0, 0, 1], port)),
            SocketAddr::from(([127, 0, 0, 2], port)),
        ])
    });
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
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

// Boundary companion to `egress_proxy_dns_overflow_defaults_to_deny_before_dial`:
// at exactly `max_addresses_per_host` the resolved address set is within budget
// and must forward. The cap comparison is strictly `len > cap` (over budget); a
// `>=` regression would treat an exactly-at-cap set as overflow and deny it. With
// cap 1 and one resolved address, correct code forwards (200) while the mutant
// denies (403 "DNS cache overflow default deny").
#[test]
fn egress_proxy_allows_dns_addresses_exactly_at_cap() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let upstream_port = upstream.addr.port();
    let resolver =
        Arc::new(move |_host: &str, port: u16| Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))]));
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream_port,
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
            "GET http://allowed.test:{upstream_port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "exactly max_addresses_per_host resolved addresses is within budget and must forward; a `>=` cap regression would deny it as overflow, got: {response}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("an exactly-at-cap DNS set must forward to upstream, not be denied as overflow");
    assert!(
        upstream_request.starts_with("GET /ok HTTP/1.1"),
        "proxy should forward the origin-form request after an at-cap DNS resolution, got: {upstream_request}"
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
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
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
        "GET http://allowed.test@127.0.0.1/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_string(),
    );
    let encoded = proxy_request(
        proxy.local_addr(),
        "CONNECT allowed.test%2eexample:443 HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_string(),
    );
    let null_byte = proxy_request(
        proxy.local_addr(),
        "CONNECT allowed.test\0example:443 HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_string(),
    );
    let numeric_http = proxy_request(
        proxy.local_addr(),
        "GET http://2130706433/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_string(),
    );

    assert!(
        userinfo.starts_with("HTTP/1.1 400 Bad Request")
            && userinfo.contains("canonical authority"),
        "userinfo authority smuggling should reject, got: {userinfo}"
    );
    assert!(
        encoded.starts_with("HTTP/1.1 400 Bad Request") && encoded.contains("canonical authority"),
        "encoded authority should reject, got: {encoded}"
    );
    assert!(
        null_byte.starts_with("HTTP/1.1 400 Bad Request")
            && null_byte.contains("canonical authority"),
        "null/control authority should reject, got: {null_byte:?}"
    );
    assert!(
        numeric_http.starts_with("HTTP/1.1 400 Bad Request")
            && numeric_http.contains("canonical authority"),
        "parser-differential numeric HTTP authority should reject before URL normalization, got: {numeric_http}"
    );
    assert_eq!(
        resolver_calls.load(Ordering::SeqCst),
        0,
        "canonicalization failures must happen before DNS resolution"
    );
}

// Rebind coverage (L19), positive: authorize and dial must both pin to the
// first resolved address. addresses[0] is a globally-classified but unreachable
// peer (192.88.99.0/24, deprecated 6to4 anycast — outside the egress internal/
// non-global deny list) and addresses[1] is the real loopback upstream. The
// proxy must authorize addresses[0] (a 403 would mean a later internal address
// was authorized) and dial addresses[0] (reaching the loopback upstream would
// mean addresses[1] was dialed).
#[test]
fn egress_proxy_pins_authorization_and_dial_to_first_resolved_address() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let upstream_port = upstream.addr.port();
    let resolver = Arc::new(move |_host: &str, port: u16| {
        Ok(vec![
            SocketAddr::from(([192, 88, 99, 1], port)),
            SocketAddr::from(([127, 0, 0, 1], port)),
        ])
    });
    let proxy = WorkloadPep::start(
        // allow_internal_ips defaults to false: only the global addresses[0] is
        // authorizable.
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream_port,
        )]))
        .with_timeouts(Duration::from_millis(400), Duration::from_secs(2))
        .with_resolver(resolver),
    )
    .expect("proxy should start");

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{upstream_port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
        ),
    );

    assert!(
        !response.starts_with("HTTP/1.1 403"),
        "addresses[0] is global, so authorization must pass; a 403 means a later internal address was authorized, got: {response}"
    );
    assert!(
        response.starts_with("HTTP/1.1 502 Bad Gateway"),
        "the dial must target the unreachable global addresses[0] (502), not fall through to a later address, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "the loopback addresses[1] must never be dialed when addresses[0] is authorized"
    );
}

// Rebind coverage (L19), negative: an internal first resolved address must be
// denied (SSRF guard) without any upstream contact. This proves authorization
// is evaluated against addresses[0].
#[test]
fn egress_proxy_denies_when_first_resolved_address_is_internal() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let upstream_port = upstream.addr.port();
    let resolver =
        Arc::new(move |_host: &str, port: u16| Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))]));
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream_port,
        )]))
        .with_timeouts(Duration::from_secs(2), Duration::from_secs(2))
        .with_resolver(resolver),
    )
    .expect("proxy should start");

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{upstream_port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden")
            && response.contains("internal/non-global targets"),
        "an internal addresses[0] must be denied as SSRF/internal egress, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(200))
            .is_err(),
        "an internal-denied request must not contact upstream"
    );
}

#[test]
fn egress_proxy_dns_budget_times_out_closed_without_disturbing_sibling() {
    let blocked_proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            80,
        )
        .allow_internal_ips(true)]))
        .with_timeouts(Duration::from_millis(200), Duration::from_secs(2))
        .with_resolver(Arc::new(|_host: &str, _port: u16| {
            thread::sleep(Duration::from_secs(3));
            Ok(Vec::new())
        })),
    )
    .expect("proxy should start");

    let response = proxy_request(
        blocked_proxy.local_addr(),
        "GET http://allowed.test:80/ HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_owned(),
    );
    assert!(
        response.starts_with("HTTP/1.1 502") && response.contains("DNS resolution failed"),
        "a wedged resolver must fail the request closed within the DNS wait bound: {response}"
    );

    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let sibling = start_test_proxy(allow_policy([EgressRule::new(
        "allowed",
        EgressProtocol::Http,
        "allowed.test",
        upstream.addr.port(),
    )
    .allow_internal_ips(true)]));
    let sibling_response = proxy_request(
        sibling.local_addr(),
        format!(
            "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream.addr.port()
        ),
    );
    assert!(
        sibling_response.starts_with("HTTP/1.1 200 OK"),
        "a sibling PEP on the shared substrate must keep serving past a wedged resolver: {sibling_response}"
    );

    // Drop stays bounded even while the wedged resolver thread is still
    // sleeping on the blocking pool.
    drop(blocked_proxy);
}
