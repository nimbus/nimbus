use super::*;

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
globalThis.__nimbusInvoke = async function(request) {
  const ctx = globalThis.__nimbusCreateContext({
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const builder = ctx.db
    .query("messages")
    .filter((q) => q.gte(q.field("priority"), 0))
    .order("desc");
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
