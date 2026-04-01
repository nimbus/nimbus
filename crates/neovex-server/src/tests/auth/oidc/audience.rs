use super::*;

#[tokio::test]
async fn convex_runtime_query_rejects_multi_audience_oidc_tokens() {
    let _guard = auth_test_guard().await;
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
