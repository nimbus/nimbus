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
