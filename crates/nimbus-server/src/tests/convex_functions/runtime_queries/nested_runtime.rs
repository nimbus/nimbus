use super::super::super::*;

#[tokio::test]
async fn convex_runtime_only_query_reuses_same_isolate_for_ctx_run_query() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:outer",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { nested }) => { globalThis.__nimbusCounter = (globalThis.__nimbusCounter ?? 0) + 1; if (nested) { return await ctx.runQuery({ name: \"messages:outer\", visibility: \"public\" }, { nested: false }); } return globalThis.__nimbusCounter; }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:outer", {
    name: "messages:outer",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx, { nested }) => { globalThis.__nimbusCounter = (globalThis.__nimbusCounter ?? 0) + 1; if (nested) { return await ctx.runQuery({ name: \"messages:outer\", visibility: \"public\" }, { nested: false }); } return globalThis.__nimbusCounter; }",
  }],
]);

async function invokeLocal(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    throw new Error(`missing definition for ${request.function_name}`);
  }
  const handler = new Function(
    "ctx",
    "args",
    "request",
    `return (${definition.runtime_handler})(ctx, args, request);`,
  );
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
    return { status: "ok", value: await invokeLocal(request) };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
    }
    throw error;
  }
};

// Same-isolate nested ctx.run* takes local dispatch because the host resolves
// these default-lane callees to this isolate's lane (op_nimbus_ctx_resolve_callee_lane).
globalThis.__nimbusInvokeNamedLocal = invokeLocal;

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex_team(fixture.engine(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused
    // by the all-fail-closed team gate; only the team-bound bearer is admitted.
    assert_convex_anonymous_query_refused(&server, "demo").await;

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:outer", json!({ "nested": true }))
        .await;
    let status = response.status();
    let response_body = response
        .text()
        .await
        .expect("same-isolate nested runtime response body should load");
    assert_eq!(status, StatusCode::OK, "{response_body}");
    let body = serde_json::from_str::<serde_json::Value>(&response_body)
        .expect("same-isolate nested runtime response should parse");
    assert_eq!(body, json!(2));
    let metrics_body = api
        .runtime_metrics()
        .await
        .json::<serde_json::Value>()
        .await
        .expect("runtime metrics response should parse");
    assert_eq!(metrics_body["metrics"]["nested_local_dispatches"], json!(1));
    assert_eq!(
        metrics_body["metrics"]["fallback_cross_runtime_dispatches"],
        json!(0)
    );
    assert_eq!(
        metrics_body["metrics"]["host_operations"]["convex.ctx.runtime.enter_nested_call"]["started"],
        json!(1)
    );
    assert_eq!(
        metrics_body["metrics"]["host_operations"]["convex.ctx.runtime.enter_nested_call"]["succeeded"],
        json!(1)
    );
    assert_eq!(
        metrics_body["metrics"]["worker_dispatched_invocations"],
        json!(1)
    );
    assert_eq!(metrics_body["metrics"]["runtime_pool_misses"], json!(1));
    assert_eq!(metrics_body["metrics"]["runtime_pool_hits"], json!(0));
    assert_eq!(
        metrics_body["metrics"]["runtime_pool_replacements"],
        json!(0)
    );
    assert_eq!(
        metrics_body["metrics"]["tenants"]["demo"]["started_invocations"],
        json!(1)
    );
    assert_eq!(
        metrics_body["metrics"]["tenants"]["demo"]["disconnect_canceled_invocations"],
        json!(0)
    );
    assert_eq!(
        metrics_body["metrics"]["tenants"]["demo"]["explicit_canceled_invocations"],
        json!(0)
    );
    assert_eq!(
        metrics_body["metrics"]["tenants"]["demo"]["completed_invocations"],
        json!(1)
    );
    assert_eq!(
        metrics_body["metrics"]["tenants"]["demo"]["queue_wait_distribution"]["samples"],
        json!(1)
    );
    assert_eq!(
        metrics_body["metrics"]["tenants"]["demo"]["execution_distribution"]["samples"],
        json!(1)
    );
    let correlations = metrics_body["metrics"]["recent_request_correlations"]
        .as_array()
        .expect("runtime metrics should include recent request correlations");
    let correlation = correlations
        .last()
        .expect("request correlation should be present");
    assert_eq!(correlation["function_name"], json!("messages:outer"));
    assert_eq!(correlation["kind"], json!("query"));
    assert_eq!(correlation["tenant_label"], json!("demo"));
    assert!(
        correlation["server_request_id"]
            .as_str()
            .is_some_and(|request_id| request_id.starts_with("convex-query-"))
    );
    assert!(
        correlation["invocation_id"]
            .as_u64()
            .is_some_and(|invocation_id| invocation_id > 0)
    );
}
