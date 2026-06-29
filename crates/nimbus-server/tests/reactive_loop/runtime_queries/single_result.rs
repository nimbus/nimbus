use super::super::*;

#[tokio::test]
async fn convex_named_first_subscription_returns_single_document_and_updates() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:latestByAuthor",
            "kind": "query",
            "plan": {
                "type": "first",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": {
                        "field": "author",
                        "direction": "desc"
                    },
                    "limit": 1
                }
            }
        }
    ]));
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert!(api.create_tenant("demo").await.status().is_success());

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
            "latest",
            "messages:latestByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("latest"));
    assert_eq!(initial["data"], json!(null));

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await
        .status()
        .is_success()
    );

    let first_push = socket.next_json().await;
    assert_eq!(first_push["type"], json!("subscription_result"));
    assert_eq!(first_push["data"]["author"], json!("Ada"));
    assert_eq!(first_push["data"]["body"], json!("Hello"));
}

#[tokio::test]
async fn convex_named_unique_subscription_sends_error_on_duplicate_matches() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:uniqueByAuthor",
            "kind": "query",
            "plan": {
                "type": "unique",
                "query": {
                    "table": "messages",
                    "filters": [
                        {
                            "field": "author",
                            "op": "eq",
                            "value": { "$arg": "author" }
                        }
                    ],
                    "order": null,
                    "limit": 2
                }
            }
        }
    ]));
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert!(api.create_tenant("demo").await.status().is_success());

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
            "unique",
            "messages:uniqueByAuthor",
            json!({ "author": "Ada" }),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("unique"));
    assert_eq!(initial["data"], json!(null));

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await
        .status()
        .is_success()
    );
    let first_push = socket.next_json().await;
    assert_eq!(first_push["type"], json!("subscription_result"));
    assert_eq!(first_push["data"]["body"], json!("Hello"));

    assert!(
        api.convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Again" }),
        )
        .await
        .status()
        .is_success()
    );
    let duplicate_error = socket.next_json().await;
    assert_eq!(duplicate_error["type"], json!("error"));
    assert!(
        duplicate_error["error"]["message"]
            .as_str()
            .expect("message should be a string")
            .contains("multiple documents")
    );
}
