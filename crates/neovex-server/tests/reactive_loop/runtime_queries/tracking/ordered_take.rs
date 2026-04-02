use super::*;

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
                    { "name": "by_priority", "fields": ["priority"] }
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
