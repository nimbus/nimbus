use super::*;

fn messages_table() -> nimbus_core::TableName {
    nimbus_core::TableName::new("messages").expect("messages table should be valid")
}

fn encode_messages_convex_id(document_id: &str) -> String {
    let raw_id = nimbus_core::DocumentId::from_key(document_id.to_string())
        .expect("fixture document id should be valid");
    nimbus_core::ResolvedDocumentId::encode_table_scoped(&messages_table(), &raw_id)
        .expect("fixture id should encode as a Convex table-scoped id")
        .to_string()
}

fn decode_messages_raw_id(convex_id: &str) -> String {
    let scoped_id = nimbus_core::DocumentId::from_key(convex_id.to_string())
        .expect("Convex document id should be valid");
    nimbus_core::ResolvedDocumentId::resolve_table_scoped(&messages_table(), scoped_id)
        .expect("Convex id should resolve to messages table")
        .into_document_id()
        .to_string()
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
    let server = ServerFixture::start(router_for_convex(fixture.service(), registry)).await;
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
    let raw_document_id = decode_messages_raw_id(&document_id);

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-get", "messages:byId", json!({ "id": document_id }))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-get"));
    let current = if initial["data"].is_null() {
        let caught_up = socket.next_json().await;
        assert_eq!(caught_up["type"], json!("subscription_result"));
        caught_up
    } else {
        initial
    };
    assert_eq!(current["data"]["body"], json!("Tracked"));

    let delete_response = api
        .delete_document("demo", "messages", &raw_document_id)
        .await;
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
globalThis.__nimbusInvoke = async function(request) {
  const ctx = globalThis.__nimbusCreateContext();
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
    let server = ServerFixture::start(router_for_convex(fixture.service(), registry)).await;
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
    let tracked_convex_id = encode_messages_convex_id(&tracked_id);

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "convex-runtime-get",
            "messages:byId",
            json!({ "id": tracked_convex_id }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-get"));
    let current = if initial["data"]["value"].is_null() {
        let caught_up = socket.next_json().await;
        assert_eq!(caught_up["type"], json!("subscription_result"));
        caught_up
    } else {
        initial
    };
    assert_eq!(current["data"]["runtime"], json!(true));
    assert_eq!(current["data"]["value"]["body"], json!("Tracked"));

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
