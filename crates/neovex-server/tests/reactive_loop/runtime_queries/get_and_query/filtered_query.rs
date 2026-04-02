use super::*;

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
    let current = if initial["data"]["value"]
        .as_array()
        .is_some_and(|documents| documents.is_empty())
    {
        let caught_up = socket.next_json().await;
        assert_eq!(caught_up["type"], json!("subscription_result"));
        caught_up
    } else {
        initial
    };
    assert_eq!(current["data"]["runtime"], json!(true));
    assert_eq!(current["data"]["value"][0]["body"], json!("Tracked Ada"));

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
