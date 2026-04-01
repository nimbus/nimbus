use super::*;

#[tokio::test]
async fn convex_named_paginated_subscription_resolves_through_manifest() {
    let registry = convex_registry(json!([
        {
            "name": "tasks:listPage",
            "kind": "paginated_query",
            "plan": {
                "table": "tasks",
                "filters": [],
                "order": { "field": "title", "direction": "asc" },
                "limit": null
            }
        },
        {
            "name": "tasks:create",
            "kind": "mutation",
            "plan": {
                "type": "insert",
                "table": "tasks",
                "fields": {
                    "title": { "$arg": "title" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named("convex-page", "tasks:listPage", json!({}))
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-page"));
    assert_eq!(initial["data"], json!([]));

    assert!(
        api.convex_named_mutation(
            "demo",
            "tasks:create",
            json!({ "title": "Paginated convex insert" }),
        )
        .await
        .status()
        .is_success()
    );

    let pushed = socket.next_json().await;
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert_eq!(pushed["data"][0]["title"], json!("Paginated convex insert"));
}

#[tokio::test]
async fn convex_named_paginated_subscription_uses_runtime_bundle_when_available() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "tasks:listPage",
                "kind": "paginated_query",
                "plan": {
                    "table": "tasks",
                    "filters": [],
                    "order": { "field": "title", "direction": "asc" },
                    "limit": null
                }
            },
            {
                "name": "tasks:create",
                "kind": "mutation",
                "plan": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": {
                        "title": { "$arg": "title" }
                    }
                }
            }
        ]),
        Some(
            r#"
const definitions = new Map([
  ["tasks:listPage", {
    name: "tasks:listPage",
    kind: "paginated_query",
    plan: {
      table: "tasks",
      filters: [],
      order: { field: "title", direction: "asc" },
      limit: null,
    },
  }],
]);

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_paginated_query", {
    query: definition.plan,
    page_size: request.page_size,
    cursor: request.cursor ?? null,
    session_id: `${request.kind}:${request.function_name}`,
  });
  return {
    status: "ok",
    value: {
      data: value.data.map((item) => ({ ...item, runtime: true })),
      next_cursor: value.next_cursor,
      has_more: value.has_more,
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

    let mut socket = WebSocketFixture::connect_raw(&api.ws_url("/convex/demo/ws"))
        .await
        .expect("convex websocket should connect");
    socket
        .subscribe_named_with_options(
            "convex-runtime-page",
            "tasks:listPage",
            json!({}),
            Some(2),
            None,
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("convex-runtime-page"));
    assert_eq!(initial["data"], json!([]));

    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Alpha" }))
            .await
            .status()
            .is_success()
    );
    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Bravo" }))
            .await
            .status()
            .is_success()
    );
    assert!(
        api.insert_document("demo", "tasks", json!({ "title": "Charlie" }))
            .await
            .status()
            .is_success()
    );

    loop {
        let pushed = socket.next_json().await;
        assert_eq!(pushed["type"], json!("subscription_result"));
        let data = pushed["data"]
            .as_array()
            .expect("runtime paginated data should be an array");
        if data.len() == 2 {
            assert_eq!(data[0]["title"], json!("Alpha"));
            assert_eq!(data[0]["runtime"], json!(true));
            assert_eq!(data[1]["title"], json!("Bravo"));
            assert_eq!(data[1]["runtime"], json!(true));
            break;
        }
    }
}
