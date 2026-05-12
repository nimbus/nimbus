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
  [...definitions.values()]
    .filter((definition) => typeof definition.runtime_handler === "string")
    .map((definition) => [
      definition.name,
      compileRuntimeHandler(definition),
    ]),
);

globalThis.__nimbusInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__nimbusCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
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

#[tokio::test]
async fn convex_websocket_runtime_subscription_survives_named_runtime_mutation_reeval() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:byAuthor",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20)"
            },
            {
                "name": "messages:send",
                "kind": "mutation",
                "plan": null,
                "runtime_handler": "async (ctx, { author, body }) => await ctx.db.insert(\"messages\", { author, body })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:byAuthor", {
    name: "messages:byAuthor",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author }) => await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20)",
  }],
  ["messages:send", {
    name: "messages:send",
    kind: "mutation",
    plan: null,
    runtime_handler: "async (ctx, { author, body }) => await ctx.db.insert(\"messages\", { author, body })",
  }],
]);

function compileRuntimeHandler(definition) {
  const source = definition.runtime_handler;
  if (typeof source !== "string" || source.length === 0) {
    return null;
  }
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + source + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()]
    .filter((definition) => typeof definition.runtime_handler === "string")
    .map((definition) => [
      definition.name,
      compileRuntimeHandler(definition),
    ]),
);

globalThis.__nimbusInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__nimbusCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
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

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "convex-runtime-subscription-named-mutation",
            "messages:byAuthor",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(
        initial["request_id"],
        json!("convex-runtime-subscription-named-mutation")
    );
    assert_eq!(initial["data"], json!([]));

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime-backed named mutation response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.as_str().is_some(),
        "mutation should return inserted id: {body}"
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(pushed["data"][0]["author"], json!("Ada"));
    assert_eq!(pushed["data"][0]["body"], json!("Hello"));
}

#[tokio::test]
async fn convex_runtime_query_flow_matches_differential_request_order() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:byAuthor",
                "kind": "query",
                "plan": {
                    "table": "messages",
                    "filters": [
                        { "field": "rank", "op": "gte", "value": 0 },
                        { "field": "author", "op": "eq", "value": { "$arg": "author" } }
                    ],
                    "order": null,
                    "limit": null
                },
                "runtime_handler": null
            },
            {
                "name": "messages:listPage",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author, paginationOpts }) =>\n    await ctx.db\n      .query(\"messages\")\n      .withIndex(\"by_rank\", (q) => q.gte(\"rank\", 0))\n      .filter((q) => q.eq(q.field(\"author\"), author))\n      .paginate(paginationOpts)",
                "runtime_bindings": {}
            },
            {
                "name": "messages:send",
                "kind": "mutation",
                "plan": {
                    "type": "insert",
                    "table": "messages",
                    "fields": {
                        "author": { "$arg": "author" },
                        "body": { "$arg": "body" },
                        "rank": { "$arg": "rank" }
                    }
                },
                "runtime_handler": null
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:byAuthor", {
    name: "messages:byAuthor",
    kind: "query",
    plan: {
      table: "messages",
      filters: [
        { field: "rank", op: "gte", value: 0 },
        { field: "author", op: "eq", value: { $arg: "author" } },
      ],
      order: null,
      limit: null,
    },
    runtime_handler: null,
  }],
  ["messages:listPage", {
    name: "messages:listPage",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author, paginationOpts }) => await ctx.db.query(\"messages\").withIndex(\"by_rank\", (q) => q.gte(\"rank\", 0)).filter((q) => q.eq(q.field(\"author\"), author)).paginate(paginationOpts)",
  }],
  ["messages:send", {
    name: "messages:send",
    kind: "mutation",
    plan: {
      type: "insert",
      table: "messages",
      fields: {
        author: { $arg: "author" },
        body: { $arg: "body" },
        rank: { $arg: "rank" },
      },
    },
    runtime_handler: null,
  }],
]);

function compileRuntimeHandler(definition) {
  const source = definition.runtime_handler;
  if (typeof source !== "string" || source.length === 0) {
    return null;
  }
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + source + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()]
    .filter((definition) => typeof definition.runtime_handler === "string")
    .map((definition) => [
      definition.name,
      compileRuntimeHandler(definition),
    ]),
);

function isPlainObject(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isQueryShape(plan) {
  return isPlainObject(plan)
    && typeof plan.table === "string"
    && Array.isArray(plan.filters)
    && Object.prototype.hasOwnProperty.call(plan, "order")
    && Object.prototype.hasOwnProperty.call(plan, "limit");
}

function resolveArgsTemplate(template, args) {
  if (template === null || typeof template !== "object") {
    return template;
  }
  if (Array.isArray(template)) {
    return template.map((item) => resolveArgsTemplate(item, args));
  }
  if (typeof template.$arg === "string" && Object.keys(template).length === 1) {
    return args[template.$arg];
  }
  const resolved = {};
  for (const [key, value] of Object.entries(template)) {
    resolved[key] = resolveArgsTemplate(value, args);
  }
  return resolved;
}

function createConstraintBuilderFromPlan(builder, filters) {
  for (const filter of filters ?? []) {
    const field = builder.field(filter.field);
    switch (filter.op) {
      case "eq":
        builder.eq(field, filter.value);
        break;
      case "gte":
        builder.gte(field, filter.value);
        break;
      default:
        throw new Error("unsupported convex filter op: " + filter.op);
    }
  }
  return builder;
}

function buildQueryFromPlan(ctx, query) {
  let builder = ctx.db.query(query.table);
  if (Array.isArray(query.filters) && query.filters.length > 0) {
    builder = builder.filter((q) => createConstraintBuilderFromPlan(q, query.filters));
  }
  return builder;
}

async function executeResolvedQueryPlan(ctx, plan) {
  if (isQueryShape(plan)) {
    const builder = buildQueryFromPlan(ctx, plan);
    return typeof plan.limit === "number"
      ? await builder.take(plan.limit)
      : await builder.collect();
  }
  return await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_query", {
    query: plan,
    session_id: "convex-runtime-query-plan",
  });
}

async function executeResolvedMutationPlan(ctx, plan) {
  if (isPlainObject(plan) && plan.type === "insert") {
    return ctx.db.insert(plan.table, plan.fields);
  }
  return await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_mutation", {
    mutation: plan,
    session_id: "convex-runtime-mutation-plan",
  });
}

globalThis.__nimbusInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    const ctx = globalThis.__nimbusCreateContext({
      request,
      sessionId: `${request.kind}:${request.function_name}`,
    });
    if (handler) {
      return {
        status: "ok",
        value: await handler(ctx, request.args ?? {}, request),
      };
    }
    const definition = definitions.get(request.function_name);
    if (!definition) {
      throw new Error(`convex function not found: ${request.function_name}`);
    }
    if (definition.kind === "query") {
      return {
        status: "ok",
        value: await executeResolvedQueryPlan(
          ctx,
          resolveArgsTemplate(definition.plan, request.args ?? {}),
        ),
      };
    }
    if (definition.kind === "mutation") {
      return {
        status: "ok",
        value: await executeResolvedMutationPlan(
          ctx,
          resolveArgsTemplate(definition.plan, request.args ?? {}),
        ),
      };
    }
    return {
      status: "error",
      error: `unsupported test definition kind: ${definition.kind}`,
    };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
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
        api.set_table_schema(
            "demo",
            "messages",
            json!({
                "table": "messages",
                "fields": [
                    { "name": "author", "field_type": "string", "required": false },
                    { "name": "body", "field_type": "string", "required": false },
                    { "name": "rank", "field_type": "number", "required": false }
                ],
                "indexes": [
                    { "name": "by_rank", "fields": ["rank"] }
                ]
            }),
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );

    for (body, rank) in [("alpha", 1), ("beta", 2)] {
        let response = api
            .convex_named_mutation(
                "demo",
                "messages:send",
                json!({ "author": "Ada", "body": body, "rank": rank }),
            )
            .await;
        let status = response.status();
        let payload = response
            .json::<serde_json::Value>()
            .await
            .expect("named mutation payload should parse");
        assert_eq!(status, StatusCode::OK, "{payload}");
        assert!(
            payload.as_str().is_some(),
            "mutation should return inserted id: {payload}"
        );
    }

    let query = api
        .convex_named_query("demo", "messages:byAuthor", json!({ "author": "Ada" }))
        .await;
    assert_eq!(query.status(), StatusCode::OK);
    let query_body = query
        .json::<serde_json::Value>()
        .await
        .expect("named query payload should parse");
    assert_eq!(query_body.as_array().map(Vec::len), Some(2));

    let first_page = api
        .convex_named_query(
            "demo",
            "messages:listPage",
            json!({ "author": "Ada", "paginationOpts": { "numItems": 1, "cursor": null } }),
        )
        .await;
    assert_eq!(first_page.status(), StatusCode::OK);
    let first_page_body = first_page
        .json::<serde_json::Value>()
        .await
        .expect("first page should parse");
    assert_eq!(first_page_body["page"].as_array().map(Vec::len), Some(1));
    let cursor = first_page_body["continueCursor"]
        .as_str()
        .expect("first page should include a cursor")
        .to_string();

    let second_page = api
        .convex_named_query(
            "demo",
            "messages:listPage",
            json!({
                "author": "Ada",
                "paginationOpts": { "numItems": 1, "cursor": cursor }
            }),
        )
        .await;
    assert_eq!(second_page.status(), StatusCode::OK);
    let second_page_body = second_page
        .json::<serde_json::Value>()
        .await
        .expect("second page should parse");
    assert_eq!(second_page_body["page"].as_array().map(Vec::len), Some(1));

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "convex-runtime-differential-order",
            "messages:byAuthor",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(
        initial["request_id"],
        json!("convex-runtime-differential-order")
    );
    assert_eq!(initial["data"].as_array().map(Vec::len), Some(2));

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "gamma", "rank": 3 }),
        )
        .await;
    let status = response.status();
    let payload = response
        .json::<serde_json::Value>()
        .await
        .expect("post-subscription mutation payload should parse");
    assert_eq!(status, StatusCode::OK, "{payload}");
    assert!(
        payload.as_str().is_some(),
        "mutation should return inserted id: {payload}"
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"].as_array().map(Vec::len), Some(3));
}
