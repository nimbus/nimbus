use std::rc::Rc;

use deno_core::{JsRuntime, PollEventLoopOptions};

use super::*;

#[tokio::test]
async fn pooled_runtime_invocations_keep_module_state_fresh() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__moduleLoadCount = (globalThis.__moduleLoadCount ?? 0) + 1;

globalThis.__neovexInvoke = async function () {
  return { moduleLoadCount: globalThis.__moduleLoadCount };
};

export {};
"#,
    )
    .expect("bundle should write");

    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let bundle = RuntimeBundle::new(&bundle_path);
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };

    let first = invoke_on_single_worker(&executor, runtime.clone(), &bundle, request.clone())
        .await
        .expect("first pooled invocation should succeed");
    let second = invoke_on_single_worker(&executor, runtime, &bundle, request)
        .await
        .expect("second pooled invocation should succeed");

    assert_eq!(first, serde_json::json!({ "moduleLoadCount": 1 }));
    assert_eq!(second, serde_json::json!({ "moduleLoadCount": 1 }));
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 1);
    assert_eq!(metrics.isolate_pool_hits, 1);
}

#[tokio::test]
async fn pooled_runtime_invocations_reset_auth_and_session_state() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({ request });
  const user = await ctx.auth.getUserIdentity();
  const host = await ctx.db.get("messages", "doc-1");
  return {
    token: user?.tokenIdentifier ?? null,
    session: host.payload.session_id,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
        max_concurrent_isolates: 1,
        worker_threads: 1,
        ..RuntimeLimits::default()
    }));
    let executor = RuntimeExecutor::new(policy.clone());
    let runtime = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy);
    let bundle = RuntimeBundle::new(&bundle_path);

    let first = invoke_on_single_worker(
        &executor,
        runtime.clone(),
        &bundle,
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "auth:first".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: Some(test_invocation_auth("token-1")),
        },
    )
    .await
    .expect("first pooled invocation should succeed");
    let second = invoke_on_single_worker(
        &executor,
        runtime,
        &bundle,
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "auth:second".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: Some(test_invocation_auth("token-2")),
        },
    )
    .await
    .expect("second pooled invocation should succeed");

    assert_eq!(
        first,
        serde_json::json!({
            "token": "token-1",
            "session": "session-1",
        })
    );
    assert_eq!(
        second,
        serde_json::json!({
            "token": "token-2",
            "session": "session-1",
        })
    );
    let metrics = executor.policy().metrics_snapshot();
    assert_eq!(metrics.isolate_pool_misses, 1);
    assert_eq!(metrics.isolate_pool_hits, 1);
    assert_eq!(metrics.isolate_pool_replacements, 0);
}

#[tokio::test]
async fn reused_runtime_refreshes_invocation_cancellation_state_before_next_invoke() {
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
        function_name: "messages:get".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
    };
    let runtime_owner = NeovexRuntime::new(Arc::new(AsyncEchoHost));
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let mut runtime = isolate_pool
        .take_runtime(&runtime_owner, &bundle)
        .expect("runtime should build from snapshot")
        .runtime;
    runtime_owner
        .load_bundle(&mut runtime, &bundle)
        .await
        .expect("bundle should load");

    let previous_cancel_handle = {
        let op_state = runtime.op_state();
        let state = op_state.borrow();
        let cancellation_state = state.borrow::<RuntimeCancellationState>();
        cancellation_state.signal.cancel();
        assert!(
            cancellation_state.signal.is_cancelled(),
            "test should poison the previous invocation state"
        );
        cancellation_state.cancel_handle.clone()
    };

    let watchdog = WatchdogTimer::new();
    let mut permit = SharedInvocationPermit::new(runtime_owner.policy(), None, None, false, None);
    permit
        .acquire_initial(std::time::Instant::now())
        .await
        .expect("permit should admit invocation");

    let mut driver = runtime_owner
        .prepare_runtime_invocation_driver(
            ReusableRuntime::fresh(runtime, RuntimeConstructionMode::StartupSnapshot),
            watchdog.clone(),
            None,
            permit.clone(),
            false,
        )
        .expect("driver preparation should reset invocation state");

    {
        let op_state = driver.runtime.op_state();
        let state = op_state.borrow();
        let cancellation_state = state.borrow::<RuntimeCancellationState>();
        assert!(
            !cancellation_state.signal.is_cancelled(),
            "fresh invocation state should not inherit the previous cancelled signal"
        );
        assert!(
            !Rc::ptr_eq(&previous_cancel_handle, &cancellation_state.cancel_handle),
            "fresh invocation state should replace the previous cancel handle"
        );
    }

    let result = runtime_owner
        .invoke_loaded_bundle(&mut driver.runtime, &request)
        .await
        .expect("fresh invocation state should allow async host work to complete");
    let result = driver
        .finalize(Ok(result))
        .await
        .expect("result should finalize");
    let ready_jobs = permit.finish_invocation().await;

    assert!(ready_jobs.is_empty());
    assert_eq!(
        result,
        serde_json::json!({
            "operation": "convex.ctx.db.get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "session_id": "query:messages:get",
            },
        })
    );
    watchdog.shutdown();
}

#[tokio::test]
async fn reused_runtime_refreshes_bootstrap_session_state_before_next_invoke() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export {};").expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let runtime_owner = NeovexRuntime::new(Arc::new(AsyncEchoHost));
    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
    let mut runtime = isolate_pool
        .take_runtime(&runtime_owner, &bundle)
        .expect("runtime should build from snapshot")
        .runtime;

    async fn issue_default_context_get(runtime: &mut JsRuntime) -> Value {
        let value = runtime
            .execute_script(
                "<neovex-runtime:test-default-context-get>",
                r#"(async () => {
  const ctx = globalThis.__neovexCreateContext();
  return await ctx.db.get("messages", "doc-1");
})()"#,
            )
            .expect("test script should execute");
        let resolve = runtime.resolve(value);
        let value = runtime
            .with_event_loop_promise(resolve, PollEventLoopOptions::default())
            .await
            .expect("promise should resolve");
        deserialize_json_value(runtime, value).expect("result should deserialize")
    }

    let first = issue_default_context_get(&mut runtime).await;
    let second_without_reset = issue_default_context_get(&mut runtime).await;

    bootstrap::reset_bootstrap_invocation_state(&mut runtime)
        .expect("bootstrap reset should succeed on reused runtime");

    let third_after_reset = issue_default_context_get(&mut runtime).await;

    assert_eq!(
        first,
        serde_json::json!({
            "operation": "convex.ctx.db.get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "session_id": "session-1",
            },
        })
    );
    assert_eq!(
        second_without_reset,
        serde_json::json!({
            "operation": "convex.ctx.db.get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "session_id": "session-2",
            },
        })
    );
    assert_eq!(
        third_after_reset,
        serde_json::json!({
            "operation": "convex.ctx.db.get",
            "payload": {
                "table": "messages",
                "id": "doc-1",
                "session_id": "session-1",
            },
        })
    );
}
