use super::*;

#[tokio::test]
async fn scheduled_mutation_over_http_drives_websocket_push() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router(service)).await;
    let api = HttpApiFixture::new(&server);

    assert!(api.create_tenant("demo").await.status().is_success());
    assert_eq!(
        api.set_table_schema(
            "demo",
            "tasks",
            json!({
                "table": "tasks",
                "fields": [
                    { "name": "priority", "field_type": "number", "required": false },
                    { "name": "title", "field_type": "string", "required": false }
                ],
                "indexes": [
                    { "name": "by_priority", "field": "priority" }
                ]
            }),
        )
        .await
        .status(),
        reqwest::StatusCode::NO_CONTENT
    );

    let mut socket = WebSocketFixture::connect(&api.ws_url("/ws"), "demo").await;
    socket
        .send_text(
            json!({
                "type": "subscribe",
                "request_id": "sched-http",
                "query": {
                    "table": "tasks",
                    "filters": [
                        { "field": "priority", "op": "lte", "value": 5 }
                    ],
                    "order": { "field": "priority", "direction": "asc" },
                    "limit": null
                }
            })
            .to_string(),
        )
        .await;

    let initial = socket.next_json().await;
    assert_eq!(initial["type"], json!("subscription_result"));
    assert_eq!(initial["request_id"], json!("sched-http"));
    assert_eq!(initial["data"], json!([]));

    let schedule = api
        .schedule_mutation(
            "demo",
            json!({
                "run_after_ms": 0,
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "Scheduled task", "priority": 1 }
                }
            }),
        )
        .await;
    assert_eq!(schedule.status(), reqwest::StatusCode::CREATED);

    let pushed = socket
        .next_json_with_timeout(Duration::from_secs(3))
        .await
        .expect("scheduled reactive push should arrive");
    assert_eq!(pushed["type"], json!("subscription_result"));
    assert!(pushed.get("request_id").is_none());
    assert_eq!(pushed["data"][0]["title"], json!("Scheduled task"));
    assert_eq!(pushed["data"][0]["priority"], json!(1));

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}
