use super::super::*;

#[tokio::test]
async fn convex_runtime_timeout_returns_request_timeout() {
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
globalThis.__neovexInvoke = async function(request) {
  const handler = new Function(
    "ctx",
    "args",
    "request",
    "return (async () => { while (true) {} })(ctx, args, request);",
  );
  return {
    status: "ok",
    value: await handler(globalThis.__neovexCreateContext(), request.args ?? {}, request),
  };
};

export {};
"#,
        ),
    )
    .with_runtime_limits(RuntimeLimits {
        execution_timeout: Duration::from_millis(10),
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
        .convex_named_query("demo", "messages:spin", json!({}))
        .await;
    assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime timeout response should parse");
    assert_eq!(body["error"], json!("operation canceled"));
}

#[tokio::test]
async fn dropped_runtime_http_request_cancels_runtime_invocation() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:spin",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async () => { while (true) {} }"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:spin", {
    name: "messages:spin",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async () => { while (true) {} }",
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

    let request = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:spin", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics(&registry, "runtime invocation to start", |metrics| {
        metrics.active_isolates >= 1 && metrics.worker_dispatched_invocations >= 1
    })
    .await;

    drop(request);

    let metrics = wait_for_runtime_metrics(
        &registry,
        "dropped runtime request cancellation",
        |metrics| metrics.active_isolates == 0 && metrics.canceled_invocations >= 1,
    )
    .await;
    assert_eq!(metrics.worker_dispatched_invocations, 1);
    assert_eq!(metrics.canceled_invocations, 1);
    assert_eq!(metrics.queued_canceled_invocations, 0);
    assert_eq!(metrics.in_flight_canceled_invocations, 1);
    assert_eq!(metrics.disconnect_canceled_invocations, 1);
    assert_eq!(metrics.explicit_canceled_invocations, 0);
    let tenant_metrics = metrics
        .tenants
        .get("demo")
        .expect("tenant runtime metrics should be present");
    assert_eq!(tenant_metrics.started_invocations, 1);
    assert_eq!(tenant_metrics.completed_invocations, 1);
    assert_eq!(tenant_metrics.queued_canceled_invocations, 0);
    assert_eq!(tenant_metrics.in_flight_canceled_invocations, 1);
    assert_eq!(tenant_metrics.disconnect_canceled_invocations, 1);
    assert_eq!(tenant_metrics.explicit_canceled_invocations, 0);
}

#[tokio::test]
async fn dropped_queued_runtime_request_never_starts_mutation() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:block",
                "kind": "query",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async () => { while (true) {} }"
            },
            {
                "name": "messages:insertQueued",
                "kind": "mutation",
                "visibility": "public",
                "plan": null,
                "runtime_handler": "async (ctx, { body }) => await ctx.db.insert(\"messages\", { body })"
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:block", {
    name: "messages:block",
    kind: "query",
    visibility: "public",
    plan: null,
    runtime_handler: "async () => { while (true) {} }",
  }],
  ["messages:insertQueued", {
    name: "messages:insertQueued",
    kind: "mutation",
    visibility: "public",
    plan: null,
    runtime_handler: "async (ctx, { body }) => await ctx.db.insert(\"messages\", { body })",
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
        max_concurrent_isolates: 1,
        ..RuntimeLimits::default()
    });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server =
        ServerFixture::start(build_router_with_convex(service.clone(), registry.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let blocker = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics(&registry, "blocking runtime query to start", |metrics| {
        metrics.active_isolates == 1 && metrics.worker_dispatched_invocations == 1
    })
    .await;

    let queued_mutation = open_json_post_stream(
        &server,
        "/convex/demo/mutation",
        &json!({ "name": "messages:insertQueued", "args": { "body": "queued" } }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        registry
            .runtime_metrics_snapshot()
            .worker_dispatched_invocations,
        1
    );

    drop(queued_mutation);
    drop(blocker);

    let metrics = wait_for_runtime_metrics(
        &registry,
        "queued runtime mutation cancellation",
        |metrics| metrics.active_isolates == 0 && metrics.canceled_invocations >= 2,
    )
    .await;
    assert_eq!(metrics.worker_dispatched_invocations, 1);
    assert_eq!(metrics.queued_canceled_invocations, 1);
    assert_eq!(metrics.in_flight_canceled_invocations, 1);
    assert_eq!(metrics.disconnect_canceled_invocations, 2);
    assert_eq!(metrics.explicit_canceled_invocations, 0);
    let tenant_metrics = metrics
        .tenants
        .get("demo")
        .expect("tenant runtime metrics should be present");
    assert_eq!(tenant_metrics.started_invocations, 1);
    assert_eq!(tenant_metrics.completed_invocations, 1);
    assert_eq!(tenant_metrics.queued_canceled_invocations, 1);
    assert_eq!(tenant_metrics.in_flight_canceled_invocations, 1);
    assert_eq!(tenant_metrics.disconnect_canceled_invocations, 2);
    assert_eq!(tenant_metrics.explicit_canceled_invocations, 0);
    assert!(
        metrics
            .recent_request_correlations
            .iter()
            .any(|correlation| {
                correlation.function_name == "messages:block"
                    && correlation.server_request_id.starts_with("convex-query-")
            })
    );
    assert!(
        metrics
            .recent_request_correlations
            .iter()
            .any(|correlation| {
                correlation.function_name == "messages:insertQueued"
                    && correlation
                        .server_request_id
                        .starts_with("convex-mutation-")
            })
    );

    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let documents = service
        .list_documents(
            &tenant_id,
            &TableName::new("messages").expect("table name should be valid"),
        )
        .expect("listing queued mutation table should succeed");
    assert!(documents.is_empty(), "queued mutation should never start");
}
