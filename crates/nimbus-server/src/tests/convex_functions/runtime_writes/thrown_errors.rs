use super::*;

// Mirrors the generated `__nimbusInvoke` wrapper (packages/codegen
// emit/runtime_bundle_dispatch_global_invoke.mjs): a handler that throws
// answers with a `function_thrown` envelope; everything else rethrows.
const THROWING_BUNDLE: &str = r#"
const definitions = new Map([
  ["messages:send", {
    name: "messages:send",
    kind: "mutation",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx, { text }) => { if (!text) { throw new Error(\"Message text must not be empty (at messages:12)\"); } return text; }",
  }],
]);

globalThis.__nimbusInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  if (!definition) {
    throw new Error(`nimbus function or route not found: ${request.function_name}`);
  }
  const handler = new Function(
    "ctx",
    "args",
    `return (${definition.runtime_handler})(ctx, args);`,
  );
  try {
    const value = await handler(
      globalThis.__nimbusCreateContext({
        hostCallSessionId: `${request.kind}:${request.function_name}`,
        request,
      }),
      request.args ?? {},
    );
    return { status: "ok", value };
  } catch (error) {
    if (error && typeof error === "object" && "nimbusHostError" in error) {
      return { status: "error", error: error.nimbusHostError };
    }
    return {
      status: "error",
      error: {
        kind: "function_thrown",
        function_path: request.function_name,
        message: error.message,
        stack: error.stack,
      },
    };
  }
};

export {};
"#;

#[tokio::test]
async fn convex_named_mutation_reports_thrown_function_errors_with_message_and_stack() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:send",
                "kind": "mutation",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { text }) => text"
            }
        ]),
        json!([]),
        Some(THROWING_BUNDLE),
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
        .convex_named_mutation("demo", "messages:send", json!({ "text": "" }))
        .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("thrown function error response should parse");
    assert_eq!(body["error"]["code"], json!("function.thrown"), "{body}");
    assert_eq!(
        body["error"]["message"],
        json!("Message text must not be empty (at messages:12)"),
        "the thrown message must cross the public boundary intact: {body}"
    );
    assert_eq!(body["error"]["retryable"], json!(false), "{body}");
    assert_eq!(body["error"]["severity"], json!("error"), "{body}");
    assert_eq!(
        body["error"]["detail"]["functionPath"],
        json!("messages:send"),
        "{body}"
    );
    assert!(
        body["error"]["detail"]["stack"]
            .as_str()
            .is_some_and(|stack| stack.contains("Message text must not be empty")),
        "the thrown stack must travel in the detail: {body}"
    );
    assert_eq!(
        body["error"]["remediation"]["action"],
        json!("fix_function"),
        "{body}"
    );
    assert!(
        body["error"]["requestId"].as_str().is_some(),
        "every envelope carries a correlation id: {body}"
    );

    // A run row records the thrown message, its lifted location, and the stack.
    // The row is written after the response leaves, so the read waits for it.
    let engine = fixture.engine();
    let system_tenant = crate::system_tenant::system_tenant_id().expect("system id should parse");
    let runs_table = nimbus_core::TableName::new("runs").expect("table should parse");
    let run = tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let rows = engine
                .list_documents_async(system_tenant.clone(), runs_table.clone())
                .await
                .unwrap_or_default();
            if let Some(row) = rows.into_iter().find(|row| {
                row.fields.get("functionPath").and_then(|v| v.as_str()) == Some("messages:send")
            }) {
                return row;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("a run row for messages:send should be recorded");
    let run = serde_json::to_value(&run.fields).expect("run fields should serialize");
    assert_eq!(run["status"], json!("error"), "{run}");
    assert_eq!(
        run["error"]["message"],
        json!("Message text must not be empty (at messages:12)"),
        "{run}"
    );
    assert_eq!(run["error"]["location"], json!("messages:12"), "{run}");
    assert!(
        run["error"]["stack"]
            .as_str()
            .is_some_and(|stack| stack.contains("Message text must not be empty")),
        "{run}"
    );

    // A handler that returns keeps answering normally.
    let ok = api
        .convex_named_mutation("demo", "messages:send", json!({ "text": "hello" }))
        .await;
    assert_eq!(ok.status(), StatusCode::OK);
}
