use super::*;

#[tokio::test]
async fn license_status_route_tracks_global_monthly_active_users_across_tenants() {
    let issuer_one = "https://issuer-one.example.com";
    let issuer_two = "https://issuer-two.example.com";
    let application_id = "nimbus-test";
    let (token_one, jwks_one) = issue_es256_test_token(
        issuer_one,
        application_id,
        "user-123",
        json!({ "email": "ada@example.com" }),
    );
    let (token_two, jwks_two) = issue_es256_test_token(
        issuer_two,
        application_id,
        "user-456",
        json!({ "email": "grace@example.com" }),
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
                    "issuer": issuer_one,
                    "jwks": jwks_one,
                    "algorithm": "ES256",
                    "applicationID": application_id
                },
                {
                    "type": "customJwt",
                    "issuer": issuer_two,
                    "jwks": jwks_two,
                    "algorithm": "ES256",
                    "applicationID": application_id
                }
            ]
        })),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    // #41: the application-Convex team gate guards `/convex/{alpha,beta}`. Bind
    // both silos to one team and bind the two custom-JWT issuers (token-one and
    // token-two) to that same team, so both verified users may select either
    // silo. MAU still counts the two distinct verified *subjects*; the gate
    // admits on the verified *issuer*. Anonymous selection stays refused below.
    let team = nimbus_convex::TeamId::new(crate::tests::CONVEX_TEAM).expect("team id valid");
    let tenancy = nimbus_convex::ConvexTenancyConfig::new().with_silo_teams(
        nimbus_convex::SiloTeamRegistry::new()
            .bind(
                &nimbus_core::TenantId::new("alpha").expect("alpha silo id valid"),
                team.clone(),
            )
            .bind(
                &nimbus_core::TenantId::new("beta").expect("beta silo id valid"),
                team.clone(),
            ),
    );
    let server = ServerFixture::start(router_for_convex_with_tenancy(
        fixture.engine(),
        registry,
        tenancy,
    ))
    .await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("alpha").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.create_tenant("beta").await.status(),
        StatusCode::CREATED
    );

    // #41 non-vacuous: with the team gate active, an *anonymous* selection of a
    // silo is refused (the accepted dev-UX consequence) — so it never reaches
    // the function and never inflates the monthly-active-user count below.
    assert_eq!(
        server
            .client()
            .post(api.convex_url("alpha", "/query"))
            .json(&json!({ "name": "auth:whoami", "args": {} }))
            .send()
            .await
            .expect("anonymous alpha query should receive a response")
            .status(),
        StatusCode::FORBIDDEN
    );

    for tenant_id in ["alpha", "beta"] {
        assert_eq!(
            server
                .client()
                .post(api.convex_url(tenant_id, "/query"))
                .header("Authorization", format!("Bearer {token_one}"))
                .json(&json!({ "name": "auth:whoami", "args": {} }))
                .send()
                .await
                .expect("authenticated token-one query should succeed")
                .status(),
            StatusCode::OK
        );
    }

    assert_eq!(
        server
            .client()
            .post(api.convex_url("beta", "/query"))
            .header("Authorization", format!("Bearer {token_two}"))
            .json(&json!({ "name": "auth:whoami", "args": {} }))
            .send()
            .await
            .expect("authenticated token-two query should succeed")
            .status(),
        StatusCode::OK
    );

    let usage = api
        .license_status()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("license usage json should parse");
    assert_eq!(usage["usage"]["monthly_active_users"], json!(2));
    assert_eq!(usage["usage"]["limit_exceeded"], json!(false));
    assert!(
        usage.get("warnings").is_none() || usage["warnings"] == json!([]),
        "warnings should be empty when usage is comfortably below the limit"
    );
}
