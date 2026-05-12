use super::*;

#[tokio::test]
async fn convex_named_query_can_use_runtime_only_handler() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:maybeByAuthor",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, { author }) => { if (author) { return await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20); } return await ctx.db.query(\"messages\").take(20); }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:maybeByAuthor", {
    name: "messages:maybeByAuthor",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, { author }) => { if (author) { return await ctx.db.query(\"messages\").filter((q) => q.eq(q.field(\"author\"), author)).take(20); } return await ctx.db.query(\"messages\").take(20); }",
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
    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "Hello" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Grace", "body": "World" })
        )
        .await
        .status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:maybeByAuthor", json!({ "author": "Ada" }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime-only convex query response should parse");
    assert_eq!(body.as_array().map(Vec::len), Some(1));
    assert_eq!(body[0]["author"], json!("Ada"));
    assert_eq!(body[0]["body"], json!("Hello"));
}
