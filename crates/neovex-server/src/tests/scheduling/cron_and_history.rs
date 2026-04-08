use super::*;

pub(crate) const SCHEDULED_JOB_HISTORY_FAILURE_CASE: DeterministicTestCase =
    DeterministicTestCase::new(
        "scheduled-job-history-failure-publication",
        "run-to-completion-snapshot",
        "scheduled job history publishes failed results once the scheduler applies the attempted mutation",
    );

#[tokio::test]
async fn cron_endpoints_create_list_and_delete_jobs() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let create = api
        .create_cron_job(
            "demo",
            json!({
                "name": "heartbeat",
                "schedule": { "type": "interval", "seconds": 10 },
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "heartbeat" }
                }
            }),
        )
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);

    let duplicate = api
        .create_cron_job(
            "demo",
            json!({
                "name": "heartbeat",
                "schedule": { "type": "interval", "seconds": 10 },
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "heartbeat" }
                }
            }),
        )
        .await;
    assert_eq!(duplicate.status(), StatusCode::CONFLICT);

    let list = api.list_cron_jobs("demo").await;
    assert_eq!(list.status(), StatusCode::OK);
    let list_body = list
        .json::<serde_json::Value>()
        .await
        .expect("cron list should parse");
    let crons = list_body["crons"]
        .as_array()
        .expect("crons should be an array");
    assert_eq!(crons.len(), 1);
    assert_eq!(crons[0]["name"], json!("heartbeat"));
    assert_eq!(crons[0]["schedule"]["type"], json!("interval"));

    let delete = api.delete_cron_job("demo", "heartbeat").await;
    assert_eq!(delete.status(), StatusCode::NO_CONTENT);

    let list = api.list_cron_jobs("demo").await;
    let list_body = list
        .json::<serde_json::Value>()
        .await
        .expect("cron list should parse");
    assert_eq!(list_body["crons"], json!([]));
}

#[tokio::test]
async fn scheduled_job_history_endpoint_reports_failures() {
    scheduled_job_history_endpoint_reports_failures_inner().await;
}

pub(crate) async fn scheduled_job_history_endpoint_reports_failures_inner() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let scheduler_handle = tokio::spawn(run_scheduler(service.clone(), shutdown_rx));
    let server = ServerFixture::start(build_router(service)).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let schema = json!({
        "table": "users",
        "fields": [
            { "name": "name", "field_type": "string", "required": true }
        ],
        "indexes": []
    });
    assert_eq!(
        api.set_table_schema("demo", "users", schema).await.status(),
        StatusCode::NO_CONTENT
    );

    let schedule = api
        .schedule_mutation(
            "demo",
            json!({
                "run_after_ms": 0,
                "mutation": {
                    "type": "insert",
                    "table": "users",
                    "fields": { "age": 42 }
                }
            }),
        )
        .await;
    assert_eq!(schedule.status(), StatusCode::CREATED);
    let job_id = schedule
        .json::<serde_json::Value>()
        .await
        .expect("schedule response should parse")["job_id"]
        .as_str()
        .expect("job_id should be present")
        .to_string();

    let history = wait_for_value(
        &SCHEDULED_JOB_HISTORY_FAILURE_CASE.failure_context_with_repro(
            "scheduled job history should become available",
            "cargo test -p neovex-server scheduled_job_history_endpoint_reports_failures -- --nocapture",
        ),
        Duration::from_secs(3),
        Duration::from_millis(50),
        || api.get_scheduled_job_result("demo", &job_id),
        |response| response.status() == StatusCode::OK,
    )
    .await;

    let body = history
        .json::<serde_json::Value>()
        .await
        .expect("history response should parse");
    assert_eq!(
        body["result"]["outcome"],
        json!("failed"),
        "{}",
        SCHEDULED_JOB_HISTORY_FAILURE_CASE.failure_context_with_repro(
            "scheduled job history should record the failed outcome",
            "cargo test -p neovex-server scheduled_job_history_endpoint_reports_failures -- --nocapture",
        )
    );
    assert!(
        body["result"]["error"]
            .as_str()
            .expect("error should be present")
            .contains("schema validation error")
    );

    let _ = shutdown_tx.send(true);
    scheduler_handle.await.expect("scheduler should shut down");
}
