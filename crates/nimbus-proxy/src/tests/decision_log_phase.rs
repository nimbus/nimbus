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
