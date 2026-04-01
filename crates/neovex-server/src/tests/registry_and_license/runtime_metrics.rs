use super::*;

#[tokio::test]
async fn runtime_metrics_route_requires_convex_support() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router(fixture.service())).await;
    let api = HttpApiFixture::new(&server);

    let response = api.runtime_metrics().await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn runtime_metrics_route_returns_limits_and_metrics_when_convex_support_is_enabled() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        ConvexRegistry::empty(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    let response = api.runtime_metrics().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime metrics json should parse");
    assert_eq!(body["limits"]["max_heap_mb"], json!(128));
    assert_eq!(body["limits"]["initial_heap_mb"], json!(8));
    assert_eq!(body["limits"]["execution_timeout_ms"], json!(30_000));
    assert!(body["limits"]["max_concurrent_isolates"].is_u64());
    assert!(body["limits"]["max_top_level_invocations_per_tenant"].is_u64());
    assert!(body["limits"]["max_queued_top_level_invocations_per_tenant"].is_u64());
    assert_eq!(body["limits"]["max_nested_runtime_invocations"], json!(64));
    assert_eq!(body["metrics"]["worker_dispatched_invocations"], json!(0));
    assert_eq!(body["metrics"]["nested_local_dispatches"], json!(0));
    assert_eq!(body["metrics"]["rejected_invocations"], json!(0));
    assert_eq!(body["metrics"]["queued_canceled_invocations"], json!(0));
    assert_eq!(body["metrics"]["in_flight_canceled_invocations"], json!(0));
    assert_eq!(body["metrics"]["disconnect_canceled_invocations"], json!(0));
    assert_eq!(body["metrics"]["explicit_canceled_invocations"], json!(0));
    assert_eq!(body["metrics"]["precanceled_host_ops"], json!(0));
    assert_eq!(body["metrics"]["in_flight_canceled_host_ops"], json!(0));
    assert_eq!(body["metrics"]["host_operations"], json!({}));
    assert_eq!(body["metrics"]["tenants"], json!({}));
    assert_eq!(body["metrics"]["recent_request_correlations"], json!([]));
    assert_eq!(
        body["metrics"]["fallback_cross_isolate_dispatches"],
        json!(0)
    );
}
