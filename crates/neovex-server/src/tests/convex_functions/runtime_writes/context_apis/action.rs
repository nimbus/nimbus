use super::*;

#[tokio::test]
async fn convex_named_action_can_use_ctx_action_host_binding() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "tasks:titles",
                "kind": "action",
                "plan": {
                    "type": "query",
                    "query": {
                        "table": "tasks",
                        "filters": [],
                        "order": { "field": "title", "direction": "asc" },
                        "limit": null
                    }
                }
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["tasks:titles", {
    name: "tasks:titles",
    kind: "action",
    plan: {
      type: "query",
      query: {
        table: "tasks",
        filters: [],
        order: { field: "title", direction: "asc" },
        limit: null,
      },
    },
  }],
]);

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_action", {
    action: definition.plan,
    session_id: `${request.kind}:${request.function_name}`,
  });
  return {
    status: "ok",
    value: {
      ctx: true,
      value,
    },
  };
};

export {};
"#,
        ),
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
    for title in ["Alpha", "Bravo"] {
        assert_eq!(
            api.insert_document("demo", "tasks", json!({ "title": title }))
                .await
                .status(),
            StatusCode::CREATED
        );
    }

    let response = api
        .convex_named_action("demo", "tasks:titles", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("ctx action host-binding response should parse");
    assert_eq!(body["ctx"], json!(true));
    assert_eq!(body["value"][0]["title"], json!("Alpha"));
    assert_eq!(body["value"][1]["title"], json!("Bravo"));
}
