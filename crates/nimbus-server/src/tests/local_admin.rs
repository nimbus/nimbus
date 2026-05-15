use std::net::Ipv4Addr;
use std::sync::Arc;
use std::time::Duration;

use reqwest::StatusCode;
use tempfile::tempdir;

use nimbus_testing::wait_for_condition;

use crate::local_server::{
    LocalServerPaths, LocalServerSecurityState, load_local_admin_token,
    load_or_create_local_admin_token,
};
use crate::router::RouterBuildConfig;
use crate::tests::{ServerFixture, ServiceFixture};
use crate::{ServeOptions, serve_with_options};

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
    local_server_security.register_session_for_test("session-a");
    let fixture = ServiceFixture::new(|path| nimbus_engine::Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
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
    assert_eq!(local_server_security.active_session_count(), 0);

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
        nimbus_engine::Service::new(temp.path().join("data")).expect("service should initialize"),
    );
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should resolve");
    let server_task = tokio::spawn(serve_with_options(
        listener,
        service.clone(),
        ServeOptions::default().with_local_server_security(local_server_security),
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
    service.quiesce().await;
}
