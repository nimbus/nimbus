use super::*;

struct FailingAsyncEnvelopeHost;

impl crate::host::HostBridge for FailingAsyncEnvelopeHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "sync host bridge path should not be used for failing async ops".to_string(),
        ))
    }

    fn call_async(
        &self,
        _request: HostCallRequest,
        _cancellation: HostCallCancellation,
    ) -> crate::host::HostBridgeFuture {
        Box::pin(async {
            Err(NimbusRuntimeError::Contract(
                "intentional async host failure".to_string(),
            ))
        })
    }
}

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
        .invoke_bundle_for_tenant(
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
            "tenant-a",
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
        .invoke_bundle_for_tenant_with_cancellation(
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
            "tenant-a",
            Some(cancellation),
        )
        .await
        .expect_err("external cancellation should stop the runtime invocation");

    assert!(matches!(error, NimbusRuntimeError::Cancelled));
}

#[tokio::test]
async fn pir4_user_timeout_pauses_during_slow_async_host_ops() {
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
    limits.system_timeout = std::time::Duration::from_secs(1);
    let runtime = NimbusRuntime::with_limits(
        Arc::new(DelayedAsyncEnvelopeHost {
            delay: std::time::Duration::from_millis(200),
        }),
        limits,
    );
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("slow async host op should not burn user execution timeout");

    assert_eq!(result, serde_json::json!({ "ok": true }));
    assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 0);
}

#[tokio::test]
async fn pir4_system_timeout_bounds_slow_async_host_wall_time() {
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
    limits.execution_timeout = std::time::Duration::from_secs(1);
    limits.system_timeout = std::time::Duration::from_millis(50);
    let runtime = NimbusRuntime::with_limits(
        Arc::new(DelayedAsyncEnvelopeHost {
            delay: std::time::Duration::from_millis(200),
        }),
        limits,
    );
    let error = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect_err("slow async host op should trip system wall timeout");

    match error {
        NimbusRuntimeError::SystemTimeout(timeout) => {
            assert_eq!(timeout, std::time::Duration::from_millis(50));
        }
        other => panic!("unexpected timeout error: {other}"),
    }
    assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 1);
}

#[tokio::test]
async fn pir4_user_timeout_resumes_after_catchable_async_host_error() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  try {
    await ctx.db.get("messages", "doc-1");
  } catch (_error) {
    while (true) {}
  }
  return { ok: false };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = std::time::Duration::from_millis(50);
    limits.system_timeout = std::time::Duration::from_secs(1);
    let runtime = NimbusRuntime::with_limits(Arc::new(FailingAsyncEnvelopeHost), limits);
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        runtime.invoke_bundle_for_tenant(
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
            "tenant-a",
        ),
    )
    .await
    .expect("runtime watchdog should fire before the outer test timeout")
    .expect_err("caught async host error should resume user timeout before JS loops");

    match error {
        NimbusRuntimeError::ExecutionTimeout(timeout) => {
            assert_eq!(timeout, std::time::Duration::from_millis(50));
        }
        other => panic!("unexpected timeout error: {other}"),
    }
    assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 1);
}

#[tokio::test]
async fn pir4_wait_until_drains_background_work_after_response_ready() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  globalThis.__nimbusWaitUntil((async () => {
    await ctx.db.get("messages", "background");
  })());
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = std::time::Duration::from_secs(1);
    limits.system_timeout = std::time::Duration::from_secs(1);
    let host = Arc::new(CountingDelayedAsyncEnvelopeHost::new(
        std::time::Duration::from_millis(10),
    ));
    let runtime = NimbusRuntime::with_limits(host.clone(), limits);
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("waitUntil background work should drain after response readiness");

    assert_eq!(result, serde_json::json!({ "ok": true }));
    assert_eq!(host.calls(), 1);
    assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 0);
}

#[tokio::test]
async fn pir4_wait_until_system_budget_is_fresh_after_response_ready() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  await ctx.db.get("messages", "response");
  globalThis.__nimbusWaitUntil((async () => {
    await ctx.db.get("messages", "background");
  })());
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = std::time::Duration::from_secs(1);
    limits.system_timeout = std::time::Duration::from_millis(180);
    let host = Arc::new(CountingDelayedAsyncEnvelopeHost::new(
        std::time::Duration::from_millis(120),
    ));
    let runtime = NimbusRuntime::with_limits(host.clone(), limits);
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("waitUntil should receive a fresh system wall budget");

    assert_eq!(result, serde_json::json!({ "ok": true }));
    assert_eq!(host.calls(), 2);
    assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 0);
}

#[tokio::test]
async fn pir4_wait_until_system_timeout_bounds_background_work() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  globalThis.__nimbusWaitUntil((async () => {
    await ctx.db.get("messages", "background");
  })());
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = std::time::Duration::from_secs(1);
    limits.system_timeout = std::time::Duration::from_millis(50);
    let host = Arc::new(CountingDelayedAsyncEnvelopeHost::new(
        std::time::Duration::from_millis(200),
    ));
    let runtime = NimbusRuntime::with_limits(host.clone(), limits);
    let error = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect_err("waitUntil background work should be bounded by system timeout");

    match error {
        NimbusRuntimeError::SystemTimeout(timeout) => {
            assert_eq!(timeout, std::time::Duration::from_millis(50));
        }
        other => panic!("unexpected waitUntil timeout error: {other}"),
    }
    assert_eq!(host.calls(), 1);
    assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 1);
}

#[tokio::test]
async fn pir4_wait_until_system_timeout_bounds_unreferenced_pending_background_work() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  globalThis.__nimbusWaitUntil(new Promise(() => {}));
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.execution_timeout = std::time::Duration::from_secs(1);
    limits.system_timeout = std::time::Duration::from_millis(300);
    let runtime = NimbusRuntime::with_limits(Arc::new(RecordingHost::default()), limits);
    let error = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect_err("unreferenced pending waitUntil work should be bounded by system timeout");

    match error {
        NimbusRuntimeError::SystemTimeout(timeout) => {
            assert_eq!(timeout, std::time::Duration::from_millis(300));
        }
        other => panic!("unexpected unreferenced waitUntil timeout error: {other}"),
    }
    assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 1);
}

#[tokio::test]
async fn pir4_wait_until_drains_on_cooperative_queries() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = async function () {
  const ctx = globalThis.__nimbusCreateContext();
  globalThis.__nimbusWaitUntil((async () => {
    await ctx.db.get("messages", "background");
  })());
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = cooperative_startup_snapshot_runtime_test_limits();
    limits.execution_timeout = std::time::Duration::from_secs(1);
    limits.system_timeout = std::time::Duration::from_secs(1);
    let host = Arc::new(CountingDelayedAsyncEnvelopeHost::new(
        std::time::Duration::from_millis(10),
    ));
    let runtime = NimbusRuntime::with_limits(host.clone(), limits);
    let result = runtime
        .invoke_bundle_for_tenant(
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
            "tenant-a",
        )
        .await
        .expect("cooperative waitUntil background work should drain before runtime reuse");

    assert_eq!(result, serde_json::json!({ "ok": true }));
    assert_eq!(host.calls(), 1);
    assert_eq!(runtime.policy.metrics_snapshot().timed_out_invocations, 0);
}
