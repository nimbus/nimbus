use std::path::Path;

fn read_package_version(package_json_path: &Path) -> String {
    let content = std::fs::read_to_string(package_json_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", package_json_path.display()));
    let parsed: serde_json::Value = serde_json::from_str(&content)
        .unwrap_or_else(|e| panic!("failed to parse {}: {e}", package_json_path.display()));
    parsed["version"]
        .as_str()
        .unwrap_or_else(|| panic!("no \"version\" field in {}", package_json_path.display()))
        .to_string()
}

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let packages_dir = Path::new(&manifest_dir).join("../../packages");

    let convex_version = read_package_version(&packages_dir.join("convex/package.json"));
    let codegen_version = read_package_version(&packages_dir.join("codegen/package.json"));

    println!("cargo:rustc-env=NIMBUS_CONVEX_VERSION={convex_version}");
    println!("cargo:rustc-env=NIMBUS_CODEGEN_VERSION={codegen_version}");

    println!("cargo:rerun-if-changed=../../packages/convex/package.json");
    println!("cargo:rerun-if-changed=../../packages/codegen/package.json");

    // nimbus-bin embeds the staged, dependency-closed JS package payloads via
    // rust-embed (src/embedded_packages.rs). Assert the staged payload exists so
    // a cargo-direct build fails with an actionable message instead of a cryptic
    // rust-embed error. `make build` stages it on demand (Makefile EMBEDDED_PKG
    // graph).
    let embedded_manifest = Path::new(&manifest_dir).join("embedded-packages/manifest.json");
    if !embedded_manifest.exists() {
        panic!(
            "embedded package payload is missing — {} does not exist. \
             Build the JS packages and run `node scripts/stage-embedded-packages.mjs`, \
             or use `make build`, which stages it. Cargo-direct builds of nimbus-bin \
             require the staged payload beforehand.",
            embedded_manifest.display()
        );
    }
    println!("cargo:rerun-if-changed=embedded-packages/manifest.json");
}
