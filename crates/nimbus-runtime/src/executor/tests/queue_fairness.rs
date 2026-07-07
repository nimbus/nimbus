use super::*;
use crate::limits::{
    RuntimeHostPressureLevel, RuntimeHostPressureSample, RuntimeHostPressureSource,
    RuntimeHostResourceBudget, RuntimeMemoryPressureDecision, RuntimeMemoryPressureLevel,
    RuntimeMemoryPressureSourceStatus,
};
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

#[derive(Debug)]
struct FixedRuntimeHostPressureSource {
    sample: RuntimeHostPressureSample,
}

impl FixedRuntimeHostPressureSource {
    fn new(sample: RuntimeHostPressureSample) -> Self {
        Self { sample }
    }
}

impl RuntimeHostPressureSource for FixedRuntimeHostPressureSource {
    fn sample(&self) -> RuntimeHostPressureSample {
        self.sample
    }
}

fn runtime_host_budget_for_four_seats() -> RuntimeHostResourceBudget {
    RuntimeHostResourceBudget {
        host_millicpus: 4000,
        system_reserved_millicpus: 0,
        nimbus_control_plane_reserved_millicpus: 0,
        runtime_hard_ceiling_millicpus: None,
        runtime_seat_millicpus: std::num::NonZeroU32::new(1000).expect("one CPU seat is nonzero"),
    }
}

fn runtime_host_budget_for_two_seats() -> RuntimeHostResourceBudget {
    RuntimeHostResourceBudget {
        host_millicpus: 2000,
        system_reserved_millicpus: 0,
        nimbus_control_plane_reserved_millicpus: 0,
        runtime_hard_ceiling_millicpus: None,
        runtime_seat_millicpus: std::num::NonZeroU32::new(1000).expect("one CPU seat is nonzero"),
    }
}

fn observed_host_pressure(level: RuntimeHostPressureLevel) -> RuntimeHostPressureSample {
    RuntimeHostPressureSample::observed(
        level,
        RuntimeMemoryPressureDecision::for_level(
            RuntimeMemoryPressureLevel::Nominal,
            RuntimeMemoryPressureSourceStatus::Observed,
        ),
        false,
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
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
            NimbusRuntime::with_policy(
                host.clone(),
                policy.clone(),
                crate::RuntimeEgressPosture::CoarsePermissions,
            ),
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
async fn host_pressure_reduces_runtime_dispatch_seats_before_tenant_quota_exhaustion() {
    let _test_lock = acquire_runtime_suite_lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 4;
    limits.worker_threads = 4;
    limits.max_active_top_level_invocations_per_tenant = 4;
    limits.max_in_flight_top_level_invocations_per_tenant = 4;
    limits.max_queued_top_level_invocations_per_tenant = 4;
    let policy = Arc::new(RuntimePolicy::with_host_resource_governor(
        limits,
        runtime_host_budget_for_four_seats(),
        Arc::new(FixedRuntimeHostPressureSource::new(observed_host_pressure(
            RuntimeHostPressureLevel::High,
        ))),
    ));
    assert_eq!(
        policy.host_resource_decision().effective_dispatch_seats,
        2,
        "high host pressure should reduce the four-seat host budget before tenant quota is exhausted"
    );
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(ControlledAsyncGetHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);

    let slow_a = test_request("slow-1");
    let slow_a_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&slow_a, "tenant-a", "req-host-pressure-a");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    bundle,
                    slow_a,
                    context,
                    None,
                )
                .await
        }
    });
    host.wait_until_started("slow-1").await;

    let slow_b = test_request("slow-2");
    let slow_b_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&slow_b, "tenant-b", "req-host-pressure-b");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    bundle,
                    slow_b,
                    context,
                    None,
                )
                .await
        }
    });
    host.wait_until_started("slow-2").await;

    let queued = test_request("fast-1");
    let queued_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&queued, "tenant-c", "req-host-pressure-c");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    bundle,
                    queued,
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
        slow_a_task
            .await
            .expect("slow tenant-a task should join")
            .expect("slow tenant-a invocation should succeed"),
        json!({ "id": "slow-1" })
    );
    assert_eq!(
        slow_b_task
            .await
            .expect("slow tenant-b task should join")
            .expect("slow tenant-b invocation should succeed"),
        json!({ "id": "slow-2" })
    );
    assert_eq!(
        queued_task
            .await
            .expect("queued tenant-c task should join")
            .expect("queued tenant-c invocation should succeed after host seat frees"),
        json!({ "id": "fast-1" })
    );
}

#[tokio::test]
async fn host_pressure_queue_promotion_respects_effective_dispatch_seats() {
    let _test_lock = acquire_runtime_suite_lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 3;
    limits.worker_threads = 3;
    limits.max_active_top_level_invocations_per_tenant = 3;
    limits.max_in_flight_top_level_invocations_per_tenant = 3;
    limits.max_queued_top_level_invocations_per_tenant = 3;
    let policy = Arc::new(RuntimePolicy::with_host_resource_governor(
        limits,
        runtime_host_budget_for_two_seats(),
        Arc::new(FixedRuntimeHostPressureSource::new(observed_host_pressure(
            RuntimeHostPressureLevel::High,
        ))),
    ));
    assert_eq!(
        policy.host_resource_decision().effective_dispatch_seats,
        1,
        "high pressure should leave one effective host dispatch seat"
    );
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(StepControlledAsyncGetHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);

    let first_request = test_request("slow-1");
    let first_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&first_request, "tenant-a", "req-host-seat-a");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
        let context = test_context_for_tenant(&second_request, "tenant-b", "req-host-seat-b");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    bundle,
                    second_request,
                    context,
                    None,
                )
                .await
        }
    });

    let third_request = test_request("slow-3");
    let third_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&third_request, "tenant-c", "req-host-seat-c");
        async move {
            executor
                .invoke_on_worker(
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    bundle,
                    third_request,
                    context,
                    None,
                )
                .await
        }
    });

    host.assert_not_started_within("slow-2", Duration::from_millis(100))
        .await;
    host.assert_not_started_within("slow-3", Duration::from_millis(100))
        .await;

    host.release("slow-1");
    host.wait_until_started("slow-2").await;
    host.assert_not_started_within("slow-3", Duration::from_millis(100))
        .await;
    assert_eq!(
        host.max_active_host_calls(),
        1,
        "queued promotion must not exceed the effective host dispatch seat"
    );

    host.release("slow-2");
    host.wait_until_started("slow-3").await;
    host.release("slow-3");

    assert_eq!(
        first_task
            .await
            .expect("first task should join")
            .expect("first invocation should succeed"),
        json!({ "id": "slow-1" })
    );
    assert_eq!(
        second_task
            .await
            .expect("second task should join")
            .expect("second invocation should succeed"),
        json!({ "id": "slow-2" })
    );
    assert_eq!(
        third_task
            .await
            .expect("third task should join")
            .expect("third invocation should succeed"),
        json!({ "id": "slow-3" })
    );
}

#[tokio::test]
async fn host_pressure_sheds_burstable_work_under_critical_pressure() {
    let _test_lock = acquire_runtime_suite_lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 4;
    limits.worker_threads = 4;
    limits.max_active_top_level_invocations_per_tenant = 4;
    limits.max_in_flight_top_level_invocations_per_tenant = 4;
    limits.max_queued_top_level_invocations_per_tenant = 4;
    let policy = Arc::new(RuntimePolicy::with_host_resource_governor(
        limits,
        runtime_host_budget_for_four_seats(),
        Arc::new(FixedRuntimeHostPressureSource::new(observed_host_pressure(
            RuntimeHostPressureLevel::Critical,
        ))),
    ));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(ControlledAsyncGetHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = test_request("fast-1");

    let error = executor
        .invoke_on_worker(
            NimbusRuntime::with_policy(
                host.clone(),
                policy.clone(),
                crate::RuntimeEgressPosture::CoarsePermissions,
            ),
            bundle,
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-host-pressure-shed"),
            None,
        )
        .await
        .expect_err("critical host pressure should shed burstable tenant work");

    assert!(
        matches!(
            error,
            NimbusRuntimeError::HostResourcePressureShed {
                work_class: "burstable",
                host_pressure_level: "critical",
            }
        ),
        "expected critical host-pressure shed error, got: {error}"
    );
    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.rejected_invocations, 1);
    assert_eq!(
        metrics
            .tenants
            .get("tenant-a")
            .expect("tenant-a metrics should be present")
            .rejected_invocations,
        1
    );
    assert!(
        !host.started_ids().iter().any(|id| id == "fast-1"),
        "shed work must not reach the runtime host"
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
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
                    NimbusRuntime::with_policy(
                        parked_host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
                    NimbusRuntime::with_policy(
                        blocker_host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    bundle,
                    slow_request,
                    context,
                    None,
                )
                .await
        }
    });
    host.wait_until_slow_started().await;

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
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
            NimbusRuntime::with_policy(
                host.clone(),
                policy.clone(),
                crate::RuntimeEgressPosture::CoarsePermissions,
            ),
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
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
                    bundle,
                    slow_request,
                    context,
                    None,
                )
                .await
        }
    });
    host.wait_until_slow_started().await;

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
                    NimbusRuntime::with_policy(
                        host,
                        policy,
                        crate::RuntimeEgressPosture::CoarsePermissions,
                    ),
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
            NimbusRuntime::with_policy(
                host.clone(),
                policy.clone(),
                crate::RuntimeEgressPosture::CoarsePermissions,
            ),
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
