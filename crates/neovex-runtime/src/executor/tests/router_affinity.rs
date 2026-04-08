use super::*;

#[tokio::test]
async fn worker_router_prefers_tenant_affinity_for_warm_worker_reuse() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_isolates = 2;
    limits.worker_threads = 2;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let test_state = executor.test_state();
    let host = Arc::new(WorkerRuntimeIdHost {
        test_state: test_state.clone(),
    });
    let request = test_request("messages:list");

    let tenant_a_first = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-affinity-a-1"),
            None,
        )
        .await
        .expect("tenant-a invocation should succeed");
    let tenant_b_first = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context_for_tenant(&request, "tenant-b", "req-affinity-b-1"),
            None,
        )
        .await
        .expect("tenant-b invocation should succeed");
    let tenant_b_second = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host, policy),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context_for_tenant(&request, "tenant-b", "req-affinity-b-2"),
            None,
        )
        .await
        .expect("second tenant-b invocation should succeed");

    let tenant_a_worker = worker_runtime_id(&tenant_a_first);
    let tenant_b_worker = worker_runtime_id(&tenant_b_first);
    let tenant_b_second_worker = worker_runtime_id(&tenant_b_second);

    assert_ne!(
        tenant_a_worker, tenant_b_worker,
        "initial tie-broken routing should spread different tenants across workers"
    );
    assert_eq!(
        tenant_b_second_worker, tenant_b_worker,
        "tenant affinity should keep follow-up work on the warmed worker"
    );

    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.worker_dispatched_invocations, 3);
    assert_eq!(metrics.worker_affinity_routed_invocations, 1);
    assert_eq!(metrics.worker_least_loaded_routed_invocations, 2);
}

#[tokio::test]
async fn worker_router_uses_least_loaded_fallback_when_affinity_is_absent() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_isolates = 2;
    limits.worker_threads = 2;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let test_state = executor.test_state();
    let host = Arc::new(ControlledAsyncWorkerRuntimeIdHost::new(test_state));
    let bundle = RuntimeBundle::new(&bundle_path);

    let slow_request = test_request("slow-1");
    let slow_task = tokio::spawn({
        let executor = executor.clone();
        let host = host.clone();
        let bundle = bundle.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&slow_request, "tenant-a", "req-router-slow");
        async move {
            executor
                .invoke_on_worker(
                    NeovexRuntime::with_policy(host, policy),
                    bundle,
                    slow_request,
                    context,
                    None,
                )
                .await
        }
    });

    host.wait_until_started("slow-1").await;
    let slow_worker = host
        .started_runtime_id("slow-1")
        .expect("slow invocation should record a worker runtime id");

    let fast_request = test_request("fast-1");
    let fast_result = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy),
            bundle,
            fast_request.clone(),
            test_context_for_tenant(&fast_request, "tenant-b", "req-router-fast"),
            None,
        )
        .await
        .expect("fast invocation should succeed");

    assert_ne!(
        worker_runtime_id(&fast_result),
        slow_worker,
        "a tenant without affinity should fall back to the least-loaded worker"
    );

    host.release_slow_jobs();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), slow_task)
            .await
            .expect("slow invocation should complete after host release")
            .expect("slow invocation task should join")
            .expect("slow invocation should succeed")
            .get("workerRuntimeId")
            .and_then(Value::as_u64)
            .map(|id| id as usize)
            .expect("slow result should include a workerRuntimeId"),
        slow_worker,
        "slow invocation should resume and finish on its original worker"
    );

    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.worker_dispatched_invocations, 2);
    assert_eq!(metrics.worker_affinity_routed_invocations, 0);
    assert_eq!(metrics.worker_least_loaded_routed_invocations, 2);
}

#[tokio::test]
async fn worker_router_can_affinitize_by_function() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.routing_affinity = crate::limits::RuntimeRoutingAffinity::Function;
    limits.max_concurrent_isolates = 2;
    limits.worker_threads = 2;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let test_state = executor.test_state();
    let host = Arc::new(WorkerRuntimeIdHost {
        test_state: test_state.clone(),
    });

    let first_request = test_request("messages:list");
    let second_request = test_request("messages:get");

    let function_a_first = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            first_request.clone(),
            test_context_for_tenant(&first_request, "tenant-a", "req-function-a-1"),
            None,
        )
        .await
        .expect("first function-affinitized invocation should succeed");
    let function_b_first = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            second_request.clone(),
            test_context_for_tenant(&second_request, "tenant-a", "req-function-b-1"),
            None,
        )
        .await
        .expect("second function-affinitized invocation should succeed");
    let function_b_second = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host, policy),
            RuntimeBundle::new(&bundle_path),
            second_request.clone(),
            test_context_for_tenant(&second_request, "tenant-a", "req-function-b-2"),
            None,
        )
        .await
        .expect("repeated function-affinitized invocation should succeed");

    let function_a_worker = worker_runtime_id(&function_a_first);
    let function_b_worker = worker_runtime_id(&function_b_first);
    let function_b_second_worker = worker_runtime_id(&function_b_second);

    assert_ne!(
        function_a_worker, function_b_worker,
        "different functions within one tenant should not share function affinity"
    );
    assert_eq!(
        function_b_second_worker, function_b_worker,
        "matching tenant+function should route back to the warmed worker"
    );
}

#[tokio::test]
async fn worker_router_can_affinitize_by_script_identity() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir_a, bundle_path_a) = write_runtime_id_bundle();
    let (_bundle_dir_b, bundle_path_b) = write_runtime_id_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.routing_affinity = crate::limits::RuntimeRoutingAffinity::Script;
    limits.max_concurrent_isolates = 2;
    limits.worker_threads = 2;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let test_state = executor.test_state();
    let host = Arc::new(WorkerRuntimeIdHost {
        test_state: test_state.clone(),
    });
    let request = test_request("messages:list");

    let script_a_first = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path_a),
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-script-a-1"),
            None,
        )
        .await
        .expect("first script-affinitized invocation should succeed");
    let script_b_first = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path_b),
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-script-b-1"),
            None,
        )
        .await
        .expect("second script-affinitized invocation should succeed");
    let script_b_second = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host, policy),
            RuntimeBundle::new(&bundle_path_b),
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-script-b-2"),
            None,
        )
        .await
        .expect("repeated script-affinitized invocation should succeed");

    let script_a_worker = worker_runtime_id(&script_a_first);
    let script_b_worker = worker_runtime_id(&script_b_first);
    let script_b_second_worker = worker_runtime_id(&script_b_second);

    assert_ne!(
        script_a_worker, script_b_worker,
        "different bundle identities should not share script affinity"
    );
    assert_eq!(
        script_b_second_worker, script_b_worker,
        "matching bundle identity should route back to the warmed worker"
    );
}

#[tokio::test]
async fn worker_router_can_disable_affinity() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.routing_affinity = crate::limits::RuntimeRoutingAffinity::None;
    limits.max_concurrent_isolates = 2;
    limits.worker_threads = 2;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let test_state = executor.test_state();
    let host = Arc::new(WorkerRuntimeIdHost {
        test_state: test_state.clone(),
    });
    let request = test_request("messages:list");

    executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-no-affinity-1"),
            None,
        )
        .await
        .expect("first no-affinity invocation should succeed");
    executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host, policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-no-affinity-2"),
            None,
        )
        .await
        .expect("second no-affinity invocation should succeed");

    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.worker_affinity_routed_invocations, 0);
    assert_eq!(metrics.worker_least_loaded_routed_invocations, 2);
    assert_eq!(metrics.worker_affinity_cache_entries, 0);
    assert_eq!(metrics.worker_affinity_cache_evictions, 0);
}

#[tokio::test]
async fn worker_router_bounds_affinity_cache_and_records_evictions() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.routing_affinity = crate::limits::RuntimeRoutingAffinity::Tenant;
    limits.routing_affinity_max_entries = 1;
    limits.max_concurrent_isolates = 2;
    limits.worker_threads = 2;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let test_state = executor.test_state();
    let host = Arc::new(WorkerRuntimeIdHost {
        test_state: test_state.clone(),
    });
    let request = test_request("messages:list");

    executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-affinity-cap-a-1"),
            None,
        )
        .await
        .expect("first bounded-affinity invocation should succeed");
    executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context_for_tenant(&request, "tenant-b", "req-affinity-cap-b-1"),
            None,
        )
        .await
        .expect("second bounded-affinity invocation should succeed");
    executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host, policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-affinity-cap-a-2"),
            None,
        )
        .await
        .expect("third bounded-affinity invocation should succeed");

    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.worker_affinity_routed_invocations, 0);
    assert_eq!(metrics.worker_least_loaded_routed_invocations, 3);
    assert_eq!(metrics.worker_affinity_cache_entries, 1);
    assert_eq!(metrics.worker_affinity_cache_evictions, 2);
}
