use std::sync::Arc;

use axum::http::{HeaderValue, StatusCode, header};
use futures::{SinkExt, StreamExt};
use tokio_tungstenite::{connect_async, tungstenite::client::IntoClientRequest};

use super::*;
use crate::local_server::{
    LOCAL_SESSION_COOKIE_NAME, LocalServerPaths, LocalServerSecurityState,
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
async fn ui_shell_serves_index_html_for_deep_routes_with_session_cookie() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
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
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!("token={}", token.token))
        .send()
        .await
        .expect("session bootstrap request should send");
    assert_eq!(session_response.status(), StatusCode::OK);
    let cookie = extract_cookie(&session_response);

    let root = server
        .client()
        .get(server.http_url("/ui/"))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .expect("ui root request should send");
    assert_eq!(root.status(), StatusCode::OK);
    let root_html = root.text().await.expect("ui root body should read");

    let deep = server
        .client()
        .get(server.http_url("/ui/machines/m_abc/services"))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .expect("ui deep route request should send");
    assert_eq!(deep.status(), StatusCode::OK);
    let deep_html = deep.text().await.expect("ui deep body should read");
    assert_eq!(
        root_html, deep_html,
        "spa fallback should return index.html"
    );
}

#[tokio::test]
async fn ui_root_response_carries_expected_csp() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
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
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!("token={}", token.token))
        .send()
        .await
        .expect("session bootstrap request should send");
    assert_eq!(session_response.status(), StatusCode::OK);
    let cookie = extract_cookie(&session_response);

    let response = server
        .client()
        .get(server.http_url("/ui/"))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .expect("ui root request should send");
    assert_eq!(response.status(), StatusCode::OK);
    let csp = response
        .headers()
        .get(header::CONTENT_SECURITY_POLICY)
        .and_then(|value| value.to_str().ok())
        .expect("csp header should be present on /ui/");
    assert!(csp.contains("default-src 'self'"));
    assert!(csp.contains("script-src 'self'"));
    assert!(!csp.contains("'unsafe-eval'"));
    assert!(!csp.contains("'unsafe-inline'") || !csp.contains("script-src 'self' 'unsafe-inline'"));
}

#[tokio::test]
async fn ui_asset_shaped_request_for_missing_file_returns_not_found() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
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
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(format!("token={}", token.token))
        .send()
        .await
        .expect("session bootstrap request should send");
    assert_eq!(session_response.status(), StatusCode::OK);

    let missing = server
        .client()
        .get(server.http_url("/ui/__nonexistent.js"))
        .send()
        .await
        .expect("missing asset request should send");
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
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
    // DA5 — the form-encoded POST path re-renders the auth page with an
    // inline error block instead of falling through to the generic JSON
    // 401. JSON callers still get the structured error (covered by
    // `invalid_token_json_post_returns_structured_unauthorized`).
    let invalid_body = invalid.text().await.expect("invalid response should read");
    assert!(
        invalid_body.contains("aria-invalid=\"true\""),
        "invalid token POST should re-render the form with aria-invalid"
    );
    assert!(
        invalid_body.contains("class=\"error-message\""),
        "invalid token POST should surface an inline error block above the input"
    );

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
        .post(server.http_url("/api/system/token/rotate"))
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

#[tokio::test]
async fn ui_auth_page_renders_brand_and_cli_hint_for_unauthenticated_visitors() {
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
        .expect("auth page request should send");
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(header::CONTENT_SECURITY_POLICY)
            .is_some(),
        "auth page must carry CSP header"
    );
    let body = response.text().await.expect("auth page body should read");
    assert!(
        body.contains("brand-wordmark") && body.contains(">nimbus<"),
        "auth page should render the nimbus brand wordmark"
    );
    // C4: every CLI recovery path goes through the `nimbus auth` surface;
    // the legacy `nimbus dev --open` shortcut is no longer offered here.
    assert!(
        body.contains("nimbus auth url"),
        "auth page should recommend `nimbus auth url` for the URL flows"
    );
    assert!(
        body.contains("nimbus auth token --copy"),
        "auth page should surface `nimbus auth token --copy` for the token flow"
    );
    assert!(
        !body.contains("nimbus dev --open"),
        "auth page should not surface `nimbus dev --open` as a recovery CTA"
    );
    // Visible label is `Enter auth token` (the form input the operator types
    // into). The matching `id=\"auth-token\"` keeps the label/input
    // association and the label-for hop in sync.
    assert!(
        body.contains("<span>Enter auth token</span>")
            && body.contains("id=\"auth-token\"")
            && body.contains("for=\"auth-token\""),
        "auth page should render the `Enter auth token` label tied to id=\"auth-token\""
    );
    assert!(
        !body.contains("Local admin token")
            && !body.contains("local-admin-token")
            && !body.contains("<span>Local token</span>")
            && !body.contains("id=\"local-token\""),
        "auth page should not retain prior `Local admin token` / `Local token` labels or their ids"
    );
    // The `.hint` block is gone. The token recovery surface is a
    // full-width `.shell-block` (DESIGN.md §Code Block treatment —
    // surface-2 fill, hairline border, header strip with `shell`
    // language label + copy affordance, `$` prompt body) that sits
    // between the CONTINUE button and the `How to login` disclosure.
    // The disclosure now catalogs every recovery path (Token first, then
    // the two URL flows) so it reads as the complete how-to-login menu.
    assert!(
        !body.contains("class=\"hint\""),
        "auth page should no longer render the standalone `.hint` block"
    );
    assert!(
        !body.contains("auth-token-section") && !body.contains("copyable-hero"),
        "auth page should no longer render the legacy `.auth-token-section` / `.copyable-hero` chrome"
    );
    assert!(
        body.contains("<details class=\"other-ways\"")
            && body.contains("<summary>How to login</summary>")
            && !body.contains("Other ways to login"),
        "auth page should wrap the recovery catalog inside a `How to login` disclosure"
    );
    assert!(
        body.contains("<button type=\"button\" class=\"shell-block\"")
            && body.contains("data-copy=\"nimbus auth token --copy\"")
            && body.contains("<span class=\"shell-block-lang\">terminal</span>")
            && body.contains("<span class=\"shell-block-prompt\"")
            && body.contains("<span class=\"shell-block-cmd\">nimbus auth token --copy</span>",),
        "auth page should expose a full-width `.shell-block` with a `terminal` chrome label, `$` prompt, and `nimbus auth token --copy` body"
    );
    assert!(
        body.contains("<h2 class=\"other-section-title\">Copy Token</h2>")
            && body.contains("<h2 class=\"other-section-title\">Open URL</h2>")
            && body.contains("<h2 class=\"other-section-title\">Copy URL</h2>"),
        "`How to login` disclosure should contain Copy Token, Open URL, and Copy URL entries"
    );
    assert!(
        !body.contains("<h2 class=\"other-section-title\">Token</h2>")
            && !body.contains("<h2 class=\"other-section-title\">Auto Login</h2>"),
        "auth page should not retain prior `Token` / `Auto Login` titles"
    );
    let copy_token_pos = body
        .find("<h2 class=\"other-section-title\">Copy Token</h2>")
        .expect("Copy Token section must render inside the disclosure");
    let open_url_pos = body
        .find("<h2 class=\"other-section-title\">Open URL</h2>")
        .expect("Open URL section must render inside the disclosure");
    let copy_url_pos = body
        .find("<h2 class=\"other-section-title\">Copy URL</h2>")
        .expect("Copy URL section must render inside the disclosure");
    assert!(
        copy_token_pos < open_url_pos && open_url_pos < copy_url_pos,
        "disclosure ordering must be Copy Token → Open URL → Copy URL"
    );
    // Body copy follows two unified templates: copy flows use
    // "Run <chip>, then paste the <thing> into <destination>"; the
    // one-shot flow uses "Run <chip> to open a single-use sign-in URL
    // in your browser". Asserting the load-bearing phrases catches both
    // accidental drift in tone and accidental cross-pollination between
    // the two URL entries.
    assert!(
        body.contains(", then paste the token into the field above.")
            && body.contains("to open a single-use sign-in URL in your browser.")
            && body.contains(", then paste the URL into your browser's address bar."),
        "disclosure bodies must use the unified copy templates"
    );
    assert!(
        !body.contains("Auto Login")
            && !body.contains("single-use launch URL")
            && !body.contains("paste the printed URL")
            && !body.contains("to copy your local admin token to the clipboard"),
        "auth page should drop prior inconsistent copy variants"
    );
    let shell_block_pos = body
        .find("<button type=\"button\" class=\"shell-block\"")
        .expect(".shell-block must render on the unauthenticated page");
    let disclosure_pos = body
        .find("<details class=\"other-ways\"")
        .expect("`How to login` disclosure must render on the unauthenticated page");
    let continue_pos = body
        .find("<button type=\"submit\">Continue</button>")
        .expect("`Continue` submit button must render on the unauthenticated page");
    assert!(
        continue_pos < shell_block_pos && shell_block_pos < disclosure_pos,
        "`.shell-block` must sit between the CONTINUE button and the `How to login` disclosure"
    );
    assert!(
        !body.contains(
            "<div class=\"other-ways-body\">\n      <button type=\"button\" class=\"shell-block\""
        ),
        "`.shell-block` must not be nested back inside `.other-ways-body`"
    );
    // CL3: the auth-page chrome should no longer *use* the
    // operator-console `--color-brand` token; the page is brand-tier only.
    // (Documenting comments may still mention the token by name to explain
    // why the page diverges, so we look for any `var(--color-brand)`
    // reference rather than the literal substring.)
    assert!(
        !body.contains("var(--color-brand)"),
        "auth page CSS should not reach for var(--color-brand) (brand-tier only)"
    );
    // DA5 — M2: in the unauthenticated GET path, the form should render
    // without an aria-invalid bit on the input element or an .error-message
    // block. (CSS selectors that match `aria-invalid="true"` may appear in
    // the stylesheet; the negative check targets the input element by
    // looking for the closing slug that only renders when no aria-invalid
    // attribute is substituted in.) The error path is exercised separately
    // in `invalid_token_form_post_rerenders_auth_with_error_state`.
    assert!(
        body.contains("spellcheck=\"false\" />"),
        "auth GET input should close cleanly with no aria-invalid attribute"
    );
    assert!(
        !body.contains("class=\"error-message\""),
        "auth GET should not render an inline error block"
    );
    assert!(
        body.contains("@font-face") && body.contains("JetBrains Mono"),
        "auth page should embed JetBrains Mono via @font-face"
    );
    assert!(
        body.contains("/ui/assets/") && body.contains("jetbrains-mono-latin-400-normal"),
        "auth page should reference the embedded JetBrains Mono asset path"
    );
    // DA1 — canonical Nimbus brand mark replaces the arcs+dot placeholder.
    assert!(
        body.contains("viewBox=\"0 0 322 201\"") && body.contains("<title>Nimbus</title>"),
        "auth page should embed the canonical nimbus-mark SVG (322x201 viewBox + Nimbus title)"
    );
    assert!(
        !body.contains("M4 20c0-6 4-10 10-10"),
        "auth page should no longer carry the arcs+dot placeholder mark path"
    );
    // Version chip lives in the card chrome (upper-right of the brand row)
    // and copies its own version string. Pin the class + copy hook so a
    // refactor that drops or relocates it surfaces here.
    assert!(
        body.contains("class=\"brand-version\"") && body.contains("aria-label=\"Copy version v"),
        "auth page should render the brand-version chip with a copy aria-label"
    );
    assert!(
        !body.contains("<footer>") && !body.contains("footer .wordmark"),
        "auth page should not duplicate the wordmark in a footer block"
    );
    // DA1 — trust microcopy (Local-only · 127.0.0.1) replaces the footer slot.
    assert!(
        body.contains("local-only") && body.contains("Local-only") && body.contains("127.0.0.1"),
        "auth page should display the Local-only trust line"
    );
    // DA1 — brand-tier color treatment locked: brand-blue token present and
    // independent of the chrome-tier --color-brand used elsewhere.
    assert!(
        body.contains("--brand-blue"),
        "auth page should declare the brand-tier --brand-blue token for the mark"
    );
}

#[tokio::test]
async fn mint_ui_launch_ticket_requires_admin_bearer_and_returns_consume_url() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let unauth = server
        .client()
        .post(server.http_url("/ui/auth/launch-ticket"))
        .send()
        .await
        .expect("unauthenticated mint request should send");
    assert_eq!(unauth.status(), StatusCode::UNAUTHORIZED);

    let minted = server
        .client()
        .post(server.http_url("/ui/auth/launch-ticket"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("authenticated mint request should send");
    assert_eq!(minted.status(), StatusCode::OK);
    let body = minted
        .json::<serde_json::Value>()
        .await
        .expect("mint response should parse as json");
    let ticket = body["ticket"]
        .as_str()
        .expect("mint response should include ticket");
    let url = body["url"]
        .as_str()
        .expect("mint response should include url");
    assert!(
        ticket.starts_with("nimbus_lt_"),
        "ticket should be prefixed nimbus_lt_, got {ticket}"
    );
    assert_eq!(url, format!("/ui/launch?lt={ticket}"));
}

#[tokio::test]
async fn consume_ui_launch_ticket_sets_session_cookie_and_redirects_to_ui_root() {
    let temp = tempdir().expect("tempdir should build");
    let (local_server_security, token) = local_server_security(temp.path());
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_local_server_security(local_server_security)
            .build(),
    )
    .await;

    let minted = server
        .client()
        .post(server.http_url("/ui/auth/launch-ticket"))
        .bearer_auth(&token.token)
        .send()
        .await
        .expect("mint request should send");
    assert_eq!(minted.status(), StatusCode::OK);
    let body = minted
        .json::<serde_json::Value>()
        .await
        .expect("mint response should parse");
    let ticket = body["ticket"]
        .as_str()
        .expect("mint response should include ticket")
        .to_string();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("redirect-disabled client should build");
    let consumed = client
        .get(server.http_url(&format!("/ui/launch?lt={ticket}")))
        .send()
        .await
        .expect("consume request should send");
    assert_eq!(consumed.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        consumed
            .headers()
            .get(header::LOCATION)
            .and_then(|value| value.to_str().ok()),
        Some("/ui/")
    );
    let cookie = extract_cookie(&consumed);
    assert!(
        cookie.starts_with(&format!("{LOCAL_SESSION_COOKIE_NAME}=")),
        "consume should issue the local session cookie, got {cookie}"
    );

    let ui_response = server
        .client()
        .get(server.http_url("/ui/"))
        .header(header::COOKIE, &cookie)
        .send()
        .await
        .expect("ui shell request should send");
    assert_eq!(ui_response.status(), StatusCode::OK);

    let reuse = client
        .get(server.http_url(&format!("/ui/launch?lt={ticket}")))
        .send()
        .await
        .expect("ticket reuse request should send");
    assert_eq!(reuse.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn consume_ui_launch_ticket_rejects_missing_or_unknown_tickets() {
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

    let missing = client
        .get(server.http_url("/ui/launch"))
        .send()
        .await
        .expect("consume without ticket should send");
    assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

    let bogus = client
        .get(server.http_url("/ui/launch?lt=nimbus_lt_not_a_real_ticket"))
        .send()
        .await
        .expect("consume with unknown ticket should send");
    assert_eq!(bogus.status(), StatusCode::UNAUTHORIZED);
}

/// DA5 — When a form-encoded POST to `/ui/auth/session` carries a bad
/// token, the server re-renders the auth page with the error block above
/// the input and an `aria-invalid="true"` bit on the field. The status is
/// still 401 so the form submission round-trips cleanly. JSON callers
/// keep the structured error envelope (covered separately by
/// `invalid_token_json_post_returns_structured_unauthorized`).
#[tokio::test]
async fn invalid_token_form_post_rerenders_auth_with_error_state() {
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
        .post(server.http_url("/ui/auth/session"))
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body("token=this-is-not-the-token")
        .send()
        .await
        .expect("invalid form post should send");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_string();
    assert!(
        content_type.contains("text/html"),
        "form-encoded failure should respond with HTML, got content-type {content_type}"
    );
    let body = response.text().await.expect("error body should read");
    assert!(
        body.contains("class=\"error-message\"") && body.contains("role=\"alert\""),
        "error response should render the .error-message block with role=alert"
    );
    assert!(
        body.contains("aria-invalid=\"true\""),
        "error response should mark the token input as aria-invalid"
    );
    assert!(
        body.contains("invalid local admin token"),
        "error response should surface the specific reason copy"
    );
    // The form structure must remain intact so the user can retry without
    // losing the brand chrome or the disclosure CTA.
    assert!(
        body.contains("<details class=\"other-ways\"") && body.contains("nimbus auth url"),
        "error response should still render the canonical hint + disclosure"
    );
    assert!(
        !body.contains("Set-Cookie: nimbus_local_session="),
        "error response should not issue a session cookie"
    );
}

/// DA5 — JSON callers still get the structured 401 envelope (no HTML
/// re-render). Verifies the `Accept: application/json` branch keeps the
/// existing programmatic contract intact while form posts get the new
/// inline error rendering.
#[tokio::test]
async fn invalid_token_json_post_returns_structured_unauthorized() {
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
        .post(server.http_url("/ui/auth/session"))
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json")
        .body(r#"{"token":"this-is-not-the-token"}"#)
        .send()
        .await
        .expect("invalid json post should send");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("json failure response should parse");
    assert_eq!(body["error"]["message"], json!("invalid local admin token"));
}
