use super::*;

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

/// EE2 wired end-to-end: the node-global allow-ceiling narrows the sandbox
/// policy in the REAL request path. The sandbox allows two hosts; the ceiling
/// lists only one. The intersection host flows; the sandbox-only host is
/// denied by the ceiling (403 naming the allow-ceiling) without contacting
/// the upstream — a union/vec-merge would have allowed it.
#[test]
fn egress_proxy_global_ceiling_narrows_sandbox_policy_end_to_end() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let port = upstream.addr.port();
    let sandbox = allow_policy([
        EgressRule::new("api", EgressProtocol::Http, "allowed.test", port).allow_internal_ips(true),
        EgressRule::new("internal", EgressProtocol::Http, "internal.test", port)
            .allow_internal_ips(true),
    ]);
    let ceiling =
        allow_policy([
            EgressRule::new("ceiling-api", EgressProtocol::Http, "allowed.test", port)
                .allow_internal_ips(true),
        ]);
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(sandbox)
            .with_global_ceiling(ceiling)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
            .with_resolver(loopback_test_resolver()),
    )
    .expect("proxy with ceiling should start");

    let denied = proxy_request(
        proxy.local_addr(),
        format!("GET http://internal.test:{port}/x HTTP/1.1\r\nHost: internal.test\r\n\r\n"),
    );
    assert!(
        denied.starts_with("HTTP/1.1 403 Forbidden"),
        "ceiling must deny the sandbox-only host (intersection, not union), got: {denied}"
    );
    assert!(
        denied.contains("allow-ceiling"),
        "denial must name the ceiling layer, got: {denied}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "ceiling-denied requests must not contact upstream"
    );

    let allowed = proxy_request(
        proxy.local_addr(),
        format!("GET http://allowed.test:{port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"),
    );
    assert!(
        allowed.starts_with("HTTP/1.1 200 OK"),
        "intersection host must flow through both layers, got: {allowed}"
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

    let invalid = EgressPolicy::new([EgressRule::new("wildcard", EgressProtocol::Http, "*", 80)]);
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
fn egress_proxy_config_defaults_to_loopback_ephemeral_bind() {
    let config = WorkloadPepConfig::new(CompiledEgressPolicy::deny_all());

    assert_eq!(config.bind_addr, SocketAddr::from(([127, 0, 0, 1], 0)));
    assert_eq!(config.max_connections, DEFAULT_MAX_CONNECTIONS);
}

#[test]
fn egress_proxy_rejects_zero_connection_limit() {
    let error = match WorkloadPep::start(
        WorkloadPepConfig::new(CompiledEgressPolicy::deny_all()).with_max_connections(0),
    ) {
        Ok(_) => panic!("zero connection limit should be rejected"),
        Err(error) => error,
    };

    assert!(
        error.to_string().contains("max_connections"),
        "error should identify the invalid connection limit: {error}"
    );
}
