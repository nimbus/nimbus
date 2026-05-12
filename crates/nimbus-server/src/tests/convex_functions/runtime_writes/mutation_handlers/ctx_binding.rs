use super::*;

#[tokio::test]
async fn convex_named_mutation_can_use_ctx_mutation_host_binding() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
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
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:send", {
    name: "messages:send",
    kind: "mutation",
    plan: {
      type: "insert",
      table: "messages",
      fields: {
        author: { $arg: "author" },
        body: { $arg: "body" },
      },
    },
  }],
]);

function resolveTemplate(template, args) {
  if (template === null || typeof template !== "object") {
    return template;
  }
  if (Array.isArray(template)) {
    return template.map((item) => resolveTemplate(item, args));
  }
  if (typeof template.$arg === "string" && Object.keys(template).length === 1) {
    return args[template.$arg];
  }
  const resolved = {};
  for (const [key, value] of Object.entries(template)) {
    resolved[key] = resolveTemplate(value, args);
  }
  return resolved;
}

globalThis.__nimbusInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  const value = await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_mutation", {
    mutation: resolveTemplate(definition.plan, request.args ?? {}),
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
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Hello" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("ctx mutation host-binding response should parse");
    assert_eq!(body["ctx"], json!(true));
    assert!(body["value"].as_str().is_some());

    let second_response = api
        .convex_named_mutation(
            "demo",
            "messages:send",
            json!({ "author": "Ada", "body": "Again" }),
        )
        .await;
    assert_eq!(second_response.status(), StatusCode::OK);
    let second_body = second_response
        .json::<serde_json::Value>()
        .await
        .expect("second ctx mutation host-binding response should parse");
    assert_eq!(second_body["ctx"], json!(true));
    assert!(second_body["value"].as_str().is_some());

    let listed = api.list_documents("demo", "messages").await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = listed
        .json::<serde_json::Value>()
        .await
        .expect("inserted documents should parse");
    assert_eq!(listed_body["data"].as_array().map(Vec::len), Some(2));
    assert_eq!(listed_body["data"][0]["author"], json!("Ada"));
    assert_eq!(listed_body["data"][0]["body"], json!("Hello"));
    assert_eq!(listed_body["data"][1]["author"], json!("Ada"));
    assert_eq!(listed_body["data"][1]["body"], json!("Again"));
}
