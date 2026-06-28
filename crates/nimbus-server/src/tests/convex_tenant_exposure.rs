//! #41 — convex application-surface tenant exposure: proof-of-hole + the
//! fork-independent fail-closed network-bind stopgap.
//!
//! The convex application routes (`/convex/{tenant_id}/…`) select the tenant from
//! the caller-supplied URL with no verified principal→tenant binding, so an
//! unverified caller can reach an arbitrary tenant's data partition (#41). These
//! tests (1) anchor the hole in an observed run — anonymous names another tenant,
//! runs a *mutation*, and writes that tenant's partition — and (2) prove the
//! stopgap refuses the whole convex surface on a non-loopback bind, across all
//! six route types.

use std::net::SocketAddr;

use super::*;

const PUBLIC_BIND: &str = "203.0.113.5:8080";

/// A raw convex insert mutation targeting the `tasks` collection.
fn cross_tenant_insert() -> serde_json::Value {
    json!({
        "mutation": {
            "type": "insert",
            "table": "tasks",
            "fields": { "title": "cross-tenant write" }
        }
    })
}

fn tenant_b_tasks(engine: &Arc<Engine>, tenant_b: &TenantId) -> Vec<nimbus_core::Document> {
    engine
        .list_documents(tenant_b, &TableName::new("tasks").expect("tasks table id"))
        .expect("listing tenant-b tasks should succeed")
}

/// PROOF OF HOLE (loopback): an anonymous request names `tenant-b` in the URL,
/// runs an insert mutation, and it SUCCEEDS against tenant-b's data partition —
/// with no verified binding to tenant-b. This is the #41 hole, observed.
///
/// The stopgap deliberately leaves loopback working (it is network-unreachable;
/// the complete fork-resolved fix closes it everywhere). So on loopback this
/// still succeeds — that is the anchor the non-loopback stopgap test flips.
#[tokio::test]
async fn convex_anonymous_cross_tenant_mutation_succeeds_on_loopback_proof_of_hole() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let _tenant_a = fixture.create_tenant("tenant-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("tenant-b", Engine::create_tenant);
    let engine = fixture.engine();
    let server = ServerFixture::start(router_for_convex(
        engine.clone(),
        convex_registry(json!([])),
    ))
    .await;

    // No Authorization header: a fully anonymous caller, naming tenant-b.
    let response = server
        .client()
        .post(server.http_url("/convex/tenant-b/mutation"))
        .json(&cross_tenant_insert())
        .send()
        .await
        .expect("convex mutation request should send");
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "PROOF OF HOLE: anonymous cross-tenant mutation is admitted (loopback)"
    );

    // The write actually landed in tenant-b's partition — partition selected by
    // an unverified caller.
    let docs = tenant_b_tasks(&engine, &tenant_b);
    assert_eq!(docs.len(), 1, "the anonymous write reached tenant-b's data");
    assert_eq!(
        docs[0].get_field("title"),
        Some(&json!("cross-tenant write"))
    );
}

/// STOPGAP (non-loopback): the SAME anonymous cross-tenant mutation is REFUSED
/// (403) by the #41 network-bind guard, and NOTHING is written. Closed on the
/// run, through the served path.
#[tokio::test]
async fn convex_cross_tenant_mutation_refused_on_non_loopback_bind() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let _tenant_a = fixture.create_tenant("tenant-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("tenant-b", Engine::create_tenant);
    let engine = fixture.engine();
    let public_addr: SocketAddr = PUBLIC_BIND.parse().expect("public addr should parse");
    let router = RouterBuildConfig::core(engine.clone())
        .with_convex(convex_registry(json!([])))
        .with_listen_addr(public_addr)
        .build();
    let server = ServerFixture::start(router).await;

    let response = server
        .client()
        .post(server.http_url("/convex/tenant-b/mutation"))
        .json(&cross_tenant_insert())
        .send()
        .await
        .expect("convex mutation request should send");
    assert_eq!(
        response.status(),
        StatusCode::FORBIDDEN,
        "the #41 stopgap must refuse the convex surface on a non-loopback bind"
    );

    // Refused before execution: nothing written to tenant-b.
    assert!(
        tenant_b_tasks(&engine, &tenant_b).is_empty(),
        "a refused request must not write tenant-b's partition"
    );
}

/// COVERAGE: every one of the six convex application route types
/// (query, mutation, action, http, ws, schedule) is refused on a non-loopback
/// bind — the guard is a route-layer over the whole convex router, so a single
/// unflipped route type cannot survive in a corner.
#[tokio::test]
async fn convex_all_route_types_refused_on_non_loopback_bind() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let _tenant_a = fixture.create_tenant("tenant-a", Engine::create_tenant);
    let _tenant_b = fixture.create_tenant("tenant-b", Engine::create_tenant);
    let engine = fixture.engine();
    let public_addr: SocketAddr = PUBLIC_BIND.parse().expect("public addr should parse");
    let router = RouterBuildConfig::core(engine)
        .with_convex(convex_registry(json!([])))
        .with_listen_addr(public_addr)
        .build();
    let server = ServerFixture::start(router).await;

    // POST route types: query (+ paginated), mutation, action, http action,
    // schedule. The guard runs before body parsing, so an empty body still 403s.
    let post_paths = [
        "/convex/tenant-b/query",
        "/convex/tenant-b/query/paginated",
        "/convex/tenant-b/mutation",
        "/convex/tenant-b/action",
        "/convex/tenant-b/http",
        "/convex/tenant-b/schedule/run_after",
    ];
    for path in post_paths {
        let response = server
            .client()
            .post(server.http_url(path))
            .json(&json!({}))
            .send()
            .await
            .unwrap_or_else(|error| panic!("request to {path} should send: {error}"));
        assert_eq!(
            response.status(),
            StatusCode::FORBIDDEN,
            "convex route {path} must be refused on a non-loopback bind"
        );
    }

    // ws is a GET (websocket upgrade); the guard runs before the upgrade.
    let ws_response = server
        .client()
        .get(server.http_url("/convex/tenant-b/ws"))
        .send()
        .await
        .expect("ws request should send");
    assert_eq!(
        ws_response.status(),
        StatusCode::FORBIDDEN,
        "convex ws route must be refused on a non-loopback bind"
    );
}

/// The stopgap is scoped to the convex application surface: a non-convex public
/// route (the readiness check) is unaffected by a non-loopback bind, so the guard
/// did not over-reach into the rest of the server.
#[tokio::test]
async fn non_convex_routes_unaffected_by_the_convex_guard_on_non_loopback_bind() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let public_addr: SocketAddr = PUBLIC_BIND.parse().expect("public addr should parse");
    let router = RouterBuildConfig::core(engine)
        .with_convex(convex_registry(json!([])))
        .with_listen_addr(public_addr)
        .build();
    let server = ServerFixture::start(router).await;

    let response = server
        .client()
        .get(server.http_url("/health"))
        .send()
        .await
        .expect("health request should send");
    assert_ne!(
        response.status(),
        StatusCode::FORBIDDEN,
        "the convex guard must not refuse non-convex routes"
    );
}
