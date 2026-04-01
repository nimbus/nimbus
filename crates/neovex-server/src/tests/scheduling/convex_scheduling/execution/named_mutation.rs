use super::*;

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
