use super::super::super::*;

#[tokio::test]
async fn convex_runtime_only_query_can_run_runtime_only_query() {
    let registry = convex_registry_with_routes_and_bundle(
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
                "runtime_handler": "async (ctx, { author }) => await ctx.runQuery({ name: \"messages:inner\", visibility: \"public\" }, { author })"
            }
        ]),
        json!([]),
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
    runtime_handler: "async (ctx, { author }) => await ctx.runQuery({ name: \"messages:inner\", visibility: \"public\" }, { author })",
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
    let registry_for_router = registry.clone();
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry_for_router,
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for (author, body) in [("Ada", "Hello"), ("Ada", "Again"), ("Bob", "Ignored")] {
        assert_eq!(
            api.insert_document(
                "demo",
                "messages",
                json!({ "author": author, "body": body })
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }

    let response = api
        .convex_named_query("demo", "messages:outer", json!({ "author": "Ada" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("nested runtime query response should parse");
    assert_eq!(body.as_array().map(Vec::len), Some(2));
    assert!(
        body.as_array()
            .unwrap()
            .iter()
            .all(|doc| doc["author"] == json!("Ada"))
    );
    let metrics_body = api
        .runtime_metrics()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("runtime metrics response should parse");
    assert_eq!(
        metrics_body["metrics"]["fallback_cross_isolate_dispatches"],
        json!(1)
    );
    assert_eq!(
        metrics_body["metrics"]["worker_dispatched_invocations"],
        json!(2)
    );
    let metrics = registry.runtime_metrics_snapshot();
    assert_eq!(metrics.fallback_cross_isolate_dispatches, 1);
    assert_eq!(metrics.worker_dispatched_invocations, 2);
}

#[tokio::test]
async fn convex_runtime_only_query_paginate_keeps_continuation_cursor_for_full_terminal_page() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:listPage",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author, paginationOpts }) => await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).paginate(paginationOpts)"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:listPage", {
    name: "messages:listPage",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author, paginationOpts }) => await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).paginate(paginationOpts)",
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
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for body in ["alpha", "beta"] {
        assert_eq!(
            api.insert_document(
                "demo",
                "messages",
                json!({ "author": "Ada", "body": body, "rank": 1 })
            )
            .await
            .status(),
            StatusCode::CREATED
        );
    }

    let first_page = api
        .convex_named_query(
            "demo",
            "messages:listPage",
            json!({
                "author": "Ada",
                "paginationOpts": { "numItems": 1, "cursor": null }
            }),
        )
        .await;
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_body = first_page
        .json::<serde_json::Value>()
        .await
        .expect("first runtime paginate response should parse");
    let first_cursor = first_body["continueCursor"]
        .as_str()
        .expect("first runtime paginate response should include a cursor")
        .to_string();
    assert_eq!(first_body["isDone"], json!(false));

    let second_page = api
        .convex_named_query(
            "demo",
            "messages:listPage",
            json!({
                "author": "Ada",
                "paginationOpts": { "numItems": 1, "cursor": first_cursor }
            }),
        )
        .await;
    assert_eq!(second_page.status(), StatusCode::OK);
    let second_body = second_page
        .json::<serde_json::Value>()
        .await
        .expect("second runtime paginate response should parse");
    assert_eq!(second_body["page"].as_array().map(Vec::len), Some(1));
    assert_eq!(second_body["page"][0]["body"], json!("beta"));
    assert_eq!(second_body["isDone"], json!(false));
    assert!(
        second_body["continueCursor"]
            .as_str()
            .is_some_and(|cursor| !cursor.is_empty()),
        "second runtime paginate response should retain a continuation cursor for the final full page"
    );
}
