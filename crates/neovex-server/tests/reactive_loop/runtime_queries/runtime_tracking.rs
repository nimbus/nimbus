use super::super::*;

#[tokio::test]
async fn convex_runtime_only_query_subscription_bootstraps_and_tracks_reads() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "messages:maybeByAuthor",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => { if (author) { return await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20); } return await ctx.db.query(\"messages\").take(20); }"
            }
        ]),
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
      value: {
        runtime: true,
        value: await handler(
          globalThis.__neovexCreateContext({
            sessionId: `${request.kind}:${request.function_name}`,
          }),
          request.args ?? {},
          request,
        ),
      },
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
            "convex-runtime-only",
            "messages:maybeByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-only"));
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
        "runtime-only subscription should stay idle for non-matching writes"
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
        .expect("runtime-only filtered data should be an array");
    assert_eq!(data.len(), 2);
    assert!(
        data.iter()
            .all(|document| document["author"] == json!("Ada"))
    );
}

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
}

#[tokio::test]
async fn convex_runtime_multi_table_subscription_tracks_matching_writes_across_tables() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "dashboard:counts",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx) => ({ openTasks: (await ctx.db.query(\"tasks\").filter((q) => q.eq(q.field(\"status\"), \"open\")).collect()).length, coreProfiles: (await ctx.db.query(\"profiles\").filter((q) => q.eq(q.field(\"team\"), \"core\")).collect()).length })"
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(_request) {
  const ctx = globalThis.__neovexCreateContext();
  const openTasks = await ctx.db
    .query("tasks")
    .filter((q) => q.eq(q.field("status"), "open"))
    .collect();
  const coreProfiles = await ctx.db
    .query("profiles")
    .filter((q) => q.eq(q.field("team"), "core"))
    .collect();
  return {
    status: "ok",
    value: {
      runtime: true,
      openTasks: openTasks.length,
      coreProfiles: coreProfiles.length,
    },
  };
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    assert!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Tracked", "status": "open" })
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Closed", "status": "done" })
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        api.insert_document("demo", "profiles", json!({ "name": "Ada", "team": "core" }))
            .await
            .status()
            .is_success()
    );
    assert!(
        api.insert_document(
            "demo",
            "profiles",
            json!({ "name": "Bob", "team": "support" })
        )
        .await
        .status()
        .is_success()
    );

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-runtime-multi", "dashboard:counts", json!({}))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-multi"));
    assert_eq!(initial["data"]["runtime"], json!(true));
    assert_eq!(initial["data"]["openTasks"], json!(1));
    assert_eq!(initial["data"]["coreProfiles"], json!(1));

    assert!(
        api.insert_document("demo", "messages", json!({ "body": "Ignored" }))
            .await
            .status()
            .is_success()
    );
    assert!(
        socket
            .next_json_with_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "multi-table runtime subscription should stay idle for unrelated tables"
    );

    assert!(
        api.insert_document(
            "demo",
            "profiles",
            json!({ "name": "Eve", "team": "support" })
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        socket
            .next_json_with_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "multi-table runtime subscription should stay idle for non-matching writes on a tracked table"
    );

    assert!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Second tracked", "status": "open" }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(pushed["data"]["openTasks"], json!(2));
    assert_eq!(pushed["data"]["coreProfiles"], json!(1));

    assert!(
        api.insert_document("demo", "profiles", json!({ "name": "Lin", "team": "core" }))
            .await
            .status()
            .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(pushed["data"]["openTasks"], json!(2));
    assert_eq!(pushed["data"]["coreProfiles"], json!(2));
}

#[tokio::test]
async fn convex_runtime_ordered_take_subscription_ignores_matching_writes_outside_visible_window() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "messages:topByAuthor",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => ({ runtime: true, value: await ctx.db.query(\"messages\").withIndex(\"by_priority\", (q) => q.gte(q.field(\"priority\"), 0)).filter((q) => q.eq(q.field(\"author\"), author)).order(\"desc\").take(2) })"
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(request) {
  const ctx = globalThis.__neovexCreateContext();
  return {
    status: "ok",
    value: {
      runtime: true,
      value: await ctx.db
        .query("messages")
        .withIndex("by_priority", (q) => q.gte(q.field("priority"), 0))
        .filter((q) => q.eq(q.field("author"), request.args.author))
        .order("desc")
        .take(2),
    },
  };
};

export {};
"#,
        ),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    assert_eq!(
        api.set_table_schema(
            "demo",
            "messages",
            json!({
                "table": "messages",
                "fields": [
                    { "name": "author", "field_type": "string", "required": false },
                    { "name": "priority", "field_type": "number", "required": false }
                ],
                "indexes": [
                    { "name": "by_priority", "field": "priority" }
                ]
            }),
        )
        .await
        .status(),
        reqwest::StatusCode::NO_CONTENT
    );
    for (author, priority) in [("Ada", 100), ("Ada", 90), ("Ada", 80), ("Bob", 110)] {
        assert!(
            api.insert_document(
                "demo",
                "messages",
                json!({ "author": author, "priority": priority }),
            )
            .await
            .status()
            .is_success()
        );
    }

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "convex-runtime-top",
            "messages:topByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-top"));
    assert_eq!(initial["data"]["runtime"], json!(true));
    assert_eq!(initial["data"]["value"][0]["priority"], json!(100));
    assert_eq!(initial["data"]["value"][1]["priority"], json!(90));

    assert!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Bob", "priority": 120 }),
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        socket
            .next_json_with_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "ordered take subscription should stay idle for non-matching writes"
    );

    assert!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "priority": 70 }),
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        socket
            .next_json_with_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "ordered take subscription should stay idle for matching writes outside the visible window"
    );

    assert!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "priority": 95 }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(pushed["data"]["value"][0]["priority"], json!(100));
    assert_eq!(pushed["data"]["value"][1]["priority"], json!(95));
}
