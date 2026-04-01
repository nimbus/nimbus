use super::auth_fixtures::*;
use super::*;

#[test]
fn convex_registry_requires_runtime_bundle_hash_sidecar() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".neovex").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": [] }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");
    fs::write(
        convex_dir.join("bundle.mjs"),
        "globalThis.__neovexInvoke = async function () { return { status: \"ok\", value: null }; }; export {};",
    )
    .expect("convex runtime bundle should write");

    let error = ConvexRegistry::from_app_dir(tempdir.path())
        .expect_err("bundle without sidecar hash should be rejected");
    assert!(
        error.to_string().contains("bundle.sha256"),
        "unexpected registry error: {error}"
    );
}

#[tokio::test]
async fn health_route_returns_ok() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);
    let response = api.health().await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("health json should parse")["ok"],
        serde_json::json!(true)
    );
}

#[tokio::test]
async fn license_status_route_returns_community_defaults() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let response = api.license_status().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("license status json should parse");
    assert_eq!(body["kind"], json!("community"));
    assert_eq!(body["status"], json!("community"));
    assert_eq!(body["source"]["kind"], json!("community_default"));
    assert_eq!(body["revenue_limit_usd"], json!(10_000_000));
    assert_eq!(body["monthly_active_user_limit"], json!(500));
    assert_eq!(body["usage"]["monthly_active_users"], json!(0));
    assert_eq!(body["usage"]["limit"], json!(500));
    assert_eq!(body["usage"]["limit_exceeded"], json!(false));
}

#[tokio::test]
async fn license_status_route_returns_trial_license_details() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let license_state = LicenseState::from_document(
        LicenseDocument {
            schema_version: 1,
            kind: LicenseKind::Trial,
            issued_to: Some("Acme Corp".to_string()),
            issued_by: Some("Neovex".to_string()),
            issued_at_unix_ms: Some(1_700_000_000_000),
            expires_at_unix_ms: None,
            trial_expires_at_unix_ms: Some(u64::MAX),
            revenue_limit_usd: Some(10_000_000),
            monthly_active_user_limit: Some(500),
            entitlements: LicenseEntitlements {
                premium_support: true,
                custom_terms: true,
                ..LicenseEntitlements::default()
            },
            notes: None,
        },
        LicenseSourceInfo {
            kind: LicenseSourceKind::ExplicitFile,
            path: Some("/tmp/license.json".to_string()),
        },
    );
    let server =
        ServerFixture::start(build_router_with_license(fixture.service(), license_state)).await;
    let api = HttpApiFixture::new(&server);

    let response = api.license_status().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("license status json should parse");
    assert_eq!(body["kind"], json!("trial"));
    assert_eq!(body["status"], json!("trial_active"));
    assert_eq!(body["issued_to"], json!("Acme Corp"));
    assert_eq!(body["source"]["kind"], json!("explicit_file"));
    assert_eq!(body["source"]["path"], json!("/tmp/license.json"));
    assert_eq!(body["entitlements"]["premium_support"], json!(true));
    assert_eq!(body["entitlements"]["custom_terms"], json!(true));
    assert_eq!(body["usage"]["monthly_active_users"], json!(0));
}

#[tokio::test]
async fn license_status_route_tracks_global_monthly_active_users_across_tenants() {
    let issuer_one = "https://issuer-one.example.com";
    let issuer_two = "https://issuer-two.example.com";
    let application_id = "neovex-test";
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);
    assert_eq!(
        api.create_tenant("alpha").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.create_tenant("beta").await.status(),
        StatusCode::CREATED
    );

    assert_eq!(
        server
            .client()
            .post(api.convex_url("alpha", "/query"))
            .json(&json!({ "name": "auth:whoami", "args": {} }))
            .send()
            .await
            .expect("unauthenticated alpha query should succeed")
            .status(),
        StatusCode::OK
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

#[tokio::test]
async fn runtime_metrics_route_requires_convex_support() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let response = api.runtime_metrics().await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn runtime_metrics_route_returns_limits_and_metrics_when_convex_support_is_enabled() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    let response = api.runtime_metrics().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime metrics json should parse");
    assert_eq!(body["limits"]["max_heap_mb"], json!(128));
    assert_eq!(body["limits"]["initial_heap_mb"], json!(8));
    assert_eq!(body["limits"]["execution_timeout_ms"], json!(30_000));
    assert!(body["limits"]["max_concurrent_isolates"].is_u64());
    assert_eq!(body["limits"]["max_nested_runtime_invocations"], json!(64));
    assert_eq!(body["metrics"]["worker_dispatched_invocations"], json!(0));
    assert_eq!(body["metrics"]["nested_local_dispatches"], json!(0));
    assert_eq!(body["metrics"]["queued_canceled_invocations"], json!(0));
    assert_eq!(body["metrics"]["in_flight_canceled_invocations"], json!(0));
    assert_eq!(body["metrics"]["disconnect_canceled_invocations"], json!(0));
    assert_eq!(body["metrics"]["explicit_canceled_invocations"], json!(0));
    assert_eq!(body["metrics"]["precanceled_host_ops"], json!(0));
    assert_eq!(body["metrics"]["in_flight_canceled_host_ops"], json!(0));
    assert_eq!(body["metrics"]["host_operations"], json!({}));
    assert_eq!(body["metrics"]["tenants"], json!({}));
    assert_eq!(body["metrics"]["recent_request_correlations"], json!([]));
    assert_eq!(
        body["metrics"]["fallback_cross_isolate_dispatches"],
        json!(0)
    );
}

#[tokio::test]
async fn neovex_demo_html_is_served_without_convex_support() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;

    let response = server
        .client()
        .get(server.http_url("/demos/neovex/html/"))
        .send()
        .await
        .expect("demo request should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.text().await.expect("demo body should load");
    assert!(body.contains("Neovex HTML Demo"));
    assert!(body.contains("Live tasks over HTTP writes and WebSocket subscriptions."));
}
