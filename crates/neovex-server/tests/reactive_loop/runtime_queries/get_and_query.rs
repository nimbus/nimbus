use super::super::*;

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
