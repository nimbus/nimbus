use super::*;

#[test]
fn egress_proxy_intercept_required_connect_fails_closed_without_tls_authority() {
    let upstream = TestTcpServer::start(b"pong");
    let proxy = start_test_proxy_with_store(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
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
            "CONNECT allowed.test:{} HTTP/1.1\r\nHost: allowed.test:{}\r\n\r\n",
            upstream.addr.port(),
            upstream.addr.port()
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden")
            && response.contains("TLS authority is unavailable"),
        "credentialed CONNECT must fail closed without a TLS authority, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "missing TLS authority must deny before upstream contact"
    );
}

#[test]
fn egress_proxy_intercepts_https_and_injects_credentials_after_tls_decryption() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nAlt-Svc: h3=\":443\"\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let (log_tx, log_rx) = mpsc::channel();
    let captured = Arc::new(Mutex::new(Vec::new()));
    let proxy = start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]).into_provider(),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok?token=secret HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS response should read");

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "intercepted HTTPS should receive upstream response, got: {response}"
    );
    assert!(
        !response.to_ascii_lowercase().contains("alt-svc"),
        "intercepted HTTPS must strip Alt-Svc to prevent QUIC/H3 bypass, got: {response}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("upstream should receive decrypted and re-originated h1 request");
    assert!(
        upstream_request.contains("Authorization: Bearer secret-token"),
        "intercepted HTTPS must inject credentials after TLS decryption: {upstream_request}"
    );
    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("intercepted HTTPS should emit one terminal decision log");
    assert!(log.is_allowed());
    assert_eq!(log.credential_identity(), Some("api-token"));
    assert!(
        log.destination().contains("token=<redacted>") && !log.destination().contains("secret"),
        "intercepted HTTPS log must redact query values: {}",
        log.destination()
    );
    assert!(log_rx.recv_timeout(Duration::from_millis(200)).is_err());
    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        3,
        "successful intercepted HTTPS must produce outer intent + inner intent + terminal rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(
        records[0].is_allowed() && records[0].credential_identity().is_none(),
        "first row must be the outer CONNECT intent without inner credentials: {:?}",
        records[0]
    );
    let outer_destination = format!("https://allowed.test:{}", upstream.addr.port());
    assert_eq!(records[0].destination(), outer_destination.as_str());
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Intent);
    assert!(
        records[1].is_allowed(),
        "second row must be the inner HTTPS allow intent: {:?}",
        records[1]
    );
    assert_eq!(records[1].credential_identity(), Some("api-token"));
    assert!(
        records[1].destination().contains("/ok?token=<redacted>")
            && !records[1].destination().contains("secret"),
        "inner intent must carry the redacted inner URL: {:?}",
        records[1]
    );
    assert_eq!(records[2].record_kind(), DecisionRecordKind::Terminal);
    assert!(
        records[2].is_allowed(),
        "third row must be the successful inner terminal allow: {:?}",
        records[2]
    );
    assert_eq!(records[2].credential_identity(), Some("api-token"));
    assert_eq!(records[1].destination(), records[2].destination());
    assert!(
        records
            .iter()
            .all(|record| record.request_id() == records[0].request_id()),
        "all intercepted HTTPS durable rows must share one request id: {records:?}"
    );
}

#[test]
fn egress_proxy_intercepted_https_inner_intent_append_failure_denies_before_upstream() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let proxy = start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]).into_provider(),
        Arc::new(|_| {}),
        failing_second_durable_sink_for_test(Arc::clone(&captured)),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("inner audit failure response should read");

    assert!(
        response.starts_with("HTTP/1.1 502 Bad Gateway")
            && response.contains("inner decision audit append failed"),
        "failed inner intent append must return an inner deny response, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "failed inner intent append must stop before upstream TLS contact"
    );
    let readiness = proxy.readiness().expect("readiness should be observable");
    assert!(
        !readiness.audit_healthy && !readiness.ready,
        "failed inner intent append must make readiness fail closed: {readiness:?}"
    );
    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        2,
        "failed inner intent append should leave only outer intent + terminal deny rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !records[1].is_allowed()
            && records[1]
                .reason()
                .contains("inner decision audit append failed"),
        "terminal row must record the fail-closed audit append denial: {:?}",
        records[1]
    );
    assert_eq!(records[0].request_id(), records[1].request_id());
}

#[test]
fn egress_proxy_intercepted_https_uses_provider_and_fails_closed_for_missing_secret() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let proxy = start_test_proxy_with_provider_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        mock_credential_provider([("other-token", "not-used")]),
        Arc::new(|_| {}),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS response should read");

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden")
            && response.contains("credential material is unavailable"),
        "missing provider material must fail closed, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "missing credential material must deny before upstream TLS contact"
    );
}

#[test]
fn egress_proxy_intercepted_https_does_not_inject_when_policy_only_requires_dlp() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "dlp-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_dlp_rules([
            EgressDlpRule::new("no-secret", "secret").with_max_inspection_bytes(64)
        ])]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]),
        Arc::new(|_| {}),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    let body = "payload=safe";
    tls.write_all(
        format!(
            "POST /upload HTTP/1.1\r\nHost: allowed.test:{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            upstream.addr.port(),
            body.len(),
            body
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS response should read");

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "DLP-only intercepted HTTPS should pass safe bodies, got: {response}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("safe DLP-only HTTPS request should reach upstream");
    assert!(
        !upstream_request.contains("Authorization:") && !upstream_request.contains("secret-token"),
        "DLP-only intercepted HTTPS must not inject credential material: {upstream_request}"
    );
}

#[test]
fn egress_proxy_intercepted_https_denies_caller_credentials_before_upstream() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]),
        Arc::new(|_| {}),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{}\r\nAuthorization: Bearer caller-token\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS response should read");

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden")
            && response.contains("credential-bearing caller header"),
        "caller credentials inside intercepted HTTPS must fail closed, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "caller credential denial must happen before upstream TLS contact"
    );
}

#[test]
fn egress_proxy_intercepted_https_inner_deny_writes_terminal_row_before_403() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (durable_sink, terminal_started, release_terminal) =
        blocking_second_durable_sink_for_test(Arc::clone(&captured));
    let proxy = start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]).into_provider(),
        Arc::new(|_| {}),
        durable_sink,
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );
    let proxy_addr = proxy.local_addr();
    let port = upstream.addr.port();
    let client_authority = proxy_authority.clone();
    let (response_tx, response_rx) = mpsc::channel();
    let client = thread::spawn(move || {
        let mut tls =
            connect_tls_through_proxy(proxy_addr, &client_authority, "allowed.test", port);
        tls.write_all(
            format!(
                "GET /ok HTTP/1.1\r\nHost: allowed.test:{port}\r\nAuthorization: Bearer caller-token\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .expect("inner HTTPS request should write");
        let mut response = String::new();
        read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS deny response should read");
        let _ = response_tx.send(response);
    });

    terminal_started
        .recv_timeout(Duration::from_secs(2))
        .expect("inner terminal durable append should start before the TLS 403");
    assert!(
        response_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "the TLS 403 must not reach the client before the terminal durable append returns"
    );
    release_terminal
        .send(())
        .expect("terminal durable append release should send");
    let response = response_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("client should receive the TLS 403 after terminal append returns");
    client.join().expect("client thread should finish");

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden")
            && response.contains("credential-bearing caller header"),
        "caller credentials inside intercepted HTTPS must fail closed, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "inner credential denial must happen before upstream TLS contact"
    );
    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        2,
        "intercepted inner deny must produce outer intent + inner terminal rows: {records:?}"
    );
    assert!(
        records[0].is_allowed(),
        "first durable row must be the outer CONNECT allow intent: {:?}",
        records[0]
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(
        !records[1].is_allowed()
            && records[1]
                .reason()
                .contains("credential-bearing caller header"),
        "second durable row must be the inner terminal deny: {:?}",
        records[1]
    );
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Terminal);
    assert_eq!(
        records[0].request_id(),
        records[1].request_id(),
        "outer CONNECT intent and inner terminal deny must pair by request id: {records:?}"
    );
}

#[test]
fn egress_proxy_intercepted_https_denies_cookie_configured_header_and_userinfo() {
    let cases = [
        (
            "cookie",
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{port}\r\nCookie: session=caller-secret\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 403 Forbidden",
            "credential-bearing caller header",
        ),
        (
            "configured header",
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{port}\r\nX-Api-Key: caller-key\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 403 Forbidden",
            "credential-bearing caller header",
        ),
        (
            "userinfo",
            "GET https://user:caller-secret@allowed.test:{port}/ok HTTP/1.1\r\nHost: allowed.test:{port}\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 400 Bad Request",
            "origin-form",
        ),
    ];

    for (name, request_template, expected_status, expected_body) in cases {
        let upstream_authority =
            WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
        let upstream = TestHttpsServer::start(
            &upstream_authority,
            "allowed.test",
            "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
        );
        let proxy_authority =
            WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
                upstream_authority.trust_anchor_der(),
            ])
            .expect("proxy authority should trust upstream test CA");
        let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
            allow_policy([EgressRule::new(
                "credentialed-https",
                EgressProtocol::Https,
                "allowed.test",
                upstream.addr.port(),
            )
            .allow_internal_ips(true)
            .with_credential_injection(
                EgressCredentialInjection::new("api-token", "X-Api-Key")
                    .with_value_prefix("Token "),
            )]),
            CredentialSecretStore::from_entries([("api-token", "proxy-secret")]),
            Arc::new(|_| {}),
            proxy_authority.clone(),
            Arc::new(|_| {}),
        );

        let mut tls = connect_tls_through_proxy(
            proxy.local_addr(),
            &proxy_authority,
            "allowed.test",
            upstream.addr.port(),
        );
        let request = request_template.replace("{port}", &upstream.addr.port().to_string());
        tls.write_all(request.as_bytes())
            .expect("inner HTTPS request should write");
        let mut response = String::new();
        read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS response should read");

        assert!(
            response.starts_with(expected_status) && response.contains(expected_body),
            "{name} must be denied before upstream contact, got: {response}"
        );
        assert!(
            upstream
                .request
                .recv_timeout(Duration::from_millis(100))
                .is_err(),
            "{name} denial must happen before upstream TLS contact"
        );
    }
}

#[test]
fn egress_proxy_intercepted_https_allows_configured_header_when_policy_permits_caller_supply() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "X-Api-Key")
                .with_value_prefix("Token ")
                .allow_caller_header(true),
        )]),
        CredentialSecretStore::from_entries([("api-token", "proxy-secret")]),
        Arc::new(|_| {}),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{}\r\nX-Api-Key: caller-key\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS response should read");

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "explicit caller-supply permission should allow configured header, got: {response}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("allowed caller header should reach upstream");
    assert!(
        upstream_request.contains("X-Api-Key: caller-key")
            && !upstream_request.contains("proxy-secret"),
        "caller-supplied configured header should be preserved only when policy permits it: {upstream_request}"
    );
}

#[test]
fn egress_proxy_intercepted_https_redirect_follow_does_not_leak_injected_credentials() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let redirect_upstream = TestHttpsServer::start(
        &upstream_authority,
        "redirect.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nredirect",
    );
    let redirect_location = format!(
        "https://redirect.test:{}/landing",
        redirect_upstream.addr.port()
    );
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        format!("HTTP/1.1 302 Found\r\nLocation: {redirect_location}\r\nContent-Length: 0\r\n\r\n"),
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([
            EgressRule::new(
                "credentialed-https",
                EgressProtocol::Https,
                "allowed.test",
                upstream.addr.port(),
            )
            .allow_internal_ips(true)
            .with_credential_injection(
                EgressCredentialInjection::new("api-token", "Authorization")
                    .with_value_prefix("Bearer "),
            ),
            EgressRule::new(
                "redirect-credentialed-https",
                EgressProtocol::Https,
                "redirect.test",
                redirect_upstream.addr.port(),
            )
            .allow_internal_ips(true)
            .with_credential_injection(
                EgressCredentialInjection::new("redirect-token", "Authorization")
                    .with_value_prefix("Bearer "),
            ),
        ]),
        CredentialSecretStore::from_entries([
            ("api-token", "secret-token"),
            ("redirect-token", "redirect-secret"),
        ]),
        Arc::new(|_| {}),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut original_tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    original_tls
        .write_all(
            format!(
                "GET /redirect HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
                upstream.addr.port()
            )
            .as_bytes(),
        )
        .expect("original HTTPS request should write");
    let mut original_response = String::new();
    read_tls_to_string(&mut original_tls, &mut original_response)
        .expect("original HTTPS redirect response should read");
    assert!(
        original_response.starts_with("HTTP/1.1 302 Found")
            && original_response.contains(&redirect_location)
            && !original_response.contains("secret-token"),
        "redirect response should expose Location but never injected credential material, got: {original_response}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("original upstream should receive credentialed request");
    assert!(
        upstream_request.contains("Authorization: Bearer secret-token"),
        "original upstream should receive only its authorized injected credential: {upstream_request}"
    );

    let mut redirected_tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "redirect.test",
        redirect_upstream.addr.port(),
    );
    redirected_tls
        .write_all(
            format!(
                "GET /landing HTTP/1.1\r\nHost: redirect.test:{}\r\nAuthorization: Bearer secret-token\r\nConnection: close\r\n\r\n",
                redirect_upstream.addr.port()
            )
            .as_bytes(),
        )
        .expect("redirected HTTPS request should write");
    let mut redirected_response = String::new();
    read_tls_to_string(&mut redirected_tls, &mut redirected_response)
        .expect("redirected HTTPS response should read");
    assert!(
        redirected_response.starts_with("HTTP/1.1 403 Forbidden")
            && redirected_response.contains("credential-bearing caller header"),
        "redirect follow carrying stale credentials must fail closed, got: {redirected_response}"
    );
    assert!(
        redirect_upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "redirect target must not receive stale injected credentials"
    );
}

#[test]
fn egress_proxy_intercepted_https_logs_redact_query_and_caller_secret_material() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok?api_key=query-secret HTTP/1.1\r\nHost: allowed.test:{}\r\nAuthorization: Bearer caller-secret\r\nCookie: session=cookie-secret\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS response should read");

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden"),
        "caller credential denial should fail closed, got: {response}"
    );
    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("intercepted HTTPS denial should emit one terminal log");
    assert!(!log.is_allowed());
    assert_eq!(log.reason_class(), "credential");
    assert!(
        log.destination().contains("api_key=<redacted>")
            && !log.destination().contains("query-secret"),
        "intercepted HTTPS destination must redact query values: {}",
        log.destination()
    );
    let rendered_log = format!("{log:?}");
    for secret in [
        "caller-secret",
        "cookie-secret",
        "secret-token",
        "Bearer caller-secret",
    ] {
        assert!(
            !rendered_log.contains(secret),
            "intercepted HTTPS decision log must not expose {secret}: {rendered_log}"
        );
    }
    assert!(log_rx.recv_timeout(Duration::from_millis(200)).is_err());
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "redacted caller-credential denial must not contact upstream"
    );
}

#[test]
fn egress_proxy_intercepted_https_dlp_blocks_before_upstream() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "dlp-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_dlp_rules([
            EgressDlpRule::new("no-secret", "secret").with_max_inspection_bytes(64)
        ])]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    let body = "payload=secret";
    tls.write_all(
        format!(
            "POST /upload HTTP/1.1\r\nHost: allowed.test:{}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            upstream.addr.port(),
            body.len(),
            body
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS response should read");

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden") && response.contains("DLP rule"),
        "DLP rule inside intercepted HTTPS must fail closed, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "DLP-denied intercepted HTTPS must not contact upstream"
    );
}

#[test]
fn egress_proxy_intercepted_https_dlp_fails_closed_for_oversized_and_client_aborted_bodies() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let oversized_upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let aborted_upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let policy = allow_policy([
        EgressRule::new(
            "small-dlp",
            EgressProtocol::Https,
            "allowed.test",
            oversized_upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_dlp_rules([EgressDlpRule::new("small", "secret").with_max_inspection_bytes(4)]),
        EgressRule::new(
            "aborted-dlp",
            EgressProtocol::Https,
            "allowed.test",
            aborted_upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_dlp_rules([EgressDlpRule::new("no-secret", "secret").with_max_inspection_bytes(64)]),
    ]);
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        policy,
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut oversized_tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        oversized_upstream.addr.port(),
    );
    oversized_tls
        .write_all(
            format!(
                "POST /upload HTTP/1.1\r\nHost: allowed.test:{}\r\nContent-Length: 10\r\nConnection: close\r\n\r\nnotsecret!",
                oversized_upstream.addr.port(),
            )
            .as_bytes(),
        )
        .expect("oversized HTTPS DLP request should write");
    let mut oversized_response = String::new();
    read_tls_to_string(&mut oversized_tls, &mut oversized_response)
        .expect("oversized HTTPS DLP response should read");
    assert!(
        oversized_response.starts_with("HTTP/1.1 403 Forbidden")
            && oversized_response.contains("truncated"),
        "oversized HTTPS DLP input must fail closed before upstream, got: {oversized_response}"
    );
    assert!(
        oversized_upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "oversized HTTPS DLP input must not contact upstream"
    );

    let mut aborted_tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        aborted_upstream.addr.port(),
    );
    aborted_tls
        .write_all(
            format!(
                "POST /upload HTTP/1.1\r\nHost: allowed.test:{}\r\nContent-Length: 20\r\nConnection: close\r\n\r\nshort",
                aborted_upstream.addr.port(),
            )
            .as_bytes(),
        )
        .expect("partial HTTPS DLP request should write");
    aborted_tls
        .sock
        .shutdown(Shutdown::Write)
        .expect("client write side should close");
    let mut aborted_response = String::new();
    read_tls_to_string(&mut aborted_tls, &mut aborted_response)
        .expect("client-aborted HTTPS DLP response should read");
    assert!(
        aborted_response.starts_with("HTTP/1.1 403 Forbidden")
            && aborted_response.contains("DLP inspection input unavailable"),
        "client-aborted HTTPS DLP input must fail closed, got: {aborted_response}"
    );
    assert!(
        aborted_upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "client-aborted HTTPS DLP input must not contact upstream"
    );
}

#[test]
fn egress_proxy_intercepted_https_maps_upstream_dial_failure_to_tls_502_and_deny_log() {
    let dead = TcpListener::bind(("127.0.0.1", 0)).expect("dead port should bind");
    let dead_port = dead.local_addr().expect("dead addr should read").port();
    drop(dead);
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([])
            .expect("proxy authority should build");
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            dead_port,
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        dead_port,
    );
    tls.write_all(
        format!("GET /ok HTTP/1.1\r\nHost: allowed.test:{dead_port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("dial-failure response should read");

    assert!(
        response.starts_with("HTTP/1.1 502 Bad Gateway")
            && response.contains("upstream dial failed"),
        "intercepted upstream dial failure must produce the structured 502, got: {response}"
    );
    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("dial failure should emit one terminal deny log");
    assert!(!log.is_allowed());
    assert!(log.reason().contains("upstream dial failed"));
    assert!(log_rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn egress_proxy_intercepted_https_maps_upstream_response_head_failure_to_tls_502_and_deny_log() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsCloseServer::start_after_request(&upstream_authority, "allowed.test");
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("response-read failure should read");

    assert!(
        response.starts_with("HTTP/1.1 502 Bad Gateway")
            && response.contains("upstream response read failed"),
        "upstream response head failure must produce the structured 502, got: {response}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("upstream should receive the re-originated request before closing");
    assert!(upstream_request.contains("Authorization: Bearer secret-token"));
    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("response read failure should emit one terminal deny log");
    assert!(!log.is_allowed());
    assert!(log.reason().contains("upstream response read failed"));
    assert!(log_rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn egress_proxy_intercepted_https_maps_upstream_write_or_read_failure_to_tls_502_and_deny_log() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsCloseServer::start_after_handshake(&upstream_authority, "allowed.test");
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("upstream close response should read");

    assert!(
        response.starts_with("HTTP/1.1 502 Bad Gateway")
            && (response.contains("upstream write failed")
                || response.contains("upstream response read failed")),
        "upstream close during request/response must produce a structured 502, got: {response}"
    );
    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("upstream close should emit one terminal deny log");
    assert!(!log.is_allowed());
    assert!(
        log.reason().contains("upstream write failed")
            || log.reason().contains("upstream response read failed")
    );
    assert!(log_rx.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn egress_proxy_intercepted_https_audits_allow_when_upstream_drops_mid_body() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    // A valid 200 head that declares 100 body bytes but delivers only 5 before
    // the upstream drops: the request is authorized and executed, and its
    // response head reaches the client, so a mid-body transport failure must
    // NOT flip the audit verdict to deny.
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (log_tx, log_rx) = mpsc::channel();
    // A credential rule forces interception (a plain rule would splice); the
    // point of the test is the audit verdict once interception has committed.
    let proxy = start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]).into_provider(),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    let _ = read_tls_to_string(&mut tls, &mut response);
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "client must receive the delivered upstream response head: {response}"
    );

    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("a mid-body drop must still emit exactly one terminal event");
    assert!(
        log.is_allowed(),
        "an authorized request that reached upstream must audit as ALLOW even when the upstream drops mid-body: {log:?}"
    );
    assert_eq!(log.record_kind(), DecisionRecordKind::TerminalAfterResponse);
    assert!(
        log_rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "the mid-body drop must not add a second terminal event"
    );
    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        3,
        "mid-body upstream close must produce outer intent + inner intent + after-response durable rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(
        records[0].is_allowed() && records[0].credential_identity().is_none(),
        "the outer CONNECT intent row must remain an allow without inner credential identity: {:?}",
        records[0]
    );
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Intent);
    assert!(
        records[1].is_allowed() && records[1].credential_identity() == Some("api-token"),
        "the inner intent row must carry the credential identity: {:?}",
        records[1]
    );
    assert!(
        records[1].destination().contains("/ok"),
        "inner intent must carry the inner path: {:?}",
        records[1]
    );
    assert_eq!(
        records[2].record_kind(),
        DecisionRecordKind::TerminalAfterResponse
    );
    assert!(
        records[2].is_allowed(),
        "post-response transport failure is executed egress and must not audit as deny: {:?}",
        records[2]
    );
    assert!(
        records
            .iter()
            .all(|record| record.request_id() == records[0].request_id()),
        "intercept intent and after-response terminal must pair by request id: {records:?}"
    );
    assert_eq!(records[1].destination(), records[2].destination());
    assert_eq!(records[2].credential_identity(), Some("api-token"));
    assert_eq!(
        records[2].reason(),
        crate::decision_log::UPSTREAM_FAILURE_AFTER_RESPONSE_REASON
    );
    assert!(
        !records[2].reason().contains("short"),
        "after-response reason must not include upstream body bytes: {:?}",
        records[2]
    );
}

#[test]
fn egress_proxy_intercepted_https_informational_response_then_upstream_close_is_terminal_deny() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 100 Continue\r\n\r\n",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]).into_provider(),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /one-hundred-then-close HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response)
        .expect("informational response followed by local 502 should read");
    assert!(
        response.starts_with("HTTP/1.1 100 Continue")
            && response.contains("HTTP/1.1 502 Bad Gateway")
            && response.contains("upstream response read failed"),
        "the client should see the informational response followed by the local final 502: {response}"
    );

    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("1xx followed by upstream close should emit one terminal deny log");
    assert_eq!(log.record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !log.is_allowed() && log.reason().contains("upstream response read failed"),
        "upstream failure after only a 1xx must audit as pre-final deny: {log:?}"
    );
    assert!(log_rx.recv_timeout(Duration::from_millis(200)).is_err());

    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        3,
        "1xx followed by upstream close should produce outer intent + inner intent + pre-final terminal rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Intent);
    assert!(
        records[1].is_allowed() && records[1].credential_identity() == Some("api-token"),
        "second durable row must be the inner allow intent: {:?}",
        records[1]
    );
    assert!(
        records[1].destination().contains("/one-hundred-then-close"),
        "inner intent must carry the inner path: {:?}",
        records[1]
    );
    assert_eq!(records[2].record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !records[2].is_allowed()
            && records[2]
                .reason()
                .contains("upstream response read failed"),
        "upstream failure after only a 1xx must not become an after-response allow: {:?}",
        records[2]
    );
    assert!(
        records
            .iter()
            .all(|record| record.request_id() == records[0].request_id()),
        "intercepted 1xx pre-final terminal must pair with the request intent row: {records:?}"
    );
}

#[test]
fn egress_proxy_intercepted_https_rejects_upstream_switching_protocols_before_forwarding() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]).into_provider(),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /upgrade HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("upgrade denial should read");

    assert!(
        response.starts_with("HTTP/1.1 502 Bad Gateway")
            && response.contains("HTTPS interception does not support protocol upgrades")
            && !response.contains("101 Switching Protocols"),
        "intercepted upstream 101 must be replaced with the local deny response: {response}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("upstream should receive the re-originated request before returning 101");
    assert!(
        upstream_request.contains("GET /upgrade HTTP/1.1")
            && upstream_request.contains("Authorization: Bearer secret-token"),
        "upstream should see the authorized inner request before its unsupported 101: {upstream_request}"
    );

    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("unsupported 101 should emit one terminal deny log");
    assert_eq!(log.record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !log.is_allowed() && log.reason().contains("does not support protocol upgrades"),
        "unsupported 101 must audit as a pre-final deny: {log:?}"
    );
    assert!(log_rx.recv_timeout(Duration::from_millis(200)).is_err());

    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        3,
        "unsupported 101 must produce outer intent + inner intent + terminal deny rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Intent);
    assert!(
        records[1].is_allowed() && records[1].credential_identity() == Some("api-token"),
        "second durable row must be the inner allow intent: {:?}",
        records[1]
    );
    assert!(
        records[1].destination().contains("/upgrade"),
        "inner intent must carry the inner path: {:?}",
        records[1]
    );
    assert_eq!(records[2].record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !records[2].is_allowed()
            && records[2]
                .reason()
                .contains("does not support protocol upgrades"),
        "third durable row must be the unsupported-upgrade terminal deny: {:?}",
        records[2]
    );
    assert!(
        records
            .iter()
            .all(|record| record.request_id() == records[0].request_id()),
        "unsupported-upgrade rows must pair by request id: {records:?}"
    );
}

#[test]
fn egress_proxy_intercepted_https_bounds_informational_responses_and_fails_closed() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    // 12 informational heads and no final response: over the cap of 8.
    let endless_informational = "HTTP/1.1 100 Continue\r\n\r\n".repeat(12);
    let upstream = TestHttpsServer::start(
        &upstream_authority,
        "allowed.test",
        Box::leak(endless_informational.into_boxed_str()),
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]).into_provider(),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /endless-informational HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response)
        .expect("bounded informational stream followed by local 502 should read");
    assert!(
        response.contains("HTTP/1.1 502 Bad Gateway")
            && response.contains("informational-response limit"),
        "exceeding the 1xx cap must fail closed with the limit reason: {response}"
    );

    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("exceeding the 1xx cap should emit one terminal deny log");
    assert_eq!(log.record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !log.is_allowed() && log.reason().contains("informational-response limit"),
        "the 1xx-cap terminal must audit as a pre-final deny: {log:?}"
    );
    assert!(log_rx.recv_timeout(Duration::from_millis(200)).is_err());

    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        3,
        "1xx-cap termination should produce outer intent + inner intent + pre-final terminal rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Intent);
    assert!(
        records[1].is_allowed() && records[1].credential_identity() == Some("api-token"),
        "second durable row must be the inner allow intent: {:?}",
        records[1]
    );
    assert!(
        records[1].destination().contains("/endless-informational"),
        "inner intent must carry the inner path: {:?}",
        records[1]
    );
    assert_eq!(records[2].record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !records[2].is_allowed(),
        "the 1xx-cap terminal row must not be an after-response allow: {:?}",
        records[2]
    );
    assert!(
        records
            .iter()
            .all(|record| record.request_id() == records[0].request_id()),
        "the 1xx-cap terminal must pair with the request intent row: {records:?}"
    );
}

#[test]
fn egress_proxy_intercepted_https_abort_mid_relay_writes_after_response_terminal() {
    let upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestStallingHttpsBodyServer::start(&upstream_authority, "allowed.test");
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([
            upstream_authority.trust_anchor_der(),
        ])
        .expect("proxy authority should trust upstream test CA");
    let captured = Arc::new(Mutex::new(Vec::new()));
    let proxy = start_test_proxy_with_provider_logger_tls_durable_sink_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]).into_provider(),
        Arc::new(|_| {}),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let proxy_addr = proxy.local_addr();
    let upstream_port = upstream.addr.port();
    let client_authority = proxy_authority.clone();
    let (head_tx, head_rx) = mpsc::channel();
    let client = thread::spawn(move || {
        let mut tls =
            connect_tls_through_proxy(proxy_addr, &client_authority, "allowed.test", upstream_port);
        tls.write_all(
            format!(
                "GET /slow HTTP/1.1\r\nHost: allowed.test:{upstream_port}\r\nConnection: close\r\n\r\n"
            )
            .as_bytes(),
        )
        .expect("inner HTTPS request should write");
        let head = read_tls_headers_to_string(&mut tls).expect("response head should read");
        let _ = head_tx.send(head);
        let mut rest = String::new();
        let _ = read_tls_to_string(&mut tls, &mut rest);
    });

    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(2))
        .expect("stalled HTTPS upstream should receive the intercepted request");
    assert!(
        upstream_request.starts_with("GET /slow HTTP/1.1"),
        "intercepted request should reach upstream before cancellation: {upstream_request}"
    );
    let response_head = head_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("client should receive the upstream response head before cancellation");
    assert!(
        response_head.starts_with("HTTP/1.1 200 OK"),
        "response head must reach the client before proxy cancellation: {response_head}"
    );

    drop(proxy);
    client.join().expect("client thread should finish");
    upstream.release();

    let deadline = Instant::now() + Duration::from_secs(2);
    let records = loop {
        let records = snapshot_durable_logs(&captured);
        if records.len() >= 3 || Instant::now() >= deadline {
            break records;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        records.len(),
        3,
        "mid-relay cancellation must produce outer intent + inner intent + after-response durable rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert!(records[0].credential_identity().is_none());
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Intent);
    assert!(
        records[1].is_allowed() && records[1].credential_identity() == Some("api-token"),
        "second durable row must be the inner allow intent: {:?}",
        records[1]
    );
    assert!(
        records[1].destination().contains("/slow"),
        "inner intent must carry the inner path: {:?}",
        records[1]
    );
    assert_eq!(
        records[2].record_kind(),
        DecisionRecordKind::TerminalAfterResponse
    );
    assert!(
        records[2].is_allowed(),
        "response-started cancellation must audit as executed allow, not synthetic deny: {records:?}"
    );
    assert!(
        records
            .iter()
            .all(|record| record.request_id() == records[0].request_id()),
        "after-response terminal must pair with the original intent row: {records:?}"
    );
    assert_eq!(records[1].destination(), records[2].destination());
    assert_eq!(records[2].credential_identity(), Some("api-token"));
    assert!(
        records.iter().all(EgressDecisionLog::is_allowed),
        "response-started cancellation must not append a synthetic deny: {records:?}"
    );
    assert_eq!(
        records[2].reason(),
        crate::decision_log::ABORT_AFTER_RESPONSE_REASON
    );
}

#[test]
fn egress_proxy_intercepted_https_strictly_verifies_upstream_tls() {
    let untrusted_upstream_authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("upstream authority should build");
    let upstream = TestHttpsServer::start(
        &untrusted_upstream_authority,
        "allowed.test",
        "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok",
    );
    let proxy_authority =
        WorkloadPepTlsAuthority::generate_ephemeral_with_upstream_trust_anchors([])
            .expect("proxy authority should have an empty upstream test root set");
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([EgressRule::new(
            "credentialed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]),
        Arc::new(|_| {}),
        proxy_authority.clone(),
        Arc::new(|_| {}),
    );

    let mut tls = connect_tls_through_proxy(
        proxy.local_addr(),
        &proxy_authority,
        "allowed.test",
        upstream.addr.port(),
    );
    tls.write_all(
        format!(
            "GET /ok HTTP/1.1\r\nHost: allowed.test:{}\r\nConnection: close\r\n\r\n",
            upstream.addr.port()
        )
        .as_bytes(),
    )
    .expect("inner HTTPS request should write");
    let mut response = String::new();
    read_tls_to_string(&mut tls, &mut response).expect("inner HTTPS response should read");

    assert!(
        response.starts_with("HTTP/1.1 502 Bad Gateway")
            && response.contains("upstream TLS verification failed"),
        "untrusted upstream TLS must fail closed, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "strict upstream TLS failure must not forward decrypted request bytes"
    );
}

#[test]
fn egress_proxy_intercepted_https_phase_trace_keeps_forward_after_enforcement() {
    let phases = recorded_phases();
    let authority =
        WorkloadPepTlsAuthority::generate_ephemeral().expect("authority should generate");
    let proxy = start_test_proxy_with_store_logger_tls_and_phase_observer(
        allow_policy([
            EgressRule::new("dlp", EgressProtocol::Https, "allowed.test", 443)
                .allow_internal_ips(true)
                .with_dlp_rules([
                    EgressDlpRule::new("no-secret", "secret").with_max_inspection_bytes(64)
                ]),
        ]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        authority.clone(),
        phase_observer(&phases),
    );

    let mut tls = connect_tls_through_proxy(proxy.local_addr(), &authority, "allowed.test", 443);
    tls.write_all(
        b"POST /upload HTTP/1.1\r\nHost: allowed.test:443\r\nContent-Length: 10\r\n\r\nsecret=yes",
    )
    .expect("inner request should write");
    let mut response = String::new();
    let _ = read_tls_to_string(&mut tls, &mut response);
    assert!(
        response.contains("403"),
        "DLP-matching intercepted body must be blocked: {response}"
    );

    let trace = snapshot_phases(&phases);
    let dlp_index = trace
        .iter()
        .position(|phase| *phase == EgressProxyRequestPhase::BoundedDlpInspection)
        .expect("intercepted request must record bounded DLP inspection");
    assert!(
        !trace.contains(&EgressProxyRequestPhase::Forward),
        "a DLP-denied intercepted request must never record Forward (no upstream contact): {trace:?}"
    );
    let terminal_index = trace
        .iter()
        .rposition(|phase| *phase == EgressProxyRequestPhase::TerminalLog)
        .expect("intercepted request must record a terminal event");
    assert!(
        dlp_index < terminal_index,
        "DLP must be decided before the terminal event: {trace:?}"
    );
}
