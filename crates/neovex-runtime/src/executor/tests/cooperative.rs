use super::*;

#[tokio::test]
async fn cooperative_execution_model_processes_worker_invocations() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_isolates = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let test_state = executor.test_state();
    let host = Arc::new(WorkerRuntimeIdHost {
        test_state: test_state.clone(),
    });
    let request = test_request("messages:list");

    let first_result = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context(&request, "req-cooperative-1"),
            None,
        )
        .await
        .expect("first cooperative worker invocation should succeed");
    let second_result = executor
        .invoke_on_worker(
            NeovexRuntime::with_policy(host, policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context(&request, "req-cooperative-2"),
            None,
        )
        .await
        .expect("second cooperative worker invocation should succeed");

    assert_eq!(first_result, json!({ "workerRuntimeId": 1 }));
    assert_eq!(second_result, json!({ "workerRuntimeId": 1 }));
    assert_eq!(test_state.worker_runtime_builds(), 1);

    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 1);
    assert_eq!(metrics.isolate_pool_hits, 1);
    assert_eq!(metrics.isolate_pool_replacements, 0);
}

#[tokio::test]
async fn cooperative_execution_model_resumes_parked_invocations_after_host_completion() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_isolates = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(ControlledAsyncGetHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = test_request("slow-1");
    let parked_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&request, "tenant-a", "req-cooperative-parked");
        async move {
            executor
                .invoke_on_worker(
                    NeovexRuntime::with_policy(host, policy),
                    bundle,
                    request,
                    context,
                    None,
                )
                .await
        }
    });

    host.wait_until_started("slow-1").await;
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if policy.metrics_snapshot().active_isolates == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cooperative invocation should suspend its active isolate while parked");
    tokio::task::yield_now().await;
    assert!(
        !parked_task.is_finished(),
        "cooperative invocation should remain pending until host work completes"
    );

    host.release_slow_jobs();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), parked_task)
            .await
            .expect("cooperative invocation should resume after host completion")
            .expect("cooperative parked task should join")
            .expect("cooperative parked invocation should succeed"),
        json!({ "id": "slow-1" })
    );
}

#[tokio::test]
async fn cooperative_execution_model_startup_snapshot_handles_multiple_parked_runtimes() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = cooperative_startup_snapshot_runtime_test_limits();
    limits.max_concurrent_isolates = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(ControlledAsyncGetHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);

    let slow_requests = [
        ("slow-1", "tenant-a", "req-cooperative-slow-1"),
        ("slow-2", "tenant-b", "req-cooperative-slow-2"),
        ("slow-3", "tenant-c", "req-cooperative-slow-3"),
        ("slow-4", "tenant-d", "req-cooperative-slow-4"),
    ];

    let tasks = slow_requests.map(|(function_name, tenant_label, request_id)| {
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let request = test_request(function_name);
        let context = test_context_for_tenant(&request, tenant_label, request_id);
        tokio::spawn(async move {
            executor
                .invoke_on_worker(
                    NeovexRuntime::with_policy(host, policy),
                    bundle,
                    request,
                    context,
                    None,
                )
                .await
        })
    });

    for (function_name, _, _) in slow_requests {
        host.wait_until_started(function_name).await;
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let metrics = policy.metrics_snapshot();
            if metrics.active_isolates == 0 && host.started_ids().len() >= slow_requests.len() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("all cooperative invocations should park and release the worker isolate");

    host.release_slow_jobs();

    for (task, (function_name, _, _)) in tasks.into_iter().zip(slow_requests) {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task)
                .await
                .expect("cooperative parked invocation should resume after host completion")
                .expect("cooperative parked task should join")
                .expect("cooperative parked invocation should succeed"),
            json!({ "id": function_name })
        );
    }

    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 1);
    assert_eq!(metrics.isolate_pool_hits, 3);
    assert_eq!(metrics.isolate_pool_replacements, 0);
    assert_eq!(metrics.retained_runtime_pool_entries, 0);
}
