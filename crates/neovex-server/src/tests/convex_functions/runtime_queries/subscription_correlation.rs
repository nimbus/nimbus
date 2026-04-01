use super::super::super::*;

#[tokio::test]
async fn convex_websocket_runtime_subscription_uses_server_generated_request_correlation_ids() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:maybeByAuthor",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => { if (author) { return await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20); } return await ctx.db.query(\"messages\").take(20); }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:maybeByAuthor", {
    name: "messages:maybeByAuthor",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author }) => { if (author) { return await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20); } return await ctx.db.query(\"messages\").take(20); }",
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
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
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
    );
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
    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Tracked Ada" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "convex-runtime-correlation",
            "messages:maybeByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-correlation"));
    assert_eq!(initial["data"][0]["body"], json!("Tracked Ada"));

    let bootstrap_metrics = wait_for_runtime_metrics(
        &registry,
        "websocket runtime subscription bootstrap correlation",
        |metrics| {
            metrics
                .recent_request_correlations
                .iter()
                .any(|correlation| {
                    correlation.function_name == "messages:maybeByAuthor"
                        && correlation
                            .server_request_id
                            .starts_with("convex-ws-subscription-bootstrap-")
                })
        },
    )
    .await;
    assert!(
        bootstrap_metrics
            .recent_request_correlations
            .iter()
            .all(|correlation| correlation.server_request_id != "convex-runtime-correlation")
    );

    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Second Ada" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert!(pushed.get("request_id").is_none());
    assert_eq!(pushed["data"].as_array().map(Vec::len), Some(2));

    let reeval_metrics = wait_for_runtime_metrics(
        &registry,
        "websocket runtime subscription reevaluation correlation",
        |metrics| {
            metrics
                .recent_request_correlations
                .iter()
                .any(|correlation| {
                    correlation.function_name == "messages:maybeByAuthor"
                        && correlation
                            .server_request_id
                            .starts_with("convex-ws-subscription-reeval-")
                })
        },
    )
    .await;
    let runtime_correlations: Vec<_> = reeval_metrics
        .recent_request_correlations
        .iter()
        .filter(|correlation| correlation.function_name == "messages:maybeByAuthor")
        .collect();
    assert!(runtime_correlations.iter().any(|correlation| {
        correlation
            .server_request_id
            .starts_with("convex-ws-subscription-bootstrap-")
    }));
    assert!(runtime_correlations.iter().any(|correlation| {
        correlation
            .server_request_id
            .starts_with("convex-ws-subscription-reeval-")
    }));
    assert!(
        runtime_correlations
            .iter()
            .all(|correlation| { correlation.server_request_id != "convex-runtime-correlation" })
    );
}
