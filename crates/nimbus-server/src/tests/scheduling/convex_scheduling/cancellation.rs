use super::*;

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
    let server = ServerFixture::start(
        RouterBuildConfig::core(fixture.service())
            .with_convex(registry)
            .with_system_convex_registry(
                ConvexRegistry::from_embedded_system_bundle()
                    .expect("embedded system Convex registry should load"),
            )
            .build(),
    )
    .await;
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

    let system_jobs = api
        .convex_named_query(
            "_nimbus",
            "scheduled_jobs:list",
            json!({ "tenantId": "demo", "status": null, "limit": null }),
        )
        .await;
    assert_eq!(system_jobs.status(), StatusCode::OK);
    let system_jobs = system_jobs
        .json::<serde_json::Value>()
        .await
        .expect("system scheduled jobs query should parse");
    assert_eq!(
        system_jobs
            .as_array()
            .expect("system jobs should be an array")
            .len(),
        1
    );

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

    let system_jobs = api
        .convex_named_query(
            "_nimbus",
            "scheduled_jobs:list",
            json!({ "tenantId": "demo", "status": null, "limit": null }),
        )
        .await;
    assert_eq!(system_jobs.status(), StatusCode::OK);
    let system_jobs = system_jobs
        .json::<serde_json::Value>()
        .await
        .expect("system scheduled jobs query should parse");
    assert_eq!(system_jobs, json!([]));
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
