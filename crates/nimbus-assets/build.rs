use std::io;
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set"),
    );

    if feature_enabled("UI") {
        ensure_file(
            &manifest_dir.join("../../packages/nimbus-ui/dist/index.html"),
            "nimbus-assets ui feature requires the built operator UI. Run `npm run build -w nimbus-ui` or any Make target that builds UI prerequisites.",
        )?;
    }

    if feature_enabled("JS_PACKAGES") {
        ensure_file(
            &manifest_dir.join("embedded/packages/manifest.json"),
            "nimbus-assets js-packages feature requires the staged embedded package payload. Run `npm run build:embedded-packages` or `make build-packages`.",
        )?;
    }

    if feature_enabled("TEMPLATES") {
        ensure_dir(
            &manifest_dir.join("embedded/templates/convex"),
            "nimbus-assets templates feature requires the Convex init templates.",
        )?;
        ensure_dir(
            &manifest_dir.join("embedded/templates/cloud-functions"),
            "nimbus-assets templates feature requires the Cloud Functions init templates.",
        )?;
        ensure_dir(
            &manifest_dir.join("embedded/templates/machine"),
            "nimbus-assets templates feature requires the machine bootstrap templates.",
        )?;
    }

    Ok(())
}

fn feature_enabled(name: &str) -> bool {
    std::env::var_os(format!("CARGO_FEATURE_{name}")).is_some()
}

fn ensure_file(path: &Path, message: &str) -> io::Result<()> {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_file() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{message} Missing file: {}",
            path.display()
        )))
    }
}

fn ensure_dir(path: &Path, message: &str) -> io::Result<()> {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{message} Missing directory: {}",
            path.display()
        )))
    }
}
