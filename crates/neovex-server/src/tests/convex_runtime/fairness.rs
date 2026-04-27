use neovex_engine::Service;
use neovex_testing::{
    DeterministicTestCase, HttpApiFixture, ServerFixture, ServiceFixture, WebSocketFixture,
    bounded_fairness_runtime_test_limits,
};
use reqwest::StatusCode;
use serde_json::json;
use tokio::net::TcpStream;

use crate::tests::{
    convex_registry_with_routes_and_bundle, open_json_post_stream, wait_for_runtime_metrics,
    wait_for_runtime_metrics_case,
};
use crate::{ConvexRegistry, build_router_with_convex};

pub(crate) const FAIRNESS_HTTP_REJECTION_CASE: DeterministicTestCase = DeterministicTestCase::new(
    "runtime-tenant-fairness-http-rejection",
    "bounded-fairness",
    "bounded fairness pressure rejects extra HTTP work without losing runtime cleanup accounting",
);

pub(crate) const FAIRNESS_WEBSOCKET_REJECTION_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "runtime-tenant-fairness-websocket-rejection",
        "bounded-fairness",
        "bounded fairness pressure rejects websocket bootstrap work with stable queue-limit signaling",
    );

fn fairness_runtime_registry() -> ConvexRegistry {
    let mut limits = bounded_fairness_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
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
    .with_runtime_limits(limits)
}

async fn cleanup_fairness_blockers(
    registry: &ConvexRegistry,
    blocker: TcpStream,
    queued: TcpStream,
) {
    drop(queued);
    drop(blocker);
    let _ = wait_for_runtime_metrics(registry, "tenant fairness cleanup", |metrics| {
        metrics.active_runtime_instances == 0 && metrics.canceled_invocations >= 2
    })
    .await;
}

#[tokio::test]
async fn convex_runtime_http_rejections_return_too_many_requests() {
    convex_runtime_http_rejections_return_too_many_requests_inner().await;
}

pub(crate) async fn convex_runtime_http_rejections_return_too_many_requests_inner() {
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
    wait_for_runtime_metrics_case(
        &registry,
        FAIRNESS_HTTP_REJECTION_CASE,
        "blocking fairness runtime query to start",
        |metrics| {
            metrics.active_runtime_instances == 1 && metrics.worker_dispatched_invocations == 1
        },
    )
    .await;

    let queued = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics_case(
        &registry,
        FAIRNESS_HTTP_REJECTION_CASE,
        "queued fairness runtime query to be observed by the runtime queue",
        |metrics| {
            metrics.active_runtime_instances == 1
                && metrics
                    .recent_request_correlations
                    .iter()
                    .filter(|correlation| correlation.function_name == "messages:block")
                    .count()
                    >= 2
        },
    )
    .await;

    let response = api
        .convex_named_query("demo", "messages:list", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("fairness rejection response should parse");
    assert!(
        body["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("tenant queue limit exceeded for demo")),
        "expected queue-limit rejection message, got {body}"
    );

    let metrics = wait_for_runtime_metrics_case(
        &registry,
        FAIRNESS_HTTP_REJECTION_CASE,
        "tenant fairness rejection metrics",
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

#[tokio::test]
async fn convex_runtime_websocket_bootstrap_rejections_send_error_frames() {
    convex_runtime_websocket_bootstrap_rejections_send_error_frames_inner().await;
}

pub(crate) async fn convex_runtime_websocket_bootstrap_rejections_send_error_frames_inner() {
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
    wait_for_runtime_metrics_case(
        &registry,
        FAIRNESS_WEBSOCKET_REJECTION_CASE,
        "blocking fairness websocket query to start",
        |metrics| {
            metrics.active_runtime_instances == 1 && metrics.worker_dispatched_invocations == 1
        },
    )
    .await;

    let queued = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics_case(
        &registry,
        FAIRNESS_WEBSOCKET_REJECTION_CASE,
        "queued fairness websocket query to be observed by the runtime queue",
        |metrics| {
            metrics.active_runtime_instances == 1
                && metrics
                    .recent_request_correlations
                    .iter()
                    .filter(|correlation| correlation.function_name == "messages:block")
                    .count()
                    >= 2
        },
    )
    .await;

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("tenant-fairness", "messages:list", json!({}))
        .await;

    let message = socket.next_json().await;
    assert_eq!(message["type"], json!("op.error"));
    assert_eq!(message["id"], json!("tenant-fairness"));
    assert!(
        message["error"]["message"]
            .as_str()
            .is_some_and(|text| text.contains("tenant queue limit exceeded for demo")),
        "expected queue-limit websocket error, got {message}"
    );

    let metrics = wait_for_runtime_metrics_case(
        &registry,
        FAIRNESS_WEBSOCKET_REJECTION_CASE,
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
