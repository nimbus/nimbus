use super::helpers::runtime_request_drop_registry;
use super::*;

#[tokio::test]
async fn dropped_runtime_http_request_cancels_runtime_invocation() {
    let registry = runtime_request_drop_registry(json!([
        {
            "name": "messages:spin",
            "kind": "query",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async () => { while (true) {} }"
        }
    ]));
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let server = ServerFixture::start(build_router_with_convex(
        fixture.service(),
        registry.clone(),
    ))
    .await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let request = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:spin", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics(&registry, "runtime invocation to start", |metrics| {
        metrics.active_isolates >= 1 && metrics.worker_dispatched_invocations >= 1
    })
    .await;

    drop(request);

    let metrics = wait_for_runtime_metrics(
        &registry,
        "dropped runtime request cancellation",
        |metrics| metrics.active_isolates == 0 && metrics.canceled_invocations >= 1,
    )
    .await;
    assert_eq!(metrics.worker_dispatched_invocations, 1);
    assert_eq!(metrics.canceled_invocations, 1);
    assert_eq!(metrics.queued_canceled_invocations, 0);
    assert_eq!(metrics.in_flight_canceled_invocations, 1);
    assert_eq!(metrics.disconnect_canceled_invocations, 1);
    assert_eq!(metrics.explicit_canceled_invocations, 0);
    let tenant_metrics = metrics
        .tenants
        .get("demo")
        .expect("tenant runtime metrics should be present");
    assert_eq!(tenant_metrics.started_invocations, 1);
    assert_eq!(tenant_metrics.completed_invocations, 1);
    assert_eq!(tenant_metrics.queued_canceled_invocations, 0);
    assert_eq!(tenant_metrics.in_flight_canceled_invocations, 1);
    assert_eq!(tenant_metrics.disconnect_canceled_invocations, 1);
    assert_eq!(tenant_metrics.explicit_canceled_invocations, 0);
}
