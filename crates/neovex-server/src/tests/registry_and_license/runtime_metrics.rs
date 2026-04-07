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
    assert_eq!(body["limits"]["runtime_backend"], json!("deno_core"));
    assert_eq!(
        body["limits"]["execution_model"],
        json!("run_to_completion")
    );
    assert_eq!(
        body["limits"]["runtime_pool_kind"],
        json!("startup_snapshot_cache")
    );
    assert_eq!(
        body["limits"]["module_state_semantics"],
        json!("fresh_per_invocation")
    );
    assert_eq!(
        body["reset_capabilities"]["op_state_per_invocation"],
        json!(true)
    );
    assert_eq!(
        body["reset_capabilities"]["bootstrap_state_per_invocation"],
        json!(true)
    );
    assert_eq!(
        body["reset_capabilities"]["user_module_state_per_invocation"],
        json!(false)
    );
    assert_eq!(body["limits"]["routing_affinity"], json!("tenant"));
    assert!(body["limits"]["routing_affinity_max_entries"].is_u64());
    assert_eq!(body["limits"]["max_retained_runtimes_per_worker"], json!(4));
    assert_eq!(
        body["limits"]["max_retained_runtimes_per_affinity_key_per_worker"],
        json!(1)
    );
    assert_eq!(body["limits"]["max_retained_runtime_reuses"], json!(1000));
    assert_eq!(body["limits"]["max_heap_mb"], json!(128));
    assert_eq!(body["limits"]["initial_heap_mb"], json!(8));
    assert_eq!(body["limits"]["execution_timeout_ms"], json!(30_000));
    assert!(body["limits"]["max_concurrent_isolates"].is_u64());
    assert!(body["limits"]["worker_threads"].is_u64());
    assert!(body["limits"]["max_active_top_level_invocations_per_tenant"].is_u64());
    assert!(body["limits"]["max_in_flight_top_level_invocations_per_tenant"].is_u64());
    assert!(body["limits"]["max_queued_top_level_invocations_per_tenant"].is_u64());
    assert_eq!(body["limits"]["max_nested_runtime_invocations"], json!(64));
    assert_eq!(body["metrics"]["worker_dispatched_invocations"], json!(0));
    assert_eq!(
        body["metrics"]["worker_affinity_routed_invocations"],
        json!(0)
    );
    assert_eq!(
        body["metrics"]["worker_least_loaded_routed_invocations"],
        json!(0)
    );
    assert_eq!(body["metrics"]["worker_affinity_cache_entries"], json!(0));
    assert_eq!(body["metrics"]["worker_affinity_cache_evictions"], json!(0));
    assert_eq!(body["metrics"]["retained_runtime_pool_entries"], json!(0));
    assert_eq!(body["metrics"]["retained_runtime_pool_evictions"], json!(0));
    assert_eq!(
        body["metrics"]["retained_runtime_pool_retirements"],
        json!(0)
    );
    assert_eq!(
        body["metrics"]["retained_runtime_main_realm_resets"],
        json!(0)
    );
    assert_eq!(
        body["metrics"]["retained_runtime_main_realm_reset_nanos_total"],
        json!(0)
    );
    assert_eq!(
        body["metrics"]["retained_runtime_bootstrap_replays"],
        json!(0)
    );
    assert_eq!(
        body["metrics"]["retained_runtime_bootstrap_replay_nanos_total"],
        json!(0)
    );
    assert_eq!(body["metrics"]["bundle_loads"], json!(0));
    assert_eq!(body["metrics"]["bundle_load_nanos_total"], json!(0));
    assert_eq!(body["metrics"]["bundle_module_loads"], json!(0));
    assert_eq!(body["metrics"]["bundle_module_load_nanos_total"], json!(0));
    assert_eq!(body["metrics"]["bundle_evaluations"], json!(0));
    assert_eq!(body["metrics"]["bundle_evaluation_nanos_total"], json!(0));
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
