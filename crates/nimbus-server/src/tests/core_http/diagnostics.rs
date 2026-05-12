use super::*;

#[tokio::test]
async fn tenant_engine_metrics_route_returns_not_found_for_missing_tenant() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let response = api.tenant_engine_metrics("missing").await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn tenant_engine_metrics_route_surfaces_worker_and_serving_health_after_mixed_traffic() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );
    assert_eq!(
        api.insert_document("demo", "tasks", json!({ "title": "Ada" }))
            .await
            .status(),
        StatusCode::CREATED
    );

    let query = api
        .query_documents(
            "demo",
            json!({
                "table": "tasks",
                "filters": [],
                "order": null,
                "limit": null
            }),
        )
        .await;
    assert_eq!(query.status(), StatusCode::OK);
    let query_body = query
        .json::<serde_json::Value>()
        .await
        .expect("query response should parse");
    assert_eq!(query_body["data"].as_array().map(Vec::len), Some(1));

    let response = api.tenant_engine_metrics("demo").await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("tenant engine metrics response should parse");
    assert_eq!(body["tenant_id"], json!("demo"));

    let diagnostics = &body["diagnostics"];
    assert_eq!(diagnostics["mutation_journal"]["durable_head"], json!(1));
    assert_eq!(diagnostics["mutation_journal"]["applied_head"], json!(1));
    assert_eq!(diagnostics["mutation_journal"]["apply_lag"], json!(0));
    assert_eq!(diagnostics["mutation_journal"]["queue_depth"], json!(0));
    assert_eq!(
        diagnostics["mutation_journal"]["worker_running"],
        json!(false)
    );
    assert_eq!(
        diagnostics["mutation_journal"]["worker_start_count"],
        json!(1)
    );
    assert_eq!(
        diagnostics["mutation_journal"]["queue_rejection_count"],
        json!(0)
    );
    assert_eq!(
        diagnostics["mutation_journal"]["worker_failure_count"],
        json!(0)
    );

    assert_eq!(
        diagnostics["subscription_delivery"]["queue_depth"],
        json!(0)
    );
    assert_eq!(
        diagnostics["subscription_delivery"]["worker_running"],
        json!(false)
    );
    assert_eq!(
        diagnostics["subscription_delivery"]["worker_start_count"],
        json!(0)
    );

    assert_eq!(
        diagnostics["materialized_read_surface"]["loaded_table_count"],
        json!(1)
    );
    assert_eq!(
        diagnostics["materialized_read_surface"]["table_load_count"],
        json!(1)
    );
    assert_eq!(
        diagnostics["materialized_read_surface"]["latest_covered_sequence"],
        json!(1)
    );
    assert_eq!(
        diagnostics["serving_snapshot_manager"]["retained_snapshot_count"],
        json!(1)
    );
    assert_eq!(
        diagnostics["serving_snapshot_manager"]["latest_retained_sequence"],
        json!(1)
    );
    assert_eq!(
        diagnostics["query_planning"]["query_full_scan_count"],
        json!(1)
    );
}
