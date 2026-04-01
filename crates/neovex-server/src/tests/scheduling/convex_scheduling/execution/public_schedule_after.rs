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
