use super::*;

#[tokio::test]
async fn runtime_metrics_route_returns_null_fields_without_convex_support() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server = ServerFixture::start(router_for_engine(fixture.engine())).await;
    let api = HttpApiFixture::new(&server);

    let response = api.runtime_metrics().await;

    // Returns 200 with a stable shape so the operator settings UI sees a
    // single null-fields payload instead of a 404 on default `nimbus start`.
    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime metrics json should parse");
    assert!(body["limits"].is_null());
    assert!(body["reset_capabilities"].is_null());
    assert!(body["metrics"].is_null());
    assert_eq!(body["lanes"], json!([]));
}

#[tokio::test]
async fn runtime_metrics_route_returns_limits_and_metrics_when_convex_support_is_enabled() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let server =
        ServerFixture::start(router_for_convex(fixture.engine(), ConvexRegistry::empty())).await;
    let api = HttpApiFixture::new(&server);

    let response = api.runtime_metrics().await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = response
        .json::<serde_json::Value>()
        .await
        .expect("runtime metrics json should parse");
    assert_eq!(body["limits"]["runtime_backend"], json!("v8"));
    assert_eq!(
        body["limits"]["runtime_backend_trust_tier"],
        json!("in_process_untrusted")
    );
    assert_eq!(
        body["limits"]["runtime_backend_lockdown_profile"],
        json!("v8_deno_core")
    );
    assert_eq!(
        body["limits"]["runtime_backend_lifecycle_policy"],
        json!("v8_deno_core_pool")
    );
    assert_eq!(
        body["limits"]["javascript_evaluation_format"],
        json!("es_module")
    );
    assert_eq!(
        body["limits"]["execution_model"],
        json!("cooperative_locker")
    );
    assert_eq!(body["limits"]["runtime_pool_kind"], json!("warm_pool"));
    assert_eq!(
        body["limits"]["memory_enforcement"],
        json!("v8_isolate_heap_limit")
    );
    assert_eq!(
        body["limits"]["module_state_semantics"],
        json!("warm_per_bundle")
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
    assert!(body["limits"]["max_warm_pool_entries_per_worker"].is_u64());
    assert!(body["limits"]["max_warm_reuses"].is_u64());
    assert_eq!(body["limits"]["max_heap_mb"], json!(128));
    assert_eq!(body["limits"]["initial_heap_mb"], json!(8));
    assert_eq!(body["limits"]["execution_timeout_ms"], json!(30_000));
    assert!(body["limits"]["max_concurrent_runtime_instances"].is_u64());
    assert!(body["limits"]["worker_threads"].is_u64());
    assert!(body["limits"]["max_active_top_level_invocations_per_tenant"].is_u64());
    assert!(body["limits"]["max_in_flight_top_level_invocations_per_tenant"].is_u64());
    assert!(body["limits"]["max_queued_top_level_invocations_per_tenant"].is_u64());
    assert_eq!(body["limits"]["max_nested_runtime_invocations"], json!(64));
    assert_eq!(
        body["limits"]["tenant_budget"]["max_heap_mb_per_runtime"],
        json!(128)
    );
    assert_eq!(
        body["limits"]["tenant_budget"]["memory_enforcement"],
        json!("v8_isolate_heap_limit")
    );
    assert!(body["limits"]["tenant_budget"]["max_active_runtime_slots"].is_u64());
    assert!(body["limits"]["tenant_budget"]["max_worker_thread_slots"].is_u64());
    assert_eq!(
        body["limits"]["tenant_budget"]["execution_timeout_ms"],
        json!(30_000)
    );
    assert_eq!(
        body["limits"]["tenant_budget"]["max_nested_runtime_invocations_per_top_level"],
        json!(64)
    );
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
        body["metrics"]["fallback_cross_runtime_dispatches"],
        json!(0)
    );

    let lanes = body["lanes"].as_array().expect("lanes should be an array");
    assert_eq!(lanes.len(), 6);
    let expected_lanes = [
        (
            "default",
            true,
            "v8",
            "web_standard_isolate",
            "linked",
            "v8_isolate_heap_limit",
        ),
        (
            "node20",
            false,
            "v8",
            "node20",
            "linked",
            "v8_isolate_heap_limit",
        ),
        (
            "node22",
            false,
            "v8",
            "node22",
            "linked",
            "v8_isolate_heap_limit",
        ),
        (
            "node24",
            false,
            "v8",
            "node24",
            "linked",
            "v8_isolate_heap_limit",
        ),
        (
            "node26",
            false,
            "v8",
            "node26",
            "linked",
            "v8_isolate_heap_limit",
        ),
        (
            "bun_jsc",
            false,
            "bun_jsc",
            "bun_jsc",
            "not_linked",
            "outer_quota_required",
        ),
    ];
    for (lane, expected) in lanes.iter().zip(expected_lanes) {
        let (
            lane_name,
            default_lane,
            runtime_backend,
            compatibility_target,
            execution_adapter_state,
            memory_enforcement,
        ) = expected;
        assert_eq!(lane["lane_name"], json!(lane_name));
        assert_eq!(lane["default_lane"], json!(default_lane));
        assert_eq!(lane["executor_started"], json!(false));
        assert_eq!(
            lane["execution_adapter_state"],
            json!(execution_adapter_state)
        );
        assert_eq!(
            lane["execution_adapter_artifact"]["status"],
            json!(execution_adapter_state)
        );
        assert_eq!(lane["limits"]["runtime_backend"], json!(runtime_backend));
        assert_eq!(
            lane["limits"]["compatibility_target"],
            json!(compatibility_target)
        );
        assert_eq!(
            lane["limits"]["memory_enforcement"],
            json!(memory_enforcement)
        );
        assert_eq!(
            lane["limits"]["tenant_budget"]["memory_enforcement"],
            json!(memory_enforcement)
        );
        assert_eq!(lane["metrics"]["worker_dispatched_invocations"], json!(0));
    }
    let bun_lane = lanes
        .iter()
        .find(|lane| lane["lane_name"] == json!("bun_jsc"))
        .expect("bun_jsc lane should be present");
    assert_eq!(
        bun_lane["execution_adapter_artifact"]["source"],
        json!("build_feature_disabled")
    );
    assert_eq!(
        bun_lane["execution_adapter_artifact"]["reason_code"],
        json!("linked_adapter_feature_disabled")
    );
    assert_eq!(
        bun_lane["execution_adapter_artifact"]["expected"]["source_ref"],
        json!("nimbus-bun-jsc-proof-main-20260709")
    );
    assert!(bun_lane["execution_adapter_artifact"]["manifest"].is_null());
}
