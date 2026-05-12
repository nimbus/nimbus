use deno_core::JsRuntime;

use super::*;
use crate::backends::v8::{ReusableV8Runtime, V8RuntimeConstructionMode, V8WorkerRuntimePool};

#[tokio::test]
async fn snapshot_seeded_runtime_driver_cycles_survive_repeated_async_host_invocations() {
    init_test_tracing();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
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
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
        &request,
        "snapshot-control",
        "req-snapshot-driver-cycles",
    );
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(AsyncEchoHost),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let snapshot = runtime_owner
        .bootstrap_snapshot()
        .expect("bootstrap snapshot should build");
    let runtime = runtime_owner
        .create_runtime(&bundle, Some(snapshot), false)
        .expect("snapshot-born runtime should build");
    let expected = serde_json::json!({
        "operation": "document_get",
        "payload": {
            "table": "messages",
            "id": "doc-1",
            "session_id": "query:messages:get",
        }
    });
    let cycles = usize_env_or("NIMBUS_SNAPSHOT_DRIVER_CYCLES", 32);
    let watchdog = WatchdogTimer::new();
    let mut reusable_runtime =
        ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::StartupSnapshot);

    for cycle in 0..cycles {
        let mut permit = SharedInvocationPermit::new(
            runtime_owner.policy(),
            context.tenant_label.clone(),
            None,
            false,
            None,
        );
        permit
            .acquire_initial(std::time::Instant::now())
            .await
            .expect("permit should admit invocation");

        let mut driver = runtime_owner
            .prepare_runtime_invocation_driver(
                reusable_runtime,
                watchdog.clone(),
                None,
                permit.clone(),
                false,
            )
            .expect("driver preparation should succeed for snapshot-seeded runtime");

        runtime_owner
            .load_bundle_with_trace(
                &mut driver.runtime,
                &bundle,
                driver.construction_mode,
                Some(&context),
                Some(&request),
            )
            .await
            .expect("bundle should load during each direct driver cycle");
        let value = runtime_owner
            .invoke_loaded_bundle_with_trace(
                &mut driver.runtime,
                &request,
                Some(&bundle),
                driver.construction_mode,
                Some(&context),
            )
            .await
            .expect("direct driver cycle should complete async host work");
        let (result, returned_runtime) = driver.finalize_with_runtime(Ok(value)).await;
        let ready_jobs = permit.finish_invocation().await;
        assert!(ready_jobs.is_empty(), "no extra jobs should be scheduled");
        assert_eq!(
            result.expect("direct driver cycle should finalize cleanly"),
            expected,
            "driver cycle {cycle} should preserve the expected async-host result"
        );
        reusable_runtime = returned_runtime
            .expect("successful direct driver cycle should return the runtime for reuse");
    }

    watchdog.shutdown();
}

#[tokio::test]
async fn snapshot_seeded_runtime_driver_cycles_survive_with_fresh_runtime_owner_each_cycle() {
    init_test_tracing();
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
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
        services: Default::default(),
    };
    let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
        &request,
        "snapshot-control",
        "req-snapshot-driver-fresh-owner",
    );
    let host = Arc::new(AsyncEchoHost);
    let initial_owner = NimbusRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let snapshot = initial_owner
        .bootstrap_snapshot()
        .expect("bootstrap snapshot should build");
    let runtime = initial_owner
        .create_runtime(&bundle, Some(snapshot), false)
        .expect("snapshot-born runtime should build");
    let expected = serde_json::json!({
        "operation": "document_get",
        "payload": {
            "table": "messages",
            "id": "doc-1",
            "session_id": "query:messages:get",
        }
    });
    let cycles = usize_env_or("NIMBUS_SNAPSHOT_DRIVER_FRESH_OWNER_CYCLES", 32);
    let watchdog = WatchdogTimer::new();
    let mut reusable_runtime =
        ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::StartupSnapshot);

    for cycle in 0..cycles {
        let runtime_owner = NimbusRuntime::with_policy(
            host.clone(),
            run_to_completion_snapshot_runtime_test_policy(),
        );
        let mut permit = SharedInvocationPermit::new(
            runtime_owner.policy(),
            context.tenant_label.clone(),
            None,
            false,
            None,
        );
        permit
            .acquire_initial(std::time::Instant::now())
            .await
            .expect("permit should admit invocation");

        let mut driver = runtime_owner
            .prepare_runtime_invocation_driver(
                reusable_runtime,
                watchdog.clone(),
                None,
                permit.clone(),
                false,
            )
            .expect("driver preparation should succeed for snapshot-seeded runtime");

        runtime_owner
            .load_bundle_with_trace(
                &mut driver.runtime,
                &bundle,
                driver.construction_mode,
                Some(&context),
                Some(&request),
            )
            .await
            .expect("bundle should load during each fresh-owner driver cycle");
        let value = runtime_owner
            .invoke_loaded_bundle_with_trace(
                &mut driver.runtime,
                &request,
                Some(&bundle),
                driver.construction_mode,
                Some(&context),
            )
            .await
            .expect("fresh-owner driver cycle should complete async host work");
        let (result, returned_runtime) = driver.finalize_with_runtime(Ok(value)).await;
        let ready_jobs = permit.finish_invocation().await;
        assert!(ready_jobs.is_empty(), "no extra jobs should be scheduled");
        assert_eq!(
            result.expect("fresh-owner driver cycle should finalize cleanly"),
            expected,
            "fresh-owner driver cycle {cycle} should preserve the expected async-host result"
        );
        reusable_runtime = returned_runtime
            .expect("successful fresh-owner driver cycle should return the runtime for reuse");
    }

    watchdog.shutdown();
}

#[test]
fn snapshot_seeded_runtime_driver_cycles_survive_on_current_thread_runtime_with_delayed_async_host()
{
    init_test_tracing();
    let _test_lock = acquire_snapshot_reset_test_lock();
    let worker_thread = std::thread::spawn(move || {
        let worker_runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime should build");
        worker_runtime.block_on(async {
            let tempdir = tempdir().expect("tempdir should build");
            let bundle_path = tempdir.path().join("bundle.mjs");
            std::fs::write(
                &bundle_path,
                r#"
globalThis.__nimbusInvoke = async function (request) {
  const ctx = globalThis.__nimbusCreateContext({
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
                services: Default::default(),
            };
            let context = RuntimeInvocationContext::top_level_for_tenant_and_request(
                &request,
                "snapshot-control",
                "req-snapshot-driver-current-thread-delayed",
            );
            let runtime_owner = NimbusRuntime::with_policy(
                Arc::new(DelayedAsyncEchoHost::new(std::time::Duration::from_millis(1))),
                run_to_completion_snapshot_runtime_test_policy(),
            );
            let snapshot = runtime_owner
                .bootstrap_snapshot()
                .expect("bootstrap snapshot should build");
            let runtime = runtime_owner
                .create_runtime(&bundle, Some(snapshot), false)
                .expect("snapshot-born runtime should build");
            let expected = serde_json::json!({
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "query:messages:get",
                }
            });
            let cycles = usize_env_or("NIMBUS_SNAPSHOT_DRIVER_CURRENT_THREAD_CYCLES", 32);
            let watchdog = WatchdogTimer::new();
            let mut reusable_runtime =
                ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::StartupSnapshot);

            for cycle in 0..cycles {
                let mut permit = SharedInvocationPermit::new(
                    runtime_owner.policy(),
                    context.tenant_label.clone(),
                    None,
                    false,
                    None,
                );
                permit
                    .acquire_initial(std::time::Instant::now())
                    .await
                    .expect("permit should admit invocation");

                let mut driver = runtime_owner
                    .prepare_runtime_invocation_driver(
                        reusable_runtime,
                        watchdog.clone(),
                        None,
                        permit.clone(),
                        false,
                    )
                    .expect(
                        "driver preparation should succeed for snapshot-seeded delayed runtime",
                    );

                runtime_owner
                    .load_bundle_with_trace(
                        &mut driver.runtime,
                        &bundle,
                        driver.construction_mode,
                        Some(&context),
                        Some(&request),
                    )
                    .await
                    .expect("bundle should load during each delayed current-thread driver cycle");
                let value = runtime_owner
                    .invoke_loaded_bundle_with_trace(
                        &mut driver.runtime,
                        &request,
                        Some(&bundle),
                        driver.construction_mode,
                        Some(&context),
                    )
                    .await
                    .expect(
                        "delayed current-thread driver cycle should complete async host work",
                    );
                let (result, returned_runtime) = driver.finalize_with_runtime(Ok(value)).await;
                let ready_jobs = permit.finish_invocation().await;
                assert!(ready_jobs.is_empty(), "no extra jobs should be scheduled");
                assert_eq!(
                    result.expect("delayed current-thread driver cycle should finalize cleanly"),
                    expected,
                    "delayed current-thread driver cycle {cycle} should preserve the expected async-host result"
                );
                reusable_runtime = returned_runtime.expect(
                    "successful delayed current-thread driver cycle should return the runtime for reuse",
                );
            }

            watchdog.shutdown();
        });
    });

    worker_thread
        .join()
        .expect("current-thread worker thread should not panic");
}

#[tokio::test]
async fn reused_runtime_still_leaks_user_module_state_after_current_resets() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__userCounter = globalThis.__userCounter ?? 0;

globalThis.__nimbusInvoke = function () {
  globalThis.__userCounter += 1;
  return { counter: globalThis.__userCounter };
};

export {};
"#,
    )
    .expect("bundle should write");

    let bundle = RuntimeBundle::new(&bundle_path);
    let runtime_owner = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let mut v8_runtime_pool = V8WorkerRuntimePool::new();
    let mut runtime = v8_runtime_pool
        .take_runtime(&runtime_owner, &bundle)
        .expect("runtime should build from snapshot")
        .runtime;
    runtime_owner
        .load_bundle(&mut runtime, &bundle)
        .await
        .expect("bundle should load");

    async fn invoke_with_current_reset_contract(
        runtime_owner: &NimbusRuntime,
        runtime: &mut JsRuntime,
    ) -> Value {
        bootstrap::reset_runtime_invocation_state(
            runtime,
            SharedInvocationPermit::new(runtime_owner.policy(), None, None, true, None),
        );
        bootstrap::reset_bootstrap_invocation_state(runtime)
            .expect("bootstrap invocation reset should succeed");
        runtime_owner
            .invoke_loaded_bundle(
                runtime,
                &InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: "messages:get".to_string(),
                    args: Value::Null,
                    page_size: None,
                    cursor: None,
                    auth: None,
                    services: Default::default(),
                },
            )
            .await
            .expect("invocation should succeed")
    }

    let first = invoke_with_current_reset_contract(&runtime_owner, &mut runtime).await;
    let second = invoke_with_current_reset_contract(&runtime_owner, &mut runtime).await;

    assert_eq!(first, serde_json::json!({ "counter": 1 }));
    assert_eq!(
        second,
        serde_json::json!({ "counter": 2 }),
        "user-module/global state still persists on a reused loaded runtime even after the current Rust-side and bootstrap-state resets"
    );
}
