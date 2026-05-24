use super::*;
use nimbus_runtime::{
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeBundle, RuntimeBundleContentKind, RuntimeCompatibilityTarget,
    RuntimeExecutionModel, RuntimeJavaScriptEvaluationFormat, RuntimeLimits,
    RuntimeMemoryEnforcement, RuntimePoolKind,
};

#[test]
fn convex_registry_requires_runtime_bundle_hash_sidecar() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": [] }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");
    fs::write(
        convex_dir.join("bundle.mjs"),
        "globalThis.__nimbusInvoke = async function () { return { status: \"ok\", value: null }; }; export {};",
    )
    .expect("convex runtime bundle should write");

    let error = ConvexRegistry::from_app_dir(tempdir.path())
        .expect_err("bundle without sidecar hash should be rejected");
    assert!(
        error.to_string().contains("bundle.sha256"),
        "unexpected registry error: {error}"
    );
}

#[test]
fn convex_registry_from_app_dir_uses_product_runtime_defaults() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": [] }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");

    let registry = ConvexRegistry::from_app_dir(tempdir.path())
        .expect("convex registry should load using product defaults");
    assert_eq!(registry.runtime_limits(), RuntimeLimits::default());
}

#[test]
fn convex_registry_selects_node_runtime_lane_from_manifest_metadata() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:list",
                    "kind": "query",
                    "visibility": "public",
                    "runtime_environment": "default",
                    "runtime_engine": "v8",
                    "runtime_bundle_content_kind": "javascript",
                    "runtime_javascript_evaluation_format": "es_module",
                    "runtime_compatibility_target": "web_standard_isolate",
                    "runtime_package_resolution": "bundled",
                    "runtime_handler": null,
                    "plan": null
                },
                {
                    "name": "messages:readFile",
                    "kind": "action",
                    "visibility": "public",
                    "runtime_environment": "node",
                    "runtime_engine": "v8",
                    "runtime_bundle_content_kind": "javascript",
                    "runtime_javascript_evaluation_format": "es_module",
                    "runtime_compatibility_target": "node24",
                    "runtime_package_resolution": "node_external_packages",
                    "node_runtime_target": "node24",
                    "runtime_handler": "() => null",
                    "plan": null
                }
            ]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");

    let registry = ConvexRegistry::from_app_dir(tempdir.path())
        .expect("convex registry should load node runtime metadata");
    let default_limits = registry.runtime_limits_for_function("messages:list");
    assert_eq!(default_limits.backend_kind, RuntimeBackendKind::V8);
    assert_eq!(
        default_limits.bundle_content_kind,
        RuntimeBundleContentKind::JavaScript
    );
    assert_eq!(
        default_limits.javascript_evaluation_format,
        RuntimeJavaScriptEvaluationFormat::EsModule
    );
    assert_eq!(
        default_limits.compatibility_target,
        RuntimeCompatibilityTarget::WebStandardIsolate
    );
    let node_limits = registry.runtime_limits_for_function("messages:readFile");
    assert_eq!(node_limits.backend_kind, RuntimeBackendKind::V8);
    assert_eq!(
        node_limits.bundle_content_kind,
        RuntimeBundleContentKind::JavaScript
    );
    assert_eq!(
        node_limits.javascript_evaluation_format,
        RuntimeJavaScriptEvaluationFormat::EsModule
    );
    assert_eq!(
        node_limits.compatibility_target,
        RuntimeCompatibilityTarget::Node24
    );
}

#[test]
fn convex_registry_rejects_unsupported_runtime_content_before_invocation() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:list",
                    "kind": "query",
                    "visibility": "public",
                    "runtime_environment": "default",
                    "runtime_engine": "v8",
                    "runtime_bundle_content_kind": "wasm_component",
                    "runtime_compatibility_target": "web_standard_isolate",
                    "runtime_package_resolution": "bundled",
                    "runtime_handler": "() => null",
                    "plan": null
                }
            ]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");

    let error = ConvexRegistry::from_app_dir(tempdir.path())
        .expect_err("unsupported runtime bundle content should fail manifest loading");
    assert!(
        error.to_string().contains("V8 supports only JavaScript"),
        "unexpected registry error: {error}"
    );
}

#[test]
fn convex_registry_rejects_v8_program_wrapper_before_invocation() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:list",
                    "kind": "query",
                    "visibility": "public",
                    "runtime_environment": "default",
                    "runtime_engine": "v8",
                    "runtime_bundle_content_kind": "javascript",
                    "runtime_javascript_evaluation_format": "program_wrapper",
                    "runtime_compatibility_target": "web_standard_isolate",
                    "runtime_package_resolution": "bundled",
                    "runtime_handler": "() => null",
                    "plan": null
                }
            ]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route manifest should serialize"),
    )
    .expect("convex http route manifest should write");

    let error = ConvexRegistry::from_app_dir(tempdir.path())
        .expect_err("V8 program-wrapper metadata should fail manifest loading");
    assert!(
        error
            .to_string()
            .contains("V8 supports only ES module evaluation"),
        "unexpected registry error: {error}"
    );
}

#[test]
fn convex_registry_selects_bun_jsc_lane_from_manifest_metadata() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:bunProof",
                    "kind": "action",
                    "visibility": "public",
                    "runtime_environment": "bun",
                    "runtime_engine": "bun_jsc",
                    "runtime_bundle_content_kind": "javascript",
                    "runtime_javascript_evaluation_format": "program_wrapper",
                    "runtime_compatibility_target": "bun_jsc",
                    "runtime_package_resolution": "bun_self_contained",
                    "runtime_handler": "() => null",
                    "plan": null
                }
            ]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route manifest should serialize"),
    )
    .expect("convex http route manifest should write");

    let registry = ConvexRegistry::from_app_dir(tempdir.path())
        .expect("convex registry should load Bun/JSC runtime metadata");
    let bun_limits = registry.runtime_limits_for_function("messages:bunProof");
    assert_eq!(bun_limits.backend_kind, RuntimeBackendKind::BunJsc);
    assert_eq!(
        bun_limits.backend_trust_tier,
        RuntimeBackendTrustTier::InProcessUntrusted
    );
    assert_eq!(
        bun_limits.backend_lockdown_profile,
        RuntimeBackendLockdownProfile::BunJscInProcessUntrusted
    );
    assert_eq!(
        bun_limits.backend_lifecycle_policy,
        RuntimeBackendLifecyclePolicy::BunJscFreshDiscardPoolOuterQuotaRequired
    );
    assert_eq!(
        bun_limits.runtime_pool_kind,
        RuntimePoolKind::BunJscFreshDiscard
    );
    assert_eq!(
        bun_limits.memory_enforcement,
        RuntimeMemoryEnforcement::OuterQuotaRequired
    );
    assert_eq!(
        bun_limits.bundle_content_kind,
        RuntimeBundleContentKind::JavaScript
    );
    assert_eq!(
        bun_limits.javascript_evaluation_format,
        RuntimeJavaScriptEvaluationFormat::ProgramWrapper
    );
    assert_eq!(
        bun_limits.compatibility_target,
        RuntimeCompatibilityTarget::BunJsc
    );
    let diagnostics = registry.runtime_lane_diagnostics();
    let bun_diagnostics = diagnostics
        .iter()
        .find(|lane| lane.lane_name == "bun_jsc")
        .expect("Bun/JSC lane diagnostics should be present");
    assert!(!bun_diagnostics.executor_started);
    assert_eq!(
        bun_diagnostics.execution_adapter_state,
        nimbus_runtime::RuntimeExecutionAdapterState::NotLinked
    );
    assert_eq!(
        bun_diagnostics.limits.memory_enforcement,
        RuntimeMemoryEnforcement::OuterQuotaRequired
    );
}

#[test]
fn convex_registry_loads_bun_jsc_program_bundle_from_artifact_metadata() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:bunProof",
                    "kind": "mutation",
                    "visibility": "public",
                    "runtime_environment": "bun",
                    "runtime_engine": "bun_jsc",
                    "runtime_bundle_content_kind": "javascript",
                    "runtime_javascript_evaluation_format": "program_wrapper",
                    "runtime_compatibility_target": "bun_jsc",
                    "runtime_package_resolution": "bun_self_contained",
                    "runtime_handler": "async () => ({ ok: true })",
                    "plan": null
                }
            ]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route manifest should serialize"),
    )
    .expect("convex http route manifest should write");
    let default_bundle_path = convex_dir.join("bundle.mjs");
    fs::write(
        &default_bundle_path,
        "globalThis.__nimbusInvoke = async function () { return { status: \"ok\", value: null }; }; export {};",
    )
    .expect("default runtime bundle should write");
    let default_hash = RuntimeBundle::compute_sha256_for_path(&default_bundle_path)
        .expect("default bundle hash should compute");
    fs::write(default_bundle_path.with_extension("sha256"), default_hash)
        .expect("default runtime bundle hash should write");
    let bun_bundle_path = convex_dir.join("bun_program_bundle.js");
    fs::write(
        &bun_bundle_path,
        "globalThis.__nimbusInvoke = async function () { return { status: \"ok\", value: \"bun\" }; };",
    )
    .expect("Bun/JSC runtime program bundle should write");
    let bun_hash = RuntimeBundle::compute_sha256_for_path(&bun_bundle_path)
        .expect("Bun/JSC bundle hash should compute");
    fs::write(bun_bundle_path.with_extension("sha256"), bun_hash)
        .expect("Bun/JSC runtime program bundle hash should write");

    let registry = ConvexRegistry::from_app_dir(tempdir.path())
        .expect("convex registry should load Bun/JSC runtime metadata and program bundle");

    assert!(registry.has_runtime_bundle_for_function("messages:bunProof"));
    let loaded_bundle = registry
        .required_runtime_bundle_for_function("messages:bunProof")
        .expect("Bun/JSC runtime function should use the Bun program bundle");
    assert_eq!(loaded_bundle.entrypoint(), bun_bundle_path.as_path());
}

#[test]
fn convex_registry_bun_jsc_lane_diagnostics_reflect_runtime_adapter_state() {
    let registry = ConvexRegistry::empty();
    let diagnostics = registry.runtime_lane_diagnostics();
    let bun = diagnostics
        .iter()
        .find(|lane| lane.lane_name == "bun_jsc")
        .expect("Bun/JSC lane diagnostics should be present");
    assert_eq!(
        bun.execution_adapter_state,
        nimbus_runtime::bun_jsc_execution_adapter_state()
    );
}

#[test]
fn convex_registry_runtime_limit_overrides_do_not_leak_backend_axes_across_lanes() {
    let mut bun_shaped_override = RuntimeLimits::application_bun_jsc();
    bun_shaped_override.max_heap_mb = 96;
    bun_shaped_override.initial_heap_mb = 4;
    bun_shaped_override.execution_timeout = std::time::Duration::from_secs(7);
    bun_shaped_override.max_concurrent_runtime_instances = 2;
    bun_shaped_override.worker_threads = 3;

    let registry = ConvexRegistry::empty().with_runtime_limits(bun_shaped_override);

    let default_limits = registry.runtime_limits();
    assert_eq!(default_limits.backend_kind, RuntimeBackendKind::V8);
    assert_eq!(
        default_limits.compatibility_target,
        RuntimeCompatibilityTarget::WebStandardIsolate
    );
    assert_eq!(
        default_limits.memory_enforcement,
        RuntimeMemoryEnforcement::V8IsolateHeapLimit
    );
    assert_eq!(default_limits.max_heap_mb, 96);
    assert_eq!(
        default_limits.execution_timeout,
        std::time::Duration::from_secs(7)
    );

    let diagnostics = registry.runtime_lane_diagnostics();
    let node22 = diagnostics
        .iter()
        .find(|lane| lane.lane_name == "node22")
        .expect("Node 22 lane diagnostics should be present");
    assert_eq!(node22.limits.backend_kind, RuntimeBackendKind::V8);
    assert_eq!(
        node22.limits.compatibility_target,
        RuntimeCompatibilityTarget::Node22
    );
    assert_eq!(
        node22.limits.memory_enforcement,
        RuntimeMemoryEnforcement::V8IsolateHeapLimit
    );
    assert_eq!(node22.limits.max_heap_mb, 96);

    let bun = diagnostics
        .iter()
        .find(|lane| lane.lane_name == "bun_jsc")
        .expect("Bun/JSC lane diagnostics should be present");
    assert_eq!(bun.limits.backend_kind, RuntimeBackendKind::BunJsc);
    assert_eq!(
        bun.limits.memory_enforcement,
        RuntimeMemoryEnforcement::OuterQuotaRequired
    );
    assert_eq!(bun.limits.max_heap_mb, 96);
    assert!(!bun.executor_started);
}

#[test]
fn convex_registry_preserves_default_runtime_execution_model_override() {
    let registry = ConvexRegistry::empty()
        .with_runtime_limits(run_to_completion_snapshot_runtime_test_limits());

    let default_limits = registry.runtime_limits();
    assert_eq!(
        default_limits.execution_model,
        RuntimeExecutionModel::RunToCompletion
    );
    assert_eq!(
        default_limits.runtime_pool_kind,
        RuntimePoolKind::StartupSnapshotCache
    );

    let node22 = registry
        .runtime_lane_diagnostics()
        .into_iter()
        .find(|lane| lane.lane_name == "node22")
        .expect("Node 22 lane diagnostics should be present");
    assert_eq!(node22.limits.backend_kind, RuntimeBackendKind::V8);
    assert_eq!(
        node22.limits.compatibility_target,
        RuntimeCompatibilityTarget::Node22
    );
}

#[test]
fn convex_registry_rejects_bun_jsc_node_package_resolution_before_invocation() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:bunProof",
                    "kind": "action",
                    "visibility": "public",
                    "runtime_environment": "bun",
                    "runtime_engine": "bun_jsc",
                    "runtime_bundle_content_kind": "javascript",
                    "runtime_javascript_evaluation_format": "program_wrapper",
                    "runtime_compatibility_target": "bun_jsc",
                    "runtime_package_resolution": "node_external_packages",
                    "runtime_handler": "() => null",
                    "plan": null
                }
            ]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route manifest should serialize"),
    )
    .expect("convex http route manifest should write");

    let error = ConvexRegistry::from_app_dir(tempdir.path())
        .expect_err("Bun/JSC node package metadata should fail manifest loading");
    assert!(
        error.to_string().contains("bun_self_contained"),
        "unexpected registry error: {error}"
    );
}

#[test]
fn convex_registry_rejects_target_environment_mismatch_before_invocation() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:list",
                    "kind": "query",
                    "visibility": "public",
                    "runtime_environment": "default",
                    "runtime_engine": "v8",
                    "runtime_bundle_content_kind": "javascript",
                    "runtime_compatibility_target": "node24",
                    "runtime_package_resolution": "bundled",
                    "runtime_handler": "() => null",
                    "plan": null
                }
            ]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");

    let error = ConvexRegistry::from_app_dir(tempdir.path())
        .expect_err("runtime target mismatch should fail manifest loading");
    assert!(
        error
            .to_string()
            .contains("default runtime functions must use WebStandardIsolate"),
        "unexpected registry error: {error}"
    );
}

#[test]
fn convex_registry_rejects_conflicting_runtime_target_metadata_before_invocation() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({
            "functions": [
                {
                    "name": "messages:readFile",
                    "kind": "action",
                    "visibility": "public",
                    "runtime_environment": "node",
                    "runtime_engine": "v8",
                    "runtime_bundle_content_kind": "javascript",
                    "runtime_compatibility_target": "node22",
                    "runtime_package_resolution": "node_external_packages",
                    "node_runtime_target": "node24",
                    "runtime_handler": "() => null",
                    "plan": null
                }
            ]
        }))
        .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");

    let error = ConvexRegistry::from_app_dir(tempdir.path())
        .expect_err("conflicting runtime target metadata should fail manifest loading");
    assert!(
        error
            .to_string()
            .contains("conflicting runtime_compatibility_target"),
        "unexpected registry error: {error}"
    );
}

#[test]
fn convex_registry_validates_node_external_package_evidence_manifest() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    let staged_package_dir = convex_dir.join("node_modules").join("pkg");
    fs::create_dir_all(&staged_package_dir).expect("staged package directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": [] }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("http_routes.json"),
        serde_json::to_vec_pretty(&json!({ "routes": [] }))
            .expect("convex http route json should serialize"),
    )
    .expect("convex http route manifest should write");
    fs::write(
        convex_dir.join("node_external_packages.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "mode": "explicit",
            "configuredExternalPackages": ["pkg"],
            "stagingRoot": ".nimbus/convex/node_modules",
            "packages": [
                {
                    "packageName": "pkg",
                    "packageRoot": "node_modules/pkg",
                    "stagedPackageRoot": ".nimbus/convex/node_modules/pkg",
                    "sizeBytes": 42,
                    "resolvedSpecifiers": ["pkg"],
                    "importers": [
                        {
                            "file": "messages.ts",
                            "kind": "import",
                            "specifier": "pkg"
                        }
                    ]
                }
            ]
        }))
        .expect("node external package manifest should serialize"),
    )
    .expect("node external package manifest should write");

    ConvexRegistry::from_app_dir(tempdir.path())
        .expect("convex registry should accept valid node external package evidence");
}

#[test]
fn convex_registry_rejects_node_external_package_path_traversal() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".nimbus").join("convex");
    fs::create_dir_all(&convex_dir).expect("convex manifest directory should build");
    fs::write(
        convex_dir.join("functions.json"),
        serde_json::to_vec_pretty(&json!({ "functions": [] }))
            .expect("convex manifest json should serialize"),
    )
    .expect("convex manifest should write");
    fs::write(
        convex_dir.join("node_external_packages.json"),
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "mode": "explicit",
            "configuredExternalPackages": ["pkg"],
            "stagingRoot": ".nimbus/convex/node_modules",
            "packages": [
                {
                    "packageName": "pkg",
                    "packageRoot": "../node_modules/pkg",
                    "stagedPackageRoot": ".nimbus/convex/node_modules/pkg",
                    "sizeBytes": 42,
                    "resolvedSpecifiers": ["pkg"],
                    "importers": [
                        {
                            "file": "messages.ts",
                            "kind": "import",
                            "specifier": "pkg"
                        }
                    ]
                }
            ]
        }))
        .expect("node external package manifest should serialize"),
    )
    .expect("node external package manifest should write");

    let error = ConvexRegistry::from_app_dir(tempdir.path())
        .expect_err("convex registry should reject package manifest path traversal");
    assert!(
        error
            .to_string()
            .contains("must be a non-empty relative path without parent traversal"),
        "unexpected registry error: {error}"
    );
}
