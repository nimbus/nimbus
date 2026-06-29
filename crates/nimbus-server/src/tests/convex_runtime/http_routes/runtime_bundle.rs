use super::*;

#[tokio::test]
async fn convex_http_routes_still_dispatch_compiled_plans_when_runtime_bundle_is_present() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
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
            }
        ]),
        json!([
            {
                "method": "POST",
                "path": "/messages",
                "name": "http:inline:0",
                "plan": {
                    "operation": {
                        "type": "call_mutation",
                        "name": "messages:storeInternal",
                        "visibility": "internal",
                        "args": {
                            "author": {
                                "$request": { "source": "json", "path": "author" }
                            },
                            "body": {
                                "$request": { "source": "json", "path": "body" }
                            }
                        }
                    },
                    "response": {
                        "kind": "json",
                        "body": {
                            "id": {
                                "$result": { "index": 0, "path": "" }
                            }
                        },
                        "status": 201
                    }
                }
            },
            {
                "method": "GET",
                "path_prefix": "/messages/by-author",
                "name": "http:inline:1",
                "plan": {
                    "operation": {
                        "type": "call_query",
                        "name": "messages:byAuthor",
                        "visibility": "public",
                        "args": {
                            "author": {
                                "$request": { "source": "query", "name": "author" }
                            }
                        }
                    },
                    "response": {
                        "kind": "json",
                        "body": {
                            "$result": { "index": 0, "path": "" }
                        }
                    }
                }
            }
        ]),
        Some(
            r#"
globalThis.__nimbusInvoke = async function(request) {
  throw new Error(`runtime bundle should not be used for compiled http routes: ${request.function_name}`);
};

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex_team(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused
    // by the all-fail-closed team gate; only the team-bound bearer is admitted.
    assert_convex_anonymous_query_refused(&server, "demo").await;

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let inserted = api
        .convex_http_json(
            "demo",
            reqwest::Method::POST,
            "/messages",
            json!({ "author": "Ada", "body": "Hello from compiled httpAction" }),
        )
        .await;
    assert_eq!(inserted.status(), StatusCode::CREATED);
    let inserted_body = inserted
        .json::<serde_json::Value>()
        .await
        .expect("convex http post response should parse");
    assert!(inserted_body["id"].as_str().is_some());

    let listed = api
        .convex_http(
            "demo",
            reqwest::Method::GET,
            "/messages/by-author?author=Ada",
        )
        .await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = listed
        .json::<serde_json::Value>()
        .await
        .expect("convex http get response should parse");
    assert_eq!(listed_body[0]["author"], json!("Ada"));
    assert_eq!(
        listed_body[0]["body"],
        json!("Hello from compiled httpAction")
    );
}
