use super::*;

#[tokio::test]
async fn convex_runtime_nested_query_subscription_tracks_inner_runtime_reads() {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "messages:inner",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20)"
            },
            {
                "name": "messages:outer",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => ({ runtime: true, value: await ctx.runQuery({ name: \"messages:inner\", visibility: \"public\" }, { author }) })"
            }
        ]),
        Some(
            r#"
const definitions = new Map([
  ["messages:inner", {
    name: "messages:inner",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author }) => await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20)",
  }],
  ["messages:outer", {
    name: "messages:outer",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author }) => ({ runtime: true, value: await ctx.runQuery({ name: \"messages:inner\", visibility: \"public\" }, { author }) })",
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

async function invokeLocal(request) {
  const handler = handlers.get(request.function_name);
  return await handler(
    globalThis.__nimbusCreateContext({
      hostCallSessionId: request.hostCallSessionId ?? `${request.kind}:${request.function_name}`,
      request,
    }),
    request.args ?? {},
    request,
  );
}

globalThis.__nimbusInvoke = async function(request) {
  try {
    return {
      status: "ok",
      value: await invokeLocal(request),
    };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
    }
    throw error;
  }
};

globalThis.__nimbusInvokeNamedLocal = invokeLocal;

export {};
"#,
        ),
    )
    .with_runtime_limits(limits);
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry.clone())).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert!(api.create_tenant("demo").await.status().is_success());
    assert!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Tracked Ada" }),
        )
        .await
        .status()
        .is_success()
    );

    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    let mut socket = WebSocketFixture::connect_raw_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        &convex_team_bearer(),
    )
    .await
    .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "convex-runtime-nested",
            "messages:outer",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-nested"));
    assert_eq!(initial["data"]["runtime"], json!(true));
    assert_eq!(initial["data"]["value"][0]["body"], json!("Tracked Ada"));

    assert!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Bob", "body": "Ignored Bob" }),
        )
        .await
        .status()
        .is_success()
    );

    let maybe_update = socket
        .next_json_with_timeout(Duration::from_millis(200))
        .await;
    assert!(
        maybe_update.is_none(),
        "nested runtime subscription should stay idle for non-matching writes"
    );

    assert!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Second Ada" }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    let data = pushed["data"]["value"]
        .as_array()
        .expect("nested runtime filtered data should be an array");
    assert_eq!(data.len(), 2);
    assert!(
        data.iter()
            .all(|document| document["author"] == json!("Ada"))
    );

    let metrics_body = wait_for_value(
        "nested runtime subscription runtime pool outcomes",
        Duration::from_secs(3),
        Duration::from_millis(25),
        || async {
            api.runtime_metrics()
                .await
                .json::<serde_json::Value>()
                .await
                .expect("runtime metrics response should parse")
        },
        |body| runtime_pool_outcomes(body) == 2,
    )
    .await;
    assert_eq!(
        runtime_pool_outcomes(&metrics_body),
        2,
        "bootstrap plus one reactive reevaluation should account for two pool outcomes"
    );
    assert_eq!(
        runtime_metric_u64(&metrics_body, "runtime_pool_replacements"),
        0
    );
}

fn runtime_pool_outcomes(metrics_body: &serde_json::Value) -> u64 {
    runtime_metric_u64(metrics_body, "runtime_pool_hits")
        + runtime_metric_u64(metrics_body, "runtime_pool_misses")
}

fn runtime_metric_u64(metrics_body: &serde_json::Value, key: &str) -> u64 {
    metrics_body["metrics"][key].as_u64().unwrap_or(0)
}
