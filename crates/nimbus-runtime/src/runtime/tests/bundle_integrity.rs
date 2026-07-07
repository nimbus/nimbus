use super::*;
use crate::backends::v8::V8RuntimeConstructionMode;
use crate::limits::{
    RuntimeBundleContentKind, RuntimeExecutionModel, RuntimeMemoryEnforcement, RuntimeMode,
    RuntimeNodeFullRealmReusePolicy, RuntimePoolKind, RuntimePreset, RuntimeRoutingAffinity,
};
use crate::test_support::{RuntimeReproCase, product_default_runtime_test_policy};

pub(super) const BUNDLE_INTEGRITY_RECHECK_CASE: RuntimeReproCase = RuntimeReproCase::new(
    "runtime-bundle-integrity-recheck-after-success",
    "run-to-completion-snapshot",
    "bundle integrity is revalidated after a prior successful invocation",
);

pub(super) const PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE: IsolatedRuntimeTestCase =
    IsolatedRuntimeTestCase::new(
        "runtime-product-default-bundle-integrity-queue-health",
        "product-default",
        "product-default runtime keeps queue health after a successful invoke followed by a bundle integrity mismatch",
        "runtime::tests::bundle_integrity::runtime_product_default_bundle_integrity_recheck_after_prior_success_preserves_queue_health_subprocess",
    );

#[tokio::test]
async fn runtime_reports_heap_limit_exceeded() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  let value = "";
  while (true) {
    value += "hello world";
  }
};

export {};
"#,
    )
    .expect("bundle should write");

    let mut limits = run_to_completion_snapshot_runtime_test_limits();
    limits.max_heap_mb = 32;
    limits.initial_heap_mb = 16;
    limits.execution_timeout = std::time::Duration::from_secs(2);
    limits.max_concurrent_runtime_instances = 1;
    let runtime = NimbusRuntime::with_limits(
        Arc::new(RecordingHost::default()),
        limits,
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
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
        .expect_err("heap growth should trip the runtime heap limit");

    match error {
        NimbusRuntimeError::HeapLimitExceeded(limit) => assert_eq!(limit, 32),
        other => panic!("unexpected heap-limit error: {other}"),
    }
}

#[tokio::test]
async fn runtime_rejects_module_imports_outside_bundle_root() {
    let tempdir = tempdir().expect("tempdir should build");
    let outside_path = tempdir.path().join("outside.mjs");
    let bundle_dir = tempdir.path().join("bundle");
    std::fs::create_dir_all(&bundle_dir).expect("bundle dir should exist");
    let bundle_path = bundle_dir.join("bundle.mjs");

    std::fs::write(&outside_path, "export const secret = 'outside';")
        .expect("outside module should write");
    std::fs::write(
        &bundle_path,
        r#"
import "../outside.mjs";

globalThis.__nimbusInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
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
        .expect_err("outside import should be rejected");

    let message = error.to_string();
    assert!(
        message.contains("runtime read capability denied")
            && message.contains("outside.mjs")
            && message.contains("allowed roots"),
        "unexpected loader sandbox error: {error}"
    );
}

#[tokio::test]
async fn runtime_rejects_bundle_integrity_mismatch() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");
    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return { ok: false };
};

export {};
"#,
    )
    .expect("tampered bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let bundle = RuntimeBundle::with_expected_sha256(&bundle_path, expected_sha256)
        .expect("bundle integrity metadata should build");
    let error = runtime
        .invoke_bundle_for_tenant(
            &bundle,
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
        .expect_err("tampered bundle should fail integrity verification");

    match error {
        NimbusRuntimeError::BundleIntegrityMismatch(message) => {
            assert!(message.contains("bundle.mjs"));
        }
        other => panic!("unexpected integrity error: {other}"),
    }
}

#[test]
fn wasm_bundle_records_component_world_and_hash_identity() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("agent.component.wasm");
    std::fs::write(&bundle_path, b"wasm component bytes").expect("WASM bundle should write");
    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("WASM bundle hash should load");

    let bundle = RuntimeBundle::wasm_component_for_world_with_expected_sha256(
        &bundle_path,
        RuntimeComponentWorld::NimbusAgent,
        &expected_sha256,
    )
    .expect("WASM bundle identity should accept sha256");

    assert_eq!(
        bundle.content_kind(),
        RuntimeBundleContentKind::WasmComponent
    );
    assert_eq!(
        bundle.target_world(),
        Some(RuntimeComponentWorld::NimbusAgent)
    );
    assert_eq!(
        bundle.identity().target_world(),
        Some(RuntimeComponentWorld::NimbusAgent)
    );
    assert_eq!(
        bundle.identity().expected_sha256(),
        Some(expected_sha256.as_str())
    );
    match bundle.content() {
        RuntimeBundleContent::WasmComponent(content) => {
            assert_eq!(content.target_world(), RuntimeComponentWorld::NimbusAgent);
            assert_eq!(content.precompiled_sha256(), None);
        }
        RuntimeBundleContent::JavaScript => panic!("WASM bundle should carry component content"),
    }

    let js_bundle = RuntimeBundle::new(tempdir.path().join("bundle.mjs"));
    assert_eq!(js_bundle.target_world(), None);
    assert_eq!(js_bundle.identity().target_world(), None);
}

#[test]
fn wasm_bundle_rejects_tampered_wasm_component_bytes() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("tampered-wasm.component.wasm");
    std::fs::write(&bundle_path, b"original wasm component bytes")
        .expect("WASM bundle should write");
    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("WASM bundle hash should load");
    let bundle = RuntimeBundle::wasm_component_with_expected_sha256(&bundle_path, expected_sha256)
        .expect("WASM bundle integrity metadata should build");

    std::fs::write(&bundle_path, b"tampered WASM component bytes")
        .expect("tampered WASM bundle should write");
    let error = bundle
        .verify_integrity()
        .expect_err("tampered WASM component should fail integrity verification");

    match error {
        NimbusRuntimeError::BundleIntegrityMismatch(message) => {
            assert!(message.contains("tampered-wasm.component.wasm"));
        }
        other => panic!("unexpected tampered WASM integrity error: {other}"),
    }
}

#[test]
fn wasm_bundle_verifies_precompiled_component_hash_separately() {
    let tempdir = tempdir().expect("tempdir should build");
    let component_path = tempdir.path().join("function.component.wasm");
    let precompiled_path = tempdir.path().join("function.component.cwasm");
    std::fs::write(&component_path, b"component model bytes").expect("component should write");
    std::fs::write(&precompiled_path, b"precompiled component bytes")
        .expect("precompiled component should write");
    let component_sha256 = RuntimeBundle::compute_sha256_for_path(&component_path)
        .expect("component hash should load");
    let precompiled_sha256 = RuntimeBundle::compute_sha256_for_path(&precompiled_path)
        .expect("precompiled component hash should load");
    let bundle = RuntimeBundle::wasm_component_with_precompiled_sha256(
        &component_path,
        RuntimeComponentWorld::NimbusFunction,
        component_sha256,
        &precompiled_sha256,
    )
    .expect("WASM bundle precompile metadata should build");

    bundle
        .verify_integrity()
        .expect("component bytes should match recorded provenance");
    bundle
        .verify_precompiled_component_integrity(&precompiled_path)
        .expect("precompiled component bytes should match recorded provenance");

    std::fs::write(&precompiled_path, b"tampered precompiled WASM component")
        .expect("tampered precompiled component should write");
    let error = bundle
        .verify_precompiled_component_integrity(&precompiled_path)
        .expect_err("tampered precompiled component should fail integrity verification");
    assert!(
        matches!(error, NimbusRuntimeError::BundleIntegrityMismatch(_)),
        "unexpected precompiled WASM integrity error: {error}"
    );
}

#[tokio::test]
async fn startup_snapshot_runtime_populates_and_reuses_bundle_module_code_cache() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    let dep_path = tempdir.path().join("dep.mjs");
    std::fs::write(
        &dep_path,
        r#"
export function value() {
  return "cached";
}
"#,
    )
    .expect("dependency should write");
    std::fs::write(
        &bundle_path,
        r#"
import { value } from "./dep.mjs";

globalThis.__nimbusInvoke = async function () {
  return { value: value() };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
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

    assert_eq!(bundle.module_code_cache_entry_count(), 0);
    assert_eq!(bundle.module_code_cache_write_count(), 0);
    assert_eq!(bundle.module_code_cache_partition_count(), 0);

    let first = runtime
        .invoke_bundle_for_tenant(&bundle, &request, "tenant-a")
        .await
        .expect("first invocation should succeed");
    assert_eq!(first, serde_json::json!({ "value": "cached" }));

    let first_entry_count = bundle.module_code_cache_entry_count();
    let first_write_count = bundle.module_code_cache_write_count();
    assert_eq!(bundle.module_code_cache_partition_count(), 1);
    assert!(
        first_entry_count >= 2,
        "expected main module and dependency to populate cache"
    );
    assert!(
        first_write_count >= first_entry_count,
        "expected at least one cache write per populated module"
    );

    let second = runtime
        .invoke_bundle_for_tenant(&bundle, &request, "tenant-a")
        .await
        .expect("second invocation should succeed");
    assert_eq!(second, serde_json::json!({ "value": "cached" }));
    assert_eq!(bundle.module_code_cache_entry_count(), first_entry_count);
    assert_eq!(bundle.module_code_cache_write_count(), first_write_count);
    let metrics = runtime.policy.metrics_snapshot();
    assert_eq!(metrics.bundle_loads, 2);
    assert!(metrics.bundle_load_nanos_total > 0);
    assert_eq!(metrics.bundle_module_loads, 2);
    assert!(metrics.bundle_module_load_nanos_total > 0);
    assert_eq!(metrics.bundle_evaluations, 2);
    assert!(metrics.bundle_evaluation_nanos_total > 0);
}

#[tokio::test]
async fn startup_snapshot_module_code_cache_reloads_when_dependency_source_hash_changes() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    let dep_path = tempdir.path().join("dep.mjs");
    std::fs::write(
        &dep_path,
        r#"
export function value() {
  return "before";
}
"#,
    )
    .expect("dependency should write");
    std::fs::write(
        &bundle_path,
        r#"
import { value } from "./dep.mjs";

globalThis.__nimbusInvoke = async function () {
  return { value: value() };
};

export {};
"#,
    )
    .expect("bundle should write");

    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
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
        .invoke_bundle_for_tenant(&bundle, &request, "tenant-a")
        .await
        .expect("first invocation should succeed");
    assert_eq!(first, serde_json::json!({ "value": "before" }));
    let first_entry_count = bundle.module_code_cache_entry_count();
    let first_write_count = bundle.module_code_cache_write_count();
    assert_eq!(bundle.module_code_cache_partition_count(), 1);
    assert!(
        first_entry_count >= 2,
        "expected main module and dependency to populate cache"
    );

    std::fs::write(
        &dep_path,
        r#"
export function value() {
  return "after";
}
"#,
    )
    .expect("dependency update should write");

    let second = runtime
        .invoke_bundle_for_tenant(&bundle, &request, "tenant-a")
        .await
        .expect("second invocation should succeed after dependency update");
    assert_eq!(
        second,
        serde_json::json!({ "value": "after" }),
        "module code cache must not serve stale bytecode after a source-hash change"
    );
    assert_eq!(
        bundle.module_code_cache_partition_count(),
        1,
        "source changes should not fragment the engine cache partition"
    );
    assert_eq!(
        bundle.module_code_cache_entry_count(),
        first_entry_count,
        "the changed dependency should replace its cache entry instead of adding an authority partition"
    );
    assert!(
        bundle.module_code_cache_write_count() > first_write_count,
        "changed dependency source should compile and store fresh cached data"
    );
}

#[test]
fn runtime_bundle_module_code_cache_is_partitioned_by_engine_config() {
    let bundle = RuntimeBundle::new("unused.mjs");
    let startup_snapshot = V8RuntimeConstructionMode::StartupSnapshot;
    let unsnapshotted = V8RuntimeConstructionMode::Unsnapshotted;
    let web_limits = crate::RuntimeLimits::application_web_standard();
    let node_limits = crate::RuntimeLimits::application_node22();
    let node24_limits = crate::RuntimeLimits::application_node24();
    let mut node_custom_condition_limits = node_limits.clone();
    node_custom_condition_limits
        .node_conditions
        .push("custom".to_string());
    let mut node_service_limits = node_limits.clone();
    node_service_limits.service_capability_enabled = true;
    node_service_limits.grants.service = vec!["db".to_string()];
    let mut node_read_limits = node_limits.clone();
    node_read_limits.grants.read = vec!["/app".to_string()];
    let mut node_env_limits = node_limits.clone();
    node_env_limits.grants.env_read = vec!["NIMBUS_CACHE_TEST".to_string()];
    let mut node_run_limits = node_limits.clone();
    node_run_limits.grants.run = vec!["nimbus-tool".to_string()];
    let mut node_mode_limits = node_limits.clone();
    node_mode_limits.mode = RuntimeMode::Privileged;
    let mut node_preset_limits = node_limits.clone();
    node_preset_limits.preset = RuntimePreset::Tooling;
    let mut node_pool_limits = node_limits.clone();
    node_pool_limits.execution_model = RuntimeExecutionModel::CooperativeLocker;
    node_pool_limits.runtime_pool_kind = RuntimePoolKind::WarmContextRecycle;
    node_pool_limits.node_full_realm_reuse_policy =
        RuntimeNodeFullRealmReusePolicy::SameOwnerExactAuthority;
    let mut node_memory_limits = node_limits.clone();
    node_memory_limits.memory_enforcement = RuntimeMemoryEnforcement::OuterQuotaRequired;
    node_memory_limits.max_heap_mb += 1;
    let mut node_routing_limits = node_limits.clone();
    node_routing_limits.routing_affinity = RuntimeRoutingAffinity::Function;
    let mut node_timeout_limits = node_limits.clone();
    node_timeout_limits.execution_timeout += std::time::Duration::from_secs(1);

    let web_cache = bundle.module_code_cache(&web_limits, startup_snapshot);
    let second_web_cache = bundle.module_code_cache(&web_limits, startup_snapshot);
    let node_cache = bundle.module_code_cache(&node_limits, startup_snapshot);
    let node_unsnapshotted_cache = bundle.module_code_cache(&node_limits, unsnapshotted);
    let node24_cache = bundle.module_code_cache(&node24_limits, startup_snapshot);
    let node_custom_condition_cache =
        bundle.module_code_cache(&node_custom_condition_limits, startup_snapshot);
    let node_service_cache = bundle.module_code_cache(&node_service_limits, startup_snapshot);
    let node_read_cache = bundle.module_code_cache(&node_read_limits, startup_snapshot);
    let node_env_cache = bundle.module_code_cache(&node_env_limits, startup_snapshot);
    let node_run_cache = bundle.module_code_cache(&node_run_limits, startup_snapshot);
    let node_mode_cache = bundle.module_code_cache(&node_mode_limits, startup_snapshot);
    let node_preset_cache = bundle.module_code_cache(&node_preset_limits, startup_snapshot);
    let node_pool_cache = bundle.module_code_cache(&node_pool_limits, startup_snapshot);
    let node_memory_cache = bundle.module_code_cache(&node_memory_limits, startup_snapshot);
    let node_routing_cache = bundle.module_code_cache(&node_routing_limits, startup_snapshot);
    let node_timeout_cache = bundle.module_code_cache(&node_timeout_limits, startup_snapshot);

    assert!(Arc::ptr_eq(&web_cache, &second_web_cache));
    let cache_partitions = [
        ("web", &web_cache),
        ("node22", &node_cache),
        ("node22-unsnapshotted", &node_unsnapshotted_cache),
        ("node24", &node24_cache),
        ("node22-custom-condition", &node_custom_condition_cache),
        ("node22-service-grant", &node_service_cache),
        ("node22-read-grant", &node_read_cache),
        ("node22-env-grant", &node_env_cache),
        ("node22-run-grant", &node_run_cache),
        ("node22-mode", &node_mode_cache),
        ("node22-preset", &node_preset_cache),
        ("node22-pool-policy", &node_pool_cache),
        ("node22-memory-policy", &node_memory_cache),
        ("node22-routing-policy", &node_routing_cache),
        ("node22-timeout-policy", &node_timeout_cache),
    ];
    for (index, (left_name, left_cache)) in cache_partitions.iter().enumerate() {
        for (right_name, right_cache) in cache_partitions.iter().skip(index + 1) {
            assert!(
                !Arc::ptr_eq(left_cache, right_cache),
                "module code cache partitions must not cross authority dimension {left_name} -> {right_name}"
            );
        }
    }
    assert_eq!(
        bundle.module_code_cache_partition_count(),
        cache_partitions.len()
    );
}

#[test]
fn runtime_bundle_clones_share_normalized_identity() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    let bundle = RuntimeBundle::with_expected_sha256(
        bundle_path
            .parent()
            .expect("bundle parent should exist")
            .join(".")
            .join("bundle.mjs"),
        expected_sha256.to_ascii_uppercase(),
    )
    .expect("bundle identity metadata should build");
    let cloned = bundle.clone();
    let canonical_bundle_path = bundle_path
        .canonicalize()
        .expect("bundle path should canonicalize");

    assert!(bundle.shares_storage_with(&cloned));
    assert_eq!(bundle.identity(), cloned.identity());
    assert_eq!(bundle.identity().entrypoint(), canonical_bundle_path);
    assert_eq!(
        bundle.identity().expected_sha256(),
        Some(expected_sha256.as_str())
    );
    assert_eq!(
        bundle.canonical_entrypoint(),
        Some(canonical_bundle_path.as_path())
    );
    assert_eq!(
        bundle
            .module_root()
            .expect("bundle root should resolve from cached metadata"),
        canonical_bundle_path
            .parent()
            .expect("bundle root should exist")
            .to_path_buf()
    );
    assert_eq!(
        bundle
            .module_specifier()
            .expect("bundle specifier should resolve from cached metadata")
            .as_str(),
        deno_core::ModuleSpecifier::from_file_path(&canonical_bundle_path)
            .expect("canonical bundle path should convert to a file url")
            .as_str()
    );
    assert_eq!(
        cloned
            .module_root()
            .expect("cloned bundle should share cached root metadata"),
        canonical_bundle_path
            .parent()
            .expect("bundle root should exist")
            .to_path_buf()
    );
}

#[tokio::test]
async fn runtime_bundle_rechecks_integrity_after_prior_success() {
    runtime_bundle_rechecks_integrity_after_prior_success_inner().await;
}

pub(super) async fn runtime_bundle_rechecks_integrity_after_prior_success_inner() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    let bundle = RuntimeBundle::with_expected_sha256(&bundle_path, expected_sha256)
        .expect("bundle integrity metadata should build");
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_result = runtime
        .invoke_bundle_for_tenant(&bundle, &request, "tenant-a")
        .await
        .unwrap_or_else(|error| {
            panic!(
                "{}: {error}",
                BUNDLE_INTEGRITY_RECHECK_CASE
                    .failure_context("first bundle invocation should succeed")
            )
        });
    assert_eq!(
        first_result,
        serde_json::json!({ "ok": true }),
        "{}",
        BUNDLE_INTEGRITY_RECHECK_CASE
            .failure_context("first bundle invocation should return the original bundle result")
    );

    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return { ok: false };
};

export {};
"#,
    )
    .expect("tampered bundle should write");

    let error = runtime
        .invoke_bundle_for_tenant(&bundle, &request, "tenant-a")
        .await
        .expect_err("tampered bundle should fail integrity verification");
    assert!(
        matches!(error, NimbusRuntimeError::BundleIntegrityMismatch(_)),
        "{}; received {error}",
        BUNDLE_INTEGRITY_RECHECK_CASE.failure_context(
            "tampered bundle should fail integrity verification with a bundle-integrity mismatch"
        )
    );
}

#[test]
fn runtime_product_default_bundle_integrity_recheck_after_prior_success_preserves_queue_health() {
    run_v8_sensitive_runtime_test_in_subprocess(PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE);
}

#[test]
#[ignore = "runs in a subprocess to isolate product-default cooperative V8 state"]
fn runtime_product_default_bundle_integrity_recheck_after_prior_success_preserves_queue_health_subprocess()
 {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime should build")
        .block_on(
            runtime_product_default_bundle_integrity_recheck_after_prior_success_preserves_queue_health_inner(),
        );
}

async fn runtime_product_default_bundle_integrity_recheck_after_prior_success_preserves_queue_health_inner()
 {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    let original_source = r#"
globalThis.__nimbusInvoke = function () {
  return { ok: true };
};

export {};
"#;
    let tampered_source = r#"
globalThis.__nimbusInvoke = function () {
  return { ok: false };
};

export {};
"#;
    std::fs::write(&bundle_path, original_source).expect("bundle should write");

    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    let bundle = RuntimeBundle::with_expected_sha256(&bundle_path, expected_sha256)
        .expect("bundle integrity metadata should build");
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        product_default_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    let first_result = runtime
        .invoke_bundle_for_tenant(&bundle, &request, "tenant-a")
        .await
        .unwrap_or_else(|error| {
            panic!(
                "{}: {error}",
                PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE.failure_context(
                    "initial product-default invocation should succeed before integrity mismatch"
                )
            )
        });
    assert_eq!(
        first_result,
        serde_json::json!({ "ok": true }),
        "{}",
        PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE.failure_context(
            "initial product-default invocation should return the original bundle result"
        )
    );

    std::fs::write(&bundle_path, tampered_source).expect("tampered bundle should write");

    let error = runtime
        .invoke_bundle_for_tenant(&bundle, &request, "tenant-a")
        .await
        .expect_err("tampered bundle should fail integrity verification");
    assert!(
        matches!(error, NimbusRuntimeError::BundleIntegrityMismatch(_)),
        "{}; received {error}",
        PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE.failure_context(
            "product-default runtime should report a bundle-integrity mismatch after prior success"
        )
    );

    std::fs::write(&bundle_path, original_source).expect("restored bundle should write");

    let recovered = runtime
        .invoke_bundle_for_tenant(&bundle, &request, "tenant-a")
        .await
        .unwrap_or_else(|error| {
            panic!(
                "{}: {error}",
                PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE.failure_context(
                    "runtime should still serve new work after integrity mismatch without queue-accounting corruption"
                )
            )
        });
    assert_eq!(
        recovered,
        serde_json::json!({ "ok": true }),
        "{}",
        PRODUCT_DEFAULT_BUNDLE_QUEUE_HEALTH_CASE.failure_context(
            "restored bundle should succeed again after the mismatch without poisoning the runtime queue"
        )
    );
}

#[tokio::test]
async fn runtime_bundle_identity_canonicalizes_paths_without_changing_integrity_results() {
    let tempdir = tempdir().expect("tempdir should build");
    let bundle_path = tempdir.path().join("bundle.mjs");
    std::fs::write(
        &bundle_path,
        r#"
globalThis.__nimbusInvoke = function () {
  return { ok: true };
};

export {};
"#,
    )
    .expect("bundle should write");

    let expected_sha256 =
        RuntimeBundle::compute_sha256_for_path(&bundle_path).expect("bundle hash should load");
    let canonical_bundle = RuntimeBundle::with_expected_sha256(&bundle_path, &expected_sha256)
        .expect("canonical bundle should build");
    let dot_path_bundle = RuntimeBundle::with_expected_sha256(
        bundle_path
            .parent()
            .expect("bundle parent should exist")
            .join(".")
            .join("bundle.mjs"),
        format!("{expected_sha256}\n"),
    )
    .expect("dot path bundle should build");
    let runtime = NimbusRuntime::with_policy(
        Arc::new(RecordingHost::default()),
        run_to_completion_snapshot_runtime_test_policy(),
        crate::RuntimeEgressPosture::CoarsePermissions,
    );
    let request = InvocationRequest {
        kind: InvocationKind::Query,
        function_name: "messages:list".to_string(),
        args: Value::Null,
        page_size: None,
        cursor: None,
        auth: None,
        services: Default::default(),
    };

    assert_eq!(canonical_bundle.identity(), dot_path_bundle.identity());

    let canonical_result = runtime
        .invoke_bundle_for_tenant(&canonical_bundle, &request, "tenant-a")
        .await
        .expect("canonical bundle invocation should succeed");
    let dot_path_result = runtime
        .invoke_bundle_for_tenant(&dot_path_bundle, &request, "tenant-a")
        .await
        .expect("dot path bundle invocation should succeed");

    assert_eq!(canonical_result, serde_json::json!({ "ok": true }));
    assert_eq!(dot_path_result, canonical_result);
}
