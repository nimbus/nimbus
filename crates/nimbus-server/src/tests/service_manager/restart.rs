use std::time::Duration;

use super::*;

#[tokio::test]
async fn duplicate_service_request_returns_same_restart_epoch() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let (local_server_security, token) = local_server_security(temp.path());
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend::default());
    let server = ServerFixture::start(
        managed_router_config(engine, service_manager(backend.clone()), backend.clone())
            .with_local_server_security(local_server_security)
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let start = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services/db/start"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("service start request should send");
    assert_eq!(start.status(), StatusCode::OK);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);

    let restart_url = server.http_url("/api/tenants/tenant/services/db/restart");
    let request = json!({
        "sourceGeneration": 1,
        "requestId": "operator-restart-42",
    });
    let first = server
        .client()
        .post(&restart_url)
        .bearer_auth(&token.token)
        .json(&request)
        .send()
        .await
        .expect("first service restart request should send");
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    let first = first
        .json::<Value>()
        .await
        .expect("first service restart response should parse");

    let replay = server
        .client()
        .post(&restart_url)
        .bearer_auth(&token.token)
        .json(&request)
        .send()
        .await
        .expect("duplicate service restart request should send");
    assert_eq!(replay.status(), StatusCode::ACCEPTED);
    let replay = replay
        .json::<Value>()
        .await
        .expect("duplicate service restart response should parse");

    assert_eq!(first["tenantId"], json!("tenant"));
    assert_eq!(first["name"], json!("db"));
    assert_eq!(first["sourceGeneration"], json!(1));
    assert_eq!(first["requestId"], json!("operator-restart-42"));
    assert_eq!(first["restartEpoch"], replay["restartEpoch"]);
    assert_eq!(
        first["workloadRestartRequestId"],
        replay["workloadRestartRequestId"]
    );
    assert_eq!(first["disposition"], json!("applied"));
    assert_eq!(replay["disposition"], json!("replayed"));

    tokio::time::timeout(Duration::from_secs(5), async {
        while backend.image_starts.load(Ordering::SeqCst) != 2 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("durable restart should converge through the retained supervisor");
    assert_eq!(
        backend.stop_calls.load(Ordering::SeqCst),
        0,
        "service restart must not compose the coarse stop route"
    );
    server.shutdown().await;
}

#[tokio::test]
async fn service_restart_rejects_crossed_source_generation_before_admission() {
    let temp = tempfile::tempdir().expect("tempdir should create");
    let (local_server_security, token) = local_server_security(temp.path());
    let engine = Arc::new(Engine::new(temp.path()).expect("engine should create"));
    let backend = Arc::new(ReadySandboxBackend::default());
    let server = ServerFixture::start(
        managed_router_config(engine, service_manager(backend.clone()), backend.clone())
            .with_local_server_security(local_server_security)
            .without_deploy_admin_token()
            .build(),
    )
    .await;

    let start = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services/db/start"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("service start request should send");
    assert_eq!(start.status(), StatusCode::OK);

    let crossed = server
        .client()
        .post(server.http_url("/api/tenants/tenant/services/db/restart"))
        .bearer_auth(&token.token)
        .json(&json!({
            "sourceGeneration": 2,
            "requestId": "crossed-generation",
        }))
        .send()
        .await
        .expect("crossed service restart request should send");
    assert_eq!(crossed.status(), StatusCode::PRECONDITION_FAILED);
    assert_eq!(backend.image_starts.load(Ordering::SeqCst), 1);
    server.shutdown().await;
}
