use super::*;
use crate::backends::v8::V8WorkerRuntimePool;
use crate::executor::RuntimeExecutor;
use crate::limits::RuntimePoolKind;

// Cooperative locker tests create V8 isolates with `use_locker: true`.
// Keep them subprocess-isolated so the parent runtime suite can run with the
// normal test topology without mixing locker and non-locker V8 teardown in one
// process.

pub(super) const PARK_AND_RESUME_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "runtime-cooperative-park-resume",
    "cooperative-warm-pool",
    "cooperative locker slot parks on deferred async host work and resumes after wake",
    "runtime::tests::cooperative::runtime_cooperative_locker_slot_parks_and_resumes_after_async_host_completion_subprocess",
);

pub(super) const IMMEDIATE_ASYNC_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "runtime-cooperative-immediate-async",
    "cooperative-warm-pool",
    "cooperative locker slot completes immediate async host work without parking",
    "runtime::tests::cooperative::runtime_cooperative_locker_slot_completes_immediate_async_host_work_without_parking_subprocess",
);

pub(super) const WARM_POOL_TWO_CYCLE_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "runtime-cooperative-warm-pool-two-cycles",
    "cooperative-warm-pool",
    "warm-pool cooperative async host flow survives two cycles with runtime reuse",
    "runtime::tests::cooperative::warm_pool_cooperative_async_host_two_cycles_subprocess",
);

pub(super) const CONCURRENT_DISPATCH_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "runtime-cooperative-concurrent-dispatch",
    "cooperative-startup-snapshot-and-warm-pool",
    "cooperative concurrent dispatch does not deadlock under bounded isolate concurrency",
    "runtime::tests::cooperative::cooperative_concurrent_dispatch_does_not_deadlock_subprocess",
);

#[test]
fn runtime_cooperative_locker_slot_parks_and_resumes_after_async_host_completion() {
    run_v8_sensitive_runtime_test_in_subprocess(PARK_AND_RESUME_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative locker V8 state"]
fn runtime_cooperative_locker_slot_parks_and_resumes_after_async_host_completion_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(
            runtime_cooperative_locker_slot_parks_and_resumes_after_async_host_completion_inner(),
        );
}

async fn runtime_cooperative_locker_slot_parks_and_resumes_after_async_host_completion_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const host = await ctx.db.get("messages", "doc-1");
  return {
    ok: true,
    host,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let host = Arc::new(DeferredAsyncHost::default());
    let runtime_owner =
        NeovexRuntime::with_policy(host.clone(), cooperative_warm_pool_runtime_test_policy());
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();
    let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");

    let mut slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut v8_runtime_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle: bundle.clone(),
                    request: request.clone(),
                    context: RuntimeInvocationContext::top_level(&request),
                    external_cancellation: None,
                    permit: permit.clone(),
                },
                activity_signal: activity_signal.clone(),
            },
        )
        .await
        .expect("cooperative locker slot should start");

    assert!(!slot.is_ready_to_resume());
    let mut parked = false;
    for poll_index in 0..8 {
        match slot.poll_once().await.expect("slot poll should succeed") {
            CooperativeRuntimeSlotPoll::Runnable => continue,
            CooperativeRuntimeSlotPoll::Parked => {
                parked = true;
                break;
            }
            CooperativeRuntimeSlotPoll::Completed => {
                panic!(
                    "deferred async host work should not complete before release (poll {poll_index})"
                );
            }
        }
    }
    assert!(parked, "deferred async host work should eventually park");
    assert_eq!(
        runtime_owner
            .policy
            .metrics_snapshot()
            .active_runtime_instances,
        0
    );

    let initial_generation = activity_signal.current_generation();
    host.release();
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            if slot.is_ready_to_resume()
                || activity_signal.current_generation() > initial_generation
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("host completion should wake the cooperative slot");
    assert!(slot.is_ready_to_resume());
    let mut completed = false;
    for poll_index in 0..8 {
        match slot
            .poll_once()
            .await
            .expect("slot poll should succeed after wake")
        {
            CooperativeRuntimeSlotPoll::Runnable => continue,
            CooperativeRuntimeSlotPoll::Completed => {
                completed = true;
                break;
            }
            CooperativeRuntimeSlotPoll::Parked => {
                panic!(
                    "released async host work should not park again before completion (poll {poll_index})"
                );
            }
        }
    }
    assert!(
        completed,
        "released async host work should complete after wake"
    );

    let result = slot
        .take_result()
        .expect("slot should keep completed value");
    assert_eq!(
        result,
        serde_json::json!({
            "ok": true,
            "host": {
                "operation": "ctx_db_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "query:messages:list",
                }
            }
        })
    );

    let ready_jobs = permit.finish_invocation().await;
    assert!(ready_jobs.is_empty());
    assert_eq!(
        runtime_owner
            .policy
            .metrics_snapshot()
            .active_runtime_instances,
        0
    );
    watchdog.shutdown();
}

#[test]
fn runtime_cooperative_locker_slot_completes_immediate_async_host_work_without_parking() {
    run_v8_sensitive_runtime_test_in_subprocess(IMMEDIATE_ASYNC_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative locker V8 state"]
fn runtime_cooperative_locker_slot_completes_immediate_async_host_work_without_parking_subprocess()
{
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(runtime_cooperative_locker_slot_completes_immediate_async_host_work_without_parking_inner());
}

async fn runtime_cooperative_locker_slot_completes_immediate_async_host_work_without_parking_inner()
{
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let runtime_owner = NeovexRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        cooperative_warm_pool_runtime_test_policy(),
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();
    let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");

    let mut slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut v8_runtime_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle: bundle.clone(),
                    request: request.clone(),
                    context: RuntimeInvocationContext::top_level(&request),
                    external_cancellation: None,
                    permit,
                },
                activity_signal,
            },
        )
        .await
        .expect("cooperative locker slot should start");

    let mut sequence = Vec::new();
    for _ in 0..20 {
        let poll = slot.poll_once().await.expect("slot poll should succeed");
        sequence.push(poll);
        if poll == CooperativeRuntimeSlotPoll::Completed {
            let result = slot
                .take_result()
                .expect("completed slot should retain its result");
            assert_eq!(
                result,
                serde_json::json!({
                    "operation": "ctx_db_get",
                    "payload": {
                        "table": "messages",
                        "id": "doc-1",
                        "session_id": "query:messages:list",
                    },
                })
            );
            watchdog.shutdown();
            return;
        }
    }

    panic!(
        "cooperative locker slot should complete within a bounded number of polls; sequence={sequence:?}"
    );
}

#[test]
fn warm_pool_cooperative_async_host_two_cycles() {
    run_v8_sensitive_runtime_test_in_subprocess(WARM_POOL_TWO_CYCLE_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative locker V8 state"]
fn warm_pool_cooperative_async_host_two_cycles_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(warm_pool_cooperative_async_host_two_cycles_inner());
}

async fn warm_pool_cooperative_async_host_two_cycles_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime_owner = NeovexRuntime::with_policy(Arc::new(AsyncEchoHost), policy);
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();

    let expected = serde_json::json!({
        "operation": "ctx_db_get",
        "payload": {
            "table": "messages",
            "id": "doc-1",
            "session_id": "query:messages:list",
        },
    });

    for cycle in 0..2 {
        let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
        let mut permit =
            SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
        permit
            .acquire_initial(std::time::Instant::now())
            .await
            .expect("permit should admit invocation");

        let mut slot = runtime_owner
            .start_cooperative_locker_runtime_slot(
                &mut v8_runtime_pool,
                CooperativeRuntimeSlotStart {
                    invocation: RuntimeInvocationExecution {
                        watchdog: watchdog.clone(),
                        bundle: bundle.clone(),
                        request: request.clone(),
                        context: RuntimeInvocationContext::top_level(&request),
                        external_cancellation: None,
                        permit: permit.clone(),
                    },
                    activity_signal,
                },
            )
            .await
            .unwrap_or_else(|e| panic!("cycle {cycle}: slot should start: {e}"));

        let mut completed = false;
        for poll_index in 0..20 {
            match slot
                .poll_once()
                .await
                .unwrap_or_else(|e| panic!("cycle {cycle} poll {poll_index}: {e}"))
            {
                CooperativeRuntimeSlotPoll::Runnable => continue,
                CooperativeRuntimeSlotPoll::Completed => {
                    completed = true;
                    break;
                }
                CooperativeRuntimeSlotPoll::Parked => {
                    panic!("cycle {cycle}: immediate async host should not park");
                }
            }
        }
        assert!(completed, "cycle {cycle}: should complete within 20 polls");

        let (result, returned_runtime) = slot
            .finish_with_result_and_runtime(Ok(expected.clone()))
            .await;
        result.unwrap_or_else(|e| panic!("cycle {cycle}: finalize should succeed: {e}"));

        if let Some(mut rt) = returned_runtime {
            rt.runtime
                .reset_request_state()
                .unwrap_or_else(|e| panic!("cycle {cycle}: reset should succeed: {e}"));
            rt.warm_reuse_count = rt.warm_reuse_count.saturating_add(1);
            v8_runtime_pool.return_runtime_for_invocation(
                &runtime_owner,
                &bundle,
                Some(&RuntimeInvocationContext::top_level(&request)),
                rt,
            );
        }

        let ready_jobs = permit.finish_invocation().await;
        assert!(ready_jobs.is_empty());
    }

    watchdog.shutdown();
}

/// Exercises the fix for the cooperative worker loop greedy admission deadlock.
///
/// Before the fix, `next_slot()` drained all pending jobs from the queue in a
/// `while let` loop, each calling `block_on(acquire_initial())` which acquires
/// the global runtime-instance semaphore. With
/// `max_concurrent_runtime_instances: 1`, the second
/// admission would block forever because the first admitted job still held the
/// semaphore and couldn't release it (needs to be polled first).
///
/// The fix changes `while let` to `if let` + `continue` so each admitted job
/// gets polled (releasing the semaphore via completion or async-host parking)
/// before the next admission.
#[test]
fn cooperative_concurrent_dispatch_does_not_deadlock() {
    run_v8_sensitive_runtime_test_in_subprocess(CONCURRENT_DISPATCH_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative locker V8 state"]
fn cooperative_concurrent_dispatch_does_not_deadlock_subprocess() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  const host = await ctx.db.get("messages", "doc-1");
  return {
    ok: true,
    host,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    for &pool_kind in &[
        RuntimePoolKind::StartupSnapshotCache,
        RuntimePoolKind::WarmPool,
    ] {
        let mut limits = match pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {
                cooperative_startup_snapshot_runtime_test_limits()
            }
            RuntimePoolKind::WarmPool => cooperative_warm_pool_runtime_test_limits(),
        };
        limits.max_concurrent_runtime_instances = 1;
        limits.worker_threads = 1;
        let policy = Arc::new(RuntimePolicy::new(limits));
        let runtime = NeovexRuntime::with_policy(Arc::new(AsyncEchoHost), policy.clone());
        let executor = RuntimeExecutor::new(policy);

        let handles: Vec<_> = (0..4)
            .map(|i| {
                let executor = executor.clone();
                let runtime = runtime.clone();
                let bundle = bundle.clone();
                let request = request.clone();
                let tenant = format!("tenant-{i}");
                std::thread::spawn(move || {
                    executor.invoke_blocking(
                        runtime,
                        bundle,
                        request.clone(),
                        RuntimeInvocationContext::top_level_for_tenant(&request, &tenant),
                    )
                })
            })
            .collect();

        for (i, handle) in handles.into_iter().enumerate() {
            // Wrap the join in a timeout: if the fix didn't work this would
            // hang forever. We want to fail the test rather than hang CI.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(handle.join());
            });
            let join_result = rx
                .recv_timeout(std::time::Duration::from_secs(10))
                .unwrap_or_else(|_| {
                    panic!(
                        "{} for {pool_kind:?} thread {i}",
                        CONCURRENT_DISPATCH_CASE
                            .failure_context("cooperative concurrent dispatch timed out")
                    )
                });
            let invocation_result = join_result.unwrap_or_else(|_| {
                panic!(
                    "{} for {pool_kind:?} thread {i}",
                    CONCURRENT_DISPATCH_CASE
                        .failure_context("cooperative concurrent dispatch thread panicked")
                )
            });
            invocation_result.unwrap_or_else(|e| {
                panic!(
                    "{} for {pool_kind:?} thread {i}: {e}",
                    CONCURRENT_DISPATCH_CASE
                        .failure_context("cooperative concurrent dispatch invocation failed")
                )
            });
        }
    }
}
