use super::*;

// Guards the SHAPE of the executable request-phase model for the egress PEP.
// Runtime trace tests below prove the worker emits these phases on real
// requests; this test catches accidental model drift.
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
        EgressProxyRequestPhase::RejectMalformedOrCallerCredentials,
        EgressProxyRequestPhase::PreDnsAuthorize,
        EgressProxyRequestPhase::ResolveDns,
        EgressProxyRequestPhase::AuthorizeResolvedIp,
        EgressProxyRequestPhase::SelectPoolKey,
        EgressProxyRequestPhase::BuildUpstreamPeer,
        EgressProxyRequestPhase::CredentialHeaderMutation,
        EgressProxyRequestPhase::BoundedDlpInspection,
        EgressProxyRequestPhase::Forward,
        EgressProxyRequestPhase::ResponseFilters,
        EgressProxyRequestPhase::TerminalLog,
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
        position(EgressProxyRequestPhase::RejectMalformedOrCallerCredentials)
            < position(EgressProxyRequestPhase::PreDnsAuthorize),
        "malformed request and caller credential screening must run before pre-DNS authorization"
    );
    assert!(
        position(EgressProxyRequestPhase::PreDnsAuthorize)
            < position(EgressProxyRequestPhase::ResolveDns),
        "host intent must be authorized before DNS"
    );
    assert!(
        position(EgressProxyRequestPhase::ResolveDns)
            < position(EgressProxyRequestPhase::AuthorizeResolvedIp),
        "DNS must resolve before the resolved peer is authorized"
    );
    assert!(
        position(EgressProxyRequestPhase::AuthorizeResolvedIp)
            < position(EgressProxyRequestPhase::SelectPoolKey),
        "the resolved peer must be authorized before pool identity is selected"
    );
    assert!(
        position(EgressProxyRequestPhase::SelectPoolKey)
            < position(EgressProxyRequestPhase::BuildUpstreamPeer),
        "pool identity must be selected before the upstream peer is built"
    );
    assert!(
        position(EgressProxyRequestPhase::BuildUpstreamPeer)
            < position(EgressProxyRequestPhase::CredentialHeaderMutation),
        "upstream peer identity must be known before credential/header mutation"
    );
    assert!(
        position(EgressProxyRequestPhase::CredentialHeaderMutation)
            < position(EgressProxyRequestPhase::BoundedDlpInspection),
        "credentials must be finalized before bounded DLP inspection"
    );
    assert!(
        position(EgressProxyRequestPhase::BoundedDlpInspection)
            < position(EgressProxyRequestPhase::Forward),
        "body-dependent DLP must complete before forwarding"
    );
    assert!(
        position(EgressProxyRequestPhase::Forward)
            < position(EgressProxyRequestPhase::ResponseFilters),
        "response filters must run after upstream forwarding"
    );
    assert!(
        position(EgressProxyRequestPhase::ResponseFilters)
            < position(EgressProxyRequestPhase::TerminalLog),
        "terminal logging must be after response filters"
    );
    assert!(
        position(EgressProxyRequestPhase::AuthorizeResolvedIp)
            < position(EgressProxyRequestPhase::Forward),
        "the resolved peer must be authorized before dialing"
    );
    assert!(
        position(EgressProxyRequestPhase::SelectPoolKey)
            < position(EgressProxyRequestPhase::Forward),
        "the pool key must be selected before dialing"
    );
}

#[test]
fn egress_proxy_emits_executable_phase_trace_for_allowed_http() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let phases = recorded_phases();
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_store_logger_and_phase_observer(
        allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .with_methods(["GET"])
        .with_path_prefixes(["/ok"])
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]),
        CredentialSecretStore::from_entries([("api-token", "secret-token")]),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
        phase_observer(&phases),
    );

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream.addr.port()
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "allowed phase-trace request should pass, got: {response}"
    );
    let upstream_request = upstream
        .request
        .recv_timeout(Duration::from_secs(1))
        .expect("allowed phase-trace request should reach upstream");
    assert!(
        upstream_request.contains("Authorization: Bearer secret-token"),
        "credential mutation must happen on the allowed path: {upstream_request}"
    );
    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("allowed request should emit a terminal decision log");
    assert!(
        log.is_allowed(),
        "allowed phase-trace request should log allow"
    );
    assert!(log_rx.recv_timeout(Duration::from_millis(200)).is_err());
    assert_eq!(
        snapshot_phases(&phases).as_slice(),
        REQUEST_PHASE_ORDER.as_slice(),
        "allowed HTTP should emit the canonical phase order exactly once"
    );
}

#[test]
fn egress_proxy_phase_trace_denies_caller_credentials_before_dns() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let upstream_addr = upstream.addr;
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let resolver_call_counter = Arc::clone(&resolver_calls);
    let resolver = Arc::new(move |_host: &str, _port: u16| {
        resolver_call_counter.fetch_add(1, Ordering::SeqCst);
        Ok(vec![upstream_addr])
    });
    let phases = recorded_phases();
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)
        .with_credential_injection(
            EgressCredentialInjection::new("api-token", "Authorization")
                .with_value_prefix("Bearer "),
        )]))
        .with_timeouts(Duration::from_secs(2), Duration::from_secs(2))
        .with_phase_observer(phase_observer(&phases))
        .with_resolver(resolver),
    )
    .expect("proxy should start");

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
        "caller credential should fail closed before DNS, got: {response}"
    );
    assert_eq!(
        resolver_calls.load(Ordering::SeqCst),
        0,
        "caller credential denial must happen before DNS resolution"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "caller credential denial must not contact upstream"
    );
    assert_eq!(
        snapshot_phases(&phases),
        [
            EgressProxyRequestPhase::CanonicalizeAuthority,
            EgressProxyRequestPhase::RejectMalformedOrCallerCredentials,
            EgressProxyRequestPhase::PreDnsAuthorize,
            EgressProxyRequestPhase::TerminalLog,
        ],
        "caller credential denial must stop before DNS, peer construction, credential mutation, DLP, or forward"
    );
}

#[test]
fn egress_proxy_phase_trace_stops_dlp_deny_before_forward() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let phases = recorded_phases();
    let proxy = start_test_proxy_with_store_logger_and_phase_observer(
        allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .with_methods(["POST"])
        .with_path_prefixes(["/upload"])
        .allow_internal_ips(true)
        .with_dlp_rules([EgressDlpRule::new("no-ssn", "ssn=").with_max_inspection_bytes(64)])]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        phase_observer(&phases),
    );

    let body = "payload=ssn=123";
    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "POST http://allowed.test:{}/upload HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: {}\r\n\r\n{}",
            upstream.addr.port(),
            body.len(),
            body
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden") && response.contains("DLP rule"),
        "DLP phase-trace request should fail closed, got: {response}"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "DLP-denied requests must not contact upstream"
    );
    let phases = snapshot_phases(&phases);
    assert!(
        phases.contains(&EgressProxyRequestPhase::BoundedDlpInspection),
        "DLP-denied trace must prove bounded DLP inspection happened: {phases:?}"
    );
    assert!(
        !phases.contains(&EgressProxyRequestPhase::Forward),
        "DLP-denied trace must stop before any upstream forward: {phases:?}"
    );
    assert_eq!(
        phases.last().copied(),
        Some(EgressProxyRequestPhase::TerminalLog),
        "DLP-denied trace must end with exactly one terminal log: {phases:?}"
    );
    assert_eq!(
        phases
            .iter()
            .filter(|phase| **phase == EgressProxyRequestPhase::TerminalLog)
            .count(),
        1,
        "DLP-denied trace must not duplicate terminal logging: {phases:?}"
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
        workload_id: "workload-a".to_string(),
        substrate: EgressProxySubstrate::Container,
        policy_generation: PolicyGeneration::initial(),
        credential_identity: Some("secret:stripe".to_string()),
        credential_dlp_mode: EgressProxyCredentialDlpMode::CredentialAndDlp,
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
        ("workload_id", |key| {
            key.workload_id = "workload-b".to_string();
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
        ("credential_dlp_mode", |key| {
            key.credential_dlp_mode = EgressProxyCredentialDlpMode::Credential;
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
    assert!(
        log.is_allowed(),
        "the allow verdict must be audited as allowed = true"
    );
    assert_eq!(log.matched_rule(), Some("allowed"));
    assert_eq!(log.policy_generation().map(PolicyGeneration::get), Some(1));
    assert_eq!(log.reason_class(), "allowed");
    assert_eq!(log.protocol(), EgressProtocol::Http);
    assert_eq!(log.canonical_host(), "allowed.test");
    assert_eq!(log.port(), upstream.addr.port());
    assert_eq!(log.credential_identity(), Some("api-token"));
    assert!(
        log.destination().contains("token=<redacted>") && !log.destination().contains("secret"),
        "decision log destination must redact query values: {}",
        log.destination()
    );
    assert!(
        log_rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "an allowed terminal decision must emit exactly one decision-log record"
    );
}

#[test]
fn egress_proxy_audits_dlp_deny_with_exactly_one_terminal_event() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let (log_tx, log_rx) = mpsc::channel();
    let proxy = start_test_proxy_with_store_and_logger(
        allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .with_methods(["POST"])
        .with_path_prefixes(["/upload"])
        .allow_internal_ips(true)
        .with_dlp_rules([
            EgressDlpRule::new("no-secret", "secret").with_max_inspection_bytes(64)
        ])]),
        CredentialSecretStore::empty(),
        Arc::new(move |log| {
            let _ = log_tx.send(log);
        }),
    );

    let body = "payload=secret";
    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "POST http://allowed.test:{}/upload?token=topsecret HTTP/1.1\r\nHost: allowed.test\r\nContent-Length: {}\r\n\r\n{}",
            upstream.addr.port(),
            body.len(),
            body
        ),
    );
    assert!(
        response.starts_with("HTTP/1.1 403 Forbidden") && response.contains("DLP rule"),
        "DLP pattern match must block before dial, got: {response}"
    );
    let log = log_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("DLP deny must emit a decision-log record");
    assert!(!log.is_allowed(), "DLP deny must be audited as false");
    assert_eq!(log.matched_rule(), Some("allowed"));
    assert_eq!(log.policy_generation().map(PolicyGeneration::get), Some(1));
    assert_eq!(log.reason_class(), "dlp");
    assert!(
        log.destination().contains("token=<redacted>") && !log.destination().contains("topsecret"),
        "DLP audit must redact query values: {}",
        log.destination()
    );
    assert!(
        log_rx.recv_timeout(Duration::from_millis(200)).is_err(),
        "a DLP terminal deny must emit exactly one decision-log record"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "DLP-denied requests must not contact upstream"
    );
}

#[test]
fn append_only_decision_log_sink_writes_redacted_correlation_event() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let path = temp_dir.path().join("audit").join("egress.jsonl");
    let sink = AppendOnlyDecisionLogSink::open(
        &path,
        DecisionLogSinkContext::new("tenant-a", "sandbox-a"),
    )
    .expect("append-only decision log sink should open");
    let parsed = match crate::request::parse_proxy_request(
        b"GET http://allowed.test:443/path?token=supersecret HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
    ) {
        Ok(parsed) => parsed,
        Err(response) => panic!("proxy request should parse: {}", response.body()),
    };
    let log = EgressDecisionLog::allowed(
        "test-request-1",
        &parsed,
        Some("Bearer credential-secret".to_owned()),
        "allowed by rule".to_owned(),
        Some("allow-rule".to_owned()),
    )
    .with_policy_generation(crate::policy_state::PolicyGeneration::initial());

    sink.append(&log)
        .expect("decision log append should succeed");
    let log_text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("decision log {} should read: {error}", path.display()));
    let lines = log_text.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        1,
        "append-only sink must write exactly one JSONL event per terminal decision"
    );
    let event: serde_json::Value =
        serde_json::from_str(lines[0]).expect("decision log line should be JSON");
    assert_eq!(event["tenant_id"], "tenant-a");
    assert_eq!(event["workload_id"], "sandbox-a");
    assert_eq!(event["request_id"], "test-request-1");
    assert_eq!(event["record_kind"], "intent");
    assert_eq!(event["policy_generation"], 1);
    assert_eq!(event["rule_id"], "allow-rule");
    assert_eq!(event["protocol"], "http");
    assert_eq!(event["canonical_host"], "allowed.test");
    assert_eq!(event["port"], 443);
    assert_eq!(event["decision"], "allow");
    assert_eq!(event["reason_class"], "allowed");
    assert_eq!(event["credential_identity"], "<redacted>");
    let rendered_event = event.to_string();
    assert!(
        rendered_event.contains("token=<redacted>")
            && !rendered_event.contains("supersecret")
            && !rendered_event.contains("credential-secret"),
        "append-only sink must redact prohibited values: {rendered_event}"
    );
}

#[test]
fn egress_proxy_failing_durable_sink_deny_path_closes_without_response_and_marks_unhealthy() {
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        CompiledEgressPolicy::deny_all(),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        failing_durable_sink_for_test(),
    );

    let response = proxy_request_until_close(
        proxy.local_addr(),
        "GET http://allowed.test:80/secret HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_owned(),
    )
    .expect("client read should finish when proxy closes");

    assert_eq!(
        response, "",
        "a failed durable deny append must close without response bytes"
    );
    let readiness = proxy.readiness().expect("readiness should be observable");
    assert!(
        !readiness.audit_healthy && !readiness.ready,
        "durable append failure must make readiness fail closed: {readiness:?}"
    );
}

#[test]
fn egress_proxy_failing_durable_sink_allow_path_does_not_contact_upstream() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        failing_durable_sink_for_test(),
    );

    let response = proxy_request_until_close(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream.addr.port()
        ),
    )
    .expect("client read should finish when proxy closes");

    assert_eq!(
        response, "",
        "a failed durable allow append must close without response bytes"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "failed durable allow append must stop before any upstream request"
    );
    let readiness = proxy.readiness().expect("readiness should be observable");
    assert!(
        !readiness.audit_healthy && !readiness.ready,
        "durable append failure must make readiness fail closed: {readiness:?}"
    );
}

#[test]
fn egress_proxy_unhealthy_audit_rejects_next_request_before_dns_even_if_sink_recovers() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let calls = Arc::new(AtomicUsize::new(0));
    let durable_sink = {
        let captured = Arc::clone(&captured);
        let calls = Arc::clone(&calls);
        Arc::new(move |log: &EgressDecisionLog| {
            let call = calls.fetch_add(1, Ordering::SeqCst) + 1;
            if call == 1 {
                return Err(io::Error::other("induced first audit sink failure"));
            }
            captured
                .lock()
                .expect("durable log capture lock should hold")
                .push(log.clone());
            Ok(())
        }) as DurableDecisionSink
    };
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let resolver_calls = Arc::new(AtomicUsize::new(0));
    let resolver = {
        let resolver_calls = Arc::clone(&resolver_calls);
        Arc::new(move |host: &str, port: u16| {
            resolver_calls.fetch_add(1, Ordering::SeqCst);
            assert_eq!(host, "allowed.test");
            Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
        })
    };
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]))
        .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
        .with_credential_store(CredentialSecretStore::empty())
        .with_decision_logger(Arc::new(|_| {}))
        .with_durable_decision_sink(durable_sink)
        .with_resolver(resolver),
    )
    .expect("proxy should start");
    let request = format!(
        "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
        upstream.addr.port()
    );

    let first_response = proxy_request_until_close(proxy.local_addr(), request.clone())
        .expect("first client read should finish when proxy closes");
    assert_eq!(
        first_response, "",
        "the induced durable failure should close the first request without response bytes"
    );
    assert_eq!(
        resolver_calls.load(Ordering::SeqCst),
        1,
        "the first request reaches pre-DNS work before the induced audit failure"
    );

    let second_response = proxy_request_until_close(proxy.local_addr(), request)
        .expect("second client read should finish when proxy closes");
    assert_eq!(
        second_response, "",
        "sticky unhealthy audit state must close the second request without proxied response bytes"
    );
    assert_eq!(
        resolver_calls.load(Ordering::SeqCst),
        1,
        "the sticky unhealthy check must reject the second request before DNS"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "neither the failed first append nor the sticky-unhealthy second request may contact upstream"
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "the second request must still attempt the durable deny append after the sink recovers"
    );
    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        1,
        "the recovered sink should capture the sticky-unhealthy terminal deny: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !records[0].is_allowed() && records[0].reason().contains("failing closed until restart"),
        "the sticky-unhealthy row must be a fail-closed deny: {:?}",
        records[0]
    );
    let readiness = proxy.readiness().expect("readiness should be observable");
    assert!(
        !readiness.audit_healthy && !readiness.ready,
        "a recovered sink must not clear sticky unhealthy readiness until restart: {readiness:?}"
    );
}

#[test]
fn egress_proxy_pure_success_allow_writes_one_durable_intent_row() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
    );

    let response = proxy_request(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream.addr.port()
        ),
    );

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "pure success allow should reach upstream, got: {response}"
    );
    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        1,
        "pure success allow must have exactly one durable intent row: {records:?}"
    );
    assert!(
        records[0].is_allowed(),
        "the sole pure-success durable row must be the allow intent: {:?}",
        records[0]
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(
        !records[0].request_id().is_empty(),
        "durable rows must carry a request id: {:?}",
        records[0]
    );
}

#[test]
fn egress_proxy_plain_http_upstream_failure_writes_terminal_row_before_502() {
    let dead = TcpListener::bind(("127.0.0.1", 0)).expect("dead port should bind");
    let dead_port = dead.local_addr().expect("dead addr should read").port();
    drop(dead);
    let captured = Arc::new(Mutex::new(Vec::new()));
    let (durable_sink, terminal_started, release_terminal) =
        blocking_second_durable_sink_for_test(Arc::clone(&captured));
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([
            EgressRule::new("allowed", EgressProtocol::Http, "allowed.test", dead_port)
                .allow_internal_ips(true),
        ]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        durable_sink,
    );
    let proxy_addr = proxy.local_addr();
    let (response_tx, response_rx) = mpsc::channel();
    let client = thread::spawn(move || {
        let response = proxy_request(
            proxy_addr,
            format!(
                "GET http://allowed.test:{dead_port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
            ),
        );
        let _ = response_tx.send(response);
    });

    terminal_started
        .recv_timeout(Duration::from_secs(2))
        .expect("terminal durable append should start before the 502 response");
    assert!(
        response_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "the 502 response must not reach the client before the terminal durable append returns"
    );
    release_terminal
        .send(())
        .expect("terminal durable append release should send");
    let response = response_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("client should receive the 502 after terminal append returns");
    client.join().expect("client thread should finish");

    assert!(
        response.starts_with("HTTP/1.1 502 Bad Gateway") && response.contains("failed to dial"),
        "plain HTTP upstream failure must surface the structured 502, got: {response}"
    );
    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        2,
        "upstream failure must produce intent + terminal durable rows: {records:?}"
    );
    assert!(
        records[0].is_allowed(),
        "first durable row must be the forward-intent allow: {:?}",
        records[0]
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(
        !records[1].is_allowed() && records[1].reason().contains("failed to dial"),
        "second durable row must be the upstream-error terminal deny: {:?}",
        records[1]
    );
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Terminal);
    assert_eq!(
        records[0].request_id(),
        records[1].request_id(),
        "intent and terminal rows for one request must pair by request id: {records:?}"
    );
}

#[test]
fn egress_proxy_plain_http_failed_terminal_append_closes_without_502_and_marks_unhealthy() {
    let dead = TcpListener::bind(("127.0.0.1", 0)).expect("dead port should bind");
    let dead_port = dead.local_addr().expect("dead addr should read").port();
    drop(dead);
    let captured = Arc::new(Mutex::new(Vec::new()));
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([
            EgressRule::new("allowed", EgressProtocol::Http, "allowed.test", dead_port)
                .allow_internal_ips(true),
        ]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        failing_second_durable_sink_for_test(Arc::clone(&captured)),
    );

    let response = proxy_request_bytes_until_close(
        proxy.local_addr(),
        format!("GET http://allowed.test:{dead_port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"),
    )
    .expect("client read should finish when failed terminal append closes the session");

    assert_eq!(
        response,
        Vec::<u8>::new(),
        "a failed upstream-error terminal append must close without 502 bytes"
    );
    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        1,
        "only the successful forward-intent allow row should be durable after terminal append failure: {records:?}"
    );
    assert!(
        records[0].is_allowed(),
        "the sole durable row should be the intent allow: {:?}",
        records[0]
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    let readiness = proxy.readiness().expect("readiness should be observable");
    assert!(
        !readiness.audit_healthy && !readiness.ready,
        "failed terminal append must make readiness fail closed: {readiness:?}"
    );
}

#[test]
fn egress_proxy_plain_http_post_response_failure_writes_terminal_after_response_row() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let upstream = TestHttpServer::start(
        "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nConnection: close\r\n\r\nshort",
    );
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
    );

    let response = proxy_request_bytes_until_close(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/short HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream.addr.port()
        ),
    )
    .expect("client read should finish after upstream closes");
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "the upstream response head must reach the client before the transport failure: {response}"
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let records = loop {
        let records = snapshot_durable_logs(&captured);
        if records.len() >= 2 || Instant::now() >= deadline {
            break records;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        records.len(),
        2,
        "post-response failure should produce intent + after-response terminal rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert_eq!(
        records[1].record_kind(),
        DecisionRecordKind::TerminalAfterResponse
    );
    assert!(
        records[1].is_allowed(),
        "post-response failures are executed egress and must not audit as deny: {:?}",
        records[1]
    );
    assert_eq!(
        records[0].request_id(),
        records[1].request_id(),
        "post-response terminal must pair with the request intent row: {records:?}"
    );
}

#[test]
fn egress_proxy_plain_http_informational_response_then_upstream_close_is_terminal_deny() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let upstream = TestHttpServer::start("HTTP/1.1 100 Continue\r\n\r\n");
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
    );

    let response = proxy_request_bytes_until_close(
        proxy.local_addr(),
        format!(
            "GET http://allowed.test:{}/one-hundred-then-close HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream.addr.port()
        ),
    )
    .expect("client read should finish after upstream closes before final response");
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 100 Continue")
            && response.contains("HTTP/1.1 502 Bad Gateway"),
        "the client should see the informational response followed by the local final 502: {response}"
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let records = loop {
        let records = snapshot_durable_logs(&captured);
        if records.len() >= 2 || Instant::now() >= deadline {
            break records;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        records.len(),
        2,
        "1xx followed by upstream close should produce intent + pre-final terminal rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !records[1].is_allowed() && records[1].reason().contains("failed to dial"),
        "upstream failure after only a 1xx must audit as pre-final deny, not after-response allow: {:?}",
        records[1]
    );
    assert_eq!(
        records[0].request_id(),
        records[1].request_id(),
        "1xx pre-final terminal must pair with the request intent row: {records:?}"
    );
}

#[test]
fn egress_proxy_concurrent_upstream_failures_pair_rows_by_request_id() {
    let dead = TcpListener::bind(("127.0.0.1", 0)).expect("dead port should bind");
    let dead_port = dead.local_addr().expect("dead addr should read").port();
    drop(dead);
    let captured = Arc::new(Mutex::new(Vec::new()));
    let intent_count = Arc::new(AtomicUsize::new(0));
    let durable_sink = {
        let captured = Arc::clone(&captured);
        let intent_count = Arc::clone(&intent_count);
        Arc::new(move |log: &EgressDecisionLog| {
            if log.record_kind() == DecisionRecordKind::Intent {
                captured
                    .lock()
                    .expect("durable log capture lock should hold")
                    .push(log.clone());
                intent_count.fetch_add(1, Ordering::SeqCst);
            } else {
                let deadline = Instant::now() + Duration::from_secs(2);
                while intent_count.load(Ordering::SeqCst) < 2 && Instant::now() < deadline {
                    thread::sleep(Duration::from_millis(10));
                }
                captured
                    .lock()
                    .expect("durable log capture lock should hold")
                    .push(log.clone());
            }
            Ok(())
        }) as DurableDecisionSink
    };
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([
            EgressRule::new("allowed", EgressProtocol::Http, "allowed.test", dead_port)
                .allow_internal_ips(true),
        ]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        durable_sink,
    );
    let proxy_addr = proxy.local_addr();
    let request =
        format!("GET http://allowed.test:{dead_port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n");
    let first_request = request.clone();
    let first = thread::spawn(move || proxy_request(proxy_addr, first_request));
    let proxy_addr = proxy.local_addr();
    let second = thread::spawn(move || proxy_request(proxy_addr, request));

    let first_response = first.join().expect("first client thread should finish");
    let second_response = second.join().expect("second client thread should finish");
    assert!(
        first_response.starts_with("HTTP/1.1 502 Bad Gateway")
            && second_response.starts_with("HTTP/1.1 502 Bad Gateway"),
        "both concurrent upstream failures should surface 502s: first={first_response:?} second={second_response:?}"
    );

    let records = snapshot_durable_logs(&captured);
    assert_eq!(
        records.len(),
        4,
        "two upstream failures should produce two intent/terminal pairs: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Intent);
    assert_ne!(
        records[0].request_id(),
        records[1].request_id(),
        "concurrent requests must not share request ids: {records:?}"
    );

    let mut by_request = std::collections::HashMap::<String, Vec<DecisionRecordKind>>::new();
    for record in &records {
        by_request
            .entry(record.request_id().to_owned())
            .or_default()
            .push(record.record_kind());
    }
    assert_eq!(
        by_request.len(),
        2,
        "durable rows must group into two request ids: {records:?}"
    );
    for (request_id, kinds) in by_request {
        assert_eq!(
            kinds,
            [DecisionRecordKind::Intent, DecisionRecordKind::Terminal],
            "request {request_id} must have one intent followed by its terminal row"
        );
    }
}

#[test]
fn egress_proxy_failing_durable_sink_connect_path_sends_no_splice_bytes() {
    let upstream = TestTcpServer::start(b"pong");
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([EgressRule::new(
            "allowed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        failing_durable_sink_for_test(),
    );

    let response = proxy_request_until_close(
        proxy.local_addr(),
        format!(
            "CONNECT allowed.test:{} HTTP/1.1\r\nHost: allowed.test:{}\r\n\r\nping",
            upstream.addr.port(),
            upstream.addr.port()
        ),
    )
    .expect("client read should finish when proxy closes");

    assert_eq!(
        response, "",
        "a failed durable CONNECT append must close without response bytes"
    );
    assert!(
        upstream
            .request
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "failed durable CONNECT append must not splice bytes to upstream"
    );
}

#[test]
fn egress_proxy_splice_abort_mid_tunnel_writes_after_response_terminal() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let upstream = TestStallingTcpTunnelServer::start();
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([EgressRule::new(
            "allowed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
    );

    let proxy_addr = proxy.local_addr();
    let upstream_port = upstream.addr.port();
    let (head_tx, head_rx) = mpsc::channel();
    let client = thread::spawn(move || {
        let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("read timeout should set");
        stream
            .set_write_timeout(Some(Duration::from_secs(2)))
            .expect("write timeout should set");
        stream
            .write_all(
                format!(
                    "CONNECT allowed.test:{upstream_port} HTTP/1.1\r\nHost: allowed.test:{upstream_port}\r\n\r\n"
                )
                .as_bytes(),
            )
            .expect("CONNECT request should write");
        let head = read_http_headers_from_raw_stream(&mut stream);
        let _ = head_tx.send(head);
        let mut chunk = [0_u8; 32];
        loop {
            match stream.read(&mut chunk) {
                Ok(0) => break,
                Ok(_) => {}
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::ConnectionReset
                            | io::ErrorKind::UnexpectedEof
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                    ) =>
                {
                    break;
                }
                Err(error) => panic!("client tunnel read should finish or be reset: {error}"),
            }
        }
    });

    upstream
        .accepted
        .recv_timeout(Duration::from_secs(2))
        .expect("upstream tunnel should be accepted");
    let response_head = head_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("client should receive CONNECT 200 before cancellation");
    assert!(
        response_head.starts_with("HTTP/1.1 200 Connection Established"),
        "CONNECT response must reach the client before proxy cancellation: {response_head}"
    );

    drop(proxy);
    client.join().expect("client thread should finish");
    upstream.release();

    let deadline = Instant::now() + Duration::from_secs(2);
    let records = loop {
        let records = snapshot_durable_logs(&captured);
        if records.len() >= 2 || Instant::now() >= deadline {
            break records;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        records.len(),
        2,
        "splice cancellation after CONNECT 200 must produce intent + after-response durable rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert_eq!(
        records[1].record_kind(),
        DecisionRecordKind::TerminalAfterResponse
    );
    assert!(
        records[1].is_allowed(),
        "response-started splice cancellation must audit as executed allow: {records:?}"
    );
    assert_eq!(
        records[0].request_id(),
        records[1].request_id(),
        "splice after-response terminal must pair with the intent row: {records:?}"
    );
    assert_eq!(
        records[1].reason(),
        crate::decision_log::ABORT_AFTER_RESPONSE_REASON
    );
}

#[test]
fn egress_proxy_splice_copy_failure_after_200_writes_after_response_terminal() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let upstream = TestStallingTcpTunnelServer::start();
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed-https",
            EgressProtocol::Https,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]))
        .with_timeouts(Duration::from_secs(2), Duration::from_millis(100))
        .with_credential_store(CredentialSecretStore::empty())
        .with_decision_logger(Arc::new(|_| {}))
        .with_durable_decision_sink(capturing_durable_sink_for_test(Arc::clone(&captured)))
        .with_resolver(loopback_test_resolver()),
    )
    .expect("proxy should start");

    let response = proxy_request_bytes_until_close(
        proxy.local_addr(),
        format!(
            "CONNECT allowed.test:{} HTTP/1.1\r\nHost: allowed.test:{}\r\n\r\n",
            upstream.addr.port(),
            upstream.addr.port()
        ),
    )
    .expect("client read should finish after tunnel copy failure");
    upstream.release();
    let response = String::from_utf8_lossy(&response);
    assert!(
        response.starts_with("HTTP/1.1 200 Connection Established"),
        "CONNECT 200 must reach the client before the tunnel copy failure: {response}"
    );

    let deadline = Instant::now() + Duration::from_secs(2);
    let records = loop {
        let records = snapshot_durable_logs(&captured);
        if records.len() >= 2 || Instant::now() >= deadline {
            break records;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        records.len(),
        2,
        "splice copy failure after CONNECT 200 must produce intent + after-response durable rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert_eq!(
        records[1].record_kind(),
        DecisionRecordKind::TerminalAfterResponse
    );
    assert!(
        records[1].is_allowed(),
        "post-200 tunnel failure must audit as executed allow: {records:?}"
    );
    assert_eq!(
        records[0].request_id(),
        records[1].request_id(),
        "splice after-response terminal must pair with the intent row: {records:?}"
    );
    assert_eq!(
        records[1].reason(),
        crate::decision_log::UPSTREAM_FAILURE_AFTER_RESPONSE_REASON
    );
}

#[test]
fn egress_proxy_healthy_durable_sink_writes_exactly_one_jsonl_line_per_allow_and_deny() {
    let temp_dir = tempfile::TempDir::new().expect("temporary directory should exist");
    let path = temp_dir.path().join("audit").join("egress.jsonl");
    let durable_decision_sink = AppendOnlyDecisionLogSink::open(
        &path,
        DecisionLogSinkContext::new("tenant-a", "sandbox-a"),
    )
    .expect("append-only decision log sink should open")
    .durable_sink();
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        durable_decision_sink,
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
        "healthy durable allow should reach upstream, got: {allowed}"
    );
    let denied = proxy_request(
        proxy.local_addr(),
        "GET http://denied.test:80/nope HTTP/1.1\r\nHost: denied.test\r\n\r\n".to_owned(),
    );
    assert!(
        denied.starts_with("HTTP/1.1 403 Forbidden"),
        "healthy durable deny should return the policy denial, got: {denied}"
    );

    let log_text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("decision log {} should read: {error}", path.display()));
    let lines = log_text.lines().collect::<Vec<_>>();
    assert_eq!(
        lines.len(),
        2,
        "durable sink must write exactly one JSONL line per request: {log_text}"
    );
    let events = lines
        .iter()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("JSONL event"))
        .collect::<Vec<_>>();
    assert_eq!(events[0]["decision"], "allow");
    assert_eq!(events[0]["allowed"], true);
    assert_eq!(events[0]["record_kind"], "intent");
    assert!(
        events[0]["request_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_eq!(events[1]["decision"], "deny");
    assert_eq!(events[1]["allowed"], false);
    assert_eq!(events[1]["record_kind"], "terminal");
    assert!(
        events[1]["request_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty())
    );
    assert_ne!(
        events[0]["request_id"], events[1]["request_id"],
        "separate requests must receive distinct request ids"
    );
}

#[test]
fn egress_proxy_audits_malformed_requests_with_terminal_deny_records() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let proxy = start_test_proxy_with_store_and_logger(
        EgressPolicy::deny_all()
            .compile()
            .expect("deny-all policy should compile"),
        CredentialSecretStore::empty(),
        Arc::new(move |log| sink.lock().expect("capture lock should hold").push(log)),
    );

    let te_response = proxy_request(
        proxy.local_addr(),
        "POST http://allowed.test:80/ HTTP/1.1\r\nHost: allowed.test\r\nTransfer-Encoding: chunked\r\n\r\n"
            .to_owned(),
    );
    assert!(
        te_response.starts_with("HTTP/1.1 400"),
        "Transfer-Encoding smuggling guard should reject: {te_response}"
    );

    let smuggle_response = proxy_request(
        proxy.local_addr(),
        "GET http://allowed.test:80/ HTTP/1.1\r\nHost: allowed.test\nAuthorization: Bearer sneak\r\n\r\n"
            .to_owned(),
    );
    assert!(
        smuggle_response.starts_with("HTTP/1.1 400"),
        "bare-LF smuggling guard should reject: {smuggle_response}"
    );

    let records = captured.lock().expect("capture lock should hold").clone();
    assert_eq!(
        records.len(),
        2,
        "each parser-level reject must emit exactly one terminal record: {records:?}"
    );
    for record in &records {
        assert!(
            !record.is_allowed(),
            "parser-level rejects must audit as denies: {record:?}"
        );
        assert_eq!(
            record.reason_class(),
            "malformed",
            "parser-level rejects must classify as malformed: {record:?}"
        );
        assert_eq!(record.destination(), "<unparsed>");
    }
    assert!(
        records[0].reason().contains("Transfer-Encoding"),
        "first record should carry the TE reason: {:?}",
        records[0]
    );
    assert!(
        records[1].reason().contains("bare LF"),
        "second record should carry the bare-LF reason: {:?}",
        records[1]
    );
}
