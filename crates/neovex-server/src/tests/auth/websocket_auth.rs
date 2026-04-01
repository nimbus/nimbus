use super::*;

#[tokio::test]
async fn convex_websocket_auth_message_sets_runtime_identity() {
    let _guard = auth_test_guard().await;
    let issuer = "https://issuer.example.com";
    let application_id = "neovex-test";
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
async fn convex_websocket_auth_change_drops_active_subscriptions_until_resubscribed() {
    let _guard = auth_test_guard().await;
    let issuer = "https://issuer.example.com";
    let application_id = "neovex-test";
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
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
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
    assert_eq!(
        socket.next_json().await,
        json!({
            "type": "error",
            "message": "authentication context changed; resubscribe active subscriptions"
        })
    );
    assert_eq!(
        socket.next_json().await,
        json!({
            "type": "authenticated",
            "is_authenticated": false
        })
    );

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
        "old subscription should be gone after auth changes"
    );

    socket
        .subscribe_named("req-2", "auth:watchIdentity", json!({}))
        .await;
    let resubscribed = socket.next_json().await;
    assert_eq!(resubscribed["type"], json!("subscription_result"));
    assert_eq!(resubscribed["data"]["identity"], json!(null));
}
