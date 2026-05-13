use super::*;
use crate::test_support::RuntimeReproCase;

pub(crate) const TENANT_QUEUE_LIMIT_REJECTION_CASE: RuntimeReproCase = RuntimeReproCase::new(
    "runtime-queue-limit-rejection-accounting",
    "bounded-fairness",
    "bounded fairness pressure rejects excess tenant work and records stable rejection accounting",
);

pub(crate) const TENANT_FAIRNESS_NO_STARVATION_CASE: RuntimeReproCase = RuntimeReproCase::new(
    "runtime-tenant-fairness-no-starvation",
    "bounded-fairness",
    "bounded fairness pressure lets a ready tenant make progress without being starved by another tenant's backlog",
);

fn runtime_harness_repro(case: RuntimeReproCase) -> String {
    format!(
        "bash scripts/verification-harness.sh repro runtime required {}",
        case.id()
    )
}

#[tokio::test]
async fn permit_suspend_frees_capacity() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 2;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(ControlledAsyncGetHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);

    let slow_request = test_request("slow-1");
    let slow_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&slow_request, "tenant-a", "req-permit-slow");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(host, policy),
                    bundle,
                    slow_request,
                    context,
                    None,
                )
                .await
        }
    });
    host.wait_until_started("slow-1").await;

    let fast_request = test_request("fast-1");
    let fast_result = tokio::time::timeout(
        Duration::from_secs(1),
        executor.invoke_on_worker(
            NimbusRuntime::with_policy(host.clone(), policy.clone()),
            bundle.clone(),
            fast_request.clone(),
            test_context_for_tenant(&fast_request, "tenant-b", "req-permit-fast"),
            None,
        ),
    )
    .await
    .expect("fast invocation should use the freed permit")
    .expect("fast invocation should succeed");

    assert_eq!(fast_result, json!({ "id": "fast-1" }));
    assert!(
        !slow_task.is_finished(),
        "slow invocation should still be parked while the second worker uses the freed permit"
    );

    host.release_slow_jobs();
    assert_eq!(
        slow_task
            .await
            .expect("slow task should join")
            .expect("slow invocation should succeed after resume"),
        json!({ "id": "slow-1" })
    );
}

#[tokio::test]
async fn parked_invocation_resumes_after_host_completion() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
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
        let context = test_context_for_tenant(&request, "tenant-a", "req-parked-resume");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(host, policy),
                    bundle,
                    request,
                    context,
                    None,
                )
                .await
        }
    });

    host.wait_until_started("slow-1").await;
    assert!(
        !parked_task.is_finished(),
        "parked invocation should remain pending until host work completes"
    );

    host.release_slow_jobs();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), parked_task)
            .await
            .expect("parked invocation should resume after host completion")
            .expect("parked task should join")
            .expect("parked invocation should succeed"),
        json!({ "id": "slow-1" })
    );
}

#[tokio::test]
async fn parked_invocation_counts_toward_in_flight_limit() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = bounded_fairness_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 2;
    limits.max_in_flight_top_level_invocations_per_tenant = 2;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(ControlledAsyncGetHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);

    let first_request = test_request("slow-1");
    let first_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&first_request, "tenant-a", "req-inflight-1");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(host, policy),
                    bundle,
                    first_request,
                    context,
                    None,
                )
                .await
        }
    });
    host.wait_until_started("slow-1").await;

    let second_request = test_request("slow-2");
    let second_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&second_request, "tenant-a", "req-inflight-2");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(host, policy),
                    bundle,
                    second_request,
                    context,
                    None,
                )
                .await
        }
    });
    host.wait_until_started("slow-2").await;

    let third_request = test_request("fast-1");
    let third_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&third_request, "tenant-a", "req-inflight-3");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(host, policy),
                    bundle,
                    third_request,
                    context,
                    None,
                )
                .await
        }
    });

    host.assert_not_started_within("fast-1", Duration::from_millis(100))
        .await;

    host.release_slow_jobs();
    assert_eq!(
        first_task
            .await
            .expect("first slow task should join")
            .expect("first slow invocation should succeed"),
        json!({ "id": "slow-1" })
    );
    assert_eq!(
        second_task
            .await
            .expect("second slow task should join")
            .expect("second slow invocation should succeed"),
        json!({ "id": "slow-2" })
    );
    assert_eq!(
        third_task
            .await
            .expect("third task should join")
            .expect("third invocation should succeed after queue promotion"),
        json!({ "id": "fast-1" })
    );
}

#[tokio::test]
async fn timeout_excludes_permit_reacquire_wait() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_async_bundle_dir, async_bundle_path) = write_function_named_get_bundle();
    let (_sync_bundle_dir, sync_bundle_path) = write_sync_query_builder_bundle();
    let mut limits = bounded_fairness_runtime_test_limits();
    limits.execution_timeout = Duration::from_millis(120);
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 2;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let parked_host = Arc::new(ControlledAsyncGetHost::default());
    let blocker_host = Arc::new(SlowSyncQueryHost::new(Duration::from_millis(80)));
    let async_bundle = RuntimeBundle::new(&async_bundle_path);
    let sync_bundle = RuntimeBundle::new(&sync_bundle_path);

    let slow_request = test_request("slow-1");
    let slow_started_at = std::time::Instant::now();
    let parked_task = tokio::spawn({
        let executor = executor.clone();
        let async_bundle = async_bundle.clone();
        let parked_host = parked_host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&slow_request, "tenant-a", "req-timeout-parked");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(parked_host, policy),
                    async_bundle,
                    slow_request,
                    context,
                    None,
                )
                .await
        }
    });
    parked_host.wait_until_started("slow-1").await;
    // This is an intentional modeled delay: keep the invocation parked on its
    // async host work long enough that, once permit re-acquire waiting is added
    // on top, end-to-end wall time exceeds the execution timeout while the
    // invocation still succeeds.
    tokio::time::sleep(Duration::from_millis(80)).await;

    let blocker_request = test_request("messages:list");
    let blocker_task = tokio::spawn({
        let executor = executor.clone();
        let sync_bundle = sync_bundle.clone();
        let blocker_host = blocker_host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&blocker_request, "tenant-b", "req-timeout-blocker");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(blocker_host, policy),
                    sync_bundle,
                    blocker_request,
                    context,
                    None,
                )
                .await
        }
    });
    blocker_host.wait_until_started().await;
    parked_host.release_slow_jobs();

    assert_eq!(
        blocker_task
            .await
            .expect("blocker task should join")
            .expect("blocker invocation should succeed"),
        json!({ "builderId": "builder-1" })
    );
    assert_eq!(
        parked_task
            .await
            .expect("parked task should join")
            .expect("parked invocation should succeed after waiting to re-acquire the permit"),
        json!({ "id": "slow-1" })
    );
    assert!(
        slow_started_at.elapsed() >= Duration::from_millis(140),
        "parked invocation wall time should exceed the execution timeout while still succeeding because permit re-acquire wait is paused"
    );
}

#[tokio::test]
async fn tenant_queue_limit_rejections_record_metrics() {
    tenant_queue_limit_rejections_record_metrics_inner().await;
}

pub(crate) async fn tenant_queue_limit_rejections_record_metrics_inner() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = bounded_fairness_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(TenantFairnessHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);

    let slow_request = test_request("slow-1");
    let slow_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&slow_request, "tenant-a", "req-slow-1");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(host, policy),
                    bundle,
                    slow_request,
                    context,
                    None,
                )
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(1), host.slow_started.notified())
        .await
        .expect("slow runtime invocation should start");

    let queued_request = test_request("slow-2");
    let queued_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&queued_request, "tenant-a", "req-slow-2");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(host, policy),
                    bundle,
                    queued_request,
                    context,
                    None,
                )
                .await
        }
    });
    host.assert_not_started_within("slow-2", Duration::from_millis(100))
        .await;

    let rejected_request = test_request("slow-3");
    let error = executor
        .invoke_on_worker(
            NimbusRuntime::with_policy(host.clone(), policy.clone()),
            bundle.clone(),
            rejected_request.clone(),
            test_context_for_tenant(&rejected_request, "tenant-a", "req-slow-3"),
            None,
        )
        .await
        .expect_err("third tenant-a invocation should be rejected");
    assert!(
        matches!(
            error,
            NimbusRuntimeError::TenantQueueLimitExceeded {
                ref tenant_label,
                limit: 1,
            } if tenant_label == "tenant-a"
        ),
        "{}; received {error}",
        TENANT_QUEUE_LIMIT_REJECTION_CASE.failure_context_with_repro(
            "bounded fairness pressure should reject the third tenant-a invocation with the tenant queue limit error",
            &runtime_harness_repro(TENANT_QUEUE_LIMIT_REJECTION_CASE),
        )
    );

    let metrics = policy.metrics_snapshot();
    assert_eq!(
        metrics.rejected_invocations,
        1,
        "{}",
        TENANT_QUEUE_LIMIT_REJECTION_CASE.failure_context_with_repro(
            "runtime metrics should record exactly one rejected invocation for the queue-limit case",
            &runtime_harness_repro(TENANT_QUEUE_LIMIT_REJECTION_CASE),
        )
    );
    assert_eq!(
        metrics
            .tenants
            .get("tenant-a")
            .expect("tenant metrics should be present")
            .rejected_invocations,
        1,
        "{}",
        TENANT_QUEUE_LIMIT_REJECTION_CASE.failure_context_with_repro(
            "tenant metrics should record the rejected invocation on tenant-a",
            &runtime_harness_repro(TENANT_QUEUE_LIMIT_REJECTION_CASE),
        )
    );

    host.release_slow_job();
    assert_eq!(
        slow_task
            .await
            .expect("slow task should join")
            .expect("slow invocation should succeed"),
        json!({ "id": "slow-1" })
    );
    assert_eq!(
        queued_task
            .await
            .expect("queued task should join")
            .expect("queued invocation should succeed"),
        json!({ "id": "slow-2" })
    );
}

#[tokio::test]
async fn tenant_fairness_prevents_one_tenant_from_starving_another() {
    tenant_fairness_prevents_one_tenant_from_starving_another_inner().await;
}

pub(crate) async fn tenant_fairness_prevents_one_tenant_from_starving_another_inner() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let policy = Arc::new(RuntimePolicy::new(bounded_fairness_runtime_test_limits()));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(TenantFairnessHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);

    let slow_request = test_request("slow-1");
    let slow_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&slow_request, "tenant-a", "req-tenant-a-1");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(host, policy),
                    bundle,
                    slow_request,
                    context,
                    None,
                )
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(1), host.slow_started.notified())
        .await
        .expect("slow tenant-a invocation should start");

    let queued_request = test_request("slow-2");
    let queued_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&queued_request, "tenant-a", "req-tenant-a-2");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(host, policy),
                    bundle,
                    queued_request,
                    context,
                    None,
                )
                .await
        }
    });
    host.assert_not_started_within("slow-2", Duration::from_millis(100))
        .await;

    let fast_request = test_request("fast-1");
    let fast_result = tokio::time::timeout(
        Duration::from_secs(1),
        executor.invoke_on_worker(
            NimbusRuntime::with_policy(host.clone(), policy.clone()),
            bundle.clone(),
            fast_request.clone(),
            test_context_for_tenant(&fast_request, "tenant-b", "req-tenant-b-1"),
            None,
        ),
    )
    .await
    .expect("tenant-b invocation should not be starved")
    .expect("tenant-b invocation should succeed");
    assert_eq!(
        fast_result,
        json!({ "id": "fast-1" }),
        "{}",
        TENANT_FAIRNESS_NO_STARVATION_CASE.failure_context_with_repro(
            "tenant-b should complete while tenant-a still has queued backlog",
            &runtime_harness_repro(TENANT_FAIRNESS_NO_STARVATION_CASE),
        )
    );
    assert!(
        !host.started_ids().iter().any(|id| id == "slow-2"),
        "{}",
        TENANT_FAIRNESS_NO_STARVATION_CASE.failure_context_with_repro(
            "tenant-a queued invocation should stay queued until tenant-a frees a fairness slot",
            &runtime_harness_repro(TENANT_FAIRNESS_NO_STARVATION_CASE),
        )
    );

    host.release_slow_job();
    assert_eq!(
        slow_task
            .await
            .expect("slow task should join")
            .expect("slow invocation should succeed"),
        json!({ "id": "slow-1" })
    );
    assert_eq!(
        queued_task
            .await
            .expect("queued task should join")
            .expect("queued invocation should succeed"),
        json!({ "id": "slow-2" }),
        "{}",
        TENANT_FAIRNESS_NO_STARVATION_CASE.failure_context_with_repro(
            "tenant-a backlog should still complete after tenant-b makes forward progress",
            &runtime_harness_repro(TENANT_FAIRNESS_NO_STARVATION_CASE),
        )
    );
}
