use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::worker::ConnectionLimiter;
use nimbus_core::TenantId;
use nimbus_egress::{
    CompiledEgressPolicy, EgressCredentialInjection, EgressDlpRule, EgressPolicy, EgressProtocol,
    EgressRule,
};

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
        "GET http://allowed.test@127.0.0.1/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_string(),
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
        encoded.starts_with("HTTP/1.1 400 Bad Request") && encoded.contains("canonical authority"),
        "encoded authority should reject, got: {encoded}"
    );
    assert_eq!(
        resolver_calls.load(Ordering::SeqCst),
        0,
        "canonicalization failures must happen before DNS resolution"
    );
}

// Guards the SHAPE of the documented request-phase model for the egress PEP's
// planned phase-driven dispatch (see `phase.rs`). The worker uses inline
// ordering today; this asserts the model's structural invariants rather than
// copying the constant back to itself: every phase appears exactly once, and the
// security-critical orderings hold (resolve DNS before authorizing the resolved
// peer, authorize before dialing, and select the pool key before dialing).
#[test]
fn egress_proxy_request_phase_model_holds_security_critical_orderings() {
    let position = |phase: EgressProxyRequestPhase| {
        REQUEST_PHASE_ORDER
            .iter()
            .position(|candidate| *candidate == phase)
            .unwrap_or_else(|| panic!("phase {phase:?} must appear in REQUEST_PHASE_ORDER"))
    };

    // Every phase appears exactly once.
    let all_phases = [
        EgressProxyRequestPhase::CanonicalizeAuthority,
        EgressProxyRequestPhase::ResolveDns,
        EgressProxyRequestPhase::AuthorizeResolvedPeer,
        EgressProxyRequestPhase::SelectPoolKey,
        EgressProxyRequestPhase::Dial,
        EgressProxyRequestPhase::Relay,
    ];
    assert_eq!(
        REQUEST_PHASE_ORDER.len(),
        all_phases.len(),
        "the phase order must contain exactly the known phases"
    );
    for phase in all_phases {
        assert_eq!(
            REQUEST_PHASE_ORDER
                .iter()
                .filter(|candidate| **candidate == phase)
                .count(),
            1,
            "phase {phase:?} must appear exactly once"
        );
    }

    // Security-critical orderings.
    assert!(
        position(EgressProxyRequestPhase::ResolveDns)
            < position(EgressProxyRequestPhase::AuthorizeResolvedPeer),
        "DNS must resolve before the resolved peer is authorized"
    );
    assert!(
        position(EgressProxyRequestPhase::AuthorizeResolvedPeer)
            < position(EgressProxyRequestPhase::Dial),
        "the resolved peer must be authorized before dialing"
    );
    assert!(
        position(EgressProxyRequestPhase::SelectPoolKey) < position(EgressProxyRequestPhase::Dial),
        "the pool key must be selected before dialing"
    );
}

// Guards the SHAPE of the connection-pool isolation key for the PLANNED egress
// connection pooling (see `pool.rs`). Pooling is not wired yet — the worker
// dials fresh per request — so this asserts the key DISTINGUISHES every
// security-relevant isolation dimension: mutating any single field yields a
// different key. Once pooling lands, that property is what guarantees two
// requests differing in any such dimension can never share a pooled connection.
// This guards the key's shape, not an enforced runtime guarantee.
#[test]
fn egress_proxy_pool_key_shape_distinguishes_every_isolation_dimension() {
    let tenant_id = TenantId::new("tenant-a").expect("tenant id should be valid");
    let other_tenant = TenantId::new("tenant-b").expect("tenant id should be valid");
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

    // Every security-relevant dimension must change the key. Each closure
    // mutates exactly one field so a missing dimension fails this test.
    type PoolKeyMutator = fn(&mut EgressProxyPoolKey);
    let mutators: Vec<(&str, PoolKeyMutator)> = vec![
        ("tenant_id", |key| {
            key.tenant_id = TenantId::new("tenant-b").expect("tenant id should be valid");
        }),
        ("substrate", |key| {
            key.substrate = EgressProxySubstrate::Isolate;
        }),
        ("policy_generation", |key| {
            key.policy_generation = key.policy_generation.next();
        }),
        ("credential_identity", |key| {
            key.credential_identity = Some("secret:github".to_string());
        }),
        ("destination", |key| {
            key.destination = "https://api.github.com:443".to_string();
        }),
        ("resolved_peer", |key| {
            key.resolved_peer = SocketAddr::from(([203, 0, 113, 11], 443));
        }),
        ("sni", |key| {
            key.sni = Some("uploads.stripe.com".to_string());
        }),
        ("tls_verification", |key| {
            key.tls_verification = TlsVerificationMode::Disabled;
        }),
        ("client_cert_identity", |key| {
            key.client_cert_identity = None;
        }),
        ("alpn", |key| {
            key.alpn = vec!["http/1.1".to_string()];
        }),
        ("proxy_settings", |key| {
            key.proxy_settings = Some("upstream-proxy-a".to_string());
        }),
    ];

    for (field, mutate) in mutators {
        let mut changed = base.clone();
        mutate(&mut changed);
        assert_ne!(
            base, changed,
            "mutating `{field}` must produce a distinct pool key so the dimension can never be pooled together"
        );
    }

    // Sanity: an unmutated clone shares the key (the baseline for "same key
    // means poolable"), and the tenant mutator above genuinely diverged.
    assert_eq!(base, base.clone());
    assert_ne!(base.tenant_id, other_tenant);
}

// Audit (M6): every terminal deny must emit exactly one decision-log record
// flagged `allowed = false`, carrying the reason and matched rule, and leaking
// no secret material. Before M6 the logger fired only on the allow path, so a
// blocked exfiltration attempt was an audit blind spot. This test fails (the
// `recv_timeout` expect panics) if the deny path stops emitting a log.
#[test]
fn egress_proxy_audits_denied_request_with_one_redacted_record() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_store_and_logger(
        CompiledEgressPolicy::deny_all(),
        CredentialSecretStore::empty(),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
    );

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/secret?token=supersecret HTTP/1.1\r\nHost: allowed.test\r\nAuthorization: Bearer topsecret\r\n\r\n",
            upstream.addr.port()
        ),
    );
    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden"),
        "deny-all policy must reject the request, got: {response}"
    );

    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("a denied request must still emit a decision-log record");
    assert!(
        !log.is_allowed(),
        "the deny verdict must be audited as allowed = false"
    );
    assert_eq!(
        log.matched_rule(),
        None,
        "a deny-all verdict matches no policy rule"
    );
    assert_eq!(
        log.credential_identity(),
        None,
        "no credential is injected on a deny"
    );
    assert!(
        !log.reason().is_empty(),
        "the deny audit must record a non-empty reason"
    );
    assert!(
        !log.reason().contains("topsecret")
            && !log.destination().contains("supersecret")
            && !log.destination().contains("topsecret")
            && log.destination().contains("token=<redacted>"),
        "the deny audit must redact secret material: reason={:?} destination={:?}",
        log.reason(),
        log.destination()
    );
    assert!(
        log_rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "a terminal deny must emit exactly one decision-log record, not several"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "an audited deny must not contact upstream"
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
    // Bind an ephemeral port, then drop the listener so the dial is refused.
    let dead = TcpListener::bind(("127.0.0.1", 0)).expect("bind to reserve a dead port");
    let dead_port = dead
        .local_addr()
        .expect("dead address should resolve")
        .port();
    drop(dead);

    let resolver = Arc::new(move |_host: &str, _port: u16| {
        Ok(vec![SocketAddr::from(([127, 0, 0, 1], dead_port))])
    });
    let proxy = EgressProxy::start(
        EgressProxyConfig::new(allow_policy([EgressRule::new(
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
    let proxy = EgressProxy::start(
        // allow_internal_ips defaults to false: only the global addresses[0] is
        // authorizable.
        EgressProxyConfig::new(allow_policy([EgressRule::new(
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
    let proxy = EgressProxy::start(
        EgressProxyConfig::new(allow_policy([EgressRule::new(
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

fn start_test_proxy(policy: CompiledEgressPolicy) -> EgressProxy {
    start_test_proxy_with_store(policy, CredentialSecretStore::empty())
}

fn start_test_proxy_with_store(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
) -> EgressProxy {
    start_test_proxy_with_store_and_logger(policy, credential_store, Arc::new(|_| {}))
}

fn start_test_proxy_with_store_and_logger(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
) -> EgressProxy {
    let resolver = Arc::new(|host: &str, port: u16| {
        let ip = match host {
            "allowed.test" | "denied.test" | "first.test" | "second.test" | "metadata.test"
            | "redirect.test" => [127, 0, 0, 1].into(),
            _ => return Err(io::Error::other(format!("unexpected host {host}"))),
        };
        Ok(vec![SocketAddr::new(ip, port)])
    });
    EgressProxy::start(
        EgressProxyConfig::new(policy)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
            .with_credential_store(credential_store)
            .with_decision_logger(decision_logger)
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
fn egress_proxy_decision_logger_receives_redacted_allowed_request() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_store_and_logger(
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
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
    );

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/ok?token=secret HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream.addr.port()
        ),
    );
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "allowed request should pass before logging assertion, got: {response}"
    );
    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("decision logger should receive allowed request log");
    assert_eq!(log.credential_identity(), Some("api-token"));
    assert!(
        log.destination().contains("token=<redacted>") && !log.destination().contains("secret"),
        "decision log destination must redact query values: {}",
        log.destination()
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
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        });
        Self {
            addr,
            body_len: body_rx,
        }
    }
}
