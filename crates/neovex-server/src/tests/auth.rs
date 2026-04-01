use super::auth_support::*;
use super::*;

#[tokio::test]
async fn convex_runtime_query_exposes_authenticated_identity_from_bearer_token() {
    let issuer = "https://issuer.example.com";
    let application_id = "neovex-test";
    let (token, jwks_data_url) = issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({
            "email": "ada@example.com",
            "name": "Ada Lovelace",
            "role": "admin",
            "given_name": "Ada",
            "updated_at": 1710000000,
            "address": {
                "formatted": "123 Analytical Engine Way"
            }
        }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
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

    let unauthenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("unauthenticated auth query should succeed");
    assert_eq!(unauthenticated.status(), StatusCode::OK);
    assert_eq!(
        unauthenticated
            .json::<serde_json::Value>()
            .await
            .expect("unauthenticated auth body should parse"),
        json!(null)
    );

    let authenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("authenticated auth query should succeed");
    assert_eq!(authenticated.status(), StatusCode::OK);
    let body = authenticated
        .json::<serde_json::Value>()
        .await
        .expect("authenticated auth body should parse");
    assert_eq!(body["tokenIdentifier"], json!(format!("{issuer}|user-123")));
    assert_eq!(body["subject"], json!("user-123"));
    assert_eq!(body["issuer"], json!(issuer));
    assert_eq!(body["email"], json!("ada@example.com"));
    assert_eq!(body["name"], json!("Ada Lovelace"));
    assert_eq!(body["role"], json!("admin"));
    assert_eq!(body["given_name"], json!("Ada"));
    assert_eq!(body["updated_at"], json!(1710000000));
    assert_eq!(
        body["address.formatted"],
        json!("123 Analytical Engine Way")
    );
    assert_eq!(body.get("givenName"), None);
    assert_eq!(body.get("updatedAt"), None);
    assert_eq!(body.get("address"), None);

    let usage = api
        .license_status()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("license status should parse after authenticated query");
    assert_eq!(usage["usage"]["monthly_active_users"], json!(1));
}

#[tokio::test]
async fn convex_runtime_query_exposes_neovex_verified_identity_extension() {
    let issuer = "https://issuer.example.com";
    let application_id = "neovex-test";
    let (token, jwks_data_url) = issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({
            "email": "ada@example.com",
            "name": "Ada Lovelace",
            "given_name": "Ada",
            "updated_at": 1710000000,
            "address": {
                "formatted": "123 Analytical Engine Way"
            },
            "role": "admin"
        }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ user: await ctx.auth.getUserIdentity(), verified: await ctx.auth.getVerifiedIdentity() })"
            }
        ]),
        json!([]),
        Some(runtime_verified_auth_bundle_source()),
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

    let authenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("authenticated auth query should succeed");
    assert_eq!(authenticated.status(), StatusCode::OK);
    let body = authenticated
        .json::<serde_json::Value>()
        .await
        .expect("authenticated auth body should parse");
    assert_eq!(body["user"]["given_name"], json!("Ada"));
    assert_eq!(body["user"]["updated_at"], json!(1710000000));
    assert_eq!(
        body["user"]["address.formatted"],
        json!("123 Analytical Engine Way")
    );
    assert_eq!(body["user"].get("givenName"), None);
    assert_eq!(body["user"].get("updatedAt"), None);
    assert_eq!(body["user"].get("address"), None);

    assert_eq!(body["verified"]["kind"], json!("custom_jwt"));
    assert_eq!(body["verified"]["name"], json!("Ada Lovelace"));
    assert_eq!(body["verified"]["givenName"], json!("Ada"));
    assert_eq!(body["verified"]["email"], json!("ada@example.com"));
    assert_eq!(body["verified"]["updatedAt"], json!("1710000000"));
    assert_eq!(
        body["verified"]["address"],
        json!("123 Analytical Engine Way")
    );
    assert_eq!(body["verified"]["role"], json!("admin"));
    assert_eq!(body["verified"].get("given_name"), None);
    assert_eq!(body["verified"].get("updated_at"), None);
    assert_eq!(body["verified"].get("address.formatted"), None);
}

#[tokio::test]
async fn convex_runtime_query_rejects_invalid_bearer_token() {
    let issuer = "https://issuer.example.com";
    let application_id = "neovex-test";
    let (_token, jwks_data_url) = issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({ "email": "ada@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
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

    let response = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", "Bearer invalid.jwt.token")
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("invalid auth query should return an HTTP response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn convex_websocket_auth_message_sets_runtime_identity() {
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
async fn convex_runtime_query_accepts_custom_jwt_issuer_without_scheme() {
    let provider_issuer = "https://issuer.example.com";
    let token_issuer = "issuer.example.com";
    let application_id = "neovex-test";
    let (token, jwks_data_url) = issue_es256_test_token_with_audience(
        token_issuer,
        json!(application_id),
        "user-123",
        json!({ "email": "ada@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "type": "customJwt",
                    "issuer": provider_issuer,
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

    let authenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("authenticated auth query should succeed");
    assert_eq!(authenticated.status(), StatusCode::OK);
}

#[tokio::test]
async fn convex_runtime_query_accepts_eddsa_oidc_tokens_and_formats_address() {
    let application_id = "neovex-test";
    let (provider, token, _jwks) = mock_oidc_provider_with_token(
        json!(application_id),
        "user-123",
        json!({
            "name": "Ada Lovelace",
            "address": {
                "formatted": "123 Analytical Engine Way"
            }
        }),
    )
    .await;
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "domain": provider.issuer(),
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

    let authenticated = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("authenticated OIDC query should succeed");
    assert_eq!(authenticated.status(), StatusCode::OK);
    let body = authenticated
        .json::<serde_json::Value>()
        .await
        .expect("authenticated OIDC body should parse");
    assert_eq!(
        body["tokenIdentifier"],
        json!(format!("{}|user-123", provider.issuer()))
    );
    assert_eq!(body["address"], json!("123 Analytical Engine Way"));
}

#[tokio::test]
async fn convex_runtime_query_rejects_multi_audience_oidc_tokens() {
    let application_id = "neovex-test";
    let (provider, token, _jwks) = mock_oidc_provider_with_token(
        json!([application_id, "other-audience"]),
        "user-123",
        json!({}),
    )
    .await;
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "domain": provider.issuer(),
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

    let response = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("multi-audience OIDC query should return an HTTP response");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn convex_oidc_jwks_are_refetched_after_rotation() {
    let application_id = "neovex-test";
    let (provider, first_token, _first_jwks) =
        mock_oidc_provider_with_token(json!(application_id), "user-123", json!({})).await;
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.auth.getUserIdentity()"
            }
        ]),
        json!([]),
        Some(runtime_auth_bundle_source()),
        Some(json!({
            "providers": [
                {
                    "domain": provider.issuer(),
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

    let first_response = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {first_token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("first OIDC query should succeed");
    assert_eq!(first_response.status(), StatusCode::OK);

    let (second_token, second_jwks) = issue_eddsa_test_token(
        provider.issuer(),
        json!(application_id),
        "user-456",
        json!({}),
    );
    provider.set_jwks(second_jwks);

    let second_response = server
        .client()
        .post(api.convex_url("demo", "/query"))
        .header("Authorization", format!("Bearer {second_token}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("second OIDC query should succeed after JWKS rotation");
    assert_eq!(second_response.status(), StatusCode::OK);
    assert!(provider.discovery_request_count() >= 2);
    assert!(provider.jwks_request_count() >= 2);
}
