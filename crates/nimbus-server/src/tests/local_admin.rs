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
async fn shutdown_handle_stops_live_server() {
    let temp = tempdir().expect("tempdir should build");
    let service = Arc::new(
        nimbus_engine::Engine::new(temp.path().join("data")).expect("service should initialize"),
    );
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .expect("listener should bind");
    let address = listener
        .local_addr()
        .expect("listener address should resolve");
    let options = ServeOptions::reconstruct_direct(service.clone())
        .expect("test server network authority should reconstruct once");
    let shutdown = options.shutdown_handle();
    let server_task = tokio::spawn(serve(listener, options));
    let client = reqwest::Client::new();
    wait_for_condition(
        "shutdown-handle test server should answer health checks",
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

    shutdown.request_shutdown();
    tokio::time::timeout(Duration::from_secs(5), server_task)
        .await
        .expect("server should exit after a handle requests shutdown")
        .expect("server task should join")
        .expect("handle-requested shutdown should be graceful");
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

/// The console's log search: the route scopes the lines to the tenant,
/// matches the text against the message, and answers with the count.
#[tokio::test]
async fn console_log_search_scopes_to_the_tenant_and_matches_text() {
    let temp = tempdir().expect("tempdir should build");
    let paths = sample_paths(temp.path());
    let current = load_or_create_local_admin_token(&paths).expect("token should exist");
    let local_server_security = Arc::new(LocalServerSecurityState::new(paths, current.clone()));
    let fixture = EngineFixture::new(|path| nimbus_engine::Engine::new(path));
    let engine = fixture.engine();
    let acme = nimbus_core::TenantId::new("acme").expect("tenant id");
    let beta = nimbus_core::TenantId::new("beta").expect("tenant id");
    for (tenant, message) in [
        (&acme, "payment accepted for order 7"),
        (&acme, "cache miss on user 3"),
        (&beta, "Payment refused: card expired"),
    ] {
        nimbus_system::record_system_event_async(
            &engine,
            nimbus_system::SystemEvent {
                tenant_id: Some(tenant),
                source: "runtime",
                level: "info",
                category: "function",
                message,
                data: serde_json::json!({}),
                correlation_id: Some("run-a"),
            },
        )
        .await
        .expect("event should record");
    }
    let server = ServerFixture::start(
        RouterBuildConfig::core(engine)
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let response = server
        .client()
        .get(server.http_url("/api/console/logs?tenant=acme&q=PAYMENT&limit=50"))
        .bearer_auth(&current.token)
        .send()
        .await
        .expect("search request should send");
    assert_eq!(response.status(), StatusCode::OK);
    let page: serde_json::Value = response.json().await.expect("search page should parse");
    assert_eq!(page["matched"], 1);
    assert_eq!(page["scanned"], 2);
    assert_eq!(page["exhaustive"], true);
    assert_eq!(page["limit"], 50);
    let lines = page["lines"].as_array().expect("lines should be an array");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["message"], "payment accepted for order 7");
    assert_eq!(lines[0]["tenantId"], "acme");

    let rejected = server
        .client()
        .get(server.http_url("/api/console/logs?tenant=not%20a%20tenant"))
        .bearer_auth(&current.token)
        .send()
        .await
        .expect("bad tenant request should send");
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn console_error_groups_fold_failed_runs_and_scope_to_the_tenant() {
    let temp = tempdir().expect("tempdir should build");
    let paths = sample_paths(temp.path());
    let current = load_or_create_local_admin_token(&paths).expect("token should exist");
    let local_server_security = Arc::new(LocalServerSecurityState::new(paths, current.clone()));
    let fixture = EngineFixture::new(|path| nimbus_engine::Engine::new(path));
    let engine = fixture.engine();
    let acme = nimbus_core::TenantId::new("acme").expect("tenant id");
    let beta = nimbus_core::TenantId::new("beta").expect("tenant id");
    let thrown = nimbus_core::Error::function_thrown(
        "messages:send",
        "Message text must not be empty (at messages:12)",
        None,
    );
    let thrown_display = thrown.to_string();
    let missing = nimbus_core::Error::InvalidInput("text is required".to_string());
    let missing_display = missing.to_string();
    for (tenant, error, display, started_at) in [
        (&acme, &thrown, &thrown_display, 1_000),
        (&acme, &thrown, &thrown_display, 2_000),
        (&beta, &missing, &missing_display, 3_000),
    ] {
        nimbus_system::record_run_async(
            &engine,
            nimbus_system::RunRecord {
                tenant_id: tenant,
                function_path: "messages:send",
                kind: "mutation",
                started_at,
                duration_ms: 4.0,
                status: "error",
                error: Some(nimbus_system::RunError::from_core_error(error, display)),
                spans: Vec::new(),
            },
        )
        .await
        .expect("run should record");
    }
    nimbus_system::record_run_async(
        &engine,
        nimbus_system::RunRecord {
            tenant_id: &acme,
            function_path: "messages:send",
            kind: "mutation",
            started_at: 4_000,
            duration_ms: 1.0,
            status: "ok",
            error: None,
            spans: Vec::new(),
        },
    )
    .await
    .expect("run should record");
    let server = ServerFixture::start(
        RouterBuildConfig::core(engine)
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let response = server
        .client()
        .get(server.http_url("/api/console/errors?limit=50"))
        .bearer_auth(&current.token)
        .send()
        .await
        .expect("error groups request should send");
    assert_eq!(response.status(), StatusCode::OK);
    let page: serde_json::Value = response.json().await.expect("page should parse");
    assert_eq!(page["scanned"], 3, "{page}");
    assert_eq!(page["exhaustive"], true, "{page}");
    assert_eq!(page["limit"], 50, "{page}");
    let groups = page["groups"]
        .as_array()
        .expect("groups should be an array");
    assert_eq!(groups.len(), 2, "{page}");
    assert_eq!(groups[0]["tenantId"], "beta", "newest group first: {page}");
    assert_eq!(groups[0]["class"], "invalid_input", "{page}");
    assert_eq!(groups[0]["count"], 1, "{page}");
    assert_eq!(groups[1]["tenantId"], "acme", "{page}");
    assert_eq!(groups[1]["class"], "function_thrown", "{page}");
    assert_eq!(groups[1]["count"], 2, "{page}");
    assert_eq!(groups[1]["firstSeen"], 1_000, "{page}");
    assert_eq!(groups[1]["lastSeen"], 2_000, "{page}");
    assert_eq!(groups[1]["location"], "messages:12", "{page}");
    assert_eq!(groups[1]["functionPath"], "messages:send", "{page}");
    assert!(
        groups[1]["latestRunId"].as_str().is_some(),
        "the newest run id opens the drill-in: {page}"
    );

    let scoped = server
        .client()
        .get(server.http_url("/api/console/errors?tenant=acme"))
        .bearer_auth(&current.token)
        .send()
        .await
        .expect("scoped request should send");
    assert_eq!(scoped.status(), StatusCode::OK);
    let page: serde_json::Value = scoped.json().await.expect("page should parse");
    let groups = page["groups"]
        .as_array()
        .expect("groups should be an array");
    assert_eq!(groups.len(), 1, "{page}");
    assert_eq!(groups[0]["tenantId"], "acme", "{page}");
    assert_eq!(page["limit"], nimbus_system::ERROR_GROUP_LIMIT, "{page}");

    let rejected = server
        .client()
        .get(server.http_url("/api/console/errors?tenant=not%20a%20tenant"))
        .bearer_auth(&current.token)
        .send()
        .await
        .expect("bad tenant request should send");
    assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
}
