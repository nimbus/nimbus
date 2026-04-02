use super::helpers::runtime_request_drop_registry;
use super::*;
use neovex_core::Timestamp;

#[tokio::test]
async fn dropped_queued_runtime_request_never_starts_mutation() {
    let registry = runtime_request_drop_registry(json!([
        {
            "name": "messages:block",
            "kind": "query",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async () => { while (true) {} }"
        },
        {
            "name": "messages:insertQueued",
            "kind": "mutation",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async (ctx, { body }) => await ctx.db.insert(\"messages\", { body })"
        }
    ]))
    .with_runtime_limits(RuntimeLimits {
        max_concurrent_isolates: 1,
        ..RuntimeLimits::default()
    });
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let server =
        ServerFixture::start(build_router_with_convex(service.clone(), registry.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let blocker = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics(&registry, "blocking runtime query to start", |metrics| {
        metrics.active_isolates == 1 && metrics.worker_dispatched_invocations == 1
    })
    .await;

    let queued_mutation = open_json_post_stream(
        &server,
        "/convex/demo/mutation",
        &json!({ "name": "messages:insertQueued", "args": { "body": "queued" } }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        registry
            .runtime_metrics_snapshot()
            .worker_dispatched_invocations,
        1
    );

    drop(queued_mutation);
    drop(blocker);

    let metrics = wait_for_runtime_metrics(
        &registry,
        "queued runtime mutation cancellation",
        |metrics| metrics.active_isolates == 0 && metrics.canceled_invocations >= 2,
    )
    .await;
    assert_eq!(metrics.worker_dispatched_invocations, 1);
    assert_eq!(metrics.queued_canceled_invocations, 1);
    assert_eq!(metrics.in_flight_canceled_invocations, 1);
    assert_eq!(metrics.disconnect_canceled_invocations, 2);
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
    assert_eq!(tenant_metrics.queued_canceled_invocations, 1);
    assert_eq!(tenant_metrics.in_flight_canceled_invocations, 1);
    assert_eq!(tenant_metrics.disconnect_canceled_invocations, 2);
    assert_eq!(tenant_metrics.explicit_canceled_invocations, 0);
    assert!(
        metrics
            .recent_request_correlations
            .iter()
            .any(|correlation| {
                correlation.function_name == "messages:block"
                    && correlation.server_request_id.starts_with("convex-query-")
            })
    );
    assert!(
        metrics
            .recent_request_correlations
            .iter()
            .any(|correlation| {
                correlation.function_name == "messages:insertQueued"
                    && correlation
                        .server_request_id
                        .starts_with("convex-mutation-")
            })
    );

    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let documents = service
        .list_documents(
            &tenant_id,
            &TableName::new("messages").expect("table name should be valid"),
        )
        .expect("listing queued mutation table should succeed");
    assert!(documents.is_empty(), "queued mutation should never start");
}

#[tokio::test]
async fn dropped_queued_runtime_request_recovers_and_serves_new_work_after_pressure_clears() {
    let registry = runtime_request_drop_registry(json!([
        {
            "name": "messages:block",
            "kind": "query",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async () => { while (true) {} }"
        },
        {
            "name": "messages:insertQueued",
            "kind": "mutation",
            "visibility": "public",
            "plan": null,
            "runtime_handler": "async (ctx, { body }) => await ctx.db.insert(\"messages\", { body })"
        }
    ]))
    .with_runtime_limits(RuntimeLimits {
        max_concurrent_isolates: 1,
        ..RuntimeLimits::default()
    });
    let harness =
        DeterministicHarness::scenario("runtime-request-drop-recovery", 75, Timestamp(75_000));
    let fixture = ServiceFixture::new_with_harness(harness.clone(), |path, harness| {
        Service::new_with_simulation(path, harness.clock(), harness.fault_injector())
    });
    let service = fixture.service();
    let server =
        ServerFixture::start(build_router_with_convex(service.clone(), registry.clone())).await;
    let api = HttpApiFixture::new(&server);

    assert_eq!(
        api.create_tenant("demo").await.status(),
        StatusCode::CREATED
    );

    let blocker = open_json_post_stream(
        &server,
        "/convex/demo/query",
        &json!({ "name": "messages:block", "args": {} }),
    )
    .await;
    wait_for_runtime_metrics(&registry, "blocking runtime query to start", |metrics| {
        metrics.active_isolates == 1 && metrics.worker_dispatched_invocations == 1
    })
    .await;

    let queued_mutation = open_json_post_stream(
        &server,
        "/convex/demo/mutation",
        &json!({ "name": "messages:insertQueued", "args": { "body": "queued" } }),
    )
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        registry
            .runtime_metrics_snapshot()
            .worker_dispatched_invocations,
        1
    );

    drop(queued_mutation);
    drop(blocker);

    let canceled = wait_for_runtime_metrics(
        &registry,
        "queued runtime mutation cancellation",
        |metrics| metrics.active_isolates == 0 && metrics.canceled_invocations >= 2,
    )
    .await;
    assert_eq!(canceled.worker_dispatched_invocations, 1);
    assert_eq!(canceled.queued_canceled_invocations, 1);
    assert_eq!(canceled.in_flight_canceled_invocations, 1);

    let recovery_response = api
        .convex_named_mutation(
            "demo",
            "messages:insertQueued",
            json!({ "body": "after-heal" }),
        )
        .await;
    assert_eq!(recovery_response.status(), StatusCode::OK);

    let recovered = wait_for_runtime_metrics(
        &registry,
        "runtime recovery after queued request drop",
        |metrics| {
            metrics.active_isolates == 0
                && metrics.worker_dispatched_invocations == 2
                && metrics.started_invocations == 2
                && metrics.completed_invocations == 2
        },
    )
    .await;
    assert_eq!(recovered.isolate_pool_hits, 1);
    assert_eq!(recovered.queued_canceled_invocations, 1);
    assert_eq!(recovered.in_flight_canceled_invocations, 1);
    assert_eq!(recovered.disconnect_canceled_invocations, 2);

    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    let documents = service
        .list_documents(
            &tenant_id,
            &TableName::new("messages").expect("table name should be valid"),
        )
        .expect("listing recovered mutation table should succeed");
    assert_eq!(
        documents.len(),
        1,
        "recovery mutation should persist exactly once"
    );
    assert_eq!(documents[0].fields.get("body"), Some(&json!("after-heal")));
}
