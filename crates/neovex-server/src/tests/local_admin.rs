use std::sync::Arc;

use reqwest::StatusCode;
use tempfile::tempdir;

use crate::local_server::{
    LocalServerPaths, LocalServerSecurityState, load_local_admin_token,
    load_or_create_local_admin_token,
};
use crate::router::RouterBuildConfig;
use crate::tests::{ServerFixture, ServiceFixture};

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
    let fixture = ServiceFixture::new(|path| neovex_engine::Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security.clone())
            .build(),
    )
    .await;

    let rotated = server
        .client()
        .post(server.http_url("/api/admin/token/rotate"))
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
        .post(server.http_url("/api/admin/token/rotate"))
        .bearer_auth(&current.token)
        .send()
        .await
        .expect("second rotate request should send");
    assert_eq!(old_token_rejected.status(), StatusCode::UNAUTHORIZED);
}
