use super::*;

#[tokio::test]
async fn convex_runtime_timeout_returns_request_timeout() {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = Duration::from_millis(10);
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:spin",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async () => { while (true) {} }"
            }
        ]),
        json!([]),
        Some(
            r#"
globalThis.__nimbusInvoke = async function(request) {
  const handler = new Function(
    "ctx",
    "args",
    "request",
    "return (async () => { while (true) {} })(ctx, args, request);",
  );
  return {
    status: "ok",
    value: await handler(globalThis.__nimbusCreateContext(), request.args ?? {}, request),
  };
};

export {};
"#,
        ),
    )
    .with_runtime_limits(limits);
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_query("demo", "messages:spin", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime timeout response should parse");
    assert_eq!(body["error"]["message"], json!("operation canceled"));
}
