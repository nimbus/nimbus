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
    let (log_tx, log_rx) = mpsc::channel();
    // A credential rule forces interception (a plain rule would splice); the
    // point of the test is the audit verdict once interception has committed.
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
    assert!(
        log_rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "the mid-body drop must not add a second terminal event"
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
