use std::fs;
use std::sync::Arc;

use axum::http::{HeaderValue, StatusCode, header};
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};

use super::*;
use crate::local_server::{
    LocalServerAuditRecord, LocalServerPaths, LocalServerSecurityState,
    load_or_create_local_admin_token,
};
use crate::router::RouterBuildConfig;

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

fn read_audit_log(path: &std::path::Path) -> (String, Vec<LocalServerAuditRecord>) {
    let raw = fs::read_to_string(path).expect("audit log should be readable");
    let records = raw
        .lines()
        .map(|line| {
            serde_json::from_str::<LocalServerAuditRecord>(line)
                .expect("audit log line should parse")
        })
        .collect::<Vec<_>>();
    (raw, records)
}

fn extract_cookie(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(axum::http::header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("set-cookie header should be present")
        .split(';')
        .next()
        .expect("cookie pair should be present")
        .to_string()
}

#[tokio::test]
async fn local_admin_and_origin_failures_are_audited_without_secret_material() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let audit_log_path = local_server_security.paths().audit_log_path.clone();
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let unauthorized = server
        .client()
        .post(server.http_url("/api/tenants"))
        .json(&json!({ "id": "demo" }))
        .send()
        .await
        .expect("unauthorized request should send");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let authorized = server
        .client()
        .post(server.http_url("/api/tenants"))
        .bearer_auth(&token.token)
        .json(&json!({ "id": "demo" }))
        .send()
        .await
        .expect("authorized request should send");
    assert_eq!(authorized.status(), StatusCode::CREATED);

    let bad_origin = server
        .client()
        .post(server.http_url("/api/tenants"))
        .header("Origin", "http://example.com")
        .header("Authorization", "Bearer not-a-real-token")
        .json(&json!({ "id": "other" }))
        .send()
        .await
        .expect("bad origin request should send");
    assert_eq!(bad_origin.status(), StatusCode::FORBIDDEN);

    let (raw, records) = read_audit_log(&audit_log_path);
    assert!(
        records.iter().any(|record| {
            record.route_family == "native_api"
                && record.auth_scope == "server_access"
                && !record.success
                && record.reason
                    == "local admin access requires Authorization: Bearer <token> or X-Nimbus-Admin-Token"
        }),
        "missing failed local-admin audit entry: {records:?}"
    );
    assert!(
        records.iter().any(|record| {
            record.route_family == "native_api"
                && record.auth_scope == "server_access"
                && record.auth_method.as_deref() == Some("local_admin_bearer")
                && record.success
                && record.reason == "authorized"
        }),
        "missing successful local-admin audit entry: {records:?}"
    );
    assert!(
        records.iter().any(|record| {
            record.route_family == "native_api"
                && record.auth_scope == "origin"
                && !record.success
                && record.origin.as_deref() == Some("http://example.com")
                && record
                    .reason
                    .contains("origin http://example.com is not allowed")
        }),
        "missing bad-origin audit entry: {records:?}"
    );
    assert!(
        !raw.contains(&token.token),
        "audit log must not contain the local admin token"
    );
    assert!(
        !raw.contains("not-a-real-token"),
        "audit log must not contain rejected bearer material"
    );
}

#[tokio::test]
async fn session_creation_and_rotation_are_audited_without_secret_material() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let audit_log_path = local_server_security.paths().audit_log_path.clone();
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let session_response = server
        .client()
        .post(server.http_url("/ui/auth/session"))
        .header(
            axum::http::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(format!("token={}", token.token))
        .send()
        .await
        .expect("session bootstrap request should send");
    assert_eq!(session_response.status(), StatusCode::OK);
    let cookie = extract_cookie(&session_response);

    let rotate = server
        .client()
        .post(server.http_url("/api/system/token/rotate"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("rotate request should send");
    assert_eq!(rotate.status(), StatusCode::OK);

    let (raw, records) = read_audit_log(&audit_log_path);
    assert!(
        records.iter().any(|record| {
            record.route_family == "ui_auth_session"
                && record.auth_scope == "session"
                && record.auth_method.as_deref() == Some("local_admin_token_post")
                && record.success
                && record.reason == "session.created"
        }),
        "missing session creation audit entry: {records:?}"
    );
    assert!(
        records.iter().any(|record| {
            record.route_family == "native_api"
                && record.auth_scope == "server_access"
                && record.auth_method.as_deref() == Some("local_admin_bearer")
                && record.success
                && record.reason == "token.rotated"
        }),
        "missing token rotation audit entry: {records:?}"
    );
    assert!(
        records.iter().any(|record| {
            record.route_family == "native_api"
                && record.auth_scope == "session"
                && record.auth_method.as_deref() == Some("token_rotation")
                && record.success
                && record.reason.starts_with("session.invalidated:")
        }),
        "missing session invalidation audit entry: {records:?}"
    );
    assert!(
        !raw.contains(&token.token),
        "audit log must not contain the local admin token"
    );
    assert!(
        !raw.contains(&cookie),
        "audit log must not contain the signed session cookie"
    );
}

#[tokio::test]
async fn firebase_origin_failures_are_audited_with_transport_specific_route_families() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, _token) = local_server_security(temp.path());
    let audit_log_path = local_server_security.paths().audit_log_path.clone();
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let rest_rejected = server
        .client()
        .post(server.http_url("/v1/projects/demo/databases/(default)/documents:commit"))
        .header("Origin", "http://example.com")
        .header("Content-Type", "text/plain;charset=UTF-8")
        .body("{}")
        .send()
        .await
        .expect("firebase rest request should send");
    assert_eq!(rest_rejected.status(), StatusCode::FORBIDDEN);

    let grpc_web_rejected = server
        .client()
        .post(server.http_url("/google.firestore.v1.Firestore/Commit"))
        .header("Origin", "http://example.com")
        .header("x-grpc-web", "1")
        .header("Content-Type", "application/grpc-web+proto")
        .header(
            "google-cloud-resource-prefix",
            "projects/demo/databases/(default)",
        )
        .body(Vec::new())
        .send()
        .await
        .expect("firebase grpc-web request should send");
    assert_eq!(grpc_web_rejected.status(), StatusCode::FORBIDDEN);

    let mut websocket_request = server
        .ws_url("/google.firestore.v1.Firestore/Listen")
        .into_client_request()
        .expect("firebase websocket request should build");
    websocket_request.headers_mut().insert(
        header::ORIGIN,
        HeaderValue::from_static("http://example.com"),
    );
    websocket_request.headers_mut().insert(
        "google-cloud-resource-prefix",
        HeaderValue::from_static("projects/demo/databases/(default)"),
    );
    let websocket_error = connect_async(websocket_request)
        .await
        .expect_err("firebase websocket request should be rejected");
    let websocket_response = match websocket_error {
        tokio_tungstenite::tungstenite::Error::Http(response) => response,
        other => panic!("unexpected websocket error: {other}"),
    };
    assert_eq!(websocket_response.status(), StatusCode::FORBIDDEN);

    let (_raw, records) = read_audit_log(&audit_log_path);
    assert!(
        records.iter().any(|record| {
            record.route_family == "firebase_rest"
                && record.tenant_id.as_deref() == Some("demo")
                && record.auth_scope == "origin"
                && !record.success
                && record.origin.as_deref() == Some("http://example.com")
        }),
        "missing firebase rest origin audit entry: {records:?}"
    );
    assert!(
        records.iter().any(|record| {
            record.route_family == "firebase_grpc_web"
                && record.tenant_id.as_deref() == Some("demo")
                && record.auth_scope == "origin"
                && !record.success
                && record.origin.as_deref() == Some("http://example.com")
        }),
        "missing firebase grpc-web origin audit entry: {records:?}"
    );
    assert!(
        records.iter().any(|record| {
            record.route_family == "firebase_websocket"
                && record.tenant_id.as_deref() == Some("demo")
                && record.auth_scope == "origin"
                && !record.success
                && record.origin.as_deref() == Some("http://example.com")
        }),
        "missing firebase websocket origin audit entry: {records:?}"
    );
}

#[tokio::test]
async fn tenant_application_auth_audit_keeps_application_scope() {
    let _guard = super::auth::auth_test_guard().await;
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, local_admin_token) = local_server_security(temp.path());
    let audit_log_path = local_server_security.paths().audit_log_path.clone();
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

    let (_raw, records) = read_audit_log(&audit_log_path);
    let app_records = records
        .iter()
        .filter(|record| record.route_family == "convex_http")
        .collect::<Vec<_>>();
    assert!(
        app_records.iter().any(|record| {
            record.tenant_id.as_deref() == Some("demo")
                && record.auth_scope == "application"
                && record.auth_method.as_deref() == Some("application_bearer")
                && record.success
                && record.reason == "application.authenticated"
        }),
        "missing tenant-scoped application audit entry: {app_records:?}"
    );
    assert!(
        app_records
            .iter()
            .all(|record| record.auth_method.as_deref() != Some("local_admin_bearer")),
        "convex audit entries must not confuse local-admin auth with application auth: {app_records:?}"
    );
}
