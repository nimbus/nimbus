use super::*;

#[tokio::test]
async fn convex_oidc_jwks_are_refetched_after_rotation() {
    let _guard = auth_test_guard().await;
    let application_id = "nimbus-test";
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex_with_tenancy(
        fixture.engine(),
        registry,
        convex_team_tenancy_for("demo"),
    ))
    .await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused;
    // both rotated tokens are admitted only because their verified issuer is bound.
    assert_convex_anonymous_query_refused(&server, "demo").await;

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
