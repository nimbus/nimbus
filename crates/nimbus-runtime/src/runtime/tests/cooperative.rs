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

pub(super) const FRESH_REALM_EARLY_FINISH_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-cooperative-fresh-realm-early-finish",
        "cooperative-context-recycle",
        "warm context recycling destroys the fresh realm when a cooperative slot is finished early",
        "runtime::tests::cooperative::warm_context_recycle_cooperative_slot_destroys_fresh_realm_on_early_finish_subprocess",
    );

pub(super) const CONCURRENT_DISPATCH_CASE: IsolatedRuntimeTestCase = IsolatedRuntimeTestCase::new(
    "runtime-cooperative-concurrent-dispatch",
    "cooperative-startup-snapshot-and-warm-pool",
    "cooperative concurrent dispatch does not deadlock under bounded isolate concurrency",
    "runtime::tests::cooperative::cooperative_concurrent_dispatch_does_not_deadlock_subprocess",
);

pub(super) const PIR4_FORGED_HOST_CALL_SESSION_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-pir4-forged-host-call-session",
        "cooperative-host-call-session",
        "cooperative runtime rejects forged host-call sessions before host dispatch",
        "runtime::tests::cooperative::pir4_rejects_forged_host_call_session_subprocess",
    );

pub(super) const REC3_QUERY_WRITE_EFFECT_VIOLATION_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-rec3-query-write-effect-violation",
        "cooperative-execution-plan",
        "cooperative query plan rejects observed document writes before host dispatch",
        "runtime::tests::cooperative::rec3_query_write_effect_violation_rejects_before_host_dispatch_subprocess",
    );

pub(super) const PIR4_INTERLEAVED_HOST_CALL_SESSION_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-pir4-interleaved-host-call-session",
        "cooperative-host-call-session",
        "interleaved cooperative query isolates preserve their original host-call sessions",
        "runtime::tests::cooperative::pir4_interleaved_queries_preserve_host_call_sessions_subprocess",
    );

pub(super) const PIR4_MUTATION_EXCLUSION_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-pir4-mutation-exclusion",
        "cooperative-host-call-session",
        "mutations bypass the cooperative read-safe scheduler and run to completion",
        "runtime::tests::cooperative::pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler_subprocess",
    );

fn cooperative_slot_progress_timeout() -> std::time::Duration {
    duration_ms_env_or(
        "NIMBUS_COOPERATIVE_SLOT_PROGRESS_TIMEOUT_MS",
        ci_or_local_duration(
            std::time::Duration::from_secs(5),
            std::time::Duration::from_secs(20),
        ),
    )
}

fn cooperative_slot_wake_timeout() -> std::time::Duration {
    duration_ms_env_or(
        "NIMBUS_COOPERATIVE_SLOT_WAKE_TIMEOUT_MS",
        ci_or_local_duration(
            std::time::Duration::from_secs(1),
            std::time::Duration::from_secs(5),
        ),
    )
}

fn cooperative_concurrent_dispatch_join_timeout() -> std::time::Duration {
    duration_ms_env_or(
        "NIMBUS_COOPERATIVE_CONCURRENT_DISPATCH_TIMEOUT_MS",
        ci_or_local_duration(
            std::time::Duration::from_secs(30),
            std::time::Duration::from_secs(90),
        ),
    )
}

fn pir4_negative_assertion_window() -> std::time::Duration {
    duration_ms_env_or(
        "NIMBUS_PIR4_NEGATIVE_ASSERTION_WINDOW_MS",
        ci_or_local_duration(
            std::time::Duration::from_millis(150),
            std::time::Duration::from_millis(500),
        ),
    )
}

fn cooperative_query_request(function_name: &str) -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::Query,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

fn cooperative_mutation_request(function_name: &str) -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::Mutation,
        function_name: function_name.to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

fn host_call_session_id(call: &HostCallRequest) -> Option<&str> {
    call.payload
        .get("host_call_session_id")
        .and_then(Value::as_str)
}

#[derive(Default)]
struct ImmediateRecordingAsyncHost {
    calls: std::sync::Mutex<Vec<HostCallRequest>>,
}

impl ImmediateRecordingAsyncHost {
    fn calls(&self) -> Vec<HostCallRequest> {
        self.calls
            .lock()
            .expect("immediate recording host lock should not be poisoned")
            .clone()
    }
}

impl HostBridge for ImmediateRecordingAsyncHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "sync host bridge path should not be used for async ops".to_string(),
        ))
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> crate::host::HostBridgeFuture {
        self.calls
            .lock()
            .expect("immediate recording host lock should not be poisoned")
            .push(request.clone());
        Box::pin(async move {
            Ok(serde_json::json!({
                "status": "ok",
                "value": {
                    "operation": request.operation,
                    "payload": request.payload,
                },
            }))
        })
    }
}

#[derive(Default)]
struct DeferredRecordingAsyncHost {
    release: Arc<tokio::sync::Notify>,
    calls: std::sync::Mutex<Vec<HostCallRequest>>,
}

impl DeferredRecordingAsyncHost {
    fn release(&self) {
        self.release.notify_waiters();
    }

    fn calls(&self) -> Vec<HostCallRequest> {
        self.calls
            .lock()
            .expect("deferred recording host lock should not be poisoned")
            .clone()
    }
}

impl HostBridge for DeferredRecordingAsyncHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "sync host bridge path should not be used for async ops".to_string(),
        ))
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> crate::host::HostBridgeFuture {
        self.calls
            .lock()
            .expect("deferred recording host lock should not be poisoned")
            .push(request.clone());
        let release = self.release.clone();
        Box::pin(async move {
            release.notified().await;
            Ok(serde_json::json!({
                "status": "ok",
                "value": {
                    "operation": request.operation,
                    "payload": request.payload,
                },
            }))
        })
    }
}

#[derive(Default)]
struct MutationGateHost {
    release_mutation: Arc<tokio::sync::Notify>,
    calls: std::sync::Mutex<Vec<HostCallRequest>>,
}

impl MutationGateHost {
    fn release_mutation(&self) {
        self.release_mutation.notify_waiters();
    }

    fn calls(&self) -> Vec<HostCallRequest> {
        self.calls
            .lock()
            .expect("mutation gate host lock should not be poisoned")
            .clone()
    }
}

impl HostBridge for MutationGateHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "sync host bridge path should not be used for async ops".to_string(),
        ))
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> crate::host::HostBridgeFuture {
        self.calls
            .lock()
            .expect("mutation gate host lock should not be poisoned")
            .push(request.clone());
        let release_mutation = self.release_mutation.clone();
        Box::pin(async move {
            if request.operation == HostCallOperation::DocumentInsert {
                release_mutation.notified().await;
            }
            Ok(serde_json::json!({
                "status": "ok",
                "value": {
                    "operation": request.operation,
                    "payload": request.payload,
                },
            }))
        })
    }
}

async fn wait_until_slot_parked(
    slot: &mut CooperativeLockerRuntimeSlot,
    case: IsolatedRuntimeTestCase,
    context: &str,
) {
    let timeout = cooperative_slot_progress_timeout();
    tokio::time::timeout(timeout, async {
        loop {
            match slot.poll_once().await.expect("slot poll should succeed") {
                CooperativeRuntimeSlotPoll::Runnable => tokio::task::yield_now().await,
                CooperativeRuntimeSlotPoll::ResponseReady => tokio::task::yield_now().await,
                CooperativeRuntimeSlotPoll::Parked => break,
                CooperativeRuntimeSlotPoll::Completed => {
                    panic!(
                        "{context}; cooperative slot completed before the deferred async host work was released"
                    );
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "{} after {timeout:?}: {context}",
            case.failure_context("cooperative slot did not park within the bounded progress timeout")
        )
    });
}

async fn wait_until_slot_completed_without_external_release(
    slot: &mut CooperativeLockerRuntimeSlot,
    case: IsolatedRuntimeTestCase,
    context: &str,
) {
    let progress_timeout = cooperative_slot_progress_timeout();
    let wake_timeout = cooperative_slot_wake_timeout();
    tokio::time::timeout(progress_timeout, async {
        loop {
            match slot.poll_once().await.expect("slot poll should succeed") {
                CooperativeRuntimeSlotPoll::Runnable => tokio::task::yield_now().await,
                CooperativeRuntimeSlotPoll::ResponseReady => tokio::task::yield_now().await,
                CooperativeRuntimeSlotPoll::Completed => break,
                CooperativeRuntimeSlotPoll::Parked => {
                    let description = case.failure_context(
                        "cooperative slot parked on immediate async host work and never self-woke",
                    );
                    wait_for_condition(
                        description.as_str(),
                        wake_timeout,
                        std::time::Duration::ZERO,
                        || async { slot.is_ready_to_resume() },
                    )
                    .await;
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "{} after {progress_timeout:?}: {context}",
            case.failure_context(
                "cooperative slot did not complete within the bounded progress timeout without external release"
            )
        )
    });
}

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
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
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
        NimbusRuntime::with_policy(host.clone(), cooperative_warm_pool_runtime_test_policy());
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();
    let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");
    let context = RuntimeInvocationContext::top_level(&request);

    let mut slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut v8_runtime_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle: bundle.clone(),
                    request: request.clone(),
                    context: context.clone(),
                    execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                        runtime_owner.policy().as_ref(),
                        &request,
                        &context,
                    ),
                    external_cancellation: None,
                    response_ready_tx: None,
                    permit: permit.clone(),
                },
                activity_signal: activity_signal.clone(),
            },
        )
        .await
        .expect("cooperative locker slot should start");

    assert!(!slot.is_ready_to_resume());
    wait_until_slot_parked(
        &mut slot,
        PARK_AND_RESUME_CASE,
        "deferred async host work should park before release",
    )
    .await;
    assert_eq!(
        runtime_owner
            .policy
            .metrics_snapshot()
            .active_runtime_instances,
        0
    );

    let initial_generation = activity_signal.current_generation();
    host.release();
    let wake_timeout = cooperative_slot_wake_timeout();
    let description =
        PARK_AND_RESUME_CASE.failure_context("host completion should wake the cooperative slot");
    wait_for_condition(
        description.as_str(),
        wake_timeout,
        std::time::Duration::ZERO,
        || async {
            slot.is_ready_to_resume() || activity_signal.current_generation() > initial_generation
        },
    )
    .await;
    assert!(slot.is_ready_to_resume());
    wait_until_slot_completed_without_external_release(
        &mut slot,
        PARK_AND_RESUME_CASE,
        "released async host work should complete after wake",
    )
    .await;

    let result = slot
        .take_result()
        .expect("slot should keep completed value");
    assert_eq!(
        result,
        serde_json::json!({
            "ok": true,
            "host": {
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "host_call_session_id": "query:messages:list",
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
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
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
    let runtime_owner = NimbusRuntime::with_policy(
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
    let context = RuntimeInvocationContext::top_level(&request);

    let mut slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut v8_runtime_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle: bundle.clone(),
                    request: request.clone(),
                    context: context.clone(),
                    execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                        runtime_owner.policy().as_ref(),
                        &request,
                        &context,
                    ),
                    external_cancellation: None,
                    response_ready_tx: None,
                    permit,
                },
                activity_signal,
            },
        )
        .await
        .expect("cooperative locker slot should start");

    wait_until_slot_completed_without_external_release(
        &mut slot,
        IMMEDIATE_ASYNC_CASE,
        "immediate async host work should complete without requiring an external release",
    )
    .await;

    let result = slot
        .take_result()
        .expect("completed slot should retain its result");
    assert_eq!(
        result,
        serde_json::json!({
            "operation": "document_get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "host_call_session_id": "query:messages:list",
            },
        })
    );
    watchdog.shutdown();
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
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
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
    let runtime_owner = NimbusRuntime::with_policy(Arc::new(AsyncEchoHost), policy);
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();

    let expected = serde_json::json!({
        "operation": "document_get",
        "payload": {
            "table": "messages",
            "id": "doc-1",
            "host_call_session_id": "query:messages:list",
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
        let context = RuntimeInvocationContext::top_level(&request);

        let mut slot = runtime_owner
            .start_cooperative_locker_runtime_slot(
                &mut v8_runtime_pool,
                CooperativeRuntimeSlotStart {
                    invocation: RuntimeInvocationExecution {
                        watchdog: watchdog.clone(),
                        bundle: bundle.clone(),
                        request: request.clone(),
                        context: context.clone(),
                        execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                            runtime_owner.policy().as_ref(),
                            &request,
                            &context,
                        ),
                        external_cancellation: None,
                        response_ready_tx: None,
                        permit: permit.clone(),
                    },
                    activity_signal,
                },
            )
            .await
            .unwrap_or_else(|e| panic!("cycle {cycle}: slot should start: {e}"));

        wait_until_slot_completed_without_external_release(
            &mut slot,
            WARM_POOL_TWO_CYCLE_CASE,
            &format!(
                "cycle {cycle}: immediate async host should complete without requiring an external release"
            ),
        )
        .await;

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

#[test]
fn pir4_rejects_forged_host_call_session() {
    run_v8_sensitive_runtime_test_in_subprocess(PIR4_FORGED_HOST_CALL_SESSION_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative locker V8 state"]
fn pir4_rejects_forged_host_call_session_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(pir4_rejects_forged_host_call_session_inner());
}

async fn pir4_rejects_forged_host_call_session_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_document_get", {
      table: "messages",
      id: "doc-1",
      host_call_session_id: "forged-session",
    });
    return { ok: false, message: "forged host call unexpectedly reached host" };
  } catch (error) {
    return { ok: true, message: String(error && error.message ? error.message : error) };
  }
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = cooperative_query_request("messages:forged");
    let host = Arc::new(ImmediateRecordingAsyncHost::default());
    let runtime_owner =
        NimbusRuntime::with_policy(host.clone(), cooperative_warm_pool_runtime_test_policy());
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();
    let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut permit = SharedInvocationPermit::new(
        runtime_owner.policy(),
        Some("tenant-a".to_string()),
        None,
        false,
        None,
    );
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

    let mut slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut v8_runtime_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle,
                    request: request.clone(),
                    context: context.clone(),
                    execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                        runtime_owner.policy().as_ref(),
                        &request,
                        &context,
                    ),
                    external_cancellation: None,
                    response_ready_tx: None,
                    permit: permit.clone(),
                },
                activity_signal,
            },
        )
        .await
        .expect("cooperative locker slot should start");

    wait_until_slot_completed_without_external_release(
        &mut slot,
        PIR4_FORGED_HOST_CALL_SESSION_CASE,
        "forged host-call session should reject before dispatch",
    )
    .await;

    let result = slot
        .take_result()
        .expect("completed slot should retain its result");
    assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
    let message = result
        .get("message")
        .and_then(Value::as_str)
        .expect("rejection should return an error message");
    assert!(
        message.contains("stale or forged"),
        "unexpected forged-session rejection message: {message}"
    );
    assert!(
        host.calls().is_empty(),
        "forged host-call session should be rejected before host dispatch"
    );
    assert!(permit.finish_invocation().await.is_empty());
    watchdog.shutdown();
}

#[test]
fn rec3_query_write_effect_violation_rejects_before_host_dispatch() {
    run_v8_sensitive_runtime_test_in_subprocess(REC3_QUERY_WRITE_EFFECT_VIOLATION_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative locker V8 state"]
fn rec3_query_write_effect_violation_rejects_before_host_dispatch_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(rec3_query_write_effect_violation_rejects_before_host_dispatch_inner());
}

async fn rec3_query_write_effect_violation_rejects_before_host_dispatch_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  try {
    await globalThis.__nimbusAsyncHostValue("op_nimbus_document_insert", {
      table: "messages",
      fields: { body: "write from query" },
      host_call_session_id: `${request.kind}:${request.function_name}`,
    });
    return { ok: false, message: "query write unexpectedly reached host" };
  } catch (error) {
    return { ok: true, message: String(error && error.message ? error.message : error) };
  }
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = cooperative_query_request("messages:query-write");
    let host = Arc::new(ImmediateRecordingAsyncHost::default());
    let runtime_owner =
        NimbusRuntime::with_policy(host.clone(), cooperative_warm_pool_runtime_test_policy());
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();
    let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut permit = SharedInvocationPermit::new(
        runtime_owner.policy(),
        Some("tenant-a".to_string()),
        None,
        false,
        None,
    );
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");
    let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");

    let mut slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut v8_runtime_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle,
                    request: request.clone(),
                    context: context.clone(),
                    execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                        runtime_owner.policy().as_ref(),
                        &request,
                        &context,
                    ),
                    external_cancellation: None,
                    response_ready_tx: None,
                    permit: permit.clone(),
                },
                activity_signal,
            },
        )
        .await
        .expect("cooperative locker slot should start");

    wait_until_slot_completed_without_external_release(
        &mut slot,
        REC3_QUERY_WRITE_EFFECT_VIOLATION_CASE,
        "query write effect violation should reject before dispatch",
    )
    .await;

    let result = slot
        .take_result()
        .expect("completed slot should retain its result");
    assert_eq!(result.get("ok").and_then(Value::as_bool), Some(true));
    let message = result
        .get("message")
        .and_then(Value::as_str)
        .expect("effect violation result should include message");
    assert!(
        message.contains("runtime host-call effect violation"),
        "unexpected effect-violation message: {message}"
    );
    assert!(
        host.calls()
            .iter()
            .all(|call| call.operation != HostCallOperation::DocumentInsert),
        "query write effect violation should reject before DocumentInsert reaches host"
    );
    assert!(permit.finish_invocation().await.is_empty());
    watchdog.shutdown();
}

#[test]
fn pir4_interleaved_queries_preserve_host_call_sessions() {
    run_v8_sensitive_runtime_test_in_subprocess(PIR4_INTERLEAVED_HOST_CALL_SESSION_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative locker V8 state"]
fn pir4_interleaved_queries_preserve_host_call_sessions_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(pir4_interleaved_queries_preserve_host_call_sessions_inner());
}

async fn pir4_interleaved_queries_preserve_host_call_sessions_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  const host = await ctx.db.get("messages", request.function_name);
  return {
    ok: true,
    functionName: request.function_name,
    host,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let host = Arc::new(DeferredRecordingAsyncHost::default());
    let runtime_owner =
        NimbusRuntime::with_policy(host.clone(), cooperative_warm_pool_runtime_test_policy());
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();

    let first_request = cooperative_query_request("messages:first");
    let first_activity = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut first_permit = SharedInvocationPermit::new(
        runtime_owner.policy(),
        Some("tenant-a".to_string()),
        None,
        false,
        None,
    );
    first_permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("first permit should admit invocation");
    let first_context = RuntimeInvocationContext::top_level_for_tenant(&first_request, "tenant-a");
    let mut first_slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut v8_runtime_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle: bundle.clone(),
                    request: first_request.clone(),
                    context: first_context.clone(),
                    execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                        runtime_owner.policy().as_ref(),
                        &first_request,
                        &first_context,
                    ),
                    external_cancellation: None,
                    response_ready_tx: None,
                    permit: first_permit.clone(),
                },
                activity_signal: first_activity,
            },
        )
        .await
        .expect("first cooperative slot should start");
    wait_until_slot_parked(
        &mut first_slot,
        PIR4_INTERLEAVED_HOST_CALL_SESSION_CASE,
        "first tenant query should park on deferred host work",
    )
    .await;
    wait_for_value(
        "first host call should be recorded before second slot starts",
        cooperative_slot_progress_timeout(),
        std::time::Duration::ZERO,
        || async { host.calls() },
        |calls| calls.len() == 1,
    )
    .await;

    let second_request = cooperative_query_request("messages:second");
    let second_activity = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut second_permit = SharedInvocationPermit::new(
        runtime_owner.policy(),
        Some("tenant-b".to_string()),
        None,
        false,
        None,
    );
    second_permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("second permit should admit after first parked and released capacity");
    let second_context =
        RuntimeInvocationContext::top_level_for_tenant(&second_request, "tenant-b");
    let mut second_slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut v8_runtime_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle,
                    request: second_request.clone(),
                    context: second_context.clone(),
                    execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                        runtime_owner.policy().as_ref(),
                        &second_request,
                        &second_context,
                    ),
                    external_cancellation: None,
                    response_ready_tx: None,
                    permit: second_permit.clone(),
                },
                activity_signal: second_activity,
            },
        )
        .await
        .expect("second cooperative slot should start while first is parked");
    wait_until_slot_parked(
        &mut second_slot,
        PIR4_INTERLEAVED_HOST_CALL_SESSION_CASE,
        "second tenant query should also park on deferred host work",
    )
    .await;
    let calls = wait_for_value(
        "both host calls should be recorded before release",
        cooperative_slot_progress_timeout(),
        std::time::Duration::ZERO,
        || async { host.calls() },
        |calls| calls.len() == 2,
    )
    .await;
    assert_eq!(
        calls.iter().map(host_call_session_id).collect::<Vec<_>>(),
        vec![Some("query:messages:first"), Some("query:messages:second")]
    );

    host.release();
    wait_until_slot_completed_without_external_release(
        &mut second_slot,
        PIR4_INTERLEAVED_HOST_CALL_SESSION_CASE,
        "released second query should complete with its original host-call session",
    )
    .await;
    wait_until_slot_completed_without_external_release(
        &mut first_slot,
        PIR4_INTERLEAVED_HOST_CALL_SESSION_CASE,
        "released first query should complete with its original host-call session",
    )
    .await;

    let first_result = first_slot
        .take_result()
        .expect("first completed slot should retain result");
    let second_result = second_slot
        .take_result()
        .expect("second completed slot should retain result");
    assert_eq!(
        first_result
            .pointer("/host/payload/host_call_session_id")
            .and_then(Value::as_str),
        Some("query:messages:first")
    );
    assert_eq!(
        second_result
            .pointer("/host/payload/host_call_session_id")
            .and_then(Value::as_str),
        Some("query:messages:second")
    );
    assert!(first_permit.finish_invocation().await.is_empty());
    assert!(second_permit.finish_invocation().await.is_empty());
    watchdog.shutdown();
}

#[test]
fn pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler() {
    run_v8_sensitive_runtime_test_in_subprocess(PIR4_MUTATION_EXCLUSION_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative locker V8 state"]
fn pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler_inner());
}

async fn pir4_mutations_do_not_enter_multiplexed_read_safe_scheduler_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({ request });
  if (request.kind === "mutation") {
    return await ctx.db.insert("messages", { body: "write" });
  }
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let host = Arc::new(MutationGateHost::default());
    let mut limits = cooperative_warm_pool_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime_owner = NimbusRuntime::with_policy(host.clone(), policy.clone());
    let executor = RuntimeExecutor::new(policy);
    let mutation_request = cooperative_mutation_request("messages:write");
    let query_request = cooperative_query_request("messages:read");

    let mutation_handle = {
        let executor = executor.clone();
        let runtime_owner = runtime_owner.clone();
        let bundle = bundle.clone();
        let mutation_request = mutation_request.clone();
        std::thread::spawn(move || {
            executor.invoke_blocking(
                runtime_owner,
                bundle,
                mutation_request.clone(),
                RuntimeInvocationContext::top_level_for_tenant(&mutation_request, "tenant-a"),
            )
        })
    };
    wait_for_value(
        "mutation host call should be recorded before query dispatch",
        cooperative_slot_progress_timeout(),
        std::time::Duration::ZERO,
        || async { host.calls() },
        |calls| calls.len() == 1 && calls[0].operation == HostCallOperation::DocumentInsert,
    )
    .await;

    let query_handle = {
        let executor = executor.clone();
        let runtime_owner = runtime_owner.clone();
        let bundle = bundle.clone();
        let query_request = query_request.clone();
        std::thread::spawn(move || {
            executor.invoke_blocking(
                runtime_owner,
                bundle,
                query_request.clone(),
                RuntimeInvocationContext::top_level_for_tenant(&query_request, "tenant-b"),
            )
        })
    };

    tokio::time::sleep(pir4_negative_assertion_window()).await;
    let calls_before_release = host.calls();
    assert_eq!(
        calls_before_release.len(),
        1,
        "query host work must not run while mutation is suspended on the cooperative worker"
    );
    assert_eq!(
        host_call_session_id(&calls_before_release[0]),
        Some("mutation:messages:write")
    );

    host.release_mutation();
    let mutation_result = mutation_handle
        .join()
        .expect("mutation invocation thread should not panic")
        .expect("mutation invocation should complete");
    let query_result = query_handle
        .join()
        .expect("query invocation thread should not panic")
        .expect("query invocation should complete");
    assert_eq!(
        mutation_result
            .pointer("/payload/host_call_session_id")
            .and_then(Value::as_str),
        Some("mutation:messages:write")
    );
    assert_eq!(
        query_result
            .pointer("/payload/host_call_session_id")
            .and_then(Value::as_str),
        Some("query:messages:read")
    );
    let calls = host.calls();
    assert_eq!(
        calls
            .iter()
            .map(|call| (call.operation, host_call_session_id(call)))
            .collect::<Vec<_>>(),
        vec![
            (
                HostCallOperation::DocumentInsert,
                Some("mutation:messages:write")
            ),
            (HostCallOperation::DocumentGet, Some("query:messages:read")),
        ]
    );
}

#[test]
fn warm_context_recycle_cooperative_slot_destroys_fresh_realm_on_early_finish() {
    run_v8_sensitive_runtime_test_in_subprocess(FRESH_REALM_EARLY_FINISH_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate cooperative locker V8 state"]
fn warm_context_recycle_cooperative_slot_destroys_fresh_realm_on_early_finish_subprocess() {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(
            warm_context_recycle_cooperative_slot_destroys_fresh_realm_on_early_finish_inner(),
        );
}

async fn warm_context_recycle_cooperative_slot_destroys_fresh_realm_on_early_finish_inner() {
    let destroy_probe =
        super::super::realm_lifecycle::test_probe::start_fresh_realm_destroy_probe();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  return await new Promise(() => {});
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:pending".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };
    let mut limits = cooperative_context_recycle_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime_owner = NimbusRuntime::with_policy(Arc::new(AsyncEchoHost), policy);
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let watchdog = WatchdogTimer::new();
    let activity_signal = Arc::new(crate::executor::WorkerActivitySignal::new());
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");
    let context = RuntimeInvocationContext::top_level(&request);

    let slot = runtime_owner
        .start_cooperative_locker_runtime_slot(
            &mut v8_runtime_pool,
            CooperativeRuntimeSlotStart {
                invocation: RuntimeInvocationExecution {
                    watchdog: watchdog.clone(),
                    bundle,
                    request: request.clone(),
                    context: context.clone(),
                    execution_plan: crate::execution_plan::RuntimeExecutionPlan::for_invocation(
                        runtime_owner.policy().as_ref(),
                        &request,
                        &context,
                    ),
                    external_cancellation: None,
                    response_ready_tx: None,
                    permit: permit.clone(),
                },
                activity_signal,
            },
        )
        .await
        .expect("fresh-realm cooperative slot should start");
    assert_eq!(
        destroy_probe.count(),
        0,
        "fresh realm should stay live while the cooperative slot owns the pending promise"
    );

    let (result, _returned_runtime) = slot
        .finish_with_result_and_runtime(Err(NimbusRuntimeError::Cancelled))
        .await;
    assert!(
        matches!(result, Err(NimbusRuntimeError::Cancelled)),
        "early-finished slot should preserve the supplied cancellation error"
    );
    assert_eq!(
        destroy_probe.count(),
        1,
        "early-finished cooperative slot should destroy the pending fresh realm"
    );
    let ready_jobs = permit.finish_invocation().await;
    assert!(ready_jobs.is_empty());
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
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
    request,
    hostCallSessionId: `${request.kind}:${request.function_name}`,
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
        RuntimePoolKind::WarmContextRecycle,
    ] {
        let mut limits = match pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {
                cooperative_startup_snapshot_runtime_test_limits()
            }
            RuntimePoolKind::WarmPool => cooperative_warm_pool_runtime_test_limits(),
            RuntimePoolKind::WarmContextRecycle => {
                cooperative_context_recycle_runtime_test_limits()
            }
            RuntimePoolKind::BunJscTrustedRetained | RuntimePoolKind::BunJscFreshDiscard => {
                unreachable!("test covers only V8/Deno pool kinds")
            }
            RuntimePoolKind::PrecompiledModuleCache | RuntimePoolKind::RetainedStorePool => {
                unreachable!("test covers only V8/Deno pool kinds")
            }
        };
        limits.max_concurrent_runtime_instances = 1;
        limits.worker_threads = 1;
        let policy = Arc::new(RuntimePolicy::new(limits));
        let runtime = NimbusRuntime::with_policy(Arc::new(AsyncEchoHost), policy.clone());
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
            // hang forever. Keep the timeout bounded but high enough for
            // coverage-instrumented CI builds, where cooperative locker and
            // startup-snapshot paths can run materially slower than a normal
            // local test build.
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(handle.join());
            });
            let join_result = rx
                .recv_timeout(cooperative_concurrent_dispatch_join_timeout())
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
