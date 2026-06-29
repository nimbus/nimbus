//! #41 — Convex application-surface team-binding acceptance matrix.
//!
//! The Convex application routes (`/convex/{tenant_id}/…`) select a data silo
//! (`TenantId`) from the caller-supplied URL. The #41 gate
//! ([`nimbus_convex::ConvexTenancyConfig::authorize_silo_selection`]) is
//! all-fail-closed: a request may select a silo only when the URL silo resolves
//! to a team, the **verified** principal resolves to a team, and the two match.
//! It runs on every bind (loopback included), superseding the removed #43
//! network-bind stopgap.
//!
//! These tests drive the served HTTP/WebSocket path end-to-end with a genuinely
//! verified principal (an ES256 `customJwt` bearer the deployment's auth verifier
//! checks). The matrix:
//!  1. anonymous selection of a registered silo → 403, nothing written.
//!  2. cross-team selection by a verified team-a principal → 403 on a loopback
//!     AND a non-loopback bind, nothing written (the non-loopback case proves the
//!     binding, not a network guard, is what refuses).
//!  3. same-team / same-silo selection → 200, the document lands in that silo.
//!  4. same-team / OTHER-silo selection → 200, the document lands in the sibling
//!     silo (the many-silos-per-team capability; each silo still isolated).
//!  5. anonymous selection is refused on every route type — five POST families
//!     plus the WebSocket upgrade.

use std::net::SocketAddr;

use axum::http::HeaderValue;
use nimbus_convex::{ConvexTenancyConfig, PrincipalTeamRegistry, SiloTeamRegistry, TeamId};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Error as TungsteniteError;

use super::auth_fixtures::issue_es256_test_token;
use super::*;

/// A non-loopback listen address. After the #43 stopgap's removal this only
/// affects listener bookkeeping; the binding gate refuses cross-team selection
/// the same way on every bind, which is exactly what case (2) proves.
const PUBLIC_BIND: &str = "203.0.113.5:8080";
const ISSUER: &str = "https://idp.example.com";
const APPLICATION_ID: &str = "nimbus-convex-team-binding";
/// The verified token subject the principal registry binds to team-a.
const SUBJECT: &str = "user-a";
const SILO_A1: &str = "team-a-silo-1";
const SILO_A2: &str = "team-a-silo-2";
const SILO_B: &str = "team-b-silo";
const TEAM_A: &str = "team-a";
const TEAM_B: &str = "team-b";

fn silo(id: &str) -> TenantId {
    TenantId::new(id).expect("silo tenant id should be valid")
}

fn team(id: &str) -> TeamId {
    TeamId::new(id).expect("team id should be valid")
}

/// The admin-provisioned #41 tenancy: team-a owns `team-a-silo-1` and
/// `team-a-silo-2`; team-b owns `team-b-silo`. The verified principal `user-a`
/// (matched by `subject`) is on team-a.
fn tenancy_config() -> ConvexTenancyConfig {
    let silo_teams = SiloTeamRegistry::new()
        .bind(&silo(SILO_A1), team(TEAM_A))
        .bind(&silo(SILO_A2), team(TEAM_A))
        .bind(&silo(SILO_B), team(TEAM_B));
    let principal_teams = PrincipalTeamRegistry::new().bind(SUBJECT, team(TEAM_A));
    ConvexTenancyConfig::new()
        .with_silo_teams(silo_teams)
        .with_principal_teams(principal_teams)
}

/// An engine fixture with all three silos created. The returned fixture must be
/// kept alive by the caller (it owns the data tempdir).
fn engine_with_silos() -> EngineFixture<Engine> {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    fixture.create_tenant(SILO_A1, Engine::create_tenant);
    fixture.create_tenant(SILO_A2, Engine::create_tenant);
    fixture.create_tenant(SILO_B, Engine::create_tenant);
    fixture
}

/// A raw Convex insert mutation into the `tasks` collection (no registered
/// function required; it deserializes into the handler's `Raw` request variant).
fn raw_insert(title: &str) -> serde_json::Value {
    json!({
        "mutation": {
            "type": "insert",
            "table": "tasks",
            "fields": { "title": title }
        }
    })
}

fn tasks_in(engine: &Arc<Engine>, silo: &TenantId) -> Vec<nimbus_core::Document> {
    engine
        .list_documents(silo, &TableName::new("tasks").expect("tasks table id"))
        .expect("listing tasks should succeed")
}

/// Mint a fresh verified `user-a` ES256 bearer token plus the JWKS data URL that
/// verifies it. Both come from one ephemeral keypair, so any registry that must
/// verify this token has to be built from this same JWKS.
fn mint_user_a_token() -> (String, String) {
    issue_es256_test_token(ISSUER, APPLICATION_ID, SUBJECT, json!({}))
}

/// A Convex registry whose `customJwt` provider verifies a `user-a` token minted
/// against `jwks_data_url`. Building the registry from the mint's JWKS is what
/// makes the bearer carry a *verified* `subject`/`issuer`, which the team gate
/// reads (it never trusts unverified claims).
fn registry_verifying(jwks_data_url: &str) -> ConvexRegistry {
    convex_registry_with_routes_and_bundle_and_auth(
        json!([]),
        json!([]),
        None,
        Some(json!({
            "providers": [{
                "type": "customJwt",
                "issuer": ISSUER,
                "jwks": jwks_data_url,
                "algorithm": "ES256",
                "applicationID": APPLICATION_ID
            }]
        })),
    )
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

/// (1) Anonymous — no `Authorization` header — naming a registered silo is
/// refused (403, `PrincipalHasNoTeam`) by the all-fail-closed gate, and nothing
/// is written. Non-vacuous: the silo partition stays empty after the refusal.
#[tokio::test]
async fn convex_anonymous_silo_selection_is_refused() {
    let fixture = engine_with_silos();
    let engine = fixture.engine();
    let server = ServerFixture::start(router_for_convex_with_tenancy(
        engine.clone(),
        convex_registry(json!([])),
        tenancy_config(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url(&format!("/convex/{SILO_A1}/mutation")))
        .json(&raw_insert("anonymous"))
        .send()
        .await
        .expect("anonymous mutation request should send");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "an anonymous caller must not select a silo"
    );
    assert!(
        tasks_in(&engine, &silo(SILO_A1)).is_empty(),
        "a refused anonymous mutation must not write the silo partition"
    );
}

/// (2) Cross-team — a verified `user-a` (team-a) token naming `team-b-silo`
/// (team-b) is refused (403, `CrossTeam`) on BOTH a loopback bind AND a
/// non-loopback bind, and nothing is written to team-b's partition. The
/// non-loopback sub-assert is the proof that the *team binding* — not the removed
/// #43 network-bind stopgap — is what refuses: the verifier is wired so `user-a`
/// is genuinely verified as team-a, yet it still cannot reach team-b's silo from
/// a public bind.
#[tokio::test]
async fn convex_cross_team_silo_selection_is_refused_on_loopback_and_non_loopback() {
    let fixture = engine_with_silos();
    let engine = fixture.engine();
    let (token, jwks) = mint_user_a_token();

    // Loopback bind: router_for_convex_with_tenancy wires the customJwt verifier
    // from the registry, so the bearer is verified as subject `user-a` (team-a).
    let loopback = ServerFixture::start(router_for_convex_with_tenancy(
        engine.clone(),
        registry_verifying(&jwks),
        tenancy_config(),
    ))
    .await;
    let loopback_response = loopback
        .client()
        .post(loopback.http_url(&format!("/convex/{SILO_B}/mutation")))
        .header("Authorization", bearer(&token))
        .json(&raw_insert("cross-team-loopback"))
        .send()
        .await
        .expect("cross-team loopback mutation should send");
    assert_eq!(
        loopback_response.status(),
        StatusCode::FORBIDDEN,
        "a verified team-a principal must not select team-b's silo on a loopback bind"
    );

    // Non-loopback bind. The verifier is wired explicitly (the raw build path does
    // not wire it from the registry), so `user-a` is again genuinely verified as
    // team-a — proving the 403 is the cross-team binding decision, not an
    // unverified-principal refusal and not a blanket network guard.
    let public_addr: SocketAddr = PUBLIC_BIND.parse().expect("public addr should parse");
    let public_registry = registry_verifying(&jwks);
    let verifier = crate::router::convex_application_auth_verifier(&public_registry);
    let public_router = RouterBuildConfig::core(engine.clone())
        .with_application_auth_verifier(verifier)
        .with_convex(public_registry)
        .with_convex_tenancy(tenancy_config())
        .with_listen_addr(public_addr)
        .build();
    let public_server = ServerFixture::start(public_router).await;
    let public_response = public_server
        .client()
        .post(public_server.http_url(&format!("/convex/{SILO_B}/mutation")))
        .header("Authorization", bearer(&token))
        .json(&raw_insert("cross-team-public"))
        .send()
        .await
        .expect("cross-team non-loopback mutation should send");
    assert_eq!(
        public_response.status(),
        StatusCode::FORBIDDEN,
        "a verified team-a principal must not select team-b's silo on a non-loopback bind"
    );

    assert!(
        tasks_in(&engine, &silo(SILO_B)).is_empty(),
        "no cross-team mutation (either bind) may write team-b's silo partition"
    );
}

/// (3) Same-team, same silo — a verified `user-a` (team-a) token naming
/// `team-a-silo-1` (team-a) is admitted (200) and the document lands in that
/// silo's partition. Non-vacuous: the sibling silo stays empty.
#[tokio::test]
async fn convex_same_team_same_silo_mutation_is_admitted_and_lands() {
    let fixture = engine_with_silos();
    let engine = fixture.engine();
    let (token, jwks) = mint_user_a_token();
    let server = ServerFixture::start(router_for_convex_with_tenancy(
        engine.clone(),
        registry_verifying(&jwks),
        tenancy_config(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url(&format!("/convex/{SILO_A1}/mutation")))
        .header("Authorization", bearer(&token))
        .json(&raw_insert("x"))
        .send()
        .await
        .expect("same-team same-silo mutation should send");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a verified team-a principal must reach its own silo"
    );

    let docs = tasks_in(&engine, &silo(SILO_A1));
    assert_eq!(
        docs.len(),
        1,
        "the admitted write must land in team-a-silo-1"
    );
    assert_eq!(docs[0].get_field("title"), Some(&json!("x")));
    assert!(
        tasks_in(&engine, &silo(SILO_A2)).is_empty(),
        "the write must not leak into a sibling silo's partition"
    );
}

/// (4) Same-team, OTHER silo — the non-vacuous many-silos-per-team case: a
/// verified `user-a` (team-a) token naming `team-a-silo-2` (also team-a) is
/// admitted (200) and the document lands in team-a-silo-2's partition. Proves a
/// team principal reaches ANOTHER of its team's silos, each still isolated.
#[tokio::test]
async fn convex_same_team_other_silo_mutation_is_admitted_and_lands() {
    let fixture = engine_with_silos();
    let engine = fixture.engine();
    let (token, jwks) = mint_user_a_token();
    let server = ServerFixture::start(router_for_convex_with_tenancy(
        engine.clone(),
        registry_verifying(&jwks),
        tenancy_config(),
    ))
    .await;

    let response = server
        .client()
        .post(server.http_url(&format!("/convex/{SILO_A2}/mutation")))
        .header("Authorization", bearer(&token))
        .json(&raw_insert("other-silo"))
        .send()
        .await
        .expect("same-team other-silo mutation should send");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "a verified team-a principal must reach another of its team's silos"
    );

    let docs = tasks_in(&engine, &silo(SILO_A2));
    assert_eq!(
        docs.len(),
        1,
        "the admitted write must land in team-a-silo-2"
    );
    assert_eq!(docs[0].get_field("title"), Some(&json!("other-silo")));
    assert!(
        tasks_in(&engine, &silo(SILO_A1)).is_empty(),
        "the write must not leak into the team's other silo"
    );
}

/// (5) Six-route coverage — anonymous selection of `team-a-silo-1` is refused
/// (403) on every Convex application route type: the five POST families
/// (query, paginated query, mutation, action, http action, scheduled mutation)
/// and the WebSocket upgrade. Each POST body deserializes into its route's
/// request type so the request reaches the team-binding gate (which runs before
/// function resolution) rather than failing body extraction first.
#[tokio::test]
async fn convex_all_application_route_types_refuse_anonymous() {
    let fixture = engine_with_silos();
    let engine = fixture.engine();
    let server = ServerFixture::start(router_for_convex_with_tenancy(
        engine,
        convex_registry(json!([])),
        tenancy_config(),
    ))
    .await;

    let named = json!({ "name": "noop", "args": {} });
    let post_cases: [(String, serde_json::Value); 6] = [
        (format!("/convex/{SILO_A1}/query"), named.clone()),
        (
            format!("/convex/{SILO_A1}/query/paginated"),
            json!({ "name": "noop", "args": {}, "page_size": 10 }),
        ),
        (format!("/convex/{SILO_A1}/mutation"), raw_insert("blocked")),
        (format!("/convex/{SILO_A1}/action"), named.clone()),
        (format!("/convex/{SILO_A1}/http"), json!({})),
        (
            format!("/convex/{SILO_A1}/schedule/run_after"),
            json!({ "name": "noop", "args": {}, "run_after_ms": 1000 }),
        ),
    ];
    for (path, body) in &post_cases {
        let response = server
            .client()
            .post(server.http_url(path))
            .json(body)
            .send()
            .await
            .unwrap_or_else(|error| panic!("request to {path} should send: {error}"));
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "anonymous selection of {path} must be refused by the #41 gate"
        );
    }

    // The WebSocket upgrade offers the v2 subprotocol (so negotiation passes) and
    // is then refused at the gate before the upgrade completes: a 403 HTTP
    // rejection rather than a successful handshake.
    let mut ws_request = server
        .ws_url(&format!("/convex/{SILO_A1}/ws"))
        .into_client_request()
        .expect("convex ws request should build");
    ws_request.headers_mut().insert(
        header::SEC_WEBSOCKET_PROTOCOL,
        HeaderValue::from_static("nimbus.v2"),
    );
    let ws_error = connect_async(ws_request)
        .await
        .expect_err("anonymous convex ws upgrade must be refused by the #41 gate");
    let TungsteniteError::Http(ws_response) = ws_error else {
        panic!("expected an HTTP websocket rejection, got {ws_error:?}");
    };
    assert_eq!(
        ws_response.status(),
        StatusCode::FORBIDDEN,
        "anonymous convex ws selection must be refused by the #41 gate"
    );
}
