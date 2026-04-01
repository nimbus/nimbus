use super::*;

#[tokio::test]
async fn convex_runtime_only_query_enforces_nested_runtime_budget() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:loop",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { depth }) => depth <= 0 ? [] : await ctx.runQuery({ name: \"messages:loop\", visibility: \"public\" }, { depth: depth - 1 })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:loop", {
    name: "messages:loop",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx, { depth }) => depth <= 0 ? [] : await ctx.runQuery({ name: \"messages:loop\", visibility: \"public\" }, { depth: depth - 1 })",
  }],
]);

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    return {
      status: "error",
      error: { kind: "internal", message: `missing definition for ${request.function_name}` },
    };
  }

  const handler = new Function(
    "ctx",
    "args",
    "request",
    `return (${definition.runtime_handler})(ctx, args, request);`,
  );

  try {
    const value = await handler(
      globalThis.__neovexCreateContext(),
      request.args ?? {},
      request,
    );
    return { status: "ok", value };
  } catch (error) {
    if (error && typeof error === "object" && "neovexHostError" in error) {
      return { status: "error", error: error.neovexHostError };
    }
    throw error;
  }
};

export {};
"#,
        ),
    )
    .with_runtime_limits(RuntimeLimits {
        max_nested_runtime_invocations: 2,
        ..RuntimeLimits::default()
    });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:loop", json!({ "depth": 3 }))
        .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("nested runtime budget error should parse");
    assert!(
        body["error"]
            .as_str()
            .expect("error message should be a string")
            .contains("nested invocation limit exceeded"),
        "unexpected nested runtime budget error body: {body}"
    );
}
