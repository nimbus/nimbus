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
        },
        {
            "name": "messages:echo",
            "kind": "query",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async (_ctx, { value }) => value"
        }
    ]))
    .with_runtime_limits(RuntimeLimits {
        max_concurrent_isolates: 1,
        ..RuntimeLimits::default()
    });
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
    assert_eq!(metrics.isolate_pool_misses, 1);
    assert_eq!(metrics.isolate_pool_hits, 0);
    assert_eq!(metrics.isolate_pool_replacements, 1);
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

    let recovery_response = api
        .convex_named_query("demo", "messages:echo", json!({ "value": "after-cancel" }))
        .await;
    assert_eq!(recovery_response.status(), StatusCode::OK);
    let recovery_body = recovery_response
        .json::<serde_json::Value>()
        .await
        .expect("recovery runtime query response should parse");
    assert_eq!(recovery_body, json!("after-cancel"));

    let recovery_metrics = wait_for_runtime_metrics(
        &registry,
        "recovery runtime invocation after cancellation",
        |metrics| {
            metrics.worker_dispatched_invocations == 2
                && metrics.completed_invocations == 2
                && metrics.isolate_pool_replacements == 1
        },
    )
    .await;
    assert_eq!(
        recovery_metrics.isolate_pool_hits + recovery_metrics.isolate_pool_misses,
        2,
        "the canceled invocation and recovery invocation should each contribute one pool outcome"
    );
}
