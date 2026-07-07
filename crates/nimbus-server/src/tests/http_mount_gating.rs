//! SR4: `adapters::http_mount::mount_adapters` gates each HTTP-mounted
//! protocol surface (Firebase, Cloudflare, the Cloud Functions fallback) on
//! its own `enabled()`. These tests drive the real `RouterBuildConfig::build`
//! composition root end to end — proving the gating survives the seam, not
//! just the generic mounting mechanism already covered by the stub-based
//! unit tests in `adapters::http_mount::tests`. Convex mounts unconditionally
//! and is smoke-tested continuously by `rest_route_parity` and the broader
//! convex test suites, so it is not repeated here.

use super::*;

const FIREBASE_COMMIT_ROUTE: &str = "/v1/projects/demo/databases/(default)/documents:commit";
const CLOUDFLARE_KV_VALUE_ROUTE: &str =
    "/client/v4/accounts/acct/storage/kv/namespaces/ns/values/some-key";

#[tokio::test]
async fn firebase_route_is_absent_without_firebase_config() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;

    let response = server
        .client()
        .post(server.http_url(FIREBASE_COMMIT_ROUTE))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .expect("firebase-absent probe should send");

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "firebase route must not be mounted when no FirebaseConfig is configured"
    );
}

#[tokio::test]
async fn firebase_route_is_mounted_with_firebase_config() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server =
        ServerFixture::start(router_for_firebase(fixture.engine(), FirebaseConfig::new())).await;

    let response = server
        .client()
        .post(server.http_url(FIREBASE_COMMIT_ROUTE))
        .header("content-type", "application/json")
        .body("{}")
        .send()
        .await
        .expect("firebase-present probe should send");

    // No bearer was supplied, so the route-layer auth/CORS gate refuses the
    // request once the route matches — any non-404 failure mode is proof the
    // route is mounted (the exact rejection status is auth-mechanics
    // territory covered by the firebase auth test suite, not this seam).
    assert_ne!(
        response.status(),
        StatusCode::NOT_FOUND,
        "firebase route must be mounted when a FirebaseConfig is configured"
    );
}

#[tokio::test]
async fn cloudflare_kv_route_is_absent_without_cloudflare_config() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;

    let response = server
        .client()
        .get(server.http_url(CLOUDFLARE_KV_VALUE_ROUTE))
        .send()
        .await
        .expect("cloudflare-absent probe should send");

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "cloudflare kv route must not be mounted when no CloudflareConfig is configured"
    );
}

#[tokio::test]
async fn cloudflare_kv_route_is_mounted_with_cloudflare_config() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.engine())
            .with_cloudflare(CloudflareConfig::default())
            .build(),
    )
    .await;

    let response = server
        .client()
        .get(server.http_url(CLOUDFLARE_KV_VALUE_ROUTE))
        .send()
        .await
        .expect("cloudflare-present probe should send");

    // No Authorization header was supplied, so the route's own auth check
    // refuses it (401) once the route matches — proof the route is mounted.
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "cloudflare kv route must be mounted (and auth-gated) when a CloudflareConfig is configured"
    );
}

#[tokio::test]
async fn cloud_functions_fallback_does_not_swallow_unmatched_routes_when_absent() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;

    let response = server
        .client()
        .get(server.http_url("/this-path-matches-no-adapter-route"))
        .send()
        .await
        .expect("no-fallback probe should send");

    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "an unmatched path must 404 (not fall through to a handler) when no cloud \
         functions registry is configured"
    );
}
