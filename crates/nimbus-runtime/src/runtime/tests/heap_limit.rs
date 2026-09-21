use super::*;

fn heap_limit_test_request() -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

fn heap_limit_test_runtime() -> NimbusRuntime {
    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_heap_mb = 32;
    limits.initial_heap_mb = 16;
    limits.execution_timeout = std::time::Duration::from_secs(5);
    limits.max_concurrent_runtime_instances = 1;
    NimbusRuntime::with_limits(
        Arc::new(RecordingHost::default()),
        limits,
        crate::RuntimeEgressPosture::CoarsePermissions,
    )
}

// V8 calls the near-heap-limit callback for a failed ArrayBuffer allocation.
// Node.js throws a catchable RangeError for it, and so must nimbus.
#[tokio::test]
async fn failed_array_buffer_allocation_throws_range_error_without_terminating() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  const errors = [];
  for (let attempt = 0; attempt < 8; attempt++) {
    try {
      new ArrayBuffer(Number.MAX_SAFE_INTEGER);
      errors.push("allocated");
    } catch (error) {
      errors.push(`${error.name}: ${error.message}`);
    }
  }
  const after = new Uint8Array(1024);
  return { errors, afterLength: after.length };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = heap_limit_test_runtime();
    let result = runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&bundle_path),
            &heap_limit_test_request(),
            "tenant-a",
        )
        .await
        .expect("a failed ArrayBuffer allocation should not terminate the invocation");

    assert_eq!(
        result,
        serde_json::json!({
            "errors": vec!["RangeError: Array buffer allocation failed"; 8],
            "afterLength": 1024,
        })
    );
}

// A failed ArrayBuffer allocation must not raise the heap limit or disable
// its enforcement. With a raised limit this invocation reaches the execution
// timeout instead of the 32 MiB heap limit.
#[tokio::test]
async fn heap_growth_after_failed_array_buffer_allocations_trips_the_heap_limit() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  for (let attempt = 0; attempt < 16; attempt++) {
    try {
      new ArrayBuffer(Number.MAX_SAFE_INTEGER);
    } catch {}
  }
  const retained = [];
  while (true) {
    retained.push(new Array(4096).fill(retained.length));
  }
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = heap_limit_test_runtime();
    let error = runtime
        .invoke_bundle_for_tenant_for_test(
            &RuntimeBundle::new(&bundle_path),
            &heap_limit_test_request(),
            "tenant-a",
        )
        .await
        .expect_err("heap growth should trip the runtime heap limit");

    match error {
        NimbusRuntimeError::HeapLimitExceeded(limit) => assert_eq!(limit, 32),
        other => panic!("unexpected heap-limit error: {other}"),
    }
}
