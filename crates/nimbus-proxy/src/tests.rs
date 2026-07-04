use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
    mpsc,
};
use std::thread;
use std::time::{Duration, Instant};

use super::*;
use crate::credentials::{CredentialSecretProvider, CredentialSecretProviderRef};
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
fn egress_proxy_streams_large_body_without_dlp_inspection_cap_coupling() {
    let body_len = 128 * 1024;
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
        "large non-DLP body should stream through Pingora, got: {response}"
    );
    let received = upstream
        .body_len
        .recv_timeout(Duration::from_secs(2))
        .expect("upstream should receive the streamed request body");
    assert_eq!(received, body_len);
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
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
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

fn start_test_proxy(policy: CompiledEgressPolicy) -> WorkloadPep {
    start_test_proxy_with_store(policy, CredentialSecretStore::empty())
}

fn start_test_proxy_on_substrate(
    policy: CompiledEgressPolicy,
    substrate: ProxySubstrate,
) -> WorkloadPep {
    WorkloadPep::start(
        WorkloadPepConfig::new(policy)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
            .with_substrate(substrate)
            .with_resolver(loopback_test_resolver()),
    )
    .expect("proxy should start")
}

fn start_test_proxy_with_store(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
) -> WorkloadPep {
    start_test_proxy_with_store_and_logger(policy, credential_store, Arc::new(|_| {}))
}

fn start_test_proxy_with_store_and_logger(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
) -> WorkloadPep {
    start_test_proxy_with_store_logger_and_phase_observer(
        policy,
        credential_store,
        decision_logger,
        Arc::new(|_| {}),
    )
}

fn start_test_proxy_with_store_logger_and_phase_observer(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
    phase_observer: crate::phase::PhaseObserver,
) -> WorkloadPep {
    WorkloadPep::start(
        WorkloadPepConfig::new(policy)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
            .with_credential_store(credential_store)
            .with_decision_logger(decision_logger)
            .with_phase_observer(phase_observer)
            .with_resolver(loopback_test_resolver()),
    )
    .expect("proxy should start")
}

fn start_test_proxy_with_store_logger_tls_and_phase_observer(
    policy: CompiledEgressPolicy,
    credential_store: CredentialSecretStore,
    decision_logger: DecisionLogger,
    tls_authority: WorkloadPepTlsAuthority,
    phase_observer: crate::phase::PhaseObserver,
) -> WorkloadPep {
    start_test_proxy_with_provider_logger_tls_and_phase_observer(
        policy,
        credential_store.into_provider(),
        decision_logger,
        tls_authority,
        phase_observer,
    )
}

fn start_test_proxy_with_provider_logger_tls_and_phase_observer(
    policy: CompiledEgressPolicy,
    credential_provider: CredentialSecretProviderRef,
    decision_logger: DecisionLogger,
    tls_authority: WorkloadPepTlsAuthority,
    phase_observer: crate::phase::PhaseObserver,
) -> WorkloadPep {
    WorkloadPep::start(
        WorkloadPepConfig::new(policy)
            .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
            .with_credential_provider(credential_provider)
            .with_decision_logger(decision_logger)
            .with_tls_authority(tls_authority)
            .with_phase_observer(phase_observer)
            .with_resolver(loopback_test_resolver()),
    )
    .expect("proxy should start")
}

fn loopback_test_resolver() -> crate::dns::Resolver {
    Arc::new(|host: &str, port: u16| {
        let ip = match host {
            "allowed.test" | "denied.test" | "first.test" | "second.test" | "metadata.test"
            | "redirect.test" => [127, 0, 0, 1].into(),
            _ => return Err(io::Error::other(format!("unexpected host {host}"))),
        };
        Ok(vec![SocketAddr::new(ip, port)])
    })
}

struct MockCredentialProvider {
    entries: Vec<(String, String)>,
}

impl CredentialSecretProvider for MockCredentialProvider {
    fn resolve_credential_secret(&self, credential_ref: &str) -> Option<String> {
        self.entries
            .iter()
            .find_map(|(key, value)| (key == credential_ref).then(|| value.clone()))
    }
}

fn mock_credential_provider(
    entries: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
) -> CredentialSecretProviderRef {
    Arc::new(MockCredentialProvider {
        entries: entries
            .into_iter()
            .map(|(key, value)| (key.into(), value.into()))
            .collect(),
    })
}

fn recorded_phases() -> Arc<Mutex<Vec<EgressProxyRequestPhase>>> {
    Arc::new(Mutex::new(Vec::new()))
}

fn phase_observer(
    phases: &Arc<Mutex<Vec<EgressProxyRequestPhase>>>,
) -> crate::phase::PhaseObserver {
    let phases = Arc::clone(phases);
    Arc::new(move |phase| {
        phases
            .lock()
            .expect("phase trace lock should not be poisoned")
            .push(phase);
    })
}

fn snapshot_phases(
    phases: &Arc<Mutex<Vec<EgressProxyRequestPhase>>>,
) -> Vec<EgressProxyRequestPhase> {
    phases
        .lock()
        .expect("phase trace lock should not be poisoned")
        .clone()
}

fn allow_policy<const N: usize>(rules: [EgressRule; N]) -> CompiledEgressPolicy {
    EgressPolicy::new(rules)
        .compile()
        .expect("policy should compile")
}

fn proxy_request(proxy_addr: SocketAddr, request: String) -> String {
    proxy_request_until_close(proxy_addr, request).expect("client should read response")
}

fn proxy_request_until_close(proxy_addr: SocketAddr, request: String) -> io::Result<String> {
    let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    stream
        .write_all(request.as_bytes())
        .expect("client should write request");
    let mut response = String::new();
    stream.read_to_string(&mut response)?;
    Ok(response)
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

struct TestStallingHttpServer {
    addr: SocketAddr,
    request: mpsc::Receiver<String>,
    release: mpsc::Sender<()>,
}

impl TestStallingHttpServer {
    fn start() -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        let (request_tx, request_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut request = Vec::new();
            let mut chunk = [0_u8; 1024];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        request.extend_from_slice(&chunk[..read]);
                        if request.windows(4).any(|window| window == b"\r\n\r\n") {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
            let _ = request_tx.send(String::from_utf8_lossy(&request).to_string());
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        Self {
            addr,
            request: request_rx,
            release: release_tx,
        }
    }

    fn release(&self) {
        let _ = self.release.send(());
    }
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

#[test]
fn egress_proxy_returns_503_when_connection_limit_is_exhausted() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
    let upstream_addr = listener
        .local_addr()
        .expect("upstream address should resolve");
    let (request_tx, request_rx) = mpsc::channel();
    thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut request = [0_u8; 1024];
        let read = stream.read(&mut request).unwrap_or_default();
        let _ = request_tx.send(String::from_utf8_lossy(&request[..read]).to_string());
        thread::sleep(Duration::from_secs(2));
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok");
    });
    let proxy = WorkloadPep::start(
        WorkloadPepConfig::new(allow_policy([EgressRule::new(
            "allowed",
            EgressProtocol::Http,
            "allowed.test",
            upstream_addr.port(),
        )
        .allow_internal_ips(true)]))
        .with_timeouts(Duration::from_secs(5), Duration::from_secs(5))
        .with_max_connections(1)
        .with_resolver(Arc::new(move |host: &str, port: u16| {
            assert_eq!(host, "allowed.test");
            Ok(vec![SocketAddr::from(([127, 0, 0, 1], port))])
        })),
    )
    .expect("proxy should start");
    let mut held = TcpStream::connect(proxy.local_addr()).expect("first client should connect");
    held.write_all(
        format!(
            "GET http://allowed.test:{}/hold HTTP/1.1\r\nHost: allowed.test\r\n\r\n",
            upstream_addr.port()
        )
        .as_bytes(),
    )
    .expect("held request should write");
    request_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("held request should reach the delayed upstream");

    let response = proxy_request(
        proxy.local_addr(),
        "GET http://blocked.test:80/ HTTP/1.1\r\nHost: blocked.test\r\n\r\n".to_owned(),
    );

    assert!(
        response.starts_with("HTTP/1.1 503 Service Unavailable")
            && response.contains("connection limit exceeded"),
        "exhausted connection limit should return the existing 503 response, got: {response}"
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

struct TestHttpsServer {
    addr: SocketAddr,
    request: mpsc::Receiver<String>,
}

impl TestHttpsServer {
    fn start(
        authority: &WorkloadPepTlsAuthority,
        hostname: &'static str,
        response: impl Into<String>,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        let server_config = authority
            .server_config_for_host(hostname)
            .expect("upstream TLS server config should build");
        let (request_tx, request_rx) = mpsc::channel();
        let response = response.into();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let Ok(server_connection) = rustls::ServerConnection::new(server_config) else {
                return;
            };
            let mut tls = rustls::StreamOwned::new(server_connection, stream);
            let Ok(request) = read_h1_request_from_stream(&mut tls) else {
                return;
            };
            let _ = request_tx.send(request);
            let _ = tls.write_all(response.as_bytes());
        });
        Self {
            addr,
            request: request_rx,
        }
    }
}

struct TestHttpsCloseServer {
    addr: SocketAddr,
    request: mpsc::Receiver<String>,
}

impl TestHttpsCloseServer {
    fn start_after_request(authority: &WorkloadPepTlsAuthority, hostname: &'static str) -> Self {
        Self::start(authority, hostname, true)
    }

    fn start_after_handshake(authority: &WorkloadPepTlsAuthority, hostname: &'static str) -> Self {
        Self::start(authority, hostname, false)
    }

    fn start(
        authority: &WorkloadPepTlsAuthority,
        hostname: &'static str,
        read_request: bool,
    ) -> Self {
        let listener = TcpListener::bind(("127.0.0.1", 0)).expect("test upstream should bind");
        let addr = listener
            .local_addr()
            .expect("upstream address should resolve");
        let server_config = authority
            .server_config_for_host(hostname)
            .expect("upstream TLS server config should build");
        let (request_tx, request_rx) = mpsc::channel();
        thread::spawn(move || {
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            let Ok(server_connection) = rustls::ServerConnection::new(server_config) else {
                return;
            };
            let mut tls = rustls::StreamOwned::new(server_connection, stream);
            if read_request {
                if let Ok(request) = read_h1_request_from_stream(&mut tls) {
                    let _ = request_tx.send(request);
                }
            } else {
                let _ = tls.conn.complete_io(&mut tls.sock);
            }
        });
        Self {
            addr,
            request: request_rx,
        }
    }
}

fn read_h1_request_from_stream(stream: &mut impl Read) -> io::Result<String> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let header_end = loop {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            return Ok(String::from_utf8_lossy(&buffer).to_string());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break header_end;
        }
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
    while buffer.len().saturating_sub(header_end + 4) < content_length {
        let read = stream.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(String::from_utf8_lossy(&buffer).to_string())
}

fn connect_tls_through_proxy(
    proxy_addr: SocketAddr,
    authority: &WorkloadPepTlsAuthority,
    host: &str,
    port: u16,
) -> rustls::StreamOwned<rustls::ClientConnection, TcpStream> {
    let mut stream = TcpStream::connect(proxy_addr).expect("client should connect to proxy");
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .expect("read timeout should set");
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .expect("write timeout should set");
    stream
        .write_all(
            format!("CONNECT {host}:{port} HTTP/1.1\r\nHost: {host}:{port}\r\n\r\n").as_bytes(),
        )
        .expect("CONNECT request should write");
    let connect_response = read_http_headers_from_raw_stream(&mut stream);
    assert!(
        connect_response.starts_with("HTTP/1.1 200 Connection Established"),
        "CONNECT should establish before TLS handshake, got: {connect_response}"
    );

    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(authority.trust_anchor_der())
        .expect("proxy CA trust anchor should parse");
    let mut config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])
    .expect("client protocol versions should configure")
    .with_root_certificates(roots)
    .with_no_client_auth();
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let server_name =
        rustls::pki_types::ServerName::try_from(host.to_owned()).expect("SNI should be valid");
    let connection = rustls::ClientConnection::new(Arc::new(config), server_name)
        .expect("client TLS connection should build");
    let mut tls = rustls::StreamOwned::new(connection, stream);
    tls.conn
        .complete_io(&mut tls.sock)
        .expect("client TLS handshake should complete");
    tls
}

fn read_http_headers_from_raw_stream(stream: &mut TcpStream) -> String {
    let mut response = Vec::new();
    let mut chunk = [0_u8; 1];
    while !response.ends_with(b"\r\n\r\n") {
        let read = stream
            .read(&mut chunk)
            .expect("client should read HTTP response headers");
        if read == 0 {
            break;
        }
        response.extend_from_slice(&chunk[..read]);
    }
    String::from_utf8_lossy(&response).to_string()
}

fn read_tls_to_string(
    stream: &mut rustls::StreamOwned<rustls::ClientConnection, TcpStream>,
    output: &mut String,
) -> io::Result<()> {
    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 1024];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => bytes.extend_from_slice(&chunk[..read]),
            Err(error)
                if error.kind() == io::ErrorKind::UnexpectedEof
                    && error
                        .to_string()
                        .contains("without sending TLS close_notify") =>
            {
                break;
            }
            Err(error) => return Err(error),
        }
    }
    output.push_str(&String::from_utf8_lossy(&bytes));
    Ok(())
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

/// EE1 reachability lint: the workload map is not referenceable from the
/// request path.
///
/// Today's tenant isolation is a type/ownership property — each accept task
/// closes over its own per-PEP context, so a request handler cannot name
/// another workload's state. The node-scoped `EgressEngine` keeps its
/// `Map<WorkloadId, WorkloadPep>` off the request path by module discipline:
/// within `nimbus-proxy`, only `engine.rs` (the definition) and `lib.rs` (the
/// export) may name `EgressEngine` or `WorkloadId`. Every other module — the
/// worker accept/handler path, the intercept path, the pingora adapter, and
/// all request-processing modules — must be unable to reach the map even by
/// name. A plain "hot path holds no `Map<SandboxId, …>`" grep would be vacuous
/// (`nimbus-proxy` has no `SandboxId` at all); scanning for the engine's own
/// key/type names is the non-vacuous form.
///
/// This is the compensating control the egress-engine plan's isolation
/// argument rests on; the plan verifier (`verify-nimbus-egress-engine.sh`)
/// enforces the same rule from outside the crate.
#[test]
fn ee1_reachability_lint_workload_map_unreachable_from_request_path() {
    let src_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    // engine.rs defines the engine; lib.rs exports it; tests.rs is this lint
    // (test-only, never part of the request path).
    let allowed = ["engine.rs", "lib.rs", "tests.rs"];
    let needles = ["EgressEngine", "WorkloadId"];

    let mut violations = Vec::new();
    let mut scanned = 0usize;
    // Recursive walk: a future src/ subdirectory (e.g. a request/ split) must
    // not silently escape the scan while the scanned-count floor stays green.
    let mut pending = vec![src_dir.clone()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("nimbus-proxy src dir must be readable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let top_level_allowed = dir == src_dir && allowed.contains(&name);
            if !name.ends_with(".rs") || top_level_allowed {
                continue;
            }
            scanned += 1;
            let contents = std::fs::read_to_string(&path).expect("source file must be readable");
            let display = path
                .strip_prefix(&src_dir)
                .unwrap_or(&path)
                .display()
                .to_string();
            for needle in needles {
                if contents.contains(needle) {
                    violations.push(format!("{display} references {needle}"));
                }
            }
        }
    }

    // Guard the lint itself against vacuousness: the scan set must cover the
    // real request-path modules, and the needles must actually exist in the
    // crate (in engine.rs) so a rename can't silently blind the lint.
    assert!(
        scanned >= 15,
        "reachability lint scanned only {scanned} files; scan set is broken"
    );
    let engine_src =
        std::fs::read_to_string(src_dir.join("engine.rs")).expect("engine.rs must exist (EE1c)");
    for needle in needles {
        assert!(
            engine_src.contains(needle),
            "lint needle {needle} no longer exists in engine.rs; update the lint"
        );
    }

    assert!(
        violations.is_empty(),
        "EE1 reachability violation — the workload map (or its key type) is nameable from \
         request-path modules: {violations:?}"
    );
}
