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

    println!("cargo:rustc-env=NEOVEX_CONVEX_VERSION={convex_version}");
    println!("cargo:rustc-env=NEOVEX_CODEGEN_VERSION={codegen_version}");

    println!("cargo:rerun-if-changed=../../packages/convex/package.json");
    println!("cargo:rerun-if-changed=../../packages/codegen/package.json");
}
