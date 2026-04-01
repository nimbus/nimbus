use super::*;

#[tokio::test]
async fn convex_runtime_query_accepts_eddsa_oidc_tokens_and_formats_address() {
    let _guard = auth_test_guard().await;
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
