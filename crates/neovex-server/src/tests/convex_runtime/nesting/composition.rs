use super::*;

#[tokio::test]
async fn convex_named_action_can_compose_query_mutation_and_action_calls() {
    let registry = convex_registry(json!([
        {
            "name": "messages:byAuthor",
            "kind": "query",
            "visibility": "public",
            "plan": {
                "table": "messages",
                "filters": [
                    {
                        "field": "author",
                        "op": "eq",
                        "value": { "$arg": "author" }
                    }
                ],
                "order": null,
                "limit": null
            }
        },
        {
            "name": "messages:storeInternal",
            "kind": "mutation",
            "visibility": "internal",
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
            "name": "messages:listInternal",
            "kind": "action",
            "visibility": "internal",
            "plan": {
                "type": "call_query",
                "name": "messages:byAuthor",
                "visibility": "public",
                "args": {
                    "author": { "$arg": "author" }
                }
            }
        },
        {
            "name": "messages:sendViaAction",
            "kind": "action",
            "visibility": "public",
            "plan": {
                "type": "call_mutation",
                "name": "messages:storeInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:listViaAction",
            "kind": "action",
            "visibility": "public",
            "plan": {
                "type": "call_action",
                "name": "messages:listInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let inserted = api
        .convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": "Ada", "body": "Hello from action" }),
        )
        .await;
    let inserted_status = inserted.status();
    let inserted_body = inserted
        .json::<serde_json::Value>()
        .await
        .expect("action response should parse");
    assert_eq!(inserted_status, StatusCode::OK, "{inserted_body}");
    assert!(inserted_body.as_str().is_some());

    let listed = api
        .convex_named_action("demo", "messages:listViaAction", json!({ "author": "Ada" }))
        .await;
    let listed_status = listed.status();
    let listed_body = listed
        .json::<serde_json::Value>()
        .await
        .expect("list via action response should parse");
    assert_eq!(listed_status, StatusCode::OK, "{listed_body}");
    assert_eq!(
        listed_body,
        json!([{
            "_creationTime": listed_body[0]["_creationTime"].clone(),
            "_id": listed_body[0]["_id"].clone(),
            "author": "Ada",
            "body": "Hello from action"
        }])
    );
}
