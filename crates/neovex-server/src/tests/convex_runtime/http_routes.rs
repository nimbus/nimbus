use super::super::*;

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

#[tokio::test]
async fn convex_http_routes_use_runtime_bundle_when_available() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([]),
        json!([
            {
                "method": "GET",
                "path": "/healthz",
                "name": "http:inline:0",
                "plan": {
                    "response": {
                        "kind": "json",
                        "body": { "ok": true }
                    }
                }
            }
        ]),
        Some(
            r#"
const routesByName = new Map([
  ["http:inline:0", {
    name: "http:inline:0",
    method: "GET",
    path: "/healthz",
    plan: {
      response: {
        kind: "json",
        body: { ok: true },
      },
    },
  }],
]);

globalThis.__neovexInvoke = async function(request) {
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_http_route", {
    request,
    route: routesByName.get(request.function_name),
  });
  return {
    status: "ok",
    value: {
      ...value,
      body: {
        runtime: true,
        value: value.body,
      },
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

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_http("demo", reqwest::Method::GET, "/healthz")
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime-backed convex http response should parse");
    assert_eq!(body["runtime"], json!(true));
    assert_eq!(body["value"]["ok"], json!(true));
}

#[tokio::test]
async fn convex_http_routes_return_404_and_405_when_appropriate() {
    let registry = convex_registry_with_routes(
        json!([]),
        json!([
            {
                "method": "GET",
                "path": "/healthz",
                "name": "http:inline:0",
                "plan": {
                    "response": {
                        "kind": "text",
                        "body": "ok"
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

    assert_eq!(
        api.convex_http("demo", reqwest::Method::GET, "/missing")
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        api.convex_http("demo", reqwest::Method::POST, "/healthz")
            .await
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn convex_public_endpoints_reject_internal_functions() {
    let registry = convex_registry(json!([
        {
            "name": "tasks:internalList",
            "kind": "query",
            "visibility": "internal",
            "plan": {
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
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

    let response = api
        .convex_named_query("demo", "tasks:internalList", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("internal convex error should parse")["error"]
            .as_str()
            .expect("internal convex error should be a string")
            .contains("not public")
    );
}
