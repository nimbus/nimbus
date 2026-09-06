//! LR8 integration: `serve` with `ServeOptions::with_tls` terminates TLS
//! on the main listener — HTTPS round-trips, plain HTTP is refused.

use super::*;

use crate::TlsConfig;

fn fixture(path: &str) -> std::path::PathBuf {
    std::env::var_os("CARGO_MANIFEST_DIR")
        .map(std::path::PathBuf::from)
        .expect("CARGO_MANIFEST_DIR should be set by Cargo/nextest for nimbus-server tests")
        .join(path)
}

#[tokio::test]
async fn https_round_trip_with_self_signed_pair_and_plain_http_refused() {
    let engine_fixture = EngineFixture::new(|path| Engine::new(path));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let addr = listener.local_addr().expect("addr should resolve");

    let options = crate::ServeOptions::reconstruct_direct(engine_fixture.engine())
        .expect("test server network authority should reconstruct once")
        .with_tls(TlsConfig::new(
            fixture("tests/fixtures/tls/localhost-cert.pem"),
            fixture("tests/fixtures/tls/localhost-key.pem"),
        ));
    let server = tokio::spawn(crate::serve(listener, options));

    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .expect("test client should build");

    // Poll /health over HTTPS until the listener is up.
    let url = format!("https://127.0.0.1:{}/health", addr.port());
    let mut healthy = None;
    for _ in 0..100 {
        assert!(!server.is_finished(), "server exited before /health");
        match client.get(&url).send().await {
            Ok(response) => {
                healthy = Some(response);
                break;
            }
            Err(_) => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
        }
    }
    let response = healthy.expect("HTTPS /health should answer");
    assert!(
        response.status().is_success(),
        "HTTPS health should be 2xx, got {}",
        response.status()
    );

    // The same port must not speak plain HTTP.
    let plain = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{}/health", addr.port()))
        .send()
        .await;
    assert!(
        plain.is_err(),
        "plain HTTP against the TLS listener must fail, got {plain:?}"
    );

    server.abort();
    let _ = server.await;
}

#[tokio::test]
async fn invalid_tls_identity_fails_the_boot_with_the_offending_path() {
    let engine_fixture = EngineFixture::new(|path| Engine::new(path));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");

    let options = crate::ServeOptions::reconstruct_direct(engine_fixture.engine())
        .expect("test server network authority should reconstruct once")
        .with_tls(TlsConfig::new(
            fixture("tests/fixtures/tls/does-not-exist.pem"),
            fixture("tests/fixtures/tls/localhost-key.pem"),
        ));
    let error = crate::serve(listener, options)
        .await
        .expect_err("serve must refuse a missing certificate at startup");
    assert!(
        error.to_string().contains("does-not-exist.pem"),
        "boot failure should name the offending path, got: {error}"
    );
}

#[tokio::test]
async fn shutdown_requested_before_tls_serve_is_not_lost() {
    let engine_fixture = EngineFixture::new(|path| Engine::new(path));
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let options = crate::ServeOptions::reconstruct_direct(engine_fixture.engine())
        .expect("test server network authority should reconstruct once")
        .with_tls(TlsConfig::new(
            fixture("tests/fixtures/tls/localhost-cert.pem"),
            fixture("tests/fixtures/tls/localhost-key.pem"),
        ));
    let shutdown = options.shutdown_handle();
    shutdown.request_shutdown();

    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::serve(listener, options),
    )
    .await
    .expect("pre-requested TLS shutdown should not wait for another signal")
    .expect("pre-requested TLS shutdown should be graceful");
}
