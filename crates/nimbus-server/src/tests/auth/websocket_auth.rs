use super::*;

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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
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

    let mut socket = WebSocketFixture::connect_for_browser(&api.ws_url("/convex/demo/ws"), "demo")
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
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

    let mut socket = WebSocketFixture::connect_for_browser(&api.ws_url("/convex/demo/ws"), "demo")
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);
    let tenant_id = nimbus_core::TenantId::new("demo").expect("tenant id should be valid");
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document("demo", "messages", json!({ "body": "Before auth change" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let mut socket = WebSocketFixture::connect_for_browser(&api.ws_url("/convex/demo/ws"), "demo")
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
