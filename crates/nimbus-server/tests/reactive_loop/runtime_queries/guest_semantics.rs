use super::super::*;

/// EX10.6 runtime half: the Convex default runtime serves the documented
/// Node-API subset through the real registry lane (node:async_hooks resolves
/// in the deployed bundle), and — per the upstream caveat — data placed in
/// AsyncLocalStorage does NOT propagate into ctx.runQuery.
#[tokio::test]
async fn convex_default_runtime_async_local_storage_does_not_propagate_into_run_query() {
    let registry = convex_registry_with_bundle(
        json!([
            {
                "name": "messages:readStore",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async () => globalThis.__testStorage.getStore()?.requestId ?? \"no-store\""
            },
            {
                "name": "messages:withStore",
                "kind": "action",
                "plan": null,
                "runtime_handler": "async (ctx) => { const storage = globalThis.__testStorage; return await storage.run({ requestId: \"outer\" }, async () => ({ runtime: true, outer: storage.getStore()?.requestId ?? null, inner: await ctx.runQuery({ name: \"messages:readStore\", visibility: \"public\" }, {}) })); }"
            }
        ]),
        Some(
            r#"
import { AsyncLocalStorage } from "node:async_hooks";

globalThis.__testStorage = new AsyncLocalStorage();

const definitions = new Map([
  ["messages:readStore", {
    name: "messages:readStore",
    kind: "query",
    plan: null,
    runtime_handler: "async () => globalThis.__testStorage.getStore()?.requestId ?? \"no-store\"",
  }],
  ["messages:withStore", {
    name: "messages:withStore",
    kind: "action",
    plan: null,
    runtime_handler: "async (ctx) => { const storage = globalThis.__testStorage; return await storage.run({ requestId: \"outer\" }, async () => ({ runtime: true, outer: storage.getStore()?.requestId ?? null, inner: await ctx.runQuery({ name: \"messages:readStore\", visibility: \"public\" }, {}) })); }",
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

async function invokeLocal(request) {
  const handler = handlers.get(request.function_name);
  return await handler(
    globalThis.__nimbusCreateContext({
      hostCallSessionId: request.hostCallSessionId ?? `${request.kind}:${request.function_name}`,
      request,
    }),
    request.args ?? {},
    request,
  );
}

globalThis.__nimbusInvoke = async function(request) {
  try {
    return {
      status: "ok",
      value: await invokeLocal(request),
    };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
    }
    throw error;
  }
};

// Declare this bundle's functions as same-lane so same-isolate nested ctx.run*
// takes local dispatch (the path whose ALS non-propagation this test asserts).
if (typeof globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment === "function") {
  globalThis.__nimbusRegisterLocalFunctionRuntimeEnvironment(
    () => globalThis.__nimbusRuntimeEnvironmentLane,
  );
}
globalThis.__nimbusInvokeNamedLocal = invokeLocal;

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry.clone())).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert!(api.create_tenant("demo").await.status().is_success());
    // Materialize the messages table so the nested query has a real target.
    assert!(
        api.insert_document(
            "demo",
            "messages",
            json!({ "author": "Ada", "body": "seed" }),
        )
        .await
        .status()
        .is_success()
    );

    let response = api
        .convex_named_action("demo", "messages:withStore", json!({}))
        .await;
    assert!(response.status().is_success(), "action should succeed");
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("action response should parse");
    assert_eq!(
        body["outer"],
        json!("outer"),
        "the action must observe its own AsyncLocalStorage store: {body}"
    );
    assert_eq!(
        body["inner"],
        json!("no-store"),
        "AsyncLocalStorage data must NOT propagate into ctx.runQuery: {body}"
    );
}
