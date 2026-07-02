use std::sync::Arc;

use super::*;
use crate::limits::{RuntimeLimits, RuntimePolicy};

struct WasmtimeStorePoolHost;

impl HostBridge for WasmtimeStorePoolHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Ok(Value::Null)
    }
}

#[tokio::test]
async fn wasmtime_store_pool_invokes_component_with_retained_store_pool() {
    let (_tempdir, bundle) = write_component_fixture(nimbus_function_component_wat());
    let policy = Arc::new(RuntimePolicy::new(wasmtime_store_pool_test_limits()));
    let runtime = NimbusRuntime::with_policy(Arc::new(WasmtimeStorePoolHost), policy.clone());

    let first = runtime
        .invoke_bundle_for_tenant(&bundle, &request(), "tenant-a")
        .await
        .expect("first retained Store pool invocation should succeed");
    let second = runtime
        .invoke_bundle_for_tenant(&bundle, &request(), "tenant-a")
        .await
        .expect("second retained Store pool invocation should reuse a reset Store");

    assert_eq!(first, serde_json::json!({ "ok": true }));
    assert_eq!(second, serde_json::json!({ "ok": true }));
    let metrics = policy.metrics().snapshot();
    assert_eq!(metrics.wasmtime_module_cache_misses, 1);
    assert_eq!(metrics.wasmtime_module_cache_hits, 1);
    assert_eq!(metrics.wasmtime_module_compilations, 1);
    assert_eq!(metrics.wasmtime_store_pool_misses, 1);
    assert_eq!(metrics.wasmtime_store_pool_hits, 1);
    assert_eq!(metrics.wasmtime_store_pool_authority_mismatches, 0);
    assert_eq!(metrics.wasmtime_fuel_exhaustions, 0);
    assert!(
        metrics.wasmtime_fuel_consumed_total > 0,
        "successful Wasmtime invocations should report consumed fuel"
    );
}

#[tokio::test]
async fn wasmtime_store_pool_resource_limiter_enforces_max_heap_mb() {
    let (_tempdir, bundle) = write_component_fixture(oversized_memory_component_wat());
    let mut limits = wasmtime_store_pool_test_limits();
    limits.max_heap_mb = 1;
    limits.initial_heap_mb = 1;
    let runtime = NimbusRuntime::with_policy(
        Arc::new(WasmtimeStorePoolHost),
        Arc::new(RuntimePolicy::new(limits)),
    );

    let error = runtime
        .invoke_bundle_for_tenant(&bundle, &request(), "tenant-a")
        .await
        .expect_err("ResourceLimiter should reject memory above max_heap_mb");

    match error {
        NimbusRuntimeError::HeapLimitExceeded(limit) => assert_eq!(limit, 1),
        other => panic!("unexpected ResourceLimiter error: {other}"),
    }
}

fn wasmtime_store_pool_test_limits() -> RuntimeLimits {
    let mut limits = RuntimeLimits::application_wasm_component_retained_store_pool();
    limits.worker_threads = 1;
    limits.max_concurrent_runtime_instances = 1;
    limits.max_active_top_level_invocations_per_tenant = 1;
    limits.max_in_flight_top_level_invocations_per_tenant = 1;
    limits
}

fn write_component_fixture(wat: &str) -> (tempfile::TempDir, RuntimeBundle) {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let component_path = tempdir.path().join("nimbus-function.component.wat");
    std::fs::write(&component_path, wat).expect("component fixture should be written");
    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&component_path).expect("fixture should hash");
    let bundle =
        RuntimeBundle::wasm_component_with_expected_sha256(&component_path, expected_sha256)
            .expect("WASM component bundle should record provenance hash");
    (tempdir, bundle)
}

fn request() -> InvocationRequest {
    InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "wasm:handler".to_string(),
        args: serde_json::json!({ "subject": "world" }),
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    }
}

fn nimbus_function_component_wat() -> &'static str {
    r#"
        (component
          (core module $main
            (type $realloc_t (func (param i32 i32 i32 i32) (result i32)))
            (type $handler_t (func (param i32 i32) (result i32)))
            (type $post_t (func (param i32)))
            (memory $memory 1)
            (global $heap (mut i32) (i32.const 64))
            (data (i32.const 0) "\10\00\00\00\0b\00\00\00")
            (data (i32.const 16) "{\"ok\":true}")
            (data (i32.const 48) "store-pool")
            (func $cabi_realloc (type $realloc_t)
              (param $old_ptr i32)
              (param $old_size i32)
              (param $align i32)
              (param $new_size i32)
              (result i32)
              (local $ptr i32)
              global.get $heap
              local.set $ptr
              global.get $heap
              local.get $new_size
              i32.add
              global.set $heap
              local.get $ptr
            )
            (func $handler (type $handler_t)
              (param $args_ptr i32)
              (param $args_len i32)
              (result i32)
              i32.const 0
            )
            (func $cabi_post_handler (type $post_t) (param $result_ptr i32))
            (export "memory" (memory $memory))
            (export "cabi_realloc" (func $cabi_realloc))
            (export "handler" (func $handler))
            (export "cabi_post_handler" (func $cabi_post_handler))
          )
          (core instance $main (instantiate $main))
          (alias core export $main "memory" (core memory $memory))
          (alias core export $main "cabi_realloc" (core func $cabi_realloc))
          (alias core export $main "handler" (core func $handler-core))
          (alias core export $main "cabi_post_handler" (core func $cabi_post_handler))
          (type $handler-ty (func (param "args" string) (result string)))
          (func $handler (type $handler-ty)
            (canon lift
              (core func $handler-core)
              (memory $memory)
              (realloc $cabi_realloc)
              string-encoding=utf8
              (post-return $cabi_post_handler)
            )
          )
          (export "handler" (func $handler))
        )
    "#
}

fn oversized_memory_component_wat() -> &'static str {
    nimbus_function_component_wat()
        .replace("(memory $memory 1)", "(memory $memory 32)")
        .leak()
}
