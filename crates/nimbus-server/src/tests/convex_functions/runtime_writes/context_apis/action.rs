use super::*;

#[tokio::test]
async fn convex_named_action_can_use_ctx_action_host_binding() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "tasks:titles",
                "kind": "action",
                "plan": null,
                "runtime_handler": "async () => null"
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

globalThis.__nimbusInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  const value = await globalThis.__nimbusAsyncHostValue("op_nimbus_ctx_action", {
    action: definition.plan,
    host_call_session_id: `${request.kind}:${request.function_name}`,
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
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let registry_for_router = registry.clone();
    let server = ServerFixture::start(router_for_convex_team(
        fixture.engine(),
        registry_for_router,
    ))
    .await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused
    // by the all-fail-closed team gate; only the team-bound bearer is admitted.
    assert_convex_anonymous_query_refused(&server, "demo").await;

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

/// Regression pin for the examples-verify failure on PR #200: an action's
/// runtime-handler `ctx.runMutation` into a SAME-LANE mutation whose handler
/// writes via `ctx.db.insert`. Local dispatch would run the callee on the
/// action's host session (no execution unit → raw writes rejected by the
/// invocation-kind guard, and non-transactional before it); the dispatcher
/// must route nested mutations from non-mutation callers through HOST
/// dispatch, which creates a proper serialized mutation invocation.
#[tokio::test]
async fn convex_named_action_run_mutation_same_lane_commits_through_host_dispatch() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "tasks:record",
                "kind": "mutation",
                "visibility": "internal",
                "plan": null,
                "runtime_handler": "async (ctx, { title }) => await ctx.db.insert(\"tasks\", { title })"
            },
            {
                "name": "tasks:recordFromAction",
                "kind": "action",
                "plan": null,
                "runtime_handler": "async () => null"
            }
        ]),
        json!([]),
        Some(
            r#"
// Mirror the generated-bundle shape: per-function dispatch plus a module-
// private local invoker handed to the context (HG2). With invokeNamedLocal
// present and a same-lane callee, the dispatcher WOULD take local dispatch —
// this test pins that nested mutations from an action refuse it and commit
// through host dispatch instead.
const handlers = new Map([
  ["tasks:record", async (ctx, args) => await ctx.db.insert("tasks", { title: args.title })],
  ["tasks:recordFromAction", async (ctx, args, request) => {
    const value = await ctx.runMutation(
      {
        kind: "mutation",
        name: "tasks:record",
        visibility: "internal",
      },
      { title: args.title },
    );
    return { ctx: true, value };
  }],
]);

async function invokeNamedDefinitionLocally(request) {
  const handler = handlers.get(request.function_name);
  if (!handler) {
    throw new Error("unknown function: " + request.function_name);
  }
  const ctx = globalThis.__nimbusCreateContext({
    hostCallSessionId:
      typeof request.hostCallSessionId === "string" && request.hostCallSessionId.length > 0
        ? request.hostCallSessionId
        : `${request.kind}:${request.function_name}`,
    request,
    invokeNamedLocal: invokeNamedDefinitionLocally,
  });
  return await handler(ctx, request.args ?? {}, request);
}

globalThis.__nimbusInvoke = function(request) {
  return (async () => {
    const value = await invokeNamedDefinitionLocally(request);
    return {
      status: "ok",
      value,
    };
  })();
};

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex_team(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_action(
            "demo",
            "tasks:recordFromAction",
            json!({ "title": "written through the nested mutation" }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("nested runMutation response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ctx"], json!(true));
    assert!(
        body["value"].as_str().is_some(),
        "nested mutation should return the inserted document id: {body}"
    );

    let documents = api.list_documents("demo", "tasks").await;
    let body = documents
        .json::<serde_json::Value>()
        .await
        .expect("task list should parse");
    assert_eq!(
        body["data"][0]["title"],
        json!("written through the nested mutation"),
        "the nested mutation's write must be committed: {body}"
    );
}
