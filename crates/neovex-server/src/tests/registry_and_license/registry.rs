use super::*;
use neovex_runtime::{RuntimeCompatibilityTarget, RuntimeLimits};

#[test]
fn convex_registry_requires_runtime_bundle_hash_sidecar() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".neovex").join("convex");
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
        "globalThis.__neovexInvoke = async function () { return { status: \"ok\", value: null }; }; export {};",
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
    let convex_dir = tempdir.path().join(".neovex").join("convex");
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
    let convex_dir = tempdir.path().join(".neovex").join("convex");
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
                    "runtime_handler": null,
                    "plan": null
                },
                {
                    "name": "messages:readFile",
                    "kind": "action",
                    "visibility": "public",
                    "runtime_environment": "node",
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
    assert_eq!(
        registry
            .runtime_limits_for_function("messages:list")
            .compatibility_target,
        RuntimeCompatibilityTarget::WebStandardIsolate
    );
    assert_eq!(
        registry
            .runtime_limits_for_function("messages:readFile")
            .compatibility_target,
        RuntimeCompatibilityTarget::Node24
    );
}

#[test]
fn convex_registry_validates_node_external_package_evidence_manifest() {
    let tempdir = tempdir().expect("convex manifest tempdir should build");
    let convex_dir = tempdir.path().join(".neovex").join("convex");
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
            "stagingRoot": ".neovex/convex/node_modules",
            "packages": [
                {
                    "packageName": "pkg",
                    "packageRoot": "node_modules/pkg",
                    "stagedPackageRoot": ".neovex/convex/node_modules/pkg",
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
    let convex_dir = tempdir.path().join(".neovex").join("convex");
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
            "stagingRoot": ".neovex/convex/node_modules",
            "packages": [
                {
                    "packageName": "pkg",
                    "packageRoot": "../node_modules/pkg",
                    "stagedPackageRoot": ".neovex/convex/node_modules/pkg",
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
