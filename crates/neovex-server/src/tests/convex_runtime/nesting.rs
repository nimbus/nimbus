use super::super::*;

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

globalThis.__neovexInvoke = async function(request) {
  try {
    const handler = handlers.get(request.function_name);
    return {
      status: "ok",
      value: await handler(
        globalThis.__neovexCreateContext({
          sessionId: `${request.kind}:${request.function_name}`,
        }),
        request.args ?? {},
        request,
      ),
    };
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

#[tokio::test]
async fn convex_named_action_can_compose_query_mutation_and_action_calls() {
    let registry = convex_registry(json!([
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
        },
        {
            "name": "messages:listInternal",
            "kind": "action",
            "visibility": "internal",
            "plan": {
                "type": "call_query",
                "name": "messages:byAuthor",
                "visibility": "public",
                "args": {
                    "author": { "$arg": "author" }
                }
            }
        },
        {
            "name": "messages:sendViaAction",
            "kind": "action",
            "visibility": "public",
            "plan": {
                "type": "call_mutation",
                "name": "messages:storeInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" },
                    "body": { "$arg": "body" }
                }
            }
        },
        {
            "name": "messages:listViaAction",
            "kind": "action",
            "visibility": "public",
            "plan": {
                "type": "call_action",
                "name": "messages:listInternal",
                "visibility": "internal",
                "args": {
                    "author": { "$arg": "author" }
                }
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

    let inserted = api
        .convex_named_action(
            "demo",
            "messages:sendViaAction",
            json!({ "author": "Ada", "body": "Hello from action" }),
        )
        .await;
    let inserted_status = inserted.status();
    let inserted_body = inserted
        .json::<serde_json::Value>()
        .await
        .expect("action response should parse");
    assert_eq!(inserted_status, StatusCode::OK, "{inserted_body}");
    assert!(inserted_body.as_str().is_some());

    let listed = api
        .convex_named_action("demo", "messages:listViaAction", json!({ "author": "Ada" }))
        .await;
    let listed_status = listed.status();
    let listed_body = listed
        .json::<serde_json::Value>()
        .await
        .expect("list via action response should parse");
    assert_eq!(listed_status, StatusCode::OK, "{listed_body}");
    assert_eq!(
        listed_body,
        json!([{
            "_creationTime": listed_body[0]["_creationTime"].clone(),
            "_id": listed_body[0]["_id"].clone(),
            "author": "Ada",
            "body": "Hello from action"
        }])
    );
}
