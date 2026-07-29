use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use reqwest::StatusCode;
use tempfile::tempdir;

use nimbus_testing::wait_for_condition;

use crate::local_server::{
    LocalServerPaths, LocalServerSecurityState, SessionValidationResult, load_local_admin_token,
    load_or_create_local_admin_token,
};
use crate::router::RouterBuildConfig;
use crate::tests::{EngineFixture, ServerFixture};
use crate::{ServeOptions, serve};

fn sample_paths(root: &std::path::Path) -> LocalServerPaths {
    LocalServerPaths {
        auth_token_path: root.join("auth").join("token"),
        server_discovery_path: root.join("run").join("server.json"),
        audit_log_path: root.join("logs").join("access.jsonl"),
    }
}

#[tokio::test]
async fn local_admin_rotate_endpoint_rotates_token_and_rejects_previous_bearer() {
    let temp = tempdir().expect("tempdir should build");
    let paths = sample_paths(temp.path());
    let current = load_or_create_local_admin_token(&paths).expect("token should exist");
    let local_server_security = Arc::new(LocalServerSecurityState::new(
        paths.clone(),
        current.clone(),
    ));
    let session = local_server_security
        .create_session_for_local_admin_token(&current.token)
        .expect("session should mint for current local admin token");
    let fixture = EngineFixture::new(|path| nimbus_engine::Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_local_server_security(local_server_security.clone())
            .build(),
    )
    .await;

    let rotated = server
        .client()
        .post(server.http_url("/api/system/token/rotate"))
        .bearer_auth(&current.token)
        .send()
        .await
        .expect("rotate request should send");
    assert_eq!(rotated.status(), StatusCode::OK);

    let rotated_record = load_local_admin_token(&paths).expect("rotated token should persist");
    assert_eq!(rotated_record.generation, current.generation + 1);
    assert_eq!(local_server_security.current_token(), rotated_record);
    assert!(matches!(
        local_server_security.authorize_session_cookie(Some(&session.value)),
        SessionValidationResult::Revoked
    ));

    let old_token_rejected = server
        .client()
        .post(server.http_url("/api/system/token/rotate"))
        .bearer_auth(&current.token)
        .send()
        .await
        .expect("second rotate request should send");
    assert_eq!(old_token_rejected.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn system_shutdown_endpoint_stops_live_server() {
    let temp = tempdir().expect("tempdir should build");
    let paths = sample_paths(temp.path());
    let token = load_or_create_local_admin_token(&paths).expect("token should exist");
    let local_server_security = Arc::new(LocalServerSecurityState::new(paths, token.clone()));
    let service = Arc::new(
        nimbus_engine::Engine::new(temp.path().join("data")).expect("service should initialize"),
    );
    let baseline_started = Instant::now();
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should resolve");
    let server_task = tokio::spawn(serve(
        listener,
        ServeOptions::reconstruct_direct(service.clone())
            .expect("test server network authority should reconstruct once")
            .with_local_server_security(local_server_security),
    ));
    let client = reqwest::Client::new();
    wait_for_condition(
        "shutdown test server should answer health checks",
        Duration::from_secs(5),
        Duration::from_millis(50),
        || async {
            client
                .get(format!("http://{address}/health"))
                .send()
                .await
                .map(|response| response.status().is_success())
                .unwrap_or(false)
        },
    )
    .await;
    let ready_elapsed = baseline_started.elapsed();

    let shutdown_started = Instant::now();
    let response = client
        .post(format!("http://{address}/api/system/shutdown"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("shutdown request should send");
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("shutdown response should parse");
    assert_eq!(body["accepted"], serde_json::json!(true));

    tokio::time::timeout(Duration::from_secs(5), server_task)
        .await
        .expect("server should exit after shutdown request")
        .expect("server task should join")
        .expect("server shutdown should be graceful");
    let shutdown_elapsed = shutdown_started.elapsed();
    eprintln!(
        "NNC0.9 listener-lifecycle-baseline ready_ns={} shutdown_ns={} total_ns={}",
        ready_elapsed.as_nanos(),
        shutdown_elapsed.as_nanos(),
        baseline_started.elapsed().as_nanos()
    );
    service.quiesce().await;
}

#[tokio::test]
async fn system_shutdown_endpoint_rejects_missing_and_invalid_credentials() {
    let temp = tempdir().expect("tempdir should build");
    let paths = sample_paths(temp.path());
    let token = load_or_create_local_admin_token(&paths).expect("token should exist");
    let local_server_security = Arc::new(LocalServerSecurityState::new(paths, token));
    let fixture = EngineFixture::new(|path| nimbus_engine::Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let missing = server
        .client()
        .post(server.http_url("/api/system/shutdown"))
        .send()
        .await
        .expect("missing-auth shutdown request should send");
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let invalid = server
        .client()
        .post(server.http_url("/api/system/shutdown"))
        .bearer_auth("not-the-local-admin-token")
        .send()
        .await
        .expect("invalid-auth shutdown request should send");
    assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn system_shutdown_endpoint_rejects_when_local_security_unconfigured() {
    let fixture = EngineFixture::new(|path| nimbus_engine::Engine::new(path));
    let server = ServerFixture::start(RouterBuildConfig::core(fixture.engine()).build()).await;

    let response = server
        .client()
        .post(server.http_url("/api/system/shutdown"))
        .send()
        .await
        .expect("shutdown request should send");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
