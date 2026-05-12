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
      sessionId: `${request.kind}:${request.function_name}`,
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

globalThis.__nimbusInvokeNamedLocal = invokeLocal;

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
        .convex_named_query("demo", "messages:outer", json!({ "nested": true }))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
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
    let metrics = registry.runtime_metrics_snapshot();
    assert_eq!(metrics.nested_local_dispatches, 1);
    assert_eq!(metrics.fallback_cross_runtime_dispatches, 0);
    assert_eq!(metrics.worker_dispatched_invocations, 1);
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 0);
    assert_eq!(metrics.runtime_pool_replacements, 0);
    assert_eq!(
        metrics
            .host_operations
            .get("convex.ctx.runtime.enter_nested_call")
            .expect("nested runtime host op metrics should be present")
            .succeeded,
        1
    );
    let correlation = metrics
        .recent_request_correlations
        .last()
        .expect("request correlation should be present");
    assert_eq!(correlation.function_name, "messages:outer");
    assert_eq!(correlation.kind, "query");
    assert_eq!(correlation.tenant_label.as_deref(), Some("demo"));
    assert!(correlation.server_request_id.starts_with("convex-query-"));
    assert!(correlation.invocation_id > 0);
    assert_eq!(
        metrics
            .tenants
            .get("demo")
            .expect("tenant runtime metrics should be present")
            .execution_distribution
            .samples,
        1
    );
}
