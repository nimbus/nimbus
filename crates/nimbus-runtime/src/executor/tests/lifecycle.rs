use std::sync::Barrier;

use super::*;

#[tokio::test]
async fn pre_canceled_worker_invocation_records_request_correlation() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let policy = product_default_runtime_test_policy();
    let executor = RuntimeExecutor::new(policy.clone());
    let request = test_request("messages:list");
    let context = test_context(&request, "req-pre-canceled");
    let cancellation = HostCallCancellation::default();
    cancellation.cancel_due_to_disconnect();

    let error = executor
        .invoke_on_worker(
            NimbusRuntime::with_policy(Arc::new(NoopHost), policy.clone()),
            RuntimeBundle::new("unused.mjs"),
            request,
            context,
            Some(cancellation),
        )
        .await
        .expect_err("pre-canceled worker invocation should fail");

    assert!(matches!(error, NimbusRuntimeError::Cancelled));

    let snapshot = policy.metrics_snapshot();
    assert_eq!(snapshot.queued_canceled_invocations, 1);
    assert_eq!(snapshot.disconnect_canceled_invocations, 1);
    assert_eq!(snapshot.recent_request_correlations.len(), 1);
    let correlation = &snapshot.recent_request_correlations[0];
    assert_eq!(correlation.server_request_id, "req-pre-canceled");
    assert_eq!(correlation.function_name, "messages:list");
    assert_eq!(correlation.kind, "query");
    assert_eq!(correlation.tenant_label.as_deref(), Some("demo"));
    assert!(correlation.invocation_id > 0);
}

#[tokio::test]
async fn worker_invocations_reuse_worker_local_tokio_runtime() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
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
            NimbusRuntime::with_policy(host.clone(), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context(&request, "req-worker-1"),
            None,
        )
        .await
        .expect("first worker invocation should succeed");
    let second_result = executor
        .invoke_on_worker(
            NimbusRuntime::with_policy(host, policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context(&request, "req-worker-2"),
            None,
        )
        .await
        .expect("second worker invocation should succeed");

    assert_eq!(first_result, json!({ "workerRuntimeId": 1 }));
    assert_eq!(second_result, json!({ "workerRuntimeId": 1 }));
    assert_eq!(test_state.worker_runtime_builds(), 1);
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
    assert_eq!(metrics.runtime_pool_replacements, 0);
}

#[test]
fn sibling_threads_can_boot_runtime_executors_in_parallel() {
    let _test_lock = runtime_executor_test_lock().blocking_lock();
    let (_bundle_dir, bundle_path) = write_constant_bundle();
    let before = crate::backends::v8::v8_bootstrap_snapshot_build_count_for_test();
    let barrier = Arc::new(Barrier::new(3));

    let worker =
        |request_id: &'static str, barrier: Arc<Barrier>, bundle_path: std::path::PathBuf| {
            std::thread::spawn(move || {
                let mut limits = run_to_completion_snapshot_runtime_test_limits();
                limits.max_concurrent_runtime_instances = 1;
                limits.worker_threads = 1;
                let policy = Arc::new(RuntimePolicy::new(limits));
                let executor = RuntimeExecutor::new(policy.clone());
                let request = test_request("messages:list");
                barrier.wait();
                executor.invoke_blocking_with_cancellation(
                    NimbusRuntime::with_policy(Arc::new(NoopHost), policy),
                    RuntimeBundle::new(bundle_path),
                    request.clone(),
                    test_context(&request, request_id),
                    None,
                )
            })
        };

    let first = worker("req-sibling-1", barrier.clone(), bundle_path.clone());
    let second = worker("req-sibling-2", barrier.clone(), bundle_path);
    barrier.wait();

    assert_eq!(
        first
            .join()
            .expect("first sibling-thread executor should join")
            .expect("first sibling-thread invocation should succeed"),
        json!("ok")
    );
    assert_eq!(
        second
            .join()
            .expect("second sibling-thread executor should join")
            .expect("second sibling-thread invocation should succeed"),
        json!("ok")
    );

    let after = crate::backends::v8::v8_bootstrap_snapshot_build_count_for_test();
    assert!(
        after.saturating_sub(before) <= 1,
        "parallel sibling-thread executor startups should reuse one process-global bootstrap snapshot"
    );
}

#[test]
fn blocking_worker_invocation_succeeds_without_tokio_runtime_on_calling_thread() {
    let _test_lock = runtime_executor_test_lock().blocking_lock();
    assert!(
        tokio::runtime::Handle::try_current().is_err(),
        "plain #[test] should not already be inside a Tokio runtime"
    );

    let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let test_state = executor.test_state();
    let request = test_request("messages:list");
    let cancellation = HostCallCancellation::default();

    let result = executor
        .invoke_blocking_with_cancellation(
            NimbusRuntime::with_policy(
                Arc::new(WorkerRuntimeIdHost {
                    test_state: test_state.clone(),
                }),
                policy.clone(),
            ),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context(&request, "req-blocking-worker"),
            Some(cancellation),
        )
        .expect("blocking worker invocation should succeed");

    assert_eq!(result, json!({ "workerRuntimeId": 1 }));
    assert_eq!(test_state.worker_runtime_builds(), 1);
}

#[tokio::test]
async fn timed_out_worker_invocations_record_runtime_pool_replacements() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_timeout_bundle_dir, timeout_bundle_path) = write_busy_loop_bundle();
    let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = Duration::from_millis(50);
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let request = test_request("messages:list");

    let error = executor
        .invoke_on_worker(
            NimbusRuntime::with_policy(Arc::new(NoopHost), policy.clone()),
            RuntimeBundle::new(&timeout_bundle_path),
            request.clone(),
            test_context(&request, "req-timeout"),
            None,
        )
        .await
        .expect_err("busy-loop invocation should time out");
    assert!(matches!(error, NimbusRuntimeError::ExecutionTimeout(_)));

    let recovery_result = executor
        .invoke_on_worker(
            NimbusRuntime::with_policy(Arc::new(NoopHost), policy.clone()),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context(&request, "req-recovery"),
            None,
        )
        .await
        .expect("follow-up invocation should succeed");
    assert_eq!(recovery_result, Value::Null);

    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
    assert_eq!(metrics.runtime_pool_replacements, 1);
}
