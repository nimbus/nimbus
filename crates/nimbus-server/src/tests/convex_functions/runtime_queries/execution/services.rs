use super::*;

#[tokio::test]
async fn convex_runtime_query_context_has_no_nimbus_service_shortcut() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "services:adapterBoundary",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async (ctx, _args, request) => ({ ctxServicesType: typeof ctx.services, hasCtxServices: Object.prototype.hasOwnProperty.call(ctx, \"services\"), requestServicesType: typeof request.services })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["services:adapterBoundary", {
    name: "services:adapterBoundary",
    kind: "query",
    plan: null,
    runtime_handler: "async (ctx, _args, request) => ({ ctxServicesType: typeof ctx.services, hasCtxServices: Object.prototype.hasOwnProperty.call(ctx, \"services\"), requestServicesType: typeof request.services })",
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
  const handler = handlers.get(request.function_name);
  return {
    status: "ok",
    value: await handler(
      globalThis.__nimbusCreateContext({
        request,
        hostCallSessionId: `${request.kind}:${request.function_name}`,
      }),
      request.args ?? {},
      request,
    ),
  };
};

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "services:adapterBoundary", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("adapter boundary response should parse");
    assert_eq!(
        body,
        json!({
            "ctxServicesType": "undefined",
            "hasCtxServices": false,
            "requestServicesType": "undefined",
        }),
        "Convex adapter ctx.services must stay absent; use @nimbus/nimbus for service features"
    );
}

#[tokio::test]
async fn convex_runtime_query_cannot_reach_raw_service_op() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "services:rawOpDenied",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async () => { try { await globalThis.__nimbusAsyncHostValue(\"op_nimbus_ctx_service_lookup\", { service_name: \"db\", host_call_session_id: \"query:services:rawOpDenied\" }); return { rawServiceOp: \"allowed\" }; } catch (error) { return { rawServiceOp: \"denied\", message: String(error && error.message ? error.message : error) }; } }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["services:rawOpDenied", {
    name: "services:rawOpDenied",
    kind: "query",
    plan: null,
    runtime_handler: "async () => { try { await globalThis.__nimbusAsyncHostValue(\"op_nimbus_ctx_service_lookup\", { service_name: \"db\", host_call_session_id: \"query:services:rawOpDenied\" }); return { rawServiceOp: \"allowed\" }; } catch (error) { return { rawServiceOp: \"denied\", message: String(error && error.message ? error.message : error) }; } }",
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
  const handler = handlers.get(request.function_name);
  return {
    status: "ok",
    value: await handler(
      globalThis.__nimbusCreateContext({
        request,
        hostCallSessionId: `${request.kind}:${request.function_name}`,
      }),
      request.args ?? {},
      request,
    ),
  };
};

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_convex(fixture.engine(), registry.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "services:rawOpDenied", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("raw service-op denial response should parse");
    assert_eq!(body["rawServiceOp"], json!("denied"));
    assert!(
        body["message"].as_str().is_some_and(|message| message
            .contains("Nimbus runtime async host op not found: op_nimbus_ctx_service_lookup")),
        "unexpected raw service-op denial body: {body}"
    );
}
