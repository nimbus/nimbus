use super::*;

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
