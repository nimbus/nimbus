use super::*;

#[tokio::test]
async fn schedule_endpoint_returns_job_id_and_lists_pending_job() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let response = api
        .schedule_mutation(
            "demo",
            json!({
                "run_after_ms": 5_000,
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "Hello" }
                }
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("schedule response should parse");
    assert!(body["job_id"].as_str().is_some());

    let jobs = api.list_scheduled_jobs("demo").await;
    assert_eq!(jobs.status(), StatusCode::OK);
    let jobs_body = jobs
        .json::<serde_json::Value>()
        .await
        .expect("scheduled jobs should parse");
    let jobs = jobs_body["jobs"]
        .as_array()
        .expect("jobs should be an array");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0]["mutation"]["type"], json!("insert"));
    assert_eq!(jobs[0]["mutation"]["table"], json!("tasks"));
}

#[tokio::test]
async fn schedule_endpoint_returns_not_found_for_unknown_tenant() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let response = api
        .schedule_mutation(
            "missing",
            json!({
                "run_after_ms": 100,
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "Hello" }
                }
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn cancel_scheduled_job_endpoint_removes_pending_job() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let schedule = api
        .schedule_mutation(
            "demo",
            json!({
                "run_after_ms": 5_000,
                "mutation": {
                    "type": "insert",
                    "table": "tasks",
                    "fields": { "title": "Cancel me" }
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
        .expect("job id should be present")
        .to_string();

    assert_eq!(
        api.cancel_scheduled_job("demo", &job_id).await.status(),
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
