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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

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

    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    let mut socket = WebSocketFixture::connect_raw_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        &convex_team_bearer(),
    )
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
async fn convex_direct_get_subscription_push_id_matches_http_query_id() {
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert!(api.create_tenant("demo").await.status().is_success());
    let inserted = api
        .convex_named_mutation("demo", "messages:send", json!({ "body": "Tracked" }))
        .await;
    assert!(inserted.status().is_success());
    let convex_id = inserted
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")
        .as_str()
        .expect("insert should return document id")
        .to_string();
    let raw_id = decode_messages_raw_id(&convex_id);

    let http_document = api
        .convex_named_query("demo", "messages:byId", json!({ "id": convex_id }))
        .await
        .json::<serde_json::Value>()
        .await
        .expect("named query response should parse");
    let http_id = http_document["_id"]
        .as_str()
        .expect("HTTP query _id should be a string")
        .to_string();

    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    let mut socket = WebSocketFixture::connect_raw_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        &convex_team_bearer(),
    )
    .await
    .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-parity", "messages:byId", json!({ "id": convex_id }))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    let current = if initial["data"].is_null() {
        let caught_up = socket.next_json().await;
        assert_eq!(caught_up["type"], json!("subscription_result"));
        caught_up
    } else {
        initial
    };
    assert_eq!(current["data"]["body"], json!("Tracked"));

    let update_response = api
        .update_document("demo", "messages", &raw_id, json!({ "body": "Tracked v2" }))
        .await;
    assert!(update_response.status().is_success());

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["body"], json!("Tracked v2"));
    let ws_id = pushed["data"]["_id"]
        .as_str()
        .expect("WS push _id should be a string");
    assert!(
        ws_id.starts_with("messages:"),
        "WS push _id must be table-scoped, got {ws_id}"
    );
    assert_eq!(
        ws_id, http_id,
        "HTTP query and WS push must carry the identical table-scoped _id"
    );
}

#[tokio::test]
async fn convex_plan_replay_get_delivers_document_created_after_null_result() {
    let registry = convex_registry_with_bundle(
        json!([
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
        // Mirror the codegen emit: replay the compiled get plan wholesale
        // through the ctx-query host op, table-scoped id and all.
        Some(
            r#"
globalThis.__nimbusInvoke = async function(request) {
  const value = await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_query", {
    query: { type: "get", table: "messages", id: request.args.id },
  });
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert!(api.create_tenant("demo").await.status().is_success());
    // Materialize the table so the null read tracks a document dependency
    // rather than a missing-table dependency.
    assert!(
        api.insert_document("demo", "messages", json!({ "body": "Existing" }))
            .await
            .status()
            .is_success()
    );

    let absent_raw_id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    let absent_convex_id = encode_messages_convex_id(absent_raw_id);

    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    let mut socket = WebSocketFixture::connect_raw_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        &convex_team_bearer(),
    )
    .await
    .expect("convex websocket should connect");
    socket
        .subscribe_named(
            "convex-null-get",
            "messages:byId",
            json!({ "id": absent_convex_id }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-null-get"));
    let current = if initial["data"].is_null() {
        let caught_up = socket.next_json().await;
        assert_eq!(caught_up["type"], json!("subscription_result"));
        caught_up
    } else {
        initial
    };
    assert_eq!(current["data"]["runtime"], json!(true));
    assert_eq!(current["data"]["value"], serde_json::Value::Null);

    // The tracked document appears only after the null read, under the exact
    // raw id the subscription's read set must cover (caller-chosen ids are a
    // real surface: non-Convex adapters insert with explicit document keys).
    let tenant_id = nimbus_core::TenantId::new("demo").expect("tenant id should build");
    fixture
        .engine()
        .insert_document_with_id(
            &tenant_id,
            messages_table(),
            nimbus_core::DocumentId::from_key(absent_raw_id.to_string())
                .expect("raw document id should build"),
            serde_json::Map::from_iter([("body".to_string(), json!("Arrived"))]),
        )
        .expect("insert with explicit id should succeed");

    let pushed = socket
        .next_json_with_timeout(Duration::from_secs(5))
        .await
        .expect("null-get subscription must re-evaluate when the tracked document appears");
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"]["runtime"], json!(true));
    assert_eq!(pushed["data"]["value"]["body"], json!("Arrived"));
    assert_eq!(pushed["data"]["value"]["_id"], json!(absent_convex_id));
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
	  const ctx = globalThis.__nimbusCreateContext({
	    hostCallSessionId: `${request.kind}:${request.function_name}`,
	    request,
	  });
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

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

    // #41 non-vacuous: an anonymous Convex WS upgrade for this silo is refused.
    assert_convex_anonymous_ws_refused(&server, "demo").await;
    let mut socket = WebSocketFixture::connect_raw_with_bearer(
        &api.ws_url("/convex/demo/ws"),
        &convex_team_bearer(),
    )
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
