use super::*;

#[tokio::test]
async fn convex_named_query_can_return_single_document_or_null() {
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

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let inserted = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    let inserted_id = inserted
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse")
        .as_str()
        .expect("insert should return id")
        .to_string();

    let found = api
        .convex_named_query("demo", "messages:byId", json!({ "id": inserted_id }))
        .await;
    assert_eq!(found.status(), StatusCode::OK);
    let found_body = found
        .json::<serde_json::Value>()
        .await
        .expect("get query response should parse");
    assert_eq!(found_body["author"], json!("Ada"));
    assert_eq!(found_body["body"], json!("Hello"));

    let missing = api
        .convex_named_query(
            "demo",
            "messages:byId",
            json!({ "id": "01ARZ3NDEKTSV4RRFFQ69G5FAV" }),
        )
        .await;
    assert_eq!(missing.status(), StatusCode::OK);
    assert_eq!(
        missing
            .json::<serde_json::Value>()
            .await
            .expect("missing get query should parse"),
        serde_json::Value::Null
    );
}
