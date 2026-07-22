use super::*;

// This wait targets the exact status-projection mutation-drain pause. Runtime
// startup can queue behind shared V8 work in the full server aggregate even
// though the focused case completes quickly, so 30 seconds is a bounded safety
// budget rather than a timing contract. Failure still names the missing pause,
// and nextest reports the test as slow after 45 seconds.
const STATUS_PROJECTION_PAUSE_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) const WEBSOCKET_DISCONNECT_CLEANUP_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "websocket-disconnect-cleanup",
        "run-to-completion-snapshot",
        "disconnecting a runtime-backed websocket subscription releases its child runtime subscription state",
    );

pub(crate) const WEBSOCKET_AUTH_CHANGE_RESUBSCRIBE_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "websocket-auth-change-resubscribe",
        "run-to-completion-snapshot",
        "auth changes drop active runtime-backed subscriptions until the client explicitly resubscribes",
    );

#[tokio::test]
async fn convex_websocket_auth_message_sets_runtime_identity() {
    let _guard = auth_test_guard().await;
    let issuer = "https://issuer.example.com";
    let application_id = "nimbus-test";
    let (token, jwks_data_url) = issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({ "email": "ada@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:watchIdentity",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ identity: await ctx.auth.getUserIdentity(), messages: await ctx.db.query(\"messages\").take(1) })"
            }
        ]),
        json!([]),
        Some(runtime_auth_subscription_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": issuer,
                    "jwks": jwks_data_url,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex_with_tenancy(
        fixture.engine(),
        registry,
        convex_team_tenancy_binding("demo", "user-123"),
    ))
    .await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused;
    // the upgrade is admitted only with a team-bound bearer presented below.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    assert_eq!(
        api.insert_document("demo", "messages", json!({ "body": "Hello" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let mut socket = WebSocketFixture::connect_for_browser_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        "demo",
        &format!("Bearer {token}"),
    )
    .await
    .expect("browser-style websocket connection should succeed");
    socket
        .send_text(
            json!({
                "type": "authenticate",
                "token": token,
            })
            .to_string(),
        )
        .await;
    let authenticated = socket.next_json().await;
    assert_eq!(
        authenticated,
        json!({
            "type": "authenticated",
            "is_authenticated": true
        })
    );

    socket
        .subscribe_named("req-1", "auth:watchIdentity", json!({}))
        .await;
    let body = socket.next_json().await;
    assert_eq!(body["type"], json!("subscription_result"));
    assert_eq!(
        body["data"]["identity"]["tokenIdentifier"],
        json!(format!("{issuer}|user-123"))
    );
    assert_eq!(body["data"]["identity"]["email"], json!("ada@example.com"));
    assert_eq!(body["data"]["messages"][0]["body"], json!("Hello"));

    let usage = api
        .license_status()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("license status should parse after websocket auth");
    assert_eq!(usage["usage"]["monthly_active_users"], json!(1));
}

#[tokio::test]
async fn convex_websocket_disconnect_releases_runtime_subscription_children() {
    convex_websocket_disconnect_releases_runtime_subscription_children_inner().await;
}

pub(crate) async fn convex_websocket_disconnect_releases_runtime_subscription_children_inner() {
    let _guard = auth_test_guard().await;
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:watchIdentity",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ identity: await ctx.auth.getUserIdentity(), messages: await ctx.db.query(\"messages\").take(1) })"
            }
        ]),
        json!([]),
        Some(runtime_auth_subscription_bundle_source()),
        None,
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_convex_team(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);
    let tenant_id = nimbus_core::TenantId::new("demo").expect("tenant id should be valid");
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused;
    // the upgrade is admitted only with a team-bound bearer presented below.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    assert_eq!(
        api.insert_document("demo", "messages", json!({ "body": "Hello" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let mut socket = WebSocketFixture::connect_for_browser_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        "demo",
        &convex_team_bearer(),
    )
    .await
    .expect("browser-style websocket connection should succeed");
    socket
        .subscribe_named("req-1", "auth:watchIdentity", json!({}))
        .await;
    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        1
    );

    drop(socket);

    wait_for_condition(
        &WEBSOCKET_DISCONNECT_CLEANUP_CASE.failure_context_with_repro(
            "disconnect should release runtime-backed websocket subscriptions",
            "cargo test -p nimbus-server convex_websocket_disconnect_releases_runtime_subscription_children -- --nocapture",
        ),
        Duration::from_secs(2),
        Duration::from_millis(10),
        || async {
            service
                .active_subscription_count(&tenant_id)
                .expect("subscription count should load")
                == 0
        },
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn runtime_subscription_status_projection_precedes_initial_result() {
    let _guard = auth_test_guard().await;
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:watchIdentity",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ identity: await ctx.auth.getUserIdentity(), messages: await ctx.db.query(\"messages\").take(1) })"
            }
        ]),
        json!([]),
        Some(runtime_auth_subscription_bundle_source()),
        None,
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_convex_team(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);
    let tenant_id = nimbus_core::TenantId::new("demo").expect("tenant id should be valid");
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document("demo", "messages", json!({ "body": "Hello" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    nimbus_system::ensure_system_tenant_async(&service)
        .await
        .expect("system tenant should be ready before arming its mutation drain");
    let system_tenant = nimbus_system::system_tenant_id().expect("system tenant id should parse");
    let pause = service
        .mutation_journal_pause_handle_for_testing(&system_tenant)
        .expect("system tenant mutation pause should load");
    pause.arm();
    let mut socket = WebSocketFixture::connect_for_browser_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        "demo",
        &convex_team_bearer(),
    )
    .await
    .expect("browser-style websocket connection should succeed");
    socket
        .subscribe_named("req-1", "auth:watchIdentity", json!({}))
        .await;

    let wait_pause = pause.clone();
    assert!(
        tokio::task::spawn_blocking(move || {
            wait_pause.wait_until_entered(STATUS_PROJECTION_PAUSE_TIMEOUT)
        })
        .await
        .expect("status projection pause waiter should join"),
        "subscription status projection should reach the paused mutation drain"
    );
    assert!(
        timeout(Duration::from_millis(100), socket.next_json())
            .await
            .is_err(),
        "the initial result must not outrun the status projection"
    );

    pause.release();
    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        1
    );

    drop(socket);
    wait_for_condition(
        "disconnect should release the runtime subscription after the status projection",
        Duration::from_secs(2),
        Duration::from_millis(10),
        || async {
            service
                .active_subscription_count(&tenant_id)
                .expect("subscription count should load")
                == 0
        },
    )
    .await;
}

#[tokio::test]
async fn convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed() {
    convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed_inner().await;
}

pub(crate) async fn convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed_inner()
 {
    let _guard = auth_test_guard().await;
    let issuer = "https://issuer.example.com";
    let application_id = "nimbus-test";
    let (first_token, jwks_data_url) = issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({ "email": "ada@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:watchIdentity",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ identity: await ctx.auth.getUserIdentity(), messages: await ctx.db.query(\"messages\").take(1) })"
            }
        ]),
        json!([]),
        Some(runtime_auth_subscription_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": issuer,
                    "jwks": jwks_data_url,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_convex_with_tenancy(
        service.clone(),
        registry,
        convex_team_tenancy_binding("demo", "user-123"),
    ))
    .await;
    let api = HttpApiFixture::new(&server);
    let tenant_id = nimbus_core::TenantId::new("demo").expect("tenant id should be valid");
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused;
    // the upgrade is admitted only with a team-bound bearer presented below.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    assert_eq!(
        api.insert_document("demo", "messages", json!({ "body": "Before auth change" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let mut socket = WebSocketFixture::connect_for_browser_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        "demo",
        &format!("Bearer {first_token}"),
    )
    .await
    .expect("browser-style websocket connection should succeed");
    socket
        .send_text(
            json!({
                "type": "authenticate",
                "token": first_token,
            })
            .to_string(),
        )
        .await;
    assert_eq!(
        socket.next_json().await,
        json!({
            "type": "authenticated",
            "is_authenticated": true
        })
    );

    socket
        .subscribe_named("req-1", "auth:watchIdentity", json!({}))
        .await;
    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(
        initial["data"]["identity"]["tokenIdentifier"],
        json!(format!("{issuer}|user-123"))
    );

    socket
        .send_text(
            json!({
                "type": "clear_auth",
            })
            .to_string(),
        )
        .await;
    let auth_changed = socket.next_json().await;
    assert_eq!(auth_changed["type"], json!("error"));
    assert_eq!(
        auth_changed["error"]["code"],
        json!("session.auth_context_changed")
    );
    assert_eq!(
        auth_changed["error"]["message"],
        json!("authentication context changed; resubscribe active subscriptions")
    );
    assert_eq!(
        socket.next_json().await,
        json!({
            "type": "authenticated",
            "is_authenticated": false
        })
    );
    wait_for_condition(
        &WEBSOCKET_AUTH_CHANGE_RESUBSCRIBE_CASE.failure_context_with_repro(
            "auth changes should explicitly release active runtime subscriptions",
            "cargo test -p nimbus-server convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed -- --nocapture",
        ),
        Duration::from_secs(2),
        Duration::from_millis(10),
        || async {
            service
                .active_subscription_count(&tenant_id)
                .expect("subscription count should load")
                == 0
        },
    )
    .await;

    assert_eq!(
        api.insert_document("demo", "messages", json!({ "body": "After auth change" }))
            .await
            .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        socket
            .next_json_with_timeout(Duration::from_millis(250))
            .await,
        None,
        "{}",
        WEBSOCKET_AUTH_CHANGE_RESUBSCRIBE_CASE.failure_context_with_repro(
            "old subscription should be gone after auth changes",
            "cargo test -p nimbus-server convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed -- --nocapture",
        )
    );

    socket
        .subscribe_named("req-2", "auth:watchIdentity", json!({}))
        .await;
    let resubscribed = socket.next_json().await;
    assert_eq!(
        resubscribed["type"],
        json!("subscription_result"),
        "{}",
        WEBSOCKET_AUTH_CHANGE_RESUBSCRIBE_CASE.failure_context_with_repro(
            "resubscribe should bootstrap a fresh runtime-backed subscription result",
            "cargo test -p nimbus-server convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed -- --nocapture",
        )
    );
    assert_eq!(
        resubscribed["data"]["identity"],
        json!(null),
        "{}",
        WEBSOCKET_AUTH_CHANGE_RESUBSCRIBE_CASE.failure_context_with_repro(
            "resubscribe should reflect the cleared auth context after runtime cleanup",
            "cargo test -p nimbus-server convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed -- --nocapture",
        )
    );
}
