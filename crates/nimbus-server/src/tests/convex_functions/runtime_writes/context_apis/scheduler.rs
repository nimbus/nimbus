use super::*;

#[tokio::test]
async fn convex_named_mutation_can_use_bootstrapped_ctx_scheduler_api() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:sendInternal",
                "kind": "mutation",
                "visibility": "internal",
                "schedulable": true,
                "plan": {
                    "type": "insert",
                    "table": "messages",
                    "fields": {
                        "body": { "$arg": "body" }
                    }
                }
            },
            {
                "name": "messages:scheduleInternal",
                "kind": "mutation",
                "plan": null,
                "runtime_handler": "async () => null"
            }
        ]),
        json!([]),
        Some(
            r#"
	globalThis.__nimbusInvoke = function(request) {
	  const ctx = globalThis.__nimbusCreateContext({
	    hostCallSessionId: `${request.kind}:${request.function_name}`,
	    request,
	  });
	  return (async () => {
    const value = await ctx.scheduler.runAfter(
      request.args.delayMs,
      {
        kind: "mutation",
        name: "messages:sendInternal",
        visibility: "internal",
      },
      {
        body: request.args.body,
      },
    );
    return {
      status: "ok",
      value: {
        ctx: true,
        value,
      },
    };
  })();
};

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(router_for_convex_team(service, registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());
    // #41 non-vacuous: an anonymous (no-bearer) selection of this silo is refused
    // by the all-fail-closed team gate; only the team-bound bearer is admitted.
    assert_convex_anonymous_query_refused(&server, "demo").await;

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:scheduleInternal",
            json!({
                "body": "Scheduled via ctx.scheduler",
                "delayMs": 0
            }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("bootstrapped ctx.scheduler response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ctx"], json!(true));
    assert!(body["value"].as_str().is_some());

    let documents = timeout(Duration::from_secs(2), async {
        loop {
            let response = api.list_documents("demo", "messages").await;
            let body = response
                .json::<serde_json::Value>()
                .await
                .expect("message list should parse");
            if body["data"].as_array().is_some_and(|documents| {
                documents
                    .iter()
                    .any(|document| document["body"] == json!("Scheduled via ctx.scheduler"))
            }) {
                break body;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("scheduled ctx.scheduler mutation should complete");
    assert_eq!(
        documents["data"][0]["body"],
        json!("Scheduled via ctx.scheduler")
    );

    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
}

/// Convex parity (PPSC3-B): actions carry `ctx.scheduler` just like
/// mutations — an action schedules immediately through the engine (no
/// execution unit) and the scheduled mutation must land.
#[tokio::test]
async fn convex_named_action_can_use_bootstrapped_ctx_scheduler_api() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:sendInternal",
                "kind": "mutation",
                "visibility": "internal",
                "schedulable": true,
                "plan": {
                    "type": "insert",
                    "table": "messages",
                    "fields": {
                        "body": { "$arg": "body" }
                    }
                }
            },
            {
                "name": "messages:scheduleFromAction",
                "kind": "action",
                "plan": null,
                "runtime_handler": "async () => null"
            }
        ]),
        json!([]),
        Some(
            r#"
	globalThis.__nimbusInvoke = function(request) {
	  const ctx = globalThis.__nimbusCreateContext({
	    hostCallSessionId: `${request.kind}:${request.function_name}`,
	    request,
	  });
	  return (async () => {
    const value = await ctx.scheduler.runAfter(
      request.args.delayMs,
      {
        kind: "mutation",
        name: "messages:sendInternal",
        visibility: "internal",
      },
      {
        body: request.args.body,
      },
    );
    return {
      status: "ok",
      value: {
        ctx: true,
        value,
      },
    };
  })();
};

export {};
"#,
        ),
    );
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let service = fixture.engine();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(router_for_convex_team(service, registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_action(
            "demo",
            "messages:scheduleFromAction",
            json!({
                "body": "Scheduled from an action",
                "delayMs": 0
            }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("action ctx.scheduler response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ctx"], json!(true));
    assert!(body["value"].as_str().is_some());

    let documents = timeout(Duration::from_secs(2), async {
        loop {
            let response = api.list_documents("demo", "messages").await;
            let body = response
                .json::<serde_json::Value>()
                .await
                .expect("message list should parse");
            if body["data"].as_array().is_some_and(|documents| {
                documents
                    .iter()
                    .any(|document| document["body"] == json!("Scheduled from an action"))
            }) {
                break body;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("action-scheduled mutation should complete");
    assert_eq!(
        documents["data"][0]["body"],
        json!("Scheduled from an action")
    );

    let _ = shutdown_tx.send(true);
    let _ = scheduler_handle.await;
}

/// Convex parity (PPSC3-B): queries have no scheduler surface. A query
/// bundle that reaches for the scheduler host op is rejected by the
/// invocation-kind guard and schedules nothing.
#[tokio::test]
async fn convex_named_query_cannot_use_ctx_scheduler_api() {
    let registry = convex_registry_with_routes_and_bundle(
        json!([
            {
                "name": "messages:sendInternal",
                "kind": "mutation",
                "visibility": "internal",
                "schedulable": true,
                "plan": {
                    "type": "insert",
                    "table": "messages",
                    "fields": {
                        "body": { "$arg": "body" }
                    }
                }
            },
            {
                "name": "messages:scheduleFromQuery",
                "kind": "query",
                "plan": null,
                "runtime_handler": "async () => null"
            }
        ]),
        json!([]),
        Some(
            r#"
	globalThis.__nimbusInvoke = function(request) {
	  const ctx = globalThis.__nimbusCreateContext({
	    hostCallSessionId: `${request.kind}:${request.function_name}`,
	    request,
	  });
	  return (async () => {
    const value = await ctx.scheduler.runAfter(
      0,
      {
        kind: "mutation",
        name: "messages:sendInternal",
        visibility: "internal",
      },
      {
        body: "must not schedule",
      },
    );
    return {
      status: "ok",
      value: {
        ctx: true,
        value,
      },
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
        .convex_named_query("demo", "messages:scheduleFromQuery", json!({}))
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("query scheduler rejection should parse");
    assert_ne!(status, StatusCode::OK, "{body}");
    // Two rejection layers exist: the JS context contract refuses to expose
    // ctx.scheduler to query handlers, and the Rust invocation-kind guard
    // rejects the host op if a bundle bypasses the context object. Accept
    // either message so the test pins the behavior, not the layer.
    assert!(
        body["error"]["message"].as_str().is_some_and(|message| {
            message.contains("cannot schedule")
                || message.contains("not available for query handlers")
        }),
        "rejection should name the scheduling guard: {body}"
    );

    let documents = api.list_documents("demo", "messages").await;
    let body = documents
        .json::<serde_json::Value>()
        .await
        .expect("message list should parse");
    assert!(
        body["data"]
            .as_array()
            .is_none_or(|documents| documents.is_empty()),
        "a rejected query schedule must leave nothing behind: {body}"
    );
}
