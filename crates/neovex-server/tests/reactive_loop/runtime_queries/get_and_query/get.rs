use super::*;

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
