use super::*;

#[tokio::test]
async fn convex_runtime_query_rejects_invalid_bearer_token() {
    let _guard = auth_test_guard().await;
    let issuer = "https://issuer.example.com";
    let application_id = "nimbus-test";
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
