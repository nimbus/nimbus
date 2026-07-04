use super::*;

#[test]
fn egress_proxy_credential_injection_attaches_secret_only_to_allowed_destination() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let denied_upstream =
        TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\ndenied");
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

    let allowed = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream.addr.port()
        ),
    );
    assert!(
        allowed.starts_with("HTTP/1.1 200 OK"),
        "credential-injected allowed request should reach upstream, got: {allowed}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("upstream should receive allowed request");
    assert!(
        upstream_request.contains("Authorization: Bearer secret-token"),
        "proxy must inject resolved credential only on the allowed request: {upstream_request}"
    );

    let denied = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://denied.test:{}/ok HTTP/1.1\r\nHost: denied.test\r\n\r\n",
            denied_upstream.addr.port()
        ),
    );
    assert!(
        denied.starts_with("HTTP/1.1 403 Forbidden"),
        "denied destinations must not receive credentials, got: {denied}"
    );
    assert!(
        denied_upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "denied credential targets must not contact upstream"
    );
}

#[test]
fn egress_proxy_denies_caller_supplied_credential_headers_unless_policy_allows() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
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
            "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\nAuthorization: Bearer caller-token\r\n\r\n",
            upstream.addr.port()
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden")
            && response.contains("credential-bearing caller header"),
        "caller-supplied credential headers must fail closed, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "denied caller credentials must not contact upstream"
    );
}

#[test]
fn egress_proxy_dlp_blocks_matching_body_and_truncated_or_unavailable_input() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let proxy = start_test_proxy(allow_policy([EgressRule::new(
        "allowed",
        EgressProtocol::Http,
        "allowed.test",
        upstream.addr.port(),
    )
    .with_methods(["POST"])
    .with_path_prefixes(["/upload"])
    .allow_internal_ips(true)
    .with_dlp_rules([EgressDlpRule::new("no-ssn", "ssn=").with_max_inspection_bytes(64)])]));

    let matching_body = "user=alice&ssn=123";
    let blocked = proxy_request(
        proxy.local_addr(),
        format!(
            "POST http://allowed.test:{}/upload HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: {}\r\n\r\n{}",
            upstream.addr.port(),
            matching_body.len(),
            matching_body
        ),
    );
    assert!(
        blocked.starts_with("HTTP/1.1 403 Forbidden") && blocked.contains("DLP rule"),
        "DLP pattern match must block before dial, got: {blocked}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "DLP-denied requests must not contact upstream"
    );

    let truncated_proxy = start_test_proxy(allow_policy([EgressRule::new(
        "small-inspection",
        EgressProtocol::Http,
        "allowed.test",
        upstream.addr.port(),
    )
    .with_methods(["POST"])
    .with_path_prefixes(["/upload"])
    .allow_internal_ips(true)
    .with_dlp_rules([EgressDlpRule::new("small", "secret").with_max_inspection_bytes(4)])]));
    let truncated = proxy_request(
        truncated_proxy.local_addr(),
        format!(
            "POST http://allowed.test:{}/upload HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: 10\r\n\r\nnotsecret!",
            upstream.addr.port(),
        ),
    );
    assert!(
        truncated.starts_with("HTTP/1.1 403 Forbidden") && truncated.contains("truncated"),
        "DLP input larger than inspection cap must fail closed, got: {truncated}"
    );

    let unavailable = proxy_request(
        truncated_proxy.local_addr(),
        format!(
            "POST http://allowed.test:{}/upload HTTP/1.1\r\nHost: allowed.test\r\n\r\nbody-without-length",
            upstream.addr.port(),
        ),
    );
    assert!(
        unavailable.starts_with("HTTP/1.1 403 Forbidden")
            && unavailable.contains("DLP inspection input unavailable"),
        "DLP without bounded inspection input must fail closed, got: {unavailable}"
    );
}
