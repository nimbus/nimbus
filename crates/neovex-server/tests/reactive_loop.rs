use std::fs;

use neovex_engine::{Service, run_scheduler};
use neovex_runtime::RuntimeBundle;
use neovex_server::{ConvexRegistry, build_router, build_router_with_convex};
use neovex_test_support::{HttpApiFixture, ServerFixture, ServiceFixture, WebSocketFixture};
use serde_json::json;
use tempfile::tempdir;
use tokio::sync::watch;
use tokio::time::Duration;
use tokio_tungstenite::tungstenite::Error as WebSocketError;

fn convex_registry(functions: serde_json::Value) -> ConvexRegistry {
    convex_registry_with_bundle(functions, None)
}

fn convex_registry_with_bundle(
    functions: serde_json::Value,
    bundle: Option<&str>,
) -> ConvexRegistry {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".neovex").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": functions }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    if let Some(bundle) = bundle {
        let bundle_path = convex_dir.join("bundle.mjs");
        fs::write(&bundle_path, bundle).expect("convex runtime bundle should write");
        let bundle_sha256 =
            RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
        fs::write(
            bundle_path.with_extension("sha256"),
            format!("{bundle_sha256}\n"),
        )
        .expect("convex runtime bundle hash should write");
    }
    let registry =
        ConvexRegistry::from_app_dir(tempdir.path()).expect("convex registry should load");
    std::mem::forget(tempdir);
    registry
}

#[tokio::test]
async fn subscribe_insert_and_receive_reactive_push() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert!(create_response.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("1", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("1"));
    assert_eq!(initial["data"], json!([]));

    let insert_response = api
        .insert_document("demo", "tasks", json!({ "title": "Hello" }))
        .await;
    assert!(insert_response.status().is_success());

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert!(pushed.get("request_id").is_none());
    let data = pushed["data"].as_array().expect("data should be an array");
    assert_eq!(data.len(), 1);
    assert!(data[0]["_id"].is_string());
    assert!(data[0]["_creationTime"].is_u64());
    assert_eq!(data[0]["title"], json!("Hello"));
}

#[tokio::test]
async fn subscribe_update_and_delete_and_receive_reactive_pushes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert!(create_response.status().is_success());

    let insert_response = api
        .insert_document("demo", "tasks", json!({ "title": "Before" }))
        .await;
    assert!(insert_response.status().is_success());
    let document_id = insert_response
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")["id"]
        .as_str()
        .expect("id should be a string")
        .to_string();

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("2", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("2"));
    assert_eq!(initial["data"][0]["title"], json!("Before"));

    let update_response = api
        .update_document("demo", "tasks", &document_id, json!({ "title": "After" }))
        .await;
    assert!(update_response.status().is_success());

    let updated = socket.next_json().await;
    assert_eq!(updated["type"], json!("subscription_result"));
    assert!(updated.get("request_id").is_none());
    assert_eq!(updated["data"][0]["title"], json!("After"));

    let delete_response = api.delete_document("demo", "tasks", &document_id).await;
    assert_eq!(delete_response.status(), reqwest::StatusCode::NO_CONTENT);

    let deleted = socket.next_json().await;
    assert_eq!(deleted["type"], json!("subscription_result"));
    assert!(deleted.get("request_id").is_none());
    assert_eq!(deleted["data"], json!([]));
}

#[tokio::test]
async fn delete_tenant_sends_subscription_error() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let create_response = api.create_tenant("demo").await;
    assert!(create_response.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("3", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("3"));

    let delete_response = api.delete_tenant("demo").await;
    assert_eq!(delete_response.status(), reqwest::StatusCode::NO_CONTENT);

    let teardown = socket.next_json().await;
    assert_eq!(teardown["type"], json!("error"));
    assert!(teardown.get("request_id").is_none());
    assert!(
        teardown["message"]
            .as_str()
            .expect("message should be a string")
            .contains("tenant deleted: demo")
    );
}

#[tokio::test]
async fn websocket_missing_tenant_header_returns_bad_request() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    match WebSocketFixture::connect_with_tenant(&api.ws_url("/ws"), None).await {
        Err(WebSocketError::Http(response)) => {
            assert_eq!(response.status(), reqwest::StatusCode::BAD_REQUEST);
        }
        Ok(_) => panic!("connection should fail"),
        Err(other) => panic!("unexpected websocket error: {other:?}"),
    }
}

#[tokio::test]
async fn websocket_nonexistent_tenant_returns_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    match WebSocketFixture::connect_with_tenant(&api.ws_url("/ws"), Some("missing")).await {
        Err(WebSocketError::Http(response)) => {
            assert_eq!(response.status(), reqwest::StatusCode::NOT_FOUND);
        }
        Ok(_) => panic!("connection should fail"),
        Err(other) => panic!("unexpected websocket error: {other:?}"),
    }
}

#[tokio::test]
async fn browser_style_websocket_query_parameter_supports_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect_for_browser(&api.ws_url("/ws"), "demo")
        .await
        .expect("browser websocket should connect");
    socket.subscribe_all("browser", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("browser"));
    assert_eq!(initial["data"], json!([]));
}

#[tokio::test]
async fn convex_websocket_path_supports_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket.subscribe_all("convex", "tasks").await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex"));
    assert_eq!(initial["data"], json!([]));

    assert!(
        api.convex_mutation(
            "demo",
            json!({
                "type": "insert",
                "table": "tasks",
                "fields": { "title": "Convex insert" }
            }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"][0]["title"], json!("Convex insert"));
}

#[tokio::test]
async fn convex_named_subscription_resolves_through_manifest() {
    let registry = convex_registry(json!([
        {
            "name": "tasks:all",
            "kind": "query",
            "plan": {
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }
        },
        {
            "name": "tasks:create",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "tasks",
                "fields": {
                    "title": { "$arg": "title" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-named", "tasks:all", json!({}))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-named"));
    assert_eq!(initial["data"], json!([]));

    assert!(
        api.convex_named_mutation(
            "demo",
            "tasks:create",
            json!({ "title": "Named convex insert" }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"][0]["title"], json!("Named convex insert"));
}

#[tokio::test]
async fn convex_named_subscription_uses_runtime_bundle_when_available() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "tasks:all",
                "kind": "query",
                "plan": {
                    "table": "tasks",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            },
            {
                "name": "tasks:create",
                "kind": "mutation",
                "plan": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": {
                        "title": { "$arg": "title" }
                    }
                }
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  switch (request.function_name) {
    case "tasks:all":
      return {
        status: "ok",
        value: {
          runtime: true,
          value: await ctx.db.query("tasks").collect(),
        },
      };
    case "tasks:create":
      return {
        status: "ok",
        value: {
          runtime: true,
          value: await ctx.db.insert("tasks", { title: request.args.title }),
        },
      };
    default:
      throw new Error(`unexpected function: ${request.function_name}`);
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

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-runtime", "tasks:all", json!({}))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime"));
    assert_eq!(initial["data"]["runtime"], json!(true));
    assert_eq!(initial["data"]["value"], json!([]));

    assert!(
        api.convex_named_mutation(
            "demo",
            "tasks:create",
            json!({ "title": "Runtime live insert" }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(
        pushed["data"]["value"][0]["title"],
        json!("Runtime live insert")
    );
}

#[tokio::test]
async fn convex_named_paginated_subscription_resolves_through_manifest() {
    let registry = convex_registry(json!([
        {
            "name": "tasks:listPage",
            "kind": "paginated_query",
            "plan": {
                "table": "tasks",
                "filters": [],
                "order": { "field": "title", "direction": "asc" },
                "limit": null
            }
        },
        {
            "name": "tasks:create",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "tasks",
                "fields": {
                    "title": { "$arg": "title" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-page", "tasks:listPage", json!({}))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-page"));
    assert_eq!(initial["data"], json!([]));

    assert!(
        api.convex_named_mutation(
            "demo",
            "tasks:create",
            json!({ "title": "Paginated convex insert" }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"][0]["title"], json!("Paginated convex insert"));
}

#[tokio::test]
async fn convex_named_paginated_subscription_uses_runtime_bundle_when_available() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "tasks:listPage",
                "kind": "paginated_query",
                "plan": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "asc" },
                    "limit": null
                }
            },
            {
                "name": "tasks:create",
                "kind": "mutation",
                "plan": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": {
                        "title": { "$arg": "title" }
                    }
                }
            }
        ]),
        Some(
            r#"
const definitions = new Map([
  ["tasks:listPage", {
    name: "tasks:listPage",
    kind: "paginated_query",
    plan: {
      table: "tasks",
      filters: [],
      order: { field: "title", direction: "asc" },
      limit: null,
    },
  }],
]);

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_paginated_query", {
    query: definition.plan,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
    session_id: `${request.kind}:${request.function_name}`,
  });
  return {
    status: "ok",
    value: {
      data: value.data.map((item) => ({ ...item, runtime: true })),
      next_cursor: value.next_cursor,
      has_more: value.has_more,
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

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named_with_options(
            "convex-runtime-page",
            "tasks:listPage",
            json!({}),
            Some(2),
            None,
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-page"));
    assert_eq!(initial["data"], json!([]));

    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Alpha" }))
            .await
            .status()
            .is_success()
    );
    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Bravo" }))
            .await
            .status()
            .is_success()
    );
    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Charlie" }))
            .await
            .status()
            .is_success()
    );

    loop {
        let pushed = socket.next_json().await;
        assert_eq!(pushed["type"], json!("subscription_result"));
        let data = pushed["data"]
            .as_array()
            .expect("runtime paginated data should be an array");
        if data.len() == 2 {
            assert_eq!(data[0]["title"], json!("Alpha"));
            assert_eq!(data[0]["runtime"], json!(true));
            assert_eq!(data[1]["title"], json!("Bravo"));
            assert_eq!(data[1]["runtime"], json!(true));
            break;
        }
    }
}

#[tokio::test]
async fn convex_runtime_only_paginated_subscription_bootstraps_and_tracks_reads() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "messages:listPage",
                "kind": "paginated_query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => { const normalizedAuthor = author?.trim(); if (normalizedAuthor) { return ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), normalizedAuthor)); } return ctx.db.query(\"messages\"); }"
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(request) {
  const ctx = globalThis.__neovexCreateContext({
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const normalizedAuthor = request.args.author?.trim();
  const builder = normalizedAuthor
    ? ctx.db.query("messages").filter((q) => q.eq(q.field("author"), normalizedAuthor))
    : ctx.db.query("messages");
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_query_paginate", {
    session_id: `${request.kind}:${request.function_name}`,
    builder_id: builder.__builderId,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
  });
  return {
    status: "ok",
    value: {
      data: value.data.map((item) => ({ ...item, runtime: true })),
      next_cursor: value.next_cursor,
      has_more: value.has_more,
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
    let tracked_insert = api
        .insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Tracked Ada" }),
        )
        .await;
    assert!(tracked_insert.status().is_success());
    let tracked_id = tracked_insert
        .json::<serde_json::Value>()
        .await
        .expect("tracked insert response should parse")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("tracked insert should return a document id")
        .to_string();

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named_with_options(
            "convex-runtime-page",
            "messages:listPage",
            json!({ "author": "Ada" }),
            Some(1),
            None,
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-page"));
    assert_eq!(initial["data"].as_array().map(Vec::len), Some(1));
    assert_eq!(initial["data"][0]["runtime"], json!(true));
    assert_eq!(initial["data"][0]["author"], json!("Ada"));

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
        "runtime-only paginated subscription should stay idle for non-matching writes"
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

    let maybe_update = socket
        .next_json_with_timeout(Duration::from_millis(200))
        .await;
    assert!(
        maybe_update.is_none(),
        "runtime-only paginated subscription should stay idle when a matching row lands after the visible page"
    );

    let delete_response = api.delete_document("demo", "messages", &tracked_id).await;
    assert!(delete_response.status().is_success());

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    let data = pushed["data"]
        .as_array()
        .expect("runtime-only paginated data should be an array");
    assert_eq!(data.len(), 1);
    assert_eq!(data[0]["runtime"], json!(true));
    assert_eq!(data[0]["author"], json!("Ada"));
}

#[tokio::test]
async fn convex_runtime_paginated_subscription_ignores_out_of_window_ordered_writes() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "messages:listTop",
                "kind": "paginated_query",
                "plan": null,
                "runtime_handler": "async (ctx) => ctx.db.query(\"messages\").filter((q) => q.gte(q.field(\"priority\"), 0)).order(\"desc\")"
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(request) {
  const ctx = globalThis.__neovexCreateContext({
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const builder = ctx.db
    .query("messages")
    .filter((q) => q.gte(q.field("priority"), 0))
    .order("desc");
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_query_paginate", {
    session_id: `${request.kind}:${request.function_name}`,
    builder_id: builder.__builderId,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
  });
  return {
    status: "ok",
    value: {
      data: value.data.map((item) => ({ ...item, runtime: true })),
      next_cursor: value.next_cursor,
      has_more: value.has_more,
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
    for priority in [100, 90, 80, 70] {
        assert!(
            api.insert_document(
                "demo",
                "messages",
                json!({ "body": format!("p-{priority}"), "priority": priority }),
            )
            .await
            .status()
            .is_success()
        );
    }

    let first_page = api
        .convex_named_paginated_query("demo", "messages:listTop", json!({}), 2, None)
        .await;
    assert!(first_page.status().is_success());
    let first_page = first_page
        .json::<serde_json::Value>()
        .await
        .expect("first page response should parse");
    let cursor = first_page["next_cursor"]
        .as_str()
        .expect("first page should include a cursor")
        .to_string();

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named_with_options(
            "convex-runtime-window",
            "messages:listTop",
            json!({}),
            Some(2),
            Some(cursor.as_str()),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    let initial_data = initial["data"]
        .as_array()
        .expect("runtime window data should be an array");
    assert_eq!(initial_data.len(), 2);
    assert_eq!(initial_data[0]["priority"], json!(80));
    assert_eq!(initial_data[1]["priority"], json!(70));
    assert_eq!(initial_data[0]["runtime"], json!(true));

    for priority in [110, 60] {
        assert!(
            api.insert_document(
                "demo",
                "messages",
                json!({ "body": format!("p-{priority}"), "priority": priority }),
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
            "ordered runtime page should stay idle for writes outside the visible window"
        );
    }

    assert!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "body": "p-85", "priority": 85 }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    let data = pushed["data"]
        .as_array()
        .expect("runtime window data should be an array");
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["priority"], json!(85));
    assert_eq!(data[1]["priority"], json!(80));
    assert_eq!(data[0]["runtime"], json!(true));
}

#[tokio::test]
async fn convex_runtime_multi_table_paginated_subscription_tracks_secondary_table_transitions() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "dashboard:listVisibleTasks",
                "kind": "paginated_query",
                "plan": null,
                "runtime_handler": "async (ctx, { team }) => { const matchingProfiles = await ctx.db.query(\"profiles\").filter((q) => q.eq(q.field(\"team\"), team)).collect(); return matchingProfiles.length >= 2 ? ctx.db.query(\"tasks\").filter((q) => q.eq(q.field(\"status\"), \"open\")) : ctx.db.query(\"tasks\").filter((q) => q.eq(q.field(\"status\"), \"done\")); }"
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(request) {
  const ctx = globalThis.__neovexCreateContext({
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const matchingProfiles = await ctx.db
    .query("profiles")
    .filter((q) => q.eq(q.field("team"), request.args.team))
    .collect();
  const builder = matchingProfiles.length >= 2
    ? ctx.db.query("tasks").filter((q) => q.eq(q.field("status"), "open"))
    : ctx.db.query("tasks").filter((q) => q.eq(q.field("status"), "done"));
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_query_paginate", {
    session_id: `${request.kind}:${request.function_name}`,
    builder_id: builder.__builderId,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
  });
  return {
    status: "ok",
    value: {
      data: value.data.map((item) => ({ ...item, runtime: true })),
      next_cursor: value.next_cursor,
      has_more: value.has_more,
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
            json!({ "title": "Done task", "status": "done" }),
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Open task one", "status": "open" }),
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Open task two", "status": "open" }),
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

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named_with_options(
            "convex-runtime-multi-page",
            "dashboard:listVisibleTasks",
            json!({ "team": "core" }),
            Some(5),
            None,
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-multi-page"));
    let initial_data = initial["data"]
        .as_array()
        .expect("initial runtime paginated data should be an array");
    assert_eq!(initial_data.len(), 1);
    assert_eq!(initial_data[0]["runtime"], json!(true));
    assert_eq!(initial_data[0]["status"], json!("done"));

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
        "multi-table paginated runtime subscription should stay idle for unrelated tables"
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
    assert!(
        socket
            .next_json_with_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "multi-table paginated runtime subscription should stay idle for non-matching writes on a tracked table"
    );

    assert!(
        api.insert_document("demo", "profiles", json!({ "name": "Lin", "team": "core" }))
            .await
            .status()
            .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    let pushed_data = pushed["data"]
        .as_array()
        .expect("refreshed runtime paginated data should be an array");
    assert_eq!(pushed_data.len(), 2);
    assert!(
        pushed_data
            .iter()
            .all(|document| document["runtime"] == json!(true))
    );
    assert!(
        pushed_data
            .iter()
            .all(|document| document["status"] == json!("open"))
    );
}

#[tokio::test]
async fn convex_named_get_subscription_returns_single_document_and_null_on_delete() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:byId",
            "kind": "query",
            "plan": {
                "type": "get",
                "table": "messages",
                "id": { "$arg": "id" }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    let inserted = api
        .convex_named_mutation("demo", "messages:send", json!({ "body": "Tracked" }))
        .await;
    assert!(inserted.status().is_success());
    let document_id = inserted
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")
        .as_str()
        .expect("insert should return document id")
        .to_string();

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-get", "messages:byId", json!({ "id": document_id }))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-get"));
    assert_eq!(initial["data"]["body"], json!("Tracked"));

    let delete_response = api.delete_document("demo", "messages", &document_id).await;
    assert_eq!(delete_response.status(), reqwest::StatusCode::NO_CONTENT);

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"], serde_json::Value::Null);
}

#[tokio::test]
async fn convex_runtime_get_subscription_skips_unrelated_writes() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "messages:send",
                "kind": "mutation",
                "plan": {
                    "type": "insert",
                    "table": "messages",
                    "fields": {
                        "body": { "$arg": "body" }
                    }
                }
            },
            {
                "name": "messages:byId",
                "kind": "query",
                "plan": {
                    "type": "get",
                    "table": "messages",
                    "id": { "$arg": "id" }
                }
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(request) {
  const ctx = globalThis.__neovexCreateContext();
  const value = await ctx.db.get("messages", request.args.id);
  return {
    status: "ok",
    value: {
      runtime: true,
      value,
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
    let tracked = api
        .insert_document("demo", "messages", json!({ "body": "Tracked" }))
        .await;
    assert!(tracked.status().is_success());
    let tracked_id = tracked
        .json::<serde_json::Value>()
        .await
        .expect("tracked insert response should parse")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("tracked insert should return id")
        .to_string();

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "convex-runtime-get",
            "messages:byId",
            json!({ "id": tracked_id }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-get"));
    assert_eq!(initial["data"]["runtime"], json!(true));
    assert_eq!(initial["data"]["value"]["body"], json!("Tracked"));

    assert!(
        api.insert_document("demo", "messages", json!({ "body": "Other" }))
            .await
            .status()
            .is_success()
    );

    let maybe_update = socket
        .next_json_with_timeout(Duration::from_millis(200))
        .await;
    assert!(
        maybe_update.is_none(),
        "runtime get subscription should stay idle for unrelated writes"
    );

    let delete_response = api.delete_document("demo", "messages", &tracked_id).await;
    assert_eq!(delete_response.status(), reqwest::StatusCode::NO_CONTENT);

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(pushed["data"]["value"], serde_json::Value::Null);
}

#[tokio::test]
async fn convex_runtime_query_subscription_tracks_result_documents_and_index_ranges() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "tasks:runtimeOpen",
                "kind": "query",
                "plan": {
                    "table": "tasks",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(_request) {
  const ctx = globalThis.__neovexCreateContext();
  const value = await ctx.db
    .query("tasks")
    .withIndex("by_status", (q) => q.eq(q.field("status"), "open"))
    .collect();
  return {
    status: "ok",
    value: {
      runtime: true,
      value,
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
            "tasks",
            json!({
                "table": "tasks",
                "fields": [
                    { "name": "title", "field_type": "string", "required": false },
                    { "name": "status", "field_type": "string", "required": false }
                ],
                "indexes": [
                    { "name": "by_status", "field": "status" }
                ]
            }),
        )
        .await
        .status(),
        reqwest::StatusCode::NO_CONTENT
    );

    let tracked = api
        .insert_document(
            "demo",
            "tasks",
            json!({ "title": "Tracked open task", "status": "open" }),
        )
        .await;
    assert!(tracked.status().is_success());
    let tracked_id = tracked
        .json::<serde_json::Value>()
        .await
        .expect("tracked task insert should parse")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("tracked task insert should return id")
        .to_string();

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-runtime-open", "tasks:runtimeOpen", json!({}))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-open"));
    assert_eq!(initial["data"]["runtime"], json!(true));
    assert_eq!(
        initial["data"]["value"][0]["title"],
        json!("Tracked open task")
    );

    assert!(
        api.insert_document(
            "demo",
            "tasks",
            json!({ "title": "Closed task", "status": "done" }),
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
        "runtime indexed subscription should stay idle for writes outside its tracked range"
    );

    assert!(
        api.update_document("demo", "tasks", &tracked_id, json!({ "status": "done" }))
            .await
            .status()
            .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(pushed["data"]["value"], json!([]));
}

#[tokio::test]
async fn convex_runtime_filtered_query_subscription_skips_non_matching_writes() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "messages:runtimeByAuthor",
                "kind": "query",
                "plan": {
                    "table": "messages",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            }
        ]),
        Some(
            r#"
globalThis.__neovexInvoke = async function(_request) {
  const ctx = globalThis.__neovexCreateContext();
  const value = await ctx.db
    .query("messages")
    .filter((q) => q.eq(q.field("author"), "Ada"))
    .collect();
  return {
    status: "ok",
    value: {
      runtime: true,
      value,
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
            "convex-runtime-filter",
            "messages:runtimeByAuthor",
            json!({}),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-filter"));
    assert_eq!(initial["data"]["runtime"], json!(true));
    assert_eq!(initial["data"]["value"][0]["body"], json!("Tracked Ada"));

    let ignored = api
        .insert_document(
            "demo",
            "messages",
            json!({ "author": "Bob", "body": "Ignored Bob" }),
        )
        .await;
    assert!(ignored.status().is_success());
    let ignored_id = ignored
        .json::<serde_json::Value>()
        .await
        .expect("ignored insert should parse")
        .get("id")
        .and_then(serde_json::Value::as_str)
        .expect("ignored insert should return id")
        .to_string();

    let maybe_update = socket
        .next_json_with_timeout(Duration::from_millis(200))
        .await;
    assert!(
        maybe_update.is_none(),
        "runtime filtered subscription should stay idle for non-matching writes"
    );

    let delete_ignored = api.delete_document("demo", "messages", &ignored_id).await;
    assert_eq!(delete_ignored.status(), reqwest::StatusCode::NO_CONTENT);

    let maybe_delete_update = socket
        .next_json_with_timeout(Duration::from_millis(200))
        .await;
    assert!(
        maybe_delete_update.is_none(),
        "runtime filtered subscription should stay idle for non-matching deletes"
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
        .expect("runtime filtered data should be an array");
    assert_eq!(data.len(), 2);
    assert!(
        data.iter()
            .all(|document| document["author"] == json!("Ada"))
    );
}

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

#[tokio::test]
async fn convex_named_first_subscription_returns_single_document_and_updates() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:latestByAuthor",
            "kind": "query",
            "plan": {
                "type": "first",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": {
                        "field": "author",
                        "direction": "desc"
                    },
                    "limit": 1
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "latest",
            "messages:latestByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("latest"));
    assert_eq!(initial["data"], json!(null));

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await
        .status()
        .is_success()
    );

    let first_push = socket.next_json().await;
    assert_eq!(first_push["type"], json!("subscription_result"));
    assert_eq!(first_push["data"]["author"], json!("Ada"));
    assert_eq!(first_push["data"]["body"], json!("Hello"));
}

#[tokio::test]
async fn convex_named_unique_subscription_sends_error_on_duplicate_matches() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:uniqueByAuthor",
            "kind": "query",
            "plan": {
                "type": "unique",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": null,
                    "limit": 2
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "unique",
            "messages:uniqueByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("unique"));
    assert_eq!(initial["data"], json!(null));

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await
        .status()
        .is_success()
    );
    let first_push = socket.next_json().await;
    assert_eq!(first_push["type"], json!("subscription_result"));
    assert_eq!(first_push["data"]["body"], json!("Hello"));

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Again" }),
        )
        .await
        .status()
        .is_success()
    );
    let duplicate_error = socket.next_json().await;
    assert_eq!(duplicate_error["type"], json!("error"));
    assert!(
        duplicate_error["message"]
            .as_str()
            .expect("message should be a string")
            .contains("multiple documents")
    );
}

#[tokio::test]
async fn websocket_invalid_message_returns_error_event() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.send_text("{not json").await;

    let message = socket.next_json().await;
    assert_eq!(message["type"], json!("error"));
    assert!(message["request_id"].is_null());
    assert!(
        message["message"]
            .as_str()
            .expect("message should be a string")
            .contains("invalid websocket message")
    );
}

#[tokio::test]
async fn websocket_unsubscribe_stops_receiving_updates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("4", "tasks").await;

    let initial = socket.next_json().await;
    let subscription_id = initial["subscription_id"]
        .as_u64()
        .expect("subscription id should be present");

    socket.unsubscribe(subscription_id).await;
    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Hello" }))
            .await
            .status()
            .is_success()
    );

    let next = socket
        .next_json_with_timeout(Duration::from_millis(150))
        .await;
    assert!(next.is_none(), "unsubscribe should stop reactive pushes");
}

#[tokio::test]
async fn websocket_multiple_subscriptions_share_a_connection() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket.subscribe_all("tasks", "tasks").await;
    socket.subscribe_all("users", "users").await;

    let first = socket.next_json().await;
    let second = socket.next_json().await;
    assert_eq!(first["type"], json!("subscription_result"));
    assert_eq!(second["type"], json!("subscription_result"));

    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Hello" }))
            .await
            .status()
            .is_success()
    );

    let update = socket.next_json().await;
    assert_eq!(update["type"], json!("subscription_result"));
    assert_eq!(update["data"][0]["title"], json!("Hello"));

    let maybe_extra = socket
        .next_json_with_timeout(Duration::from_millis(150))
        .await;
    assert!(
        maybe_extra.is_none(),
        "unrelated subscription should stay idle"
    );
}

#[tokio::test]
async fn scheduled_mutation_over_http_drives_websocket_push() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router(service)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    assert_eq!(
        api.set_table_schema(
            "demo",
            "tasks",
            json!({
                "table": "tasks",
                "fields": [
                    { "name": "priority", "field_type": "number", "required": false },
                    { "name": "title", "field_type": "string", "required": false }
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

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket
        .send_text(
            json!({
                "type": "subscribe",
                "request_id": "sched-http",
                "query": {
                    "table": "tasks",
                    "filters": [
                        { "field": "priority", "op": "lte", "value": 5 }
                    ],
                    "order": { "field": "priority", "direction": "asc" },
                    "limit": null
                }
            })
            .to_string(),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("sched-http"));
    assert_eq!(initial["data"], json!([]));

    let schedule = api
        .schedule_mutation(
            "demo",
            json!({
                "run_after_ms": 0,
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "Scheduled task", "priority": 1 }
                }
            }),
        )
        .await;
    assert_eq!(schedule.status(), reqwest::StatusCode::CREATED);

    let pushed = socket
        .next_json_with_timeout(Duration::from_secs(3))
        .await
        .expect("scheduled reactive push should arrive");
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert!(pushed.get("request_id").is_none());
    assert_eq!(pushed["data"][0]["title"], json!("Scheduled task"));
    assert_eq!(pushed["data"][0]["priority"], json!(1));

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}
