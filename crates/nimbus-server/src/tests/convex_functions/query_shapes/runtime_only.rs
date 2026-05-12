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
        metrics_body["metrics"]["fallback_cross_runtime_dispatches"],
        json!(1)
    );
    assert_eq!(
        metrics_body["metrics"]["worker_dispatched_invocations"],
        json!(2)
    );
    let metrics = registry.runtime_metrics_snapshot();
    assert_eq!(metrics.fallback_cross_runtime_dispatches, 1);
    assert_eq!(metrics.worker_dispatched_invocations, 2);
}

#[tokio::test]
async fn convex_runtime_only_full_scan_query_warms_and_reuses_materialized_serving_snapshot() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:listAll",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.db.query(\"messages\").take(20)"
            }
        ]),
        json!([]),
        Some(
            r#"
globalThis.__nimbusInvoke = async function(request) {
  try {
    return {
      status: "ok",
      value: await (async () => {
        const ctx = globalThis.__nimbusCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        });
        return await ctx.db.query("messages").take(20);
      })(),
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
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = TableName::new("messages").expect("table name should build");

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    for body in ["alpha", "beta"] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                json!({ "body": body })
                    .as_object()
                    .expect("document should be an object")
                    .clone(),
            )
            .expect("fixture insert should succeed");
    }

    let first = api
        .convex_named_query("demo", "messages:listAll", json!({}))
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let first_body = first
        .json::<serde_json::Value>()
        .await
        .expect("first runtime-only full scan should parse");
    assert_eq!(first_body.as_array().map(Vec::len), Some(2));

    let first_surface = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(first_surface.loaded_table_count, 1);
    assert_eq!(first_surface.table_load_count, 1);
    assert_eq!(first_surface.evaluation_count, 1);
    let first_snapshots = service
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot stats should load");
    assert!(
        first_snapshots.retained_snapshot_count >= 1,
        "runtime full-scan query should publish a retained serving snapshot"
    );

    let second = api
        .convex_named_query("demo", "messages:listAll", json!({}))
        .await;
    assert_eq!(second.status(), StatusCode::OK);
    let second_body = second
        .json::<serde_json::Value>()
        .await
        .expect("second runtime-only full scan should parse");
    assert_eq!(second_body.as_array().map(Vec::len), Some(2));

    let second_surface = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should reload");
    assert_eq!(
        second_surface.table_load_count, 1,
        "second runtime full-scan query should reuse the warmed table"
    );
    assert_eq!(second_surface.evaluation_count, 2);
}

#[tokio::test]
async fn convex_runtime_only_get_reuses_materialized_serving_snapshot_after_full_scan_warmup() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:warm",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => await ctx.db.query(\"messages\").take(1)"
            },
            {
                "name": "messages:getOne",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { id }) => await ctx.db.get(\"messages\", id)"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:warm", async (ctx) => await ctx.db.query("messages").take(1)],
  ["messages:getOne", async (ctx, { id }) => await ctx.db.get("messages", id)],
]);

globalThis.__nimbusInvoke = async function(request) {
  try {
    const handler = definitions.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__nimbusCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
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
    let service = fixture.service();
    let server = ServerFixture::start(build_router_with_convex(service.clone(), registry)).await;
    let api = HttpApiFixture::new(&server);
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = TableName::new("messages").expect("table name should build");

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let first_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            json!({ "body": "first" })
                .as_object()
                .expect("document should be an object")
                .clone(),
        )
        .expect("fixture insert should succeed");
    let second_id = service
        .insert_document(
            &tenant_id,
            table,
            json!({ "body": "second" })
                .as_object()
                .expect("document should be an object")
                .clone(),
        )
        .expect("fixture insert should succeed");

    let warm = api
        .convex_named_query("demo", "messages:warm", json!({}))
        .await;
    assert_eq!(warm.status(), StatusCode::OK);
    let warm_body = warm
        .json::<serde_json::Value>()
        .await
        .expect("warm query should parse");
    let warmed_id = warm_body[0]["_id"]
        .as_str()
        .expect("warm query should return a document id");
    let uncached_id = if warmed_id == first_id.to_string() {
        second_id
    } else {
        first_id
    };

    let before_get = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(before_get.get_hit_count, 0);

    let response = api
        .convex_named_query(
            "demo",
            "messages:getOne",
            json!({ "id": uncached_id.to_string() }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime-only get should parse");
    assert!(
        body["body"] == json!("first") || body["body"] == json!("second"),
        "{body}"
    );

    let after_get = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should reload");
    assert_eq!(after_get.table_load_count, 1);
    assert_eq!(
        after_get.get_hit_count, 1,
        "runtime ctx.db.get should reuse the warmed materialized serving snapshot"
    );
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
