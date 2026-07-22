use super::*;

#[tokio::test]
async fn convex_http_run_at_uses_engine_wall_clock() {
    let registry = convex_registry(json!([
        {
            "name": "messages:send",
            "kind": "mutation",
            "visibility": "public",
            "schedulable": true,
            "plan": {
                "type": "insert",
                "table": "messages",
                "fields": {}
            }
        }
    ]));
    let clock = Arc::new(nimbus_core::ManualWallClock::new(nimbus_core::Timestamp(
        50_000,
    )));
    let fixture = EngineFixture::new({
        let clock = clock.clone();
        move |path| {
            Engine::new_with_simulation(
                path,
                clock.clone(),
                Arc::new(nimbus_storage::NoopFaultInjector),
            )
        }
    });
    let service = fixture.engine();
    let server = ServerFixture::start(router_for_convex_team(service.clone(), registry)).await;
    let api = HttpApiFixture::with_convex_bearer(&server, convex_team_bearer());

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    let response = api
        .convex_schedule_at(
            "demo",
            json!({
                "name": "messages:send",
                "args": {},
                "run_at_ms": 1_000
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);

    let jobs = service
        .list_scheduled_jobs(&TenantId::new("demo").expect("tenant id should parse"))
        .expect("scheduled jobs should list");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].run_at, nimbus_core::Timestamp(1_000));
    assert_eq!(jobs[0].created_at, nimbus_core::Timestamp(50_000));
}
