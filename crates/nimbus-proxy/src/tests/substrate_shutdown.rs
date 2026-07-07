use super::*;

#[test]
fn shared_substrate_drop_preserves_sibling_proxy() {
    let first_upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nfirst");
    let sibling_before =
        TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nbefore");
    let sibling_after = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nafter");
    let first_proxy = start_test_proxy(allow_policy([EgressRule::new(
        "first",
        EgressProtocol::Http,
        "first.test",
        first_upstream.addr.port(),
    )
    .allow_internal_ips(true)]));
    let sibling_proxy = start_test_proxy(allow_policy([
        EgressRule::new(
            "sibling-before",
            EgressProtocol::Http,
            "second.test",
            sibling_before.addr.port(),
        )
        .allow_internal_ips(true),
        EgressRule::new(
            "sibling-after",
            EgressProtocol::Http,
            "second.test",
            sibling_after.addr.port(),
        )
        .allow_internal_ips(true),
    ]));

    let first_addr = first_proxy.local_addr();
    let first_port = first_upstream.addr.port();
    let sibling_addr = sibling_proxy.local_addr();
    let sibling_before_port = sibling_before.addr.port();
    let first_request = thread::spawn(move || {
        proxy_request(
            first_addr,
            format!("GET http://first.test:{first_port}/ok HTTP/1.1\r\nHost: first.test\r\n\r\n"),
        )
    });
    let sibling_request = thread::spawn(move || {
        proxy_request(
            sibling_addr,
            format!(
                "GET http://second.test:{sibling_before_port}/ok HTTP/1.1\r\nHost: second.test\r\n\r\n"
            ),
        )
    });

    let first_response = first_request
        .join()
        .expect("first proxy request thread should not panic");
    let sibling_response = sibling_request
        .join()
        .expect("sibling proxy request thread should not panic");
    assert!(
        first_response.starts_with("HTTP/1.1 200 OK") && first_response.contains("first"),
        "first shared-substrate proxy should serve concurrently, got: {first_response}"
    );
    assert!(
        sibling_response.starts_with("HTTP/1.1 200 OK") && sibling_response.contains("before"),
        "sibling shared-substrate proxy should serve concurrently, got: {sibling_response}"
    );

    drop(first_proxy);
    let sibling_after_response = proxy_request(
        sibling_proxy.local_addr(),
        format!(
            "GET http://second.test:{}/ok HTTP/1.1\r\nHost: second.test\r\n\r\n",
            sibling_after.addr.port()
        ),
    );
    assert!(
        sibling_after_response.starts_with("HTTP/1.1 200 OK")
            && sibling_after_response.contains("after"),
        "dropping one proxy must not disturb its shared-substrate sibling, got: {sibling_after_response}"
    );
}

#[test]
fn dedicated_substrate_drop_does_not_affect_shared_substrate_proxy() {
    let dedicated_substrate = ProxySubstrate::dedicated(1);
    let dedicated_upstream =
        TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\ndedicated");
    let shared_before = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nshared");
    let shared_after =
        TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 12\r\n\r\nstill-shared");
    let dedicated_proxy = start_test_proxy_on_substrate(
        allow_policy([EgressRule::new(
            "dedicated",
            EgressProtocol::Http,
            "first.test",
            dedicated_upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        dedicated_substrate.clone(),
    );
    let shared_proxy = start_test_proxy(allow_policy([
        EgressRule::new(
            "shared-before",
            EgressProtocol::Http,
            "second.test",
            shared_before.addr.port(),
        )
        .allow_internal_ips(true),
        EgressRule::new(
            "shared-after",
            EgressProtocol::Http,
            "second.test",
            shared_after.addr.port(),
        )
        .allow_internal_ips(true),
    ]));

    let dedicated_response = proxy_request(
        dedicated_proxy.local_addr(),
        format!(
            "GET http://first.test:{}/ok HTTP/1.1\r\nHost: first.test\r\n\r\n",
            dedicated_upstream.addr.port()
        ),
    );
    assert!(
        dedicated_response.starts_with("HTTP/1.1 200 OK")
            && dedicated_response.contains("dedicated"),
        "proxy on dedicated substrate should work end to end, got: {dedicated_response}"
    );
    let shared_before_response = proxy_request(
        shared_proxy.local_addr(),
        format!(
            "GET http://second.test:{}/ok HTTP/1.1\r\nHost: second.test\r\n\r\n",
            shared_before.addr.port()
        ),
    );
    assert!(
        shared_before_response.starts_with("HTTP/1.1 200 OK")
            && shared_before_response.contains("shared"),
        "shared-substrate proxy should work before dedicated shutdown, got: {shared_before_response}"
    );

    drop(dedicated_proxy);
    drop(dedicated_substrate);

    let shared_after_response = proxy_request(
        shared_proxy.local_addr(),
        format!(
            "GET http://second.test:{}/ok HTTP/1.1\r\nHost: second.test\r\n\r\n",
            shared_after.addr.port()
        ),
    );
    assert!(
        shared_after_response.starts_with("HTTP/1.1 200 OK")
            && shared_after_response.contains("still-shared"),
        "dropping a dedicated substrate must not disturb the shared substrate, got: {shared_after_response}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn egress_proxy_start_succeeds_inside_tokio_runtime() {
    let upstream = TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    let proxy = start_test_proxy(allow_policy([EgressRule::new(
        "allowed",
        EgressProtocol::Http,
        "allowed.test",
        upstream.addr.port(),
    )
    .allow_internal_ips(true)]));
    let proxy_addr = proxy.local_addr();
    let upstream_port = upstream.addr.port();

    let response = tokio::task::spawn_blocking(move || {
        proxy_request(
            proxy_addr,
            format!(
                "GET http://allowed.test:{upstream_port}/ok HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
            ),
        )
    })
    .await
    .expect("blocking proxy client should complete");

    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "starting the proxy inside a Tokio runtime should not require block_on, got: {response}"
    );
}

#[test]
fn dropping_proxy_terminates_in_flight_work_without_disturbing_sibling() {
    let stalled_upstream = TestStallingHttpServer::start();
    let sibling_upstream =
        TestHttpServer::start("HTTP/1.1 200 OK\r\nContent-Length: 7\r\n\r\nsibling");
    let stalled_proxy = start_test_proxy(allow_policy([EgressRule::new(
        "stall",
        EgressProtocol::Http,
        "allowed.test",
        stalled_upstream.addr.port(),
    )
    .allow_internal_ips(true)]));
    let sibling_proxy = start_test_proxy(allow_policy([EgressRule::new(
        "sibling",
        EgressProtocol::Http,
        "second.test",
        sibling_upstream.addr.port(),
    )
    .allow_internal_ips(true)]));
    let stalled_proxy_addr = stalled_proxy.local_addr();
    let stalled_port = stalled_upstream.addr.port();
    let (client_done_tx, client_done_rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = proxy_request_until_close(
            stalled_proxy_addr,
            format!(
                "GET http://allowed.test:{stalled_port}/slow HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
            ),
        );
        let _ = client_done_tx.send(());
    });

    let upstream_request = stalled_upstream
        .request
        .recv_timeout(Duration::from_secs(2))
        .expect("stalled upstream should receive the in-flight request");
    assert!(
        upstream_request.starts_with("GET /slow HTTP/1.1"),
        "stalled request should be in flight at the upstream, got: {upstream_request}"
    );

    let drop_started = Instant::now();
    drop(stalled_proxy);
    let drop_elapsed = drop_started.elapsed();
    assert!(
        drop_elapsed < Duration::from_millis(1500),
        "proxy drop should be bounded by tracked task abort, took {drop_elapsed:?}"
    );
    client_done_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("dropping the proxy should terminate the stalled client promptly");
    stalled_upstream.release();

    let sibling_response = proxy_request(
        sibling_proxy.local_addr(),
        format!(
            "GET http://second.test:{}/ok HTTP/1.1\r\nHost: second.test\r\n\r\n",
            sibling_upstream.addr.port()
        ),
    );
    assert!(
        sibling_response.starts_with("HTTP/1.1 200 OK") && sibling_response.contains("sibling"),
        "aborting in-flight work for one proxy must not disturb its sibling, got: {sibling_response}"
    );
}

#[test]
fn egress_proxy_drop_emits_terminal_record_for_aborted_in_flight_request() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&captured);
    let stalled_upstream = TestStallingHttpServer::start();
    let proxy = start_test_proxy_with_store_and_logger(
        allow_policy([EgressRule::new(
            "stall",
            EgressProtocol::Http,
            "allowed.test",
            stalled_upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(move |log| sink.lock().expect("capture lock should hold").push(log)),
    );
    let proxy_addr = proxy.local_addr();
    let stalled_port = stalled_upstream.addr.port();
    let client = thread::spawn(move || {
        let _ = proxy_request_until_close(
            proxy_addr,
            format!(
                "GET http://allowed.test:{stalled_port}/slow HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
            ),
        );
    });
    stalled_upstream
        .request
        .recv_timeout(Duration::from_secs(2))
        .expect("stalled upstream should receive the in-flight request");

    drop(proxy);
    client.join().expect("client thread should finish");
    stalled_upstream.release();

    let records = captured.lock().expect("capture lock should hold").clone();
    assert_eq!(
        records.len(),
        1,
        "an aborted in-flight request must emit exactly one terminal record: {records:?}"
    );
    assert!(
        !records[0].is_allowed(),
        "the abort record must not read as a completed allow: {:?}",
        records[0]
    );
    assert!(
        records[0]
            .reason()
            .contains("terminated the request before a decision"),
        "the abort record must name PEP termination: {:?}",
        records[0]
    );
}

#[test]
fn egress_proxy_abort_guard_writes_durable_terminal_after_intent_only_abort() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let stalled_upstream = TestStallingHttpServer::start();
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([EgressRule::new(
            "stall",
            EgressProtocol::Http,
            "allowed.test",
            stalled_upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
    );
    let proxy_addr = proxy.local_addr();
    let stalled_port = stalled_upstream.addr.port();
    let client = thread::spawn(move || {
        let _ = proxy_request_until_close(
            proxy_addr,
            format!(
                "GET http://allowed.test:{stalled_port}/slow HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
            ),
        );
    });
    stalled_upstream
        .request
        .recv_timeout(Duration::from_secs(2))
        .expect("stalled upstream should receive the in-flight request");

    drop(proxy);
    client.join().expect("client thread should finish");
    stalled_upstream.release();

    let records = captured.lock().expect("capture lock should hold").clone();
    assert_eq!(
        records.len(),
        2,
        "an intent-only aborted request must receive a durable synthetic terminal: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(records[0].is_allowed());
    assert_eq!(records[1].record_kind(), DecisionRecordKind::Terminal);
    assert!(
        !records[1].is_allowed()
            && records[1]
                .reason()
                .contains("terminated the request before a decision"),
        "abort guard terminal must be a synthetic deny naming the abort: {:?}",
        records[1]
    );
    assert_eq!(
        records[0].request_id(),
        records[1].request_id(),
        "abort guard terminal must pair with the request intent row: {records:?}"
    );
}

#[test]
fn egress_proxy_forward_http_abort_after_response_head_writes_after_response_terminal() {
    let captured = Arc::new(Mutex::new(Vec::new()));
    let stalled_upstream = TestStallingHttpBodyServer::start();
    let proxy = start_test_proxy_with_store_logger_and_durable_sink(
        allow_policy([EgressRule::new(
            "stall-body",
            EgressProtocol::Http,
            "allowed.test",
            stalled_upstream.addr.port(),
        )
        .allow_internal_ips(true)]),
        CredentialSecretStore::empty(),
        Arc::new(|_| {}),
        capturing_durable_sink_for_test(Arc::clone(&captured)),
    );

    let proxy_addr = proxy.local_addr();
    let stalled_port = stalled_upstream.addr.port();
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
                    "GET http://allowed.test:{stalled_port}/slow HTTP/1.1\r\nHost: allowed.test\r\n\r\n"
                )
                .as_bytes(),
            )
            .expect("client should write request");
        let head = read_http_headers_from_raw_stream(&mut stream);
        let _ = head_tx.send(head);
        let mut chunk = [0_u8; 1024];
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
                Err(error) => panic!("client body read should finish or be reset: {error}"),
            }
        }
    });

    let upstream_request = stalled_upstream
        .request
        .recv_timeout(Duration::from_secs(2))
        .expect("stalled HTTP upstream should receive the forwarded request");
    assert!(
        upstream_request.starts_with("GET /slow HTTP/1.1"),
        "forwarded request should reach upstream before cancellation: {upstream_request}"
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
    stalled_upstream.release();

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
        "forward cancellation after response head must produce exactly intent + after-response durable rows: {records:?}"
    );
    assert_eq!(records[0].record_kind(), DecisionRecordKind::Intent);
    assert!(
        records[0].is_allowed(),
        "the intent row must remain an allow: {:?}",
        records[0]
    );
    assert_eq!(
        records[1].record_kind(),
        DecisionRecordKind::TerminalAfterResponse
    );
    assert!(
        records[1].is_allowed(),
        "response-started cancellation must audit as executed allow, not synthetic deny: {records:?}"
    );
    assert_eq!(
        records[0].request_id(),
        records[1].request_id(),
        "after-response terminal must pair with the original intent row: {records:?}"
    );
    assert!(
        records.iter().all(EgressDecisionLog::is_allowed),
        "response-started cancellation must not append a synthetic deny: {records:?}"
    );
    assert_eq!(
        records[1].reason(),
        crate::decision_log::ABORT_AFTER_RESPONSE_REASON
    );
}

#[test]
fn egress_proxy_abort_guard_marks_audit_unhealthy_when_durable_append_fails() {
    let audit_healthy = Arc::new(AtomicBool::new(true));
    let resolver_entered = Arc::new(AtomicBool::new(false));
    let resolver_entered_for_call = Arc::clone(&resolver_entered);
    let resolver = Arc::new(move |_host: &str, port: u16| {
        resolver_entered_for_call.store(true, Ordering::SeqCst);
        thread::sleep(Duration::from_secs(5));
        Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
    });
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "stall",
            EgressProtocol::Http,
            "allowed.test",
            80,
        )
        .allow_internal_ips(true)]))
        .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
        .with_durable_decision_sink(failing_durable_sink_for_test())
        .with_audit_health_probe(Arc::clone(&audit_healthy))
        .with_resolver(resolver),
    )
    .expect("proxy should start");
    let proxy_addr = proxy.local_addr();
    let client = thread::spawn(move || {
        let _ = proxy_request_until_close(
            proxy_addr,
            "GET http://allowed.test:80/slow HTTP/1.1\r\nHost: allowed.test\r\n\r\n".to_owned(),
        );
    });

    let deadline = Instant::now() + Duration::from_secs(2);
    while !resolver_entered.load(Ordering::SeqCst) && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(10));
    }
    assert!(
        resolver_entered.load(Ordering::SeqCst),
        "request should reach the resolver stall before cancellation"
    );

    drop(proxy);
    client.join().expect("client thread should finish");

    assert!(
        !audit_healthy.load(Ordering::SeqCst),
        "abort guard durable append failure must flip sticky audit health"
    );
}
