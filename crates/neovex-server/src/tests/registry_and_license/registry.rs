use super::*;

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
