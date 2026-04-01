use super::*;

#[tokio::test]
async fn convex_http_routes_dispatch_compiled_http_actions() {
    let registry = convex_registry_with_routes(
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
    );
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let inserted = api
        .convex_http_json(
            "demo",
            reqwest::Method::POST,
            "/messages",
            json!({ "author": "Ada", "body": "Hello from httpAction" }),
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
    assert_eq!(listed_body[0]["body"], json!("Hello from httpAction"));
}
