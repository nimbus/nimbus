use super::*;

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
