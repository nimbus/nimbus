use std::time::Duration;

use neovex_engine::Service;
use neovex_runtime::RuntimeLimits;
use neovex_test_support::{HttpApiFixture, ServerFixture, ServiceFixture, WebSocketFixture};
use reqwest::StatusCode;
use serde_json::json;
use tokio::net::TcpStream;

use crate::tests::{
    convex_registry_with_routes_and_bundle, open_json_post_stream, wait_for_runtime_metrics,
};
use crate::{ConvexRegistry, build_router_with_convex};

fn fairness_runtime_registry() -> ConvexRegistry {
    convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:block",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async () => { while (true) {} }"
            },
            {
                "name": "messages:list",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async () => []"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:block", {
    name: "messages:block",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async () => { while (true) {} }",
  }],
  ["messages:list", {
    name: "messages:list",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async () => []",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    return {
      status: "error",
      error: { kind: "internal", message: `missing definition for ${request.function_name}` },
    };
  }

  try {
    const value = await handlers.get(request.function_name)(
      globalThis.__neovexCreateContext(),
      request.args ?? {},
      request,
    );
    return { status: "ok", value };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    )
    .with_runtime_limits(RuntimeLimits {
        max_concurrent_isolates: 1,
        max_active_top_level_invocations_per_tenant: 1,
        max_in_flight_top_level_invocations_per_tenant: 1,
        max_queued_top_level_invocations_per_tenant: 1,
        ..RuntimeLimits::default()
    })
}

async fn cleanup_fairness_blockers(
    registry: &ConvexRegistry,
    blocker: TcpStream,
    queued: TcpStream,
) {
    drop(queued);
    drop(blocker);
    let _ = wait_for_runtime_metrics(registry, "tenant fairness cleanup", |metrics| {
        metrics.active_isolates == 0 && metrics.canceled_invocations >= 2
    })
    .await;
}

#[tokio::test]
async fn convex_runtime_http_rejections_return_too_many_requests() {
    let registry = fairness_runtime_registry();
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let blocker = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics(
        &registry,
        "blocking fairness runtime query to start",
        |metrics| metrics.active_isolates == 1 && metrics.worker_dispatched_invocations == 1,
    )
    .await;

    let queued = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let response = api
        .convex_named_query("demo", "messages:list", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("fairness rejection response should parse");
    assert!(
        body["error"]
            .as_str()
            .is_some_and(|message| message.contains("tenant queue limit exceeded for demo")),
        "expected queue-limit rejection message, got {body}"
    );

    let metrics =
        wait_for_runtime_metrics(&registry, "tenant fairness rejection metrics", |metrics| {
            metrics.rejected_invocations == 1
        })
        .await;
    assert_eq!(
        metrics
            .tenants
            .get("demo")
            .expect("tenant metrics should be present")
            .rejected_invocations,
        1
    );

    cleanup_fairness_blockers(&registry, blocker, queued).await;
}

#[tokio::test]
async fn convex_runtime_websocket_bootstrap_rejections_send_error_frames() {
    let registry = fairness_runtime_registry();
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let blocker = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics(
        &registry,
        "blocking fairness websocket query to start",
        |metrics| metrics.active_isolates == 1 && metrics.worker_dispatched_invocations == 1,
    )
    .await;

    let queued = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("tenant-fairness", "messages:list", json!({}))
        .await;

    let message = socket.next_json().await;
    assert_eq!(message["type"], json!("error"));
    assert_eq!(message["request_id"], json!("tenant-fairness"));
    assert!(
        message["message"]
            .as_str()
            .is_some_and(|text| text.contains("tenant queue limit exceeded for demo")),
        "expected queue-limit websocket error, got {message}"
    );

    let metrics = wait_for_runtime_metrics(
        &registry,
        "tenant fairness websocket rejection metrics",
        |metrics| metrics.rejected_invocations == 1,
    )
    .await;
    assert_eq!(
        metrics
            .tenants
            .get("demo")
            .expect("tenant metrics should be present")
            .rejected_invocations,
        1
    );

    cleanup_fairness_blockers(&registry, blocker, queued).await;
}
