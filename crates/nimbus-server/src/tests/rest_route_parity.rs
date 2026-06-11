//! LR7 server half of `rest_client_route_parity`: every route in
//! `packages/nimbus/src/native_rest_routes.json` must be served by the
//! router with the manifest verb. A bare router 404 (no matching path —
//! axum's default empty body, unlike handler 404s which carry a JSON
//! error envelope) or a 405 (path matched, verb drifted) fails the probe.
//! The JS half (`rest_client_route_parity.mjs`) holds the client to the
//! same manifest, so a route changed on one side alone always fails.

use super::*;

#[tokio::test]
async fn rest_client_route_parity() {
    let manifest_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../packages/nimbus/src/native_rest_routes.json");
    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&manifest_path).expect("route manifest should read"),
    )
    .expect("route manifest should parse");
    let routes = manifest["routes"]
        .as_object()
        .expect("manifest should carry a routes object");
    assert!(
        routes.len() >= 20,
        "route manifest looks truncated: {} routes",
        routes.len()
    );

    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;

    for (name, route) in routes {
        let verb = route["verb"]
            .as_str()
            .expect("route verb should be a string");
        let path = route["path"]
            .as_str()
            .expect("route path should be a string")
            .replace("{tenant_id}", "parity-tenant")
            .replace("{table}", "notes")
            .replace("{document_id}", "doc-1")
            .replace("{job_id}", "job-1")
            .replace("{name}", "daily");
        let method =
            reqwest::Method::from_bytes(verb.as_bytes()).expect("manifest verb should parse");
        let mut request = server.client().request(method, server.http_url(&path));
        if matches!(verb, "POST" | "PUT" | "PATCH") {
            request = request
                .header("content-type", "application/json")
                .body("{}");
        }
        let response = request.send().await.expect("parity probe should send");
        let status = response.status();
        assert_ne!(
            status,
            reqwest::StatusCode::METHOD_NOT_ALLOWED,
            "{name}: router serves {path} but not with {verb} — verb drift"
        );
        if status == reqwest::StatusCode::NOT_FOUND {
            let body = response.text().await.expect("probe body should read");
            assert!(
                !body.is_empty(),
                "{name}: bare 404 for {verb} {path} — route missing from the router"
            );
        }
    }
}
