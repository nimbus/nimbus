use std::sync::Arc;

use axum::http::{HeaderValue, StatusCode, header};
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};

use super::*;
use crate::local_server::{
    LocalServerPaths, LocalServerSecurityState, load_or_create_local_admin_token,
};
use crate::router::RouterBuildConfig;

const DEPLOY_TOKEN: &str = "deploy-token";
const LOCAL_ADMIN_HEADER_NAME: &str = "x-neovex-admin-token";

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

#[tokio::test]
async fn bad_origin_returns_forbidden_before_local_admin_auth() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, _token) = local_server_security(temp.path());
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
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
}

#[tokio::test]
async fn deploy_admin_requires_local_admin_header_even_with_deploy_bearer() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security)
            .with_deploy_admin_token(DEPLOY_TOKEN)
            .build(),
    )
    .await;

    let request = json!({
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_firebase(FirebaseConfig::new())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let rest_response = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
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
        HeaderValue::from_static("neovex.firebase.listen.v1, neovex.firebase.auth.dW5pdC10b2tlbg"),
    );

    let (_socket, response) = connect_async(websocket_request)
        .await
        .expect("firebase websocket request should not require local admin auth");
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    assert_eq!(
        response.headers().get(header::SEC_WEBSOCKET_PROTOCOL),
        Some(&HeaderValue::from_static("neovex.firebase.listen.v1"))
    );
}

#[tokio::test]
async fn firebase_websocket_bad_origin_is_rejected_before_auth() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, _token) = local_server_security(temp.path());
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
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
        HeaderValue::from_static("neovex.firebase.listen.v1, neovex.firebase.auth.dW5pdC10b2tlbg"),
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
    let application_id = "neovex-test";
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_application_auth_verifier(crate::router::convex_application_auth_verifier(
                &registry,
            ))
            .with_convex(registry)
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
async fn convex_websocket_bad_origin_is_rejected_before_auth() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let registry = convex_registry(json!([]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_application_auth_verifier(crate::router::convex_application_auth_verifier(
                &registry,
            ))
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
