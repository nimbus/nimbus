use super::*;

#[test]
fn runtime_new_uses_product_default_runtime_policy() {
    let runtime = NeovexRuntime::new(Arc::new(RecordingHost::default()));
    assert_eq!(
        runtime.policy().limits(),
        &product_default_runtime_test_limits()
    );
}

#[tokio::test]
async fn runtime_loads_bundle_and_invokes_host_bridge() {
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

    let host = Arc::new(RecordingHost::default());
    let runtime = NeovexRuntime::with_policy(
        host.clone(),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Query,
                function_name: "messages:list".to_string(),
                args: serde_json::json!({ "author": "Ada" }),
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect("bundle invocation should succeed");

    assert_eq!(
        result,
        serde_json::json!({
            "ok": true,
            "host": {
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "query:messages:list",
                }
            }
        })
    );

    let calls = host
        .calls
        .lock()
        .expect("recording host lock should not be poisoned")
        .clone();
    assert_eq!(
        calls,
        vec![HostCallRequest::new(
            HostCallOperation::DocumentGet,
            serde_json::json!({
                "table": "messages",
                "id": "doc-1",
                "session_id": "query:messages:list",
            }),
        )]
    );
}

#[tokio::test]
async fn runtime_requires_bundle_contract() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(&bundle_path, "export const noop = 1;").expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let error = runtime
        .invoke_bundle(
            &RuntimeBundle::new(&bundle_path),
            &InvocationRequest {
                kind: InvocationKind::Action,
                function_name: "messages:missing".to_string(),
                args: Value::Null,
                page_size: None,
                cursor: None,
                auth: None,
                services: Default::default(),
            },
        )
        .await
        .expect_err("missing global invoke contract should fail");

    assert!(
        error.to_string().contains("__neovexInvoke"),
        "unexpected error: {error}"
    );
}

#[tokio::test]
async fn runtime_awaits_async_bundle_handlers() {
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
  const value = await ctx.db.get("messages", "doc-1");
  return {
    ok: true,
    awaited: await Promise.resolve({
      operation: value.operation,
      payload: value.payload,
    }),
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let host = Arc::new(RecordingHost::default());
    let runtime =
        NeovexRuntime::with_policy(host, run_to_completion_snapshot_runtime_test_policy());
    let result = runtime
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
        .expect("async bundle invocation should succeed");

    assert_eq!(
        result,
        serde_json::json!({
            "ok": true,
            "awaited": {
                "operation": "document_get",
                "payload": {
                    "table": "messages",
                    "id": "doc-1",
                    "session_id": "query:messages:list",
                }
            }
        })
    );
}

#[tokio::test]
async fn runtime_does_not_expose_legacy_host_globals() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__neovexInvoke = function () {
  return {
    rawHostCall: typeof globalThis.__neovexRawHostCall,
    hostValue: typeof globalThis.__neovexHostValue,
  };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
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
        .expect("bundle should observe runtime globals");

    assert_eq!(
        result,
        serde_json::json!({
            "rawHostCall": "undefined",
            "hostValue": "undefined",
        })
    );
}

#[tokio::test]
async fn runtime_removes_deno_global_from_bundle_execution() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
if (typeof Deno !== "undefined") {
  throw new Error("Deno should not be exposed to runtime bundles");
}

globalThis.__neovexInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NeovexRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
    );
    let result = runtime
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
        .expect("bundle should execute without exposing Deno");

    assert_eq!(result, serde_json::json!({ "ok": true }));
}

#[tokio::test]
async fn convenience_runtime_invocations_reuse_runtime_owned_executor() {
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

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_concurrent_runtime_instances = 1;
    limits.worker_threads = 1;
    let policy = Arc::new(RuntimePolicy::new(limits));
    let runtime = NeovexRuntime::with_policy(Arc::new(RecordingHost::default()), policy.clone());
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

    let first = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .expect("first convenience invocation should succeed");
    let second = runtime
        .invoke_bundle(&bundle, &request)
        .await
        .expect("second convenience invocation should succeed");

    assert_eq!(first, serde_json::json!({ "moduleLoadCount": 1 }));
    assert_eq!(second, serde_json::json!({ "moduleLoadCount": 1 }));
    let metrics = policy.metrics_snapshot();
    assert_eq!(metrics.runtime_pool_misses, 1);
    assert_eq!(metrics.runtime_pool_hits, 1);
    assert_eq!(metrics.runtime_pool_replacements, 0);
    assert_eq!(metrics.bundle_loads, 2);
    assert!(metrics.bundle_load_nanos_total > 0);
    assert_eq!(metrics.bundle_module_loads, 2);
    assert!(metrics.bundle_module_load_nanos_total > 0);
    assert_eq!(metrics.bundle_evaluations, 2);
    assert!(metrics.bundle_evaluation_nanos_total > 0);
}
