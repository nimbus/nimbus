use super::*;
use neovex_runtime::RuntimeLimits;

#[tokio::test]
async fn convex_runtime_nested_query_subscription_tracks_inner_runtime_reads() {
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
    globalThis.__neovexCreateContext({
      sessionId: `${request.kind}:${request.function_name}`,
    }),
    request.args ?? {},
    request,
  );
}

globalThis.__neovexInvoke = async function(request) {
  try {
    return {
      status: "ok",
      value: await invokeLocal(request),
    };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

globalThis.__neovexInvokeNamedLocal = invokeLocal;

export {};
"#,
        ),
    )
    .with_runtime_limits(RuntimeLimits {
        max_concurrent_isolates: 1,
        ..RuntimeLimits::default()
    });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

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

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
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

    let metrics = registry.runtime_metrics_snapshot();
    assert_eq!(
        metrics.isolate_pool_hits + metrics.isolate_pool_misses,
        2,
        "bootstrap plus one reactive reevaluation should account for two pool outcomes"
    );
    assert_eq!(metrics.isolate_pool_replacements, 0);
}
