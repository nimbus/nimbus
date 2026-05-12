use std::sync::Arc;

use axum::http::{HeaderValue, StatusCode, header};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};

use super::*;
use crate::local_server::{
    LocalServerPaths, LocalServerSecurityState, load_or_create_local_admin_token,
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

fn extract_cookie(response: &reqwest::Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .and_then(|value| value.to_str().ok())
        .expect("set-cookie header should be present")
        .split(';')
        .next()
        .expect("cookie pair should be present")
        .to_string()
}

#[tokio::test]
async fn ui_redirects_to_auth_without_session_cookie() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, _token) = local_server_security(temp.path());
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("redirect-disabled client should build");
    let response = client
        .get(server.http_url("/ui/"))
        .send()
        .await
        .expect("ui request should send");

    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(
        response
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/ui/auth")
    );
}

#[tokio::test]
async fn ui_auth_get_never_sets_a_session_cookie_and_sets_csp() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, _token) = local_server_security(temp.path());
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let response = server
        .client()
        .get(server.http_url("/ui/auth"))
        .send()
        .await
        .expect("ui auth request should send");

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get(header::SET_COOKIE).is_none());
    let csp = response
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .and_then(|value| value.to_str().ok())
        .expect("csp header should be present");
    assert!(!csp.contains("unsafe-eval"));
}

#[tokio::test]
async fn valid_token_post_creates_session_cookie_and_cookie_auth_unlocks_ui_and_ws() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
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
        .expect("create tenant request should send");
    assert_eq!(create_tenant.status(), StatusCode::CREATED);

    let seed_document = server
        .client()
        .post(server.http_url("/api/tenants/demo/documents"))
        .bearer_auth(&token.token)
        .json(&json!({ "table": "messages", "fields": { "body": "Hello" } }))
        .send()
        .await
        .expect("seed document request should send");
    assert_eq!(seed_document.status(), StatusCode::CREATED);

    let session_response = server
        .client()
        .post(server.http_url("/ui/auth/session"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!("token={}", token.token))
        .send()
        .await
        .expect("session bootstrap request should send");
    assert_eq!(session_response.status(), StatusCode::OK);
    let cookie = extract_cookie(&session_response);

    let ui_response = server
        .client()
        .get(server.http_url("/ui/"))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .expect("ui shell request should send");
    assert_eq!(ui_response.status(), StatusCode::OK);

    let mut request = server
        .ws_url("/ws")
        .into_client_request()
        .expect("websocket request should build");
    request
        .headers_mut()
        .insert("X-Tenant-Id", HeaderValue::from_static("demo"));
    request.headers_mut().insert(
        header::COOKIE,
        HeaderValue::from_str(&cookie).expect("cookie header should build"),
    );
    request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("nimbus.v2"),
    );
    let (mut socket, _) = connect_async(request)
        .await
        .expect("cookie-auth websocket should connect");
    let hello = socket
        .next()
        .await
        .expect("websocket hello should arrive")
        .expect("websocket hello frame should be valid");
    let hello_text = match hello {
        tokio_tungstenite::tungstenite::Message::Text(text) => text,
        other => panic!("unexpected websocket hello frame: {other:?}"),
    };
    let hello_body =
        serde_json::from_str::<serde_json::Value>(&hello_text).expect("hello should parse");
    assert_eq!(hello_body["type"], json!("hello"));
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            json!({
                "type": "client_hello",
                "protocol": "nimbus.v2",
                "client": {
                    "kind": "test",
                    "version": "0.0.0"
                },
                "capabilities": ["queries.v1", "subscriptions.v1"]
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("client hello should send");
    socket
        .send(tokio_tungstenite::tungstenite::Message::Text(
            json!({
                "type": "subscribe",
                "request_id": "ui-1",
                "query": {
                    "table": "messages",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            })
            .to_string()
            .into(),
        ))
        .await
        .expect("subscription message should send");
    let message = socket
        .next()
        .await
        .expect("subscription result should arrive")
        .expect("websocket message should be valid");
    let text = match message {
        tokio_tungstenite::tungstenite::Message::Text(text) => text,
        other => panic!("unexpected websocket message: {other:?}"),
    };
    let body = serde_json::from_str::<serde_json::Value>(&text).expect("json message should parse");
    assert_eq!(body["type"], json!("subscription_result"));
}

#[tokio::test]
async fn invalid_token_post_fails_and_rotated_cookie_is_revoked() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let invalid = server
        .client()
        .post(server.http_url("/ui/auth/session"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body("token=not-the-real-token")
        .send()
        .await
        .expect("invalid session bootstrap should send");
    assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);

    let valid = server
        .client()
        .post(server.http_url("/ui/auth/session"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!("token={}", token.token))
        .send()
        .await
        .expect("valid session bootstrap should send");
    assert_eq!(valid.status(), StatusCode::OK);
    let cookie = extract_cookie(&valid);

    let rotate = server
        .client()
        .post(server.http_url("/api/admin/token/rotate"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("rotate request should send");
    assert_eq!(rotate.status(), StatusCode::OK);

    let revoked = server
        .client()
        .get(server.http_url("/ui/"))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .expect("revoked cookie request should send");
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    let body = revoked
        .json::<serde_json::Value>()
        .await
        .expect("revoked response should be json");
    assert_eq!(body["error"]["message"], json!("auth.token_revoked"));
}
