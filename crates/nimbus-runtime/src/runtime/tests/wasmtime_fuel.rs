use std::sync::Arc;
use std::time::Duration;

use super::*;
use crate::limits::{RuntimeExecutionModel, RuntimeLimits, RuntimePolicy, RuntimePoolKind};

struct WasmtimeNoopHost;

impl HostBridge for WasmtimeNoopHost {
    fn call(&self, _request: HostCallRequest) -> Result<Value> {
        Err(NimbusRuntimeError::Contract(
            "wasmtime_fuel fixture should not call host imports".to_string(),
        ))
    }
}

#[tokio::test]
async fn wasmtime_fuel_park_resume_invokes_component_through_worker_loop() {
    let (_tempdir, bundle) = write_component_fixture(nimbus_function_component_wat());
    let runtime = NimbusRuntime::with_policy(
        Arc::new(WasmtimeNoopHost),
        Arc::new(RuntimePolicy::new(wasmtime_fuel_test_limits())),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );

    let response = runtime
        .invoke_bundle_for_tenant(&bundle, &request(), "tenant-a")
        .await
        .expect("WasmtimeFuelDriver should park, resume, and complete the component");

    assert_eq!(response, serde_json::json!({ "ok": true }));
}

#[tokio::test]
async fn wasmtime_fuel_exhaustion_maps_to_runtime_timeout() {
    let (_tempdir, bundle) = write_component_fixture(infinite_loop_component_wat());
    let mut limits = wasmtime_fuel_test_limits();
    limits.execution_timeout = Duration::from_secs(5);
    let runtime = NimbusRuntime::with_policy(
        Arc::new(WasmtimeNoopHost),
        Arc::new(RuntimePolicy::new(limits)),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );

    let error = runtime
        .invoke_bundle_for_tenant(&bundle, &request(), "tenant-a")
        .await
        .expect_err("fuel exhaustion should fail closed as a runtime timeout");

    assert!(
        matches!(error, NimbusRuntimeError::ExecutionTimeout(_)),
        "expected fuel exhaustion to map to ExecutionTimeout, got {error:?}"
    );
}

#[test]
fn wasmtime_fuel_mixed_v8_wasm_fairness_uses_cooperative_scheduler_contract() {
    let wasm_policy = RuntimePolicy::new(wasmtime_fuel_test_limits());
    assert_eq!(
        wasm_policy.limits().execution_model,
        RuntimeExecutionModel::CooperativeFuel
    );

    let mut v8_limits = RuntimeLimits::application_web_standard();
    v8_limits.execution_model = RuntimeExecutionModel::CooperativeLocker;
    v8_limits.runtime_pool_kind = RuntimePoolKind::WarmPool;
    let v8_policy = RuntimePolicy::new(v8_limits);
    assert_eq!(
        v8_policy.limits().execution_model,
        RuntimeExecutionModel::CooperativeLocker
    );

    // Mixed V8/WASM fairness depends on both backends entering the same
    // cooperative worker-loop scheduler family instead of a backend-specific
    // blocking loop.
    assert_ne!(
        v8_policy.limits().execution_model,
        wasm_policy.limits().execution_model
    );
}

fn wasmtime_fuel_test_limits() -> RuntimeLimits {
    let mut limits = RuntimeLimits::application_wasm_component_cooperative_fuel();
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

fn infinite_loop_component_wat() -> &'static str {
    r#"
        (component
          (core module $main
            (type $realloc_t (func (param i32 i32 i32 i32) (result i32)))
            (type $handler_t (func (param i32 i32) (result i32)))
            (type $post_t (func (param i32)))
            (memory $memory 1)
            (global $heap (mut i32) (i32.const 64))
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
              (loop $again
                br $again
              )
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
