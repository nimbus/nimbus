use super::*;

#[tokio::test]
async fn runtime_times_out_infinite_loops() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  while (true) {}
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = std::time::Duration::from_millis(50);
    let runtime = NimbusRuntime::with_limits(Arc::new(RecordingHost::default()), limits);
    let error = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect_err("infinite loop should time out");

    match error {
        NimbusRuntimeError::ExecutionTimeout(timeout) => {
            assert_eq!(timeout, std::time::Duration::from_millis(50));
        }
        other => panic!("unexpected timeout error: {other}"),
    }
    assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 1);
}

#[tokio::test]
async fn runtime_external_cancellation_stops_infinite_loops() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  while (true) {}
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = std::time::Duration::from_secs(5);
    let runtime = NimbusRuntime::with_limits(Arc::new(RecordingHost::default()), limits);
    let cancellation = HostCallCancellation::default();
    let cancellation_clone = cancellation.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(50));
        cancellation_clone.cancel();
    });

    let error = runtime
        .invoke_bundle_with_cancellation(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
            Some(cancellation),
        )
        .await
        .expect_err("external cancellation should stop the runtime invocation");

    assert!(matches!(error, NimbusRuntimeError::Cancelled));
}

#[tokio::test]
async fn runtime_times_out_slow_async_host_ops() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  await ctx.db.get("messages", "doc-1");
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = std::time::Duration::from_millis(50);
    let runtime = NimbusRuntime::with_limits(
        Arc::new(SlowEnvelopeHost {
            delay: std::time::Duration::from_secs(1),
        }),
        limits,
    );
    let error = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
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
        .expect_err("slow async host op should time out");

    match error {
        NimbusRuntimeError::ExecutionTimeout(timeout) => {
            assert_eq!(timeout, std::time::Duration::from_millis(50));
        }
        other => panic!("unexpected timeout error: {other}"),
    }
}
