use super::*;

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
globalThis.__nimbusInvoke = async function(_request) {
  const ctx = globalThis.__nimbusCreateContext();
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
                    { "name": "by_status", "fields": ["status"] }
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
    assert_eq!(
        current["data"]["value"][0]["title"],
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
