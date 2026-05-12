use super::*;

#[tokio::test]
async fn convex_runtime_query_exposes_nimbus_verified_identity_extension() {
    let _guard = auth_test_guard().await;
    let issuer = "https://issuer.example.com";
    let application_id = "nimbus-test";
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
