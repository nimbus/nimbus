use std::sync::Arc;

use axum::http::{HeaderValue, StatusCode, header};
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};

use super::*;
use crate::local_server::{
    LOCAL_SESSION_COOKIE_NAME, LocalServerPaths, LocalServerSecurityState,
    load_or_create_local_admin_token,
};
use crate::router::RouterBuildConfig;

const DEPLOY_TOKEN: &str = "deploy-token";
const LOCAL_ADMIN_HEADER_NAME: &str = "x-nimbus-admin-token";

fn sample_paths(root: &std::path::Path) -> LocalServerPaths {
    LocalServerPaths {
        auth_token_path: root.join("auth").join("token"),
        server_discovery_path: root.join("run").join("server.json"),
        audit_log_path: root.join("logs").join("access.jsonl"),
    }
}

fn local_server_security(
    root: &std::path::Path,
) -> (
    Arc<LocalServerSecurityState>,
    crate::local_server::LocalAdminTokenRecord,
) {
    let paths = sample_paths(root);
    let token = load_or_create_local_admin_token(&paths).expect("token should exist");
    (
        Arc::new(LocalServerSecurityState::new(paths, token.clone())),
        token,
    )
}

fn query_function(name: &str, table: &str) -> serde_json::Value {
    json!({
        "name": name,
        "kind": "query",
        "plan": {
            "table": table,
            "filters": [],
            "order": null,
            "limit": null
        }
    })
}

#[tokio::test]
async fn bad_origin_returns_forbidden_before_local_admin_auth() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, _token) = local_server_security(temp.path());
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_firebase(FirebaseConfig::new())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let response = server
        .client()
        .post(server.http_url("/api/tenants"))
        .header("Origin", "http://example.com")
        .header("Authorization", "Bearer not-a-real-token")
        .json(&json!({ "id": "demo" }))
        .send()
        .await
        .expect("request should send");

    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn native_api_and_debug_routes_require_local_admin_auth() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_firebase(FirebaseConfig::new())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let create_denied = server
        .client()
        .post(server.http_url("/api/tenants"))
        .json(&json!({ "id": "demo" }))
        .send()
        .await
        .expect("create request should send");
    assert_eq!(create_denied.status(), StatusCode::UNAUTHORIZED);

    let create_allowed = server
        .client()
        .post(server.http_url("/api/tenants"))
        .bearer_auth(&token.token)
        .json(&json!({ "id": "demo" }))
        .send()
        .await
        .expect("authorized create request should send");
    assert_eq!(create_allowed.status(), StatusCode::CREATED);

    let machine_start_denied = server
        .client()
        .post(server.http_url("/api/machines/default/start"))
        .send()
        .await
        .expect("machine start request should send");
    assert_eq!(machine_start_denied.status(), StatusCode::UNAUTHORIZED);

    let machine_start_authorized = server
        .client()
        .post(server.http_url("/api/machines/default/start"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("authorized machine start request should send");
    assert_eq!(machine_start_authorized.status(), StatusCode::NOT_FOUND);

    let debug_denied = server
        .client()
        .get(server.http_url("/debug/license/status"))
        .send()
        .await
        .expect("debug request should send");
    assert_eq!(debug_denied.status(), StatusCode::UNAUTHORIZED);

    let debug_allowed = server
        .client()
        .get(server.http_url("/debug/license/status"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("authorized debug request should send");
    assert_eq!(debug_allowed.status(), StatusCode::OK);

    let retention_denied = server
        .client()
        .post(server.http_url("/debug/tenants/demo/engine/retention"))
        .send()
        .await
        .expect("unauthorized retention request should send");
    assert_eq!(retention_denied.status(), StatusCode::UNAUTHORIZED);

    let retention_allowed = server
        .client()
        .post(server.http_url("/debug/tenants/demo/engine/retention"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("authorized retention request should send");
    assert_eq!(retention_allowed.status(), StatusCode::OK);
}

#[tokio::test]
async fn deploy_admin_requires_local_admin_header_even_with_deploy_bearer() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let session = local_server_security
        .create_session_for_local_admin_token(&token.token)
        .expect("local session cookie should issue");
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_local_server_security(local_server_security)
            .with_deploy_admin_token(DEPLOY_TOKEN)
            .build(),
    )
    .await;

    let request = json!({
        "convex_silo": "demo",
        "artifacts": {
            "convex": {
                "functions_json": { "functions": [] },
                "http_routes_json": { "routes": [] }
            }
        }
    });

    let missing_local_admin = server
        .client()
        .post(server.http_url("/api/admin/deploy"))
        .bearer_auth(DEPLOY_TOKEN)
        .json(&request)
        .send()
        .await
        .expect("deploy request should send");
    assert_eq!(missing_local_admin.status(), StatusCode::UNAUTHORIZED);

    let session_cookie_denied = server
        .client()
        .post(server.http_url("/api/admin/deploy"))
        .bearer_auth(DEPLOY_TOKEN)
        .header(
            header::COOKIE,
            format!("{LOCAL_SESSION_COOKIE_NAME}={}", session.value),
        )
        .json(&request)
        .send()
        .await
        .expect("session-cookie deploy request should send");
    assert_eq!(session_cookie_denied.status(), StatusCode::UNAUTHORIZED);

    let authorized = server
        .client()
        .post(server.http_url("/api/admin/deploy"))
        .bearer_auth(DEPLOY_TOKEN)
        .header(LOCAL_ADMIN_HEADER_NAME, &token.token)
        .json(&request)
        .send()
        .await
        .expect("authorized deploy request should send");
    assert_eq!(authorized.status(), StatusCode::OK);
}

#[tokio::test]
async fn native_websocket_requires_local_admin_auth() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, _token) = local_server_security(temp.path());
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let mut request = server
        .ws_url("/ws")
        .into_client_request()
        .expect("websocket request should build");
    request
        .headers_mut()
        .insert("X-Tenant-Id", HeaderValue::from_static("demo"));

    let error = connect_async(request)
        .await
        .expect_err("missing local admin auth should reject websocket");
    let response = match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => response,
        other => panic!("unexpected websocket error: {other}"),
    };
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn firebase_routes_remain_application_surfaces_without_local_admin_auth() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, _token) = local_server_security(temp.path());
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_firebase(firebase_verified_config())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    // A verified-path bearer (not a local-admin token) clears the #24 gate, so a
    // non-403/401/404 status proves the Firestore route is an application surface
    // that does not require local-admin auth. (The malformed `{}` commit then
    // returns 400, which still satisfies the application-surface assertions.)
    let rest_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header(
            header::AUTHORIZATION,
            firebase_verified_bearer("user-123", "demo"),
        )
        .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
        .body("{}")
        .send()
        .await
        .expect("firebase rest request should send");
    assert_ne!(rest_response.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(rest_response.status(), StatusCode::FORBIDDEN);
    assert_ne!(rest_response.status(), StatusCode::NOT_FOUND);

    let grpc_web_response = server
        .client()
        .post(server.http_url("/google.firestore.v1.Firestore/Commit"))
        .header("x-grpc-web", "1")
        .header(header::CONTENT_TYPE, "application/grpc-web+proto")
        .header(
            "google-cloud-resource-prefix",
            "projects/demo/databases/(default)",
        )
        .body(Vec::new())
        .send()
        .await
        .expect("firebase grpc-web request should send");
    assert_ne!(grpc_web_response.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(grpc_web_response.status(), StatusCode::FORBIDDEN);
    assert_ne!(grpc_web_response.status(), StatusCode::NOT_FOUND);

    let mut websocket_request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("firebase websocket request should build");
    websocket_request.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://localhost:5173"),
    );
    websocket_request.headers_mut().insert(
        "google-cloud-resource-prefix",
        HeaderValue::from_static("projects/demo/databases/(default)"),
    );
    websocket_request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("nimbus.firebase.listen.v1, nimbus.firebase.auth.dW5pdC10b2tlbg"),
    );

    let (_socket, response) = connect_async(websocket_request)
        .await
        .expect("firebase websocket request should not require local admin auth");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert_eq!(
        response.headers().get(header::SEC_WEBSOCKET_PROTOCOL),
        Some(&HeaderValue::from_static("nimbus.firebase.listen.v1"))
    );
}

#[tokio::test]
async fn firebase_websocket_bad_origin_is_rejected_before_auth() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, _token) = local_server_security(temp.path());
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_firebase(FirebaseConfig::new())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let mut request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("firebase websocket request should build");
    request.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://example.com"),
    );
    request.headers_mut().insert(
        "google-cloud-resource-prefix",
        HeaderValue::from_static("projects/demo/databases/(default)"),
    );
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("nimbus.firebase.listen.v1, nimbus.firebase.auth.dW5pdC10b2tlbg"),
    );

    let error = connect_async(request)
        .await
        .expect_err("bad origin should reject firebase websocket");
    let response = match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => response,
        other => panic!("unexpected websocket error: {other}"),
    };
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn convex_routes_keep_application_auth_and_reject_local_admin_bearers() {
    let _guard = super::auth::auth_test_guard().await;
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, local_admin_token) = local_server_security(temp.path());
    let issuer = "https://issuer.example.com";
    let application_id = "nimbus-test";
    let (jwt, jwks_data_url) = super::auth::issue_es256_test_token(
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
                "runtime_handler": "async (ctx) => ({ user: await ctx.auth.getUserIdentity() })"
            }
        ]),
        json!([]),
        Some(super::auth::runtime_auth_bundle_source()),
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_convex_silo_auth_verifier(
                &TenantId::new("demo").expect("demo silo id"),
                crate::router::convex_application_auth_verifier(&registry),
            )
            .with_convex(registry)
            // Bind this registry's verifier directly to `demo`.
            .with_convex_tenancy(convex_team_tenancy_for("demo"))
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let create_tenant = server
        .client()
        .post(server.http_url("/api/tenants"))
        .bearer_auth(&local_admin_token.token)
        .json(&json!({ "id": "demo" }))
        .send()
        .await
        .expect("tenant create request should send");
    assert_eq!(create_tenant.status(), StatusCode::CREATED);

    let application_auth = server
        .client()
        .post(server.http_url("/convex/demo/query"))
        .header("Authorization", format!("Bearer {jwt}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("application auth query should send");
    assert_eq!(application_auth.status(), StatusCode::OK);
    let body = application_auth
        .json::<serde_json::Value>()
        .await
        .expect("application auth body should parse");
    assert_eq!(body["tokenIdentifier"], json!(format!("{issuer}|user-123")));

    let local_admin_as_app_auth = server
        .client()
        .post(server.http_url("/convex/demo/query"))
        .header(
            "Authorization",
            format!("Bearer {}", local_admin_token.token),
        )
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("local admin bearer query should send");
    assert_eq!(local_admin_as_app_auth.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn convex_route_rejects_application_bearer_for_different_tenant() {
    let _guard = super::auth::auth_test_guard().await;
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, local_admin_token) = local_server_security(temp.path());
    let issuer = "https://issuer.example.com";
    let application_id = "nimbus-test";
    let (tenant_b_jwt, jwks_data_url) = super::auth::issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({ "tenant_id": "tenant-b", "email": "ada@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([
            {
                "name": "auth:whoami",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ user: await ctx.auth.getUserIdentity() })"
            }
        ]),
        json!([]),
        Some(super::auth::runtime_auth_bundle_source()),
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_convex_silo_auth_verifier(
                &TenantId::new("tenant-b").expect("tenant-b silo id"),
                crate::router::convex_application_auth_verifier(&registry),
            )
            .with_convex(registry)
            // Only tenant-b receives this registry's verifier.
            .with_convex_tenancy(convex_cross_tenant_tenancy())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    for tenant in ["tenant-a", "tenant-b"] {
        let response = server
            .client()
            .post(server.http_url("/api/tenants"))
            .bearer_auth(&local_admin_token.token)
            .json(&json!({ "id": tenant }))
            .send()
            .await
            .expect("tenant create request should send");
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    let authorized = server
        .client()
        .post(server.http_url("/convex/tenant-b/query"))
        .header("Authorization", format!("Bearer {tenant_b_jwt}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("same-tenant application auth query should send");
    let authorized_status = authorized.status();
    let authorized_body = authorized
        .text()
        .await
        .expect("same-tenant application auth body should read");
    assert_eq!(
        authorized_status,
        StatusCode::OK,
        "same-tenant query body: {authorized_body}"
    );

    let rejected = server
        .client()
        .post(server.http_url("/convex/tenant-a/query"))
        .header("Authorization", format!("Bearer {tenant_b_jwt}"))
        .json(&json!({ "name": "auth:whoami", "args": {} }))
        .send()
        .await
        .expect("swapped-tenant application auth query should send");
    let rejected_status = rejected.status();
    let rejected_body = rejected
        .text()
        .await
        .expect("swapped-tenant application auth body should read");
    assert_eq!(
        rejected_status,
        StatusCode::UNAUTHORIZED,
        "swapped-tenant query body: {rejected_body}"
    );
    assert!(
        rejected_body.contains("no Convex auth providers are configured for silo `tenant-a`"),
        "the unprovisioned target silo must fail before bearer verification: {rejected_body}"
    );
}

#[tokio::test]
async fn convex_http_action_rejects_application_bearer_for_different_tenant() {
    let _guard = super::auth::auth_test_guard().await;
    let issuer = "https://issuer.example.com";
    let application_id = "nimbus-test";
    let (tenant_b_jwt, jwks_data_url) = super::auth::issue_es256_test_token(
        issuer,
        application_id,
        "user-123",
        json!({ "tenant_id": "tenant-b", "email": "ada@example.com" }),
    );
    let registry = convex_registry_with_routes_and_bundle_and_auth(
        json!([]),
        json!([
            {
                "method": "GET",
                "path": "/secure",
                "name": "http:inline:tenant-proof",
                "plan": {
                    "response": {
                        "kind": "json",
                        "body": {
                            "ok": true
                        }
                    }
                }
            }
        ]),
        None,
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    for tenant in ["tenant-a", "tenant-b"] {
        service
            .create_tenant(TenantId::new(tenant).expect("tenant id should parse"))
            .expect("tenant should create");
    }
    let server = ServerFixture::start(
        RouterBuildConfig::core(service)
            .with_convex_silo_auth_verifier(
                &TenantId::new("tenant-b").expect("tenant-b silo id"),
                crate::router::convex_application_auth_verifier(&registry),
            )
            .with_convex(registry)
            // Same tenant-b-only verifier binding as the query case.
            .with_convex_tenancy(convex_cross_tenant_tenancy())
            .build(),
    )
    .await;

    let authorized = server
        .client()
        .get(server.http_url("/convex/tenant-b/http/secure"))
        .header("Authorization", format!("Bearer {tenant_b_jwt}"))
        .send()
        .await
        .expect("same-tenant convex http action should send");
    let authorized_status = authorized.status();
    let authorized_body = authorized
        .text()
        .await
        .expect("same-tenant convex http action body should read");
    assert_eq!(
        authorized_status,
        StatusCode::OK,
        "same-tenant convex http action body: {authorized_body}"
    );

    let rejected = server
        .client()
        .get(server.http_url("/convex/tenant-a/http/secure"))
        .header("Authorization", format!("Bearer {tenant_b_jwt}"))
        .send()
        .await
        .expect("swapped-tenant convex http action should send");
    let rejected_status = rejected.status();
    let rejected_body = rejected
        .text()
        .await
        .expect("swapped-tenant convex http action body should read");
    assert_eq!(
        rejected_status,
        StatusCode::UNAUTHORIZED,
        "swapped-tenant convex http action body: {rejected_body}"
    );
    assert!(
        rejected_body.contains("no Convex auth providers are configured for silo `tenant-a`"),
        "the unprovisioned target silo must fail before bearer verification: {rejected_body}"
    );
}

#[tokio::test]
async fn system_tenant_convex_routes_use_system_registry_not_application_registry() {
    let system_registry = convex_registry(json!([query_function("routes:list", "routes")]));
    let application_registry = convex_registry(json!([query_function("notes:list", "notes")]));
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    crate::system_tenant::prepare_system_tenant_async(&service, None)
        .await
        .expect("system tenant should prepare");
    let server = ServerFixture::start(
        with_convex_team_binding(
            RouterBuildConfig::core(service)
                .with_system_convex_registry(system_registry)
                .with_convex(application_registry),
            "demo",
        )
        .build(),
    )
    .await;
    // `api` stays anonymous for the native and operator-gated `_nimbus` routes;
    // `app_api` carries the team bearer for the gated application surface.
    let api = HttpApiFixture::new(&server);
    let app_api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "notes",
            json!({ "title": "Application tenant note" }),
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    // #41 non-vacuous: an anonymous selection of the application silo is refused.
    assert_convex_anonymous_query_refused(&server, "demo").await;

    let system_routes = api
        .convex_named_query("_nimbus", "routes:list", json!({}))
        .await;
    assert_eq!(system_routes.status(), StatusCode::OK);
    let routes = system_routes
        .json::<serde_json::Value>()
        .await
        .expect("system route query body should parse");
    assert!(
        routes.as_array().is_some_and(|routes| routes
            .iter()
            .any(|route| route["path"] == "/health" && route["adapter"] == "native")),
        "system Convex registry should read the seeded _nimbus route inventory: {routes}"
    );

    let application_notes = app_api
        .convex_named_query("demo", "notes:list", json!({}))
        .await;
    assert_eq!(application_notes.status(), StatusCode::OK);
    let notes = application_notes
        .json::<serde_json::Value>()
        .await
        .expect("application query body should parse");
    assert_eq!(notes[0]["title"], "Application tenant note");
}

#[tokio::test]
async fn system_tenant_convex_routes_require_local_admin_auth_when_configured() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let system_registry = convex_registry(json!([query_function("routes:list", "routes")]));
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    crate::system_tenant::prepare_system_tenant_async(&service, None)
        .await
        .expect("system tenant should prepare");
    let server = ServerFixture::start(
        RouterBuildConfig::core(service)
            .with_system_convex_registry(system_registry)
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let missing_auth = server
        .client()
        .post(server.http_url("/convex/_nimbus/query"))
        .json(&json!({ "name": "routes:list", "args": {} }))
        .send()
        .await
        .expect("missing auth system query should send");
    assert_eq!(missing_auth.status(), StatusCode::UNAUTHORIZED);

    let authorized = server
        .client()
        .post(server.http_url("/convex/_nimbus/query"))
        .bearer_auth(&token.token)
        .json(&json!({ "name": "routes:list", "args": {} }))
        .send()
        .await
        .expect("authorized system query should send");
    assert_eq!(authorized.status(), StatusCode::OK);
    let routes = authorized
        .json::<serde_json::Value>()
        .await
        .expect("authorized system route body should parse");
    assert!(
        routes.as_array().is_some_and(|routes| !routes.is_empty()),
        "authorized system query should return seeded route inventory: {routes}"
    );
}

#[tokio::test]
async fn tenant_runtime_cannot_read_system_tenant_routes() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let system_registry = convex_registry(json!([query_function("routes:list", "routes")]));
    let application_registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "notes:systemRoutes",
                "kind": "query",
                "plan": {
                    "table": "routes",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            }
        ]),
        json!([]),
        Some(
            r#"
globalThis.__nimbusInvoke = async function(request) {
  const routes = await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_query", {
    query: {
      table: "routes",
      filters: [],
      order: null,
      limit: null,
    },
    host_call_session_id: `${request.kind}:${request.function_name}`,
  });
  return {
    status: "ok",
    value: routes,
  };
};

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant("demo", Engine::create_tenant);
    let service = fixture.engine();
    crate::system_tenant::prepare_system_tenant_async(&service, None)
        .await
        .expect("system tenant should prepare");
    let server = ServerFixture::start(
        with_convex_team_binding(
            RouterBuildConfig::core(service)
                .with_system_convex_registry(system_registry)
                .with_convex(application_registry)
                .with_local_server_security(local_server_security),
            "demo",
        )
        .build(),
    )
    .await;
    let app_api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    // #41 non-vacuous: an anonymous selection of the application silo is refused;
    // the verified team bearer below is admitted and still cannot read _nimbus.
    assert_convex_anonymous_query_refused(&server, "demo").await;

    let application_routes = app_api
        .convex_named_query("demo", "notes:systemRoutes", json!({}))
        .await;
    let application_status = application_routes.status();
    let application_body = application_routes
        .json::<serde_json::Value>()
        .await
        .expect("application runtime query body should parse");
    assert_eq!(
        application_status,
        StatusCode::OK,
        "application runtime query should complete against its own tenant: {application_body}"
    );
    assert_eq!(
        application_body,
        json!([]),
        "application runtime HostBridge query must not expose _nimbus.routes"
    );

    let missing_auth = server
        .client()
        .post(server.http_url("/convex/_nimbus/query"))
        .json(&json!({ "name": "routes:list", "args": {} }))
        .send()
        .await
        .expect("missing auth system query should send");
    assert_eq!(missing_auth.status(), StatusCode::UNAUTHORIZED);

    let system_routes = server
        .client()
        .post(server.http_url("/convex/_nimbus/query"))
        .bearer_auth(&token.token)
        .json(&json!({ "name": "routes:list", "args": {} }))
        .send()
        .await
        .expect("operator-authenticated system query should send");
    assert_eq!(system_routes.status(), StatusCode::OK);
    let routes = system_routes
        .json::<serde_json::Value>()
        .await
        .expect("operator-authenticated system route body should parse");
    assert!(
        routes.as_array().is_some_and(|routes| routes
            .iter()
            .any(|route| route["path"] == "/health" && route["adapter"] == "native")),
        "operator-authenticated _nimbus query should return system route inventory: {routes}"
    );
}

#[tokio::test]
async fn convex_websocket_bad_origin_is_rejected_before_auth() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let registry = convex_registry(json!([]));
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_convex_silo_auth_verifier(
                &TenantId::new("demo").expect("demo silo id"),
                crate::router::convex_application_auth_verifier(&registry),
            )
            .with_convex(registry)
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let create_tenant = server
        .client()
        .post(server.http_url("/api/tenants"))
        .bearer_auth(&token.token)
        .json(&json!({ "id": "demo" }))
        .send()
        .await
        .expect("tenant create request should send");
    assert_eq!(create_tenant.status(), StatusCode::CREATED);

    let mut request = server
        .ws_url("/convex/demo/ws")
        .into_client_request()
        .expect("websocket request should build");
    request.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://example.com"),
    );
    request.headers_mut().insert(
        header::AUTHORIZATION,
        HeaderValue::from_static("Bearer invalid.jwt.token"),
    );

    let error = connect_async(request)
        .await
        .expect_err("bad origin should reject websocket");
    let response = match error {
        tokio_tungstenite::tungstenite::Error::Http(response) => response,
        other => panic!("unexpected websocket error: {other}"),
    };
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
