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
globalThis.__neovexInvoke = function(request) {
  const ctx = globalThis.__neovexCreateContext();
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
