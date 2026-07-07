use super::*;

use crate::test_support::{IsolatedRuntimeTestCase, run_v8_sensitive_runtime_test_in_subprocess};

const STARTUP_SNAPSHOT_MULTIPLE_PARKED_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "executor-cooperative-startup-snapshot-multiple-parked",
    "cooperative-startup-snapshot",
    "startup snapshot cooperative execution parks multiple runtime invocations without cross-test anchor contention",
    "executor::tests::cooperative::cooperative_execution_model_startup_snapshot_handles_multiple_parked_runtimes_subprocess",
);
const SYNTHETIC_AWAIT_FOUR_TENANTS_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "executor-cooperative-synthetic-await-four-tenants",
    "cooperative-warm-pool",
    "cooperative warm-pool synthetic await handles four tenants without cross-test anchor contention",
    "executor::tests::cooperative::cooperative_warm_pool_handles_synthetic_await_four_tenants_subprocess",
);
const REJECTED_WAIT_UNTIL_RESPONSE_READY_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "executor-cooperative-rejected-wait-until-response-ready",
        "cooperative-response-ready",
        "response-ready completion reports rejected waitUntil without cross-test anchor contention",
        "executor::tests::cooperative::response_ready_completion_reports_rejected_wait_until_background_work_subprocess",
    );

#[tokio::test]
async fn cooperative_execution_model_processes_worker_invocations() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
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
            NimbusRuntime::with_policy(
                host.clone(),
                policy.clone(),
                crate::RuntimeEgressPosture::CoarsePermissions,
            ),
            RuntimeBundle::new(&bundle_path),
            request.clone(),
            test_context(&request, "req-cooperative-1"),
            None,
        )
        .await
        .expect("first cooperative worker invocation should succeed");
    let second_result = executor
        .invoke_on_worker(
            NimbusRuntime::with_policy(
                host,
                policy.clone(),
                crate::RuntimeEgressPosture::CoarsePermissions,
            ),
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
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
    assert_eq!(metrics.runtime_pool_replacements, 0);
}

#[tokio::test]
async fn cooperative_execution_model_resumes_parked_invocations_after_host_completion() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
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
        let context = test_context_for_tenant(&request, "tenant-a", "req-cooperative-parked");
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
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if policy.metrics_snapshot().active_runtime_instances == 0 {
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
async fn cooperative_execution_model_cancels_parked_invocations_on_shutdown() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(ControlledAsyncGetHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = test_request("slow-shutdown");
    let parked_task = tokio::spawn({
        let executor = executor.clone();
        let bundle = bundle.clone();
        let host = host.clone();
        let policy = policy.clone();
        let context = test_context_for_tenant(&request, "tenant-a", "req-cooperative-shutdown");
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

    host.wait_until_started("slow-shutdown").await;
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            if policy.metrics_snapshot().active_runtime_instances == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cooperative invocation should release its active isolate while parked");

    executor.inner.shutdown.cancel();
    executor.inner.router.close();

    let result = tokio::time::timeout(Duration::from_secs(1), parked_task)
        .await
        .expect("shutdown should complete the parked invocation")
        .expect("parked invocation task should join");
    assert!(matches!(result, Err(NimbusRuntimeError::Cancelled)));

    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.in_flight_canceled_invocations, 1);
    assert_eq!(metrics.active_runtime_instances, 0);
}

#[tokio::test]
async fn pir4_response_ready_returns_before_wait_until_background_completion() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_wait_until_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(ControlledAsyncGetHost::default());
    let bundle = RuntimeBundle::new(&bundle_path);
    // Drive this through the default Query kind. These tests exercise the
    // executor's kind-agnostic response-ready + waitUntil-drain plumbing using
    // `ctx.db.get` as the controllable async host op, and `ctx.db.get` is
    // correctly denied to action handlers (see runtime/tests/host_bridge.rs).
    // waitUntil itself is available to queries (see the pool_reuse stalled-
    // waitUntil tests), so Query exercises the full mechanism without violating
    // the action context contract.
    let request = test_request("messages:http_action");

    let response_ready = tokio::time::timeout(
        Duration::from_secs(1),
        executor.invoke_on_worker_response_ready(
            NimbusRuntime::with_policy(
                host.clone(),
                policy,
                crate::RuntimeEgressPosture::CoarsePermissions,
            ),
            bundle,
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-pir4-response-ready"),
            None,
        ),
    )
    .await
    .expect("response-ready API should return before waitUntil completion")
    .expect("response-ready invocation should succeed");

    assert_eq!(
        response_ready.response(),
        &json!({ "responseReady": true }),
        "caller should observe the response before background completion"
    );

    host.wait_until_started("slow-background").await;
    let completion = response_ready.wait_until_complete();
    tokio::pin!(completion);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut completion)
            .await
            .is_err(),
        "waitUntil completion should remain pending while background host work is blocked"
    );

    host.release_slow_jobs();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(1), completion)
            .await
            .expect("waitUntil completion should finish after host release")
            .expect("waitUntil completion should succeed"),
        json!({ "responseReady": true })
    );
}

#[test]
fn response_ready_completion_reports_rejected_wait_until_background_work() {
    run_v8_sensitive_runtime_test_in_subprocess(REJECTED_WAIT_UNTIL_RESPONSE_READY_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate response-ready V8 anchor state"]
fn response_ready_completion_reports_rejected_wait_until_background_work_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("response-ready cooperative test runtime should build")
        .block_on(response_ready_completion_reports_rejected_wait_until_background_work_inner());
}

async fn response_ready_completion_reports_rejected_wait_until_background_work_inner() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_rejected_wait_until_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let host = Arc::new(RejectingAsyncGetHost::new("reject-background"));
    let bundle = RuntimeBundle::new(&bundle_path);
    // Drive this through the default Query kind. These tests exercise the
    // executor's kind-agnostic response-ready + waitUntil-drain plumbing using
    // `ctx.db.get` as the controllable async host op, and `ctx.db.get` is
    // correctly denied to action handlers (see runtime/tests/host_bridge.rs).
    // waitUntil itself is available to queries (see the pool_reuse stalled-
    // waitUntil tests), so Query exercises the full mechanism without violating
    // the action context contract.
    let request = test_request("messages:http_action");

    let response_ready = executor
        .invoke_on_worker_response_ready(
            NimbusRuntime::with_policy(
                host,
                policy,
                crate::RuntimeEgressPosture::CoarsePermissions,
            ),
            bundle,
            request.clone(),
            test_context_for_tenant(&request, "tenant-a", "req-rejected-wait-until"),
            None,
        )
        .await
        .expect("response-ready invocation should return the user response");

    assert_eq!(
        response_ready.response(),
        &json!({ "responseReady": true }),
        "response-ready callers should observe the response before background rejection"
    );

    let error = response_ready
        .wait_until_complete()
        .await
        .expect_err("rejected waitUntil background work should fail completion");
    assert!(
        matches!(
            error,
            NimbusRuntimeError::JavaScript(ref message)
                if message.contains("waitUntil background drain rejected 1 promise")
        ),
        "expected waitUntil rejection completion error, got {error}"
    );
}

#[test]
fn cooperative_execution_model_startup_snapshot_handles_multiple_parked_runtimes() {
    run_v8_sensitive_runtime_test_in_subprocess(STARTUP_SNAPSHOT_MULTIPLE_PARKED_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate startup-snapshot V8 anchor state"]
fn cooperative_execution_model_startup_snapshot_handles_multiple_parked_runtimes_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("startup-snapshot cooperative test runtime should build")
        .block_on(
            cooperative_execution_model_startup_snapshot_handles_multiple_parked_runtimes_inner(),
        );
}

async fn cooperative_execution_model_startup_snapshot_handles_multiple_parked_runtimes_inner() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = cooperative_startup_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
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
        })
    });

    for (function_name, _, _) in slow_requests {
        host.wait_until_started(function_name).await;
    }
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let metrics = policy.metrics_snapshot();
            if metrics.active_runtime_instances == 0
                && host.started_ids().len() >= slow_requests.len()
            {
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
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 3);
    assert_eq!(metrics.runtime_pool_replacements, 0);
    assert_eq!(metrics.retained_runtime_pool_entries, 0);
}

#[test]
fn cooperative_warm_pool_handles_synthetic_await_four_tenants() {
    run_v8_sensitive_runtime_test_in_subprocess(SYNTHETIC_AWAIT_FOUR_TENANTS_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative warm-pool V8 anchor state"]
fn cooperative_warm_pool_handles_synthetic_await_four_tenants_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("synthetic-await cooperative test runtime should build")
        .block_on(cooperative_warm_pool_handles_synthetic_await_four_tenants_inner());
}

async fn cooperative_warm_pool_handles_synthetic_await_four_tenants_inner() {
    let _test_lock = runtime_executor_test_lock().lock().await;
    let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    limits.max_warm_reuses = 1_000_000;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NimbusRuntime::with_policy(
        Arc::new(SyntheticAwaitHost::new(Duration::ZERO)),
        policy.clone(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let bundle = RuntimeBundle::new(&bundle_path);
    const BATCHES: usize = 16;
    const TENANTS: [(&str, &str); 4] = [
        ("tenant-a", "req-synthetic-await-a"),
        ("tenant-b", "req-synthetic-await-b"),
        ("tenant-c", "req-synthetic-await-c"),
        ("tenant-d", "req-synthetic-await-d"),
    ];

    for batch in 0..BATCHES {
        let handles = TENANTS.map(|(tenant_label, request_id)| {
            let executor = executor.clone();
            let runtime = runtime.clone();
            let bundle = bundle.clone();
            let request = test_request("doc-1");
            let request_id = format!("{request_id}-{batch}");
            let context = test_context_for_tenant(&request, tenant_label, &request_id);
            std::thread::spawn(move || executor.invoke_blocking(runtime, bundle, request, context))
        });

        for handle in handles {
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(handle.join());
            });
            let policy_for_timeout = policy.clone();
            assert_eq!(
                rx.recv_timeout(Duration::from_secs(5))
                    .unwrap_or_else(|_| {
                        let metrics = policy_for_timeout.metrics_snapshot();
                        panic!(
                            "synthetic-await warm-pool blocking invocation should complete in batch {batch}; active={}, queued={}, dispatched={}, started={}, completed={}, retained={}",
                            metrics.active_runtime_instances,
                            metrics.queued_invocations,
                            metrics.worker_dispatched_invocations,
                            metrics.started_invocations,
                            metrics.completed_invocations,
                            metrics.retained_runtime_pool_entries,
                        )
                    })
                    .expect("synthetic-await caller thread should not panic")
                    .unwrap_or_else(|error| {
                        let metrics = policy.metrics_snapshot();
                        panic!(
                            "synthetic-await invocation should succeed in batch {batch}; error={error}; active={}, queued={}, dispatched={}, started={}, completed={}, retained={}",
                            metrics.active_runtime_instances,
                            metrics.queued_invocations,
                            metrics.worker_dispatched_invocations,
                            metrics.started_invocations,
                            metrics.completed_invocations,
                            metrics.retained_runtime_pool_entries,
                        )
                    }),
                json!({
                    "operation": "document_get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "host_call_session_id": "query:doc-1",
                    }
                })
            );
        }
    }

    let metrics = policy.metrics_snapshot();
    let total_invocations = (BATCHES * TENANTS.len()) as u64;
    assert_eq!(metrics.worker_dispatched_invocations, total_invocations);
    assert_eq!(
        metrics.warm_pool_hits + metrics.warm_pool_misses,
        total_invocations
    );
    assert!(
        metrics.warm_pool_hits > 0,
        "synthetic-await batch should exercise retained warm-pool reuse"
    );
    assert_eq!(
        metrics.runtime_pool_hits + metrics.runtime_pool_misses,
        total_invocations
    );
    assert_eq!(metrics.runtime_pool_hits, metrics.warm_pool_hits);
    assert_eq!(metrics.runtime_pool_misses, metrics.warm_pool_misses);
    assert_eq!(metrics.warm_pool_discard_unquiesced, 0);
    assert!(
        (1..=4).contains(&metrics.retained_runtime_pool_entries),
        "successful synthetic-await batch should leave bounded retained warm-pool entries"
    );
    assert_eq!(metrics.runtime_pool_replacements, 0);
}
