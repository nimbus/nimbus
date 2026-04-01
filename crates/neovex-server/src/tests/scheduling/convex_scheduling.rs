use super::*;

#[tokio::test]
async fn convex_schedule_after_executes_named_public_mutation() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "body": { "$arg": "body" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(service, registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schedule = api
        .convex_schedule_after(
            "demo",
            json!({
                "name": "messages:send",
                "args": { "body": "Convex scheduled" },
                "run_after_ms": 0
            }),
        )
        .await;
    assert_eq!(schedule.status(), StatusCode::CREATED);

    let history = timeout(Duration::from_secs(3), async {
        loop {
            let list = api.list_documents("demo", "messages").await;
            let body = list
                .json::<serde_json::Value>()
                .await
                .expect("list response should parse");
            let data = body["data"].as_array().expect("data should be an array");
            if !data.is_empty() {
                break data[0].clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("scheduled mutation should execute");
    assert_eq!(history["body"], json!("Convex scheduled"));

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_named_mutation_can_schedule_internal_generated_mutation() {
    let registry = convex_registry(json!([
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
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "schedule_run_after",
                "delay_ms": { "$arg": "delayMs" },
                "name": "messages:sendInternal",
                "visibility": "internal",
                "args": {
                    "body": { "$arg": "body" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(service, registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:scheduleInternal",
            json!({
                "body": "Scheduled via handler",
                "delayMs": 0
            }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("convex named mutation should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(body.as_str().is_some());

    let inserted = timeout(Duration::from_secs(3), async {
        loop {
            let body = api
                .list_documents("demo", "messages")
                .await
                .json::<serde_json::Value>()
                .await
                .expect("documents should parse");
            let data = body["data"].as_array().expect("data should be an array");
            if let Some(document) = data.first() {
                break document.clone();
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("scheduled internal mutation should execute");
    assert_eq!(inserted["body"], json!("Scheduled via handler"));

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_ctx_mutation_host_binding_can_schedule_internal_generated_mutation() {
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
                "visibility": "public",
                "schedulable": true,
                "plan": {
                    "type": "schedule_run_after",
                    "delay_ms": { "$arg": "delayMs" },
                    "name": "messages:sendInternal",
                    "visibility": "internal",
                    "args": {
                        "body": { "$arg": "body" }
                    }
                }
            }
        ]),
        json!([]),
        Some(
            r#"
const definitions = new Map([
  ["messages:scheduleInternal", {
    name: "messages:scheduleInternal",
    kind: "mutation",
    plan: {
      type: "schedule_run_after",
      delay_ms: { $arg: "delayMs" },
      name: "messages:sendInternal",
      visibility: "internal",
      args: {
        body: { $arg: "body" },
      },
    },
  }],
]);

function resolveTemplate(template, args) {
  if (template === null || typeof template !== "object") {
    return template;
  }
  if (Array.isArray(template)) {
    return template.map((item) => resolveTemplate(item, args));
  }
  if (typeof template.$arg === "string" && Object.keys(template).length === 1) {
    return args[template.$arg];
  }
  const resolved = {};
  for (const [key, value] of Object.entries(template)) {
    resolved[key] = resolveTemplate(value, args);
  }
  return resolved;
}

globalThis.__neovexInvoke = async function(request) {
  const definition = definitions.get(request.function_name);
  const value = await globalThis.__neovexAsyncHostValue("op_neovex_ctx_mutation", {
    mutation: resolveTemplate(definition.plan, request.args ?? {}),
    session_id: `${request.kind}:${request.function_name}`,
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
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router_with_convex(service, registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_named_mutation(
            "demo",
            "messages:scheduleInternal",
            json!({
                "body": "Scheduled via ctx.mutation host binding",
                "delayMs": 0
            }),
        )
        .await;
    let status = response.status();
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("ctx mutation scheduler response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["ctx"], json!(true));
    assert!(body["value"].as_str().is_some());

    let inserted = timeout(Duration::from_secs(3), async {
        loop {
            let body = api
                .list_documents("demo", "messages")
                .await
                .json::<serde_json::Value>()
                .await
                .expect("scheduled documents should parse");
            if body["data"].as_array().is_some_and(|documents| {
                documents.iter().any(|document| {
                    document["body"] == json!("Scheduled via ctx.mutation host binding")
                })
            }) {
                return body;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("runtime scheduler host-binding mutation should execute");
    assert!(
        inserted["data"]
            .as_array()
            .is_some_and(|documents| !documents.is_empty())
    );

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}

#[tokio::test]
async fn convex_schedule_endpoints_reject_internal_mutations() {
    let registry = convex_registry(json!([
        {
            "name": "messages:internalSend",
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
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .convex_schedule_after(
            "demo",
            json!({
                "name": "messages:internalSend",
                "args": { "body": "Nope" },
                "run_after_ms": 0
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(
        response
            .json::<serde_json::Value>()
            .await
            .expect("schedule error should parse")["error"]
            .as_str()
            .expect("error should be a string")
            .contains("not public")
    );
}

#[tokio::test]
async fn convex_cancel_scheduled_job_removes_pending_named_mutation() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {
                    "body": { "$arg": "body" }
                }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schedule = api
        .convex_schedule_after(
            "demo",
            json!({
                "name": "messages:send",
                "args": { "body": "Later" },
                "run_after_ms": 60_000
            }),
        )
        .await;
    assert_eq!(schedule.status(), StatusCode::CREATED);
    let job_id = schedule
        .json::<serde_json::Value>()
        .await
        .expect("convex schedule response should parse")["job_id"]
        .as_str()
        .expect("convex job id should be present")
        .to_string();

    assert_eq!(
        api.convex_cancel_scheduled_job("demo", &job_id)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    let jobs = api
        .list_scheduled_jobs("demo")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("jobs should parse");
    assert_eq!(jobs["jobs"], json!([]));
}

#[tokio::test]
async fn convex_named_mutation_can_cancel_scheduled_job() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "visibility": "public",
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
            "name": "jobs:cancel",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "schedule_cancel",
                "job_id": { "$arg": "jobId" }
            }
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(fixture.service(), registry)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let scheduled = api
        .convex_schedule_after(
            "demo",
            json!({
                "name": "messages:send",
                "args": { "body": "Later" },
                "run_after_ms": 60_000
            }),
        )
        .await;
    assert_eq!(scheduled.status(), StatusCode::CREATED);
    let job_id = scheduled
        .json::<serde_json::Value>()
        .await
        .expect("schedule response should parse")["job_id"]
        .as_str()
        .expect("job id should be present")
        .to_string();

    let cancelled = api
        .convex_named_mutation("demo", "jobs:cancel", json!({ "jobId": job_id }))
        .await;
    let status = cancelled.status();
    let body = cancelled
        .json::<serde_json::Value>()
        .await
        .expect("cancel response should parse");
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body, serde_json::Value::Null);

    let jobs = api
        .list_scheduled_jobs("demo")
        .await
        .json::<serde_json::Value>()
        .await
        .expect("jobs should parse");
    assert_eq!(jobs["jobs"], json!([]));
}
