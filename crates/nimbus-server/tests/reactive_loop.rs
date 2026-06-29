use std::fs;
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use base64::Engine as _;
use nimbus_engine::{Engine, run_scheduler};
use nimbus_runtime::RuntimeBundle;
use nimbus_server::{
    ConvexRegistry, ConvexTenancyConfig, PrincipalTeamRegistry, RouterOptions, SiloTeamRegistry,
    TeamId, build_router,
};
use nimbus_testing::{
    BlockingFaultInjector, DeterministicHarness, EngineFixture, HttpApiFixture, ScenarioMetadata,
    ServerFixture, WebSocketFixture, run_to_completion_snapshot_runtime_test_limits,
    wait_for_value,
};
use reqwest::StatusCode;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_FIXED_SIGNING, EcdsaKeyPair, KeyPair};
use serde_json::json;
use tempfile::tempdir;
use tokio::sync::watch;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Error as WebSocketError;

// ---------------------------------------------------------------------------
// #41 application-Convex team binding (public-API test target).
//
// The reactive_loop tests drive `/convex/demo/…` end-to-end, so under the #41
// gate every request must arrive as a *verified* principal bound to the team
// that owns `demo`. This target sees only the public surface (no crate-internal
// static verifier), so it provisions the real path: every registry carries a
// `customJwt` provider whose JWKS verifies a single shared ES256 bearer, the
// router binds `demo` and that bearer's subject to one team, and the fixtures
// carry the bearer. Anonymous requests resolve to no team and are refused — see
// `assert_convex_anonymous_query_refused`.
// ---------------------------------------------------------------------------

const CONVEX_TEAM_TENANT: &str = "demo";
const CONVEX_TEAM: &str = "team-demo";
const CONVEX_TEAM_SUBJECT: &str = "reactive-loop-user";
const CONVEX_TEAM_ISSUER: &str = "https://reactive-loop.convex.test";
const CONVEX_TEAM_APPLICATION_ID: &str = "nimbus-reactive-loop";

/// The process-wide shared verified bearer and the JWKS that verifies it. Minted
/// once so every registry's `customJwt` provider and every request agree on one
/// keypair.
fn convex_team_token_and_jwks() -> &'static (String, String) {
    static TOKEN: OnceLock<(String, String)> = OnceLock::new();
    TOKEN.get_or_init(|| {
        issue_es256_team_token(
            CONVEX_TEAM_ISSUER,
            CONVEX_TEAM_APPLICATION_ID,
            CONVEX_TEAM_SUBJECT,
        )
    })
}

/// The `Authorization` header value carrying the shared verified bearer.
fn convex_team_bearer() -> String {
    format!("Bearer {}", convex_team_token_and_jwks().0)
}

/// The #41 tenancy binding `demo`→team and the shared bearer's subject→team.
fn convex_team_tenancy() -> ConvexTenancyConfig {
    let silo = nimbus_core::TenantId::new(CONVEX_TEAM_TENANT).expect("silo tenant id");
    let team = TeamId::new(CONVEX_TEAM).expect("team id");
    ConvexTenancyConfig::new()
        .with_silo_teams(SiloTeamRegistry::new().bind(&silo, team.clone()))
        .with_principal_teams(PrincipalTeamRegistry::new().bind(CONVEX_TEAM_SUBJECT, team))
}

/// The `customJwt` auth-config provider for the shared JWKS, injected into every
/// registry so the deployment's derived verifier admits the shared bearer.
fn convex_team_auth_config() -> serde_json::Value {
    let (_, jwks_data_url) = convex_team_token_and_jwks();
    json!({
        "providers": [
            {
                "type": "customJwt",
                "issuer": CONVEX_TEAM_ISSUER,
                "jwks": jwks_data_url,
                "algorithm": "ES256",
                "applicationID": CONVEX_TEAM_APPLICATION_ID
            }
        ]
    })
}

/// Mint an ES256 JWT and the JWKS data URL that verifies it (one ephemeral
/// keypair), mirroring the lib tests' `issue_es256_test_token`.
fn issue_es256_team_token(issuer: &str, application_id: &str, subject: &str) -> (String, String) {
    let rng = SystemRandom::new();
    let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng)
        .expect("test key should generate");
    let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
        .expect("test key should parse");
    let header = json!({ "alg": "ES256", "kid": "test-key", "typ": "JWT" });
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .expect("clock after epoch")
        .as_secs();
    let claims = json!({
        "iss": issuer,
        "sub": subject,
        "aud": application_id,
        "exp": now + 300,
        "iat": now,
    });
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header_segment = b64.encode(serde_json::to_vec(&header).expect("jwt header"));
    let claims_segment = b64.encode(serde_json::to_vec(&claims).expect("jwt claims"));
    let signing_input = format!("{header_segment}.{claims_segment}");
    let signature = key_pair
        .sign(&rng, signing_input.as_bytes())
        .expect("jwt signature");
    let token = format!("{signing_input}.{}", b64.encode(signature.as_ref()));
    let public_key = key_pair.public_key().as_ref();
    let jwks = json!({
        "keys": [{
            "kid": "test-key",
            "kty": "EC",
            "crv": "P-256",
            "x": b64.encode(&public_key[1..33]),
            "y": b64.encode(&public_key[33..65]),
            "alg": "ES256",
            "use": "sig"
        }]
    });
    let jwks_data_url = format!(
        "data:application/json;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&jwks).expect("jwks"))
    );
    (token, jwks_data_url)
}

/// The non-vacuous refusal half of a migrated reactive_loop WS test: an anonymous
/// Convex WS upgrade for `tenant` is refused with HTTP 403 at the gate.
async fn assert_convex_anonymous_ws_refused(server: &ServerFixture, tenant: &str) {
    let error = match WebSocketFixture::connect_raw(&server.ws_url(&format!("/convex/{tenant}/ws")))
        .await
    {
        Ok(_) => panic!("anonymous convex ws upgrade must be refused by the #41 gate"),
        Err(error) => error,
    };
    match error {
        WebSocketError::Http(response) => assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "anonymous convex ws selection of `{tenant}` must be refused (#41 gate)"
        ),
        other => panic!("expected an HTTP 403 websocket rejection, got {other:?}"),
    }
}

fn convex_registry(functions: serde_json::Value) -> ConvexRegistry {
    convex_registry_with_bundle(functions, None)
}

fn router_for_engine(engine: Arc<Engine>) -> axum::Router {
    build_router(RouterOptions::new(engine))
}

fn router_for_convex(engine: Arc<Engine>, convex_registry: ConvexRegistry) -> axum::Router {
    build_router(
        RouterOptions::new(engine)
            .with_convex_registry(convex_registry)
            .with_convex_tenancy(convex_team_tenancy()),
    )
}

fn convex_registry_with_bundle(
    functions: serde_json::Value,
    bundle: Option<&str>,
) -> ConvexRegistry {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": functions }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    // #41: every registry carries the shared customJwt provider so the derived
    // verifier admits the shared team bearer.
    fs::write(
        convex_dir.join("auth.config.json"),
        serde_json::to_vec_pretty(&convex_team_auth_config())
            .expect("convex auth config json should serialize"),
    )
    .expect("convex auth config should write");
    if let Some(bundle) = bundle {
        let bundle_path = convex_dir.join("bundle.mjs");
        fs::write(&bundle_path, bundle).expect("convex runtime bundle should write");
        let bundle_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        fs::write(
            bundle_path.with_extension("sha256"),
            format!("{bundle_sha256}\n"),
        )
        .expect("convex runtime bundle hash should write");
    }
    let registry = ConvexRegistry::from_app_dir(tempdir.path())
        .expect("convex registry should load")
        .with_runtime_limits(run_to_completion_snapshot_runtime_test_limits());
    std::mem::forget(tempdir);
    registry
}

async fn wait_for_active_subscription_count(
    service: &std::sync::Arc<Engine>,
    tenant_id: &nimbus_core::TenantId,
    description: &str,
    expected_count: usize,
) -> usize {
    wait_for_value(
        description,
        Duration::from_secs(2),
        Duration::ZERO,
        || async {
            service
                .active_subscription_count(tenant_id)
                .expect("subscription count should load")
        },
        |count| *count == expected_count,
    )
    .await
}

#[path = "reactive_loop/manifest/mod.rs"]
mod manifest;
#[path = "reactive_loop/runtime_paginated/mod.rs"]
mod runtime_paginated;
#[path = "reactive_loop/runtime_queries.rs"]
mod runtime_queries;
#[path = "reactive_loop/socket/mod.rs"]
mod socket;
#[path = "reactive_loop/transport/mod.rs"]
mod transport;
