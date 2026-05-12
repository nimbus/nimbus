use super::*;

#[tokio::test]
async fn convex_named_query_reports_runtime_bundle_contract_errors() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:byAuthor",
                "kind": "query",
                "plan": {
                    "table": "messages",
                    "filters": [],
                    "order": null,
                    "limit": null
                }
            }
        ]),
        json!([]),
        Some("export const noop = 1;\n"),
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let registry_for_router = registry.clone();
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry_for_router,
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:byAuthor", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime contract error response should parse");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
            .contains("__nimbusInvoke"),
        "unexpected runtime error body: {body}"
    );
}

#[tokio::test]
async fn convex_named_mutation_dispatches_compiled_patch_and_delete() {
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
            "name": "messages:rename",
            "kind": "mutation",
            "plan": {
                "type": "update",
                "table": "messages",
                "id": { "$arg": "id" },
                "patch": {
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:remove",
            "kind": "mutation",
            "plan": {
                "type": "delete",
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
    let inserted_status = inserted.status();
    let inserted_body = inserted
        .json::<serde_json::Value>()
        .await
        .expect("insert response should parse");
    assert_eq!(inserted_status, StatusCode::OK, "{inserted_body}");
    let id = inserted_body
        .as_str()
        .expect("insert mutation should return a document id")
        .to_string();

    let renamed = api
        .convex_named_mutation(
            "demo",
            "messages:rename",
            json!({ "id": id, "body": "Edited" }),
        )
        .await;
    let renamed_status = renamed.status();
    let renamed_body = renamed
        .json::<serde_json::Value>()
        .await
        .expect("rename response should parse");
    assert_eq!(renamed_status, StatusCode::OK, "{renamed_body}");

    let after_rename = api.list_documents("demo", "messages").await;
    assert_eq!(after_rename.status(), StatusCode::OK);
    let after_rename_body = after_rename
        .json::<serde_json::Value>()
        .await
        .expect("documents should parse");
    assert_eq!(after_rename_body["data"][0]["body"], json!("Edited"));

    let deleted = api
        .convex_named_mutation(
            "demo",
            "messages:remove",
            json!({ "id": after_rename_body["data"][0]["_id"].clone() }),
        )
        .await;
    let deleted_status = deleted.status();
    let deleted_body = deleted
        .json::<serde_json::Value>()
        .await
        .expect("delete response should parse");
    assert_eq!(deleted_status, StatusCode::OK, "{deleted_body}");
    assert_eq!(deleted_body, serde_json::Value::Null);

    let after_delete = api
        .list_documents("demo", "messages")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("documents after delete should parse");
    assert_eq!(after_delete["data"], json!([]));
}
