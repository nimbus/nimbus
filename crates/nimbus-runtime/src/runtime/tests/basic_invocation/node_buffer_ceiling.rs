use super::support::*;
use super::*;

// Node 20 runs V8 11.3, which rejects an ArrayBuffer longer than 2^32. V8 150.4
// compiles `kMaxByteLength` at 2^53 - 1 and exposes no flag for it, but it reads
// the maximum allocation size from the array buffer allocator and caches it per
// isolate. The Node 20 target therefore wraps its allocator with a 2^32 ceiling,
// which is what `test-buffer-alloc.js:14` of the Node 20 corpus asserts. The
// ceiling is per isolate, so every other lane keeps the ceiling of the build.
#[tokio::test]
async fn the_array_buffer_ceiling_tracks_the_compatibility_target() {
    let _guard = acquire_basic_invocation_suite_lock().await;
    let (_tempdir, bundle_path) = write_app_style_bundle(
        r#"
globalThis.__nimbusInvoke = function () {
  const errorOf = (fn) => {
    try {
      fn();
      return null;
    } catch (error) {
      return `${error.name}: ${error.message}`;
    }
  };
  // A lane that keeps the ceiling of the build attempts a 4 GiB allocation for
  // this length, so the bundle runs it on the Node 20 lane only, where the
  // ceiling rejects it before it allocates.
  const isNode20 = process.versions.node.startsWith("20.");
  // A resizable ArrayBuffer reserves address space instead of memory, so the
  // over-ceiling case stays cheap on every lane.
  return {
    node: process.versions.node,
    typedArrayOverCeiling: isNode20
      ? errorOf(() => new Uint8Array(4294967297))
      : null,
    resizableOverCeiling: errorOf(
      () => new ArrayBuffer(0, { maxByteLength: 4294967297 }),
    ),
    allocates: new Uint8Array(1024).length,
  };
};

export {};
"#,
    );

    // The Node 20 lane rejects both over-ceiling requests. Every lane that keeps
    // the ceiling of the build accepts the resizable reservation, and every lane
    // still allocates an ordinary buffer.
    let lanes = [
        (
            "node20",
            RuntimeLimits::application_node20_local_development(),
            Some("RangeError: Invalid typed array length: 4294967297"),
            Some("RangeError: Invalid array buffer max length"),
        ),
        (
            "node22",
            RuntimeLimits::application_node22_local_development(),
            None,
            None,
        ),
    ];

    for (lane, limits, expected_typed_array, expected_resizable) in lanes {
        let runtime = NimbusRuntime::with_policy(
            Arc::new(RecordingHost::default()),
            runtime_test_policy_with_real_fs(limits),
            crate::RuntimeEgressPosture::CoarsePermissions,
        );
        let result = runtime
            .invoke_bundle_for_tenant_for_test(
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
            .expect("buffer ceiling bundle should execute");

        assert_eq!(
            result["typedArrayOverCeiling"].as_str(),
            expected_typed_array,
            "{lane} typed array over the ceiling: {result}"
        );
        assert_eq!(
            result["resizableOverCeiling"].as_str(),
            expected_resizable,
            "{lane} resizable ArrayBuffer over the ceiling: {result}"
        );
        assert_eq!(
            result["allocates"].as_u64(),
            Some(1024),
            "{lane} ordinary allocation: {result}"
        );
    }
}
