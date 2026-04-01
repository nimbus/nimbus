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
