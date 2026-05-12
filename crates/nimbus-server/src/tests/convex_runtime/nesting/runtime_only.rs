use super::*;

#[tokio::test]
async fn convex_runtime_only_action_can_run_runtime_only_mutation() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:storeInternal",
                "kind": "mutation",
                "visibility": "internal",
                "plan": null,
                "runtime_handler": "async (ctx, { author, body }) => await ctx.db.insert(\"messages\", { author, body })"
            },
            {
                "name": "messages:sendViaRuntime",
                "kind": "action",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { author, body }) => await ctx.runMutation({ name: \"messages:storeInternal\", visibility: \"internal\" }, { author, body })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:storeInternal", {
    name: "messages:storeInternal",
    kind: "mutation",
    visibility: "internal",
    plan: null,
    runtime_handler: "async (ctx, { author, body }) => await ctx.db.insert(\"messages\", { author, body })",
  }],
  ["messages:sendViaRuntime", {
    name: "messages:sendViaRuntime",
    kind: "action",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx, { author, body }) => await ctx.runMutation({ name: \"messages:storeInternal\", visibility: \"internal\" }, { author, body })",
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
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_action(
            "demo",
            "messages:sendViaRuntime",
            json!({ "author": "Ada", "body": "Nested runtime mutation" }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("nested runtime action response should parse")
            .as_str()
            .is_some()
    );

    let listed = api.list_documents("demo", "messages").await;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = listed
        .json::<serde_json::Value>()
        .await
        .expect("list response should parse");
    assert_eq!(listed["data"][0]["author"], json!("Ada"));
    assert_eq!(listed["data"][0]["body"], json!("Nested runtime mutation"));
}
