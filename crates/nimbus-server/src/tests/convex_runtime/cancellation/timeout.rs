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
	    value: await handler(
	      globalThis.__nimbusCreateContext({
	        hostCallSessionId: `${request.kind}:${request.function_name}`,
	        request,
	      }),
	      request.args ?? {},
	      request,
	    ),
	  };
	};

export {};
"#,
        ),
    )
    .with_runtime_limits(limits);
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
        .convex_named_query("demo", "messages:spin", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime timeout response should parse");
    assert_eq!(body["error"]["code"], json!("runtime.execution_timeout"));
    assert_eq!(
        body["error"]["message"],
        json!("runtime execution timed out after 10ms")
    );
    assert_eq!(
        body["error"]["detail"],
        json!({ "timeoutKind": "execution", "timeoutMs": 10 })
    );
}
