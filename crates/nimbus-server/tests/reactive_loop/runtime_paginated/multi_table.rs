use super::*;

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
globalThis.__nimbusInvoke = async function(request) {
  const ctx = globalThis.__nimbusCreateContext({
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const matchingProfiles = await ctx.db
    .query("profiles")
    .filter((q) => q.eq(q.field("team"), request.args.team))
    .collect();
  const builder = matchingProfiles.length >= 2
    ? ctx.db.query("tasks").filter((q) => q.eq(q.field("status"), "open"))
    : ctx.db.query("tasks").filter((q) => q.eq(q.field("status"), "done"));
  const value = await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_query_paginate", {
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
