use super::*;
use nimbus_core::DocumentId;
use std::str::FromStr;

#[tokio::test]
async fn convex_named_mutation_can_use_runtime_only_handler() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:sendTwice",
                "kind": "mutation",
                "plan": null,
                "runtime_handler": "async (ctx, { body }) => { const firstId = await ctx.db.insert(\"messages\", { body }); await ctx.db.insert(\"messages\", { body: `${body} copy` }); return firstId; }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:sendTwice", {
    name: "messages:sendTwice",
    kind: "mutation",
    plan: null,
    runtime_handler: "async (ctx, { body }) => { const firstId = await ctx.db.insert(\"messages\", { body }); await ctx.db.insert(\"messages\", { body: `${body} copy` }); return firstId; }",
  }],
]);

function compileRuntimeHandler(definition) {
  return new Function(
    "ctx",
    "args",
    "request",
    "return (" + definition.runtime_handler + ")(ctx, args, request);",
  );
}

const handlers = new Map(
  [...definitions.values()].map((definition) => [
    definition.name,
    compileRuntimeHandler(definition),
  ]),
);

globalThis.__nimbusInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__nimbusCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
    }
    throw error;
  }
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
        .convex_named_mutation("demo", "messages:sendTwice", json!({ "body": "Hello" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let returned_id = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime-only convex mutation response should parse")
        .as_str()
        .expect("runtime-only mutation should return the first inserted id")
        .to_string();
    DocumentId::from_str(&returned_id).expect("returned id should remain a valid document id");

    let listed = api.list_documents("demo", "messages").await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed_body = listed
        .json::<serde_json::Value>()
        .await
        .expect("runtime-only mutation list should parse");
    assert_eq!(listed_body["data"].as_array().map(Vec::len), Some(2));
    assert!(
        listed_body["data"]
            .as_array()
            .expect("list payload should include data rows")
            .iter()
            .any(|document| document["_id"] == returned_id),
        "listed documents should include the returned insert id"
    );
}
