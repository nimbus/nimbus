use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    ensure_ui_assets()?;

    Ok(())
}

fn ensure_ui_assets() -> io::Result<()> {
    let manifest = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set"),
    );
    let dist_dir = manifest.join("../../packages/nimbus-ui/dist");
    let index_path = dist_dir.join("index.html");
    let codegen_dir = manifest.join("../../packages/nimbus-ui/.nimbus/convex");

    println!("cargo:rerun-if-changed={}", dist_dir.display());
    println!("cargo:rerun-if-changed={}", codegen_dir.display());

    if !index_path.exists() {
        return Err(io::Error::other(format!(
            "nimbus-ui dist is missing — {} does not exist. \
             Run any `make` target (e.g. `make build-ui`, `make check`, `make test`); \
             Make's dependency graph will build the SPA on demand. \
             Cargo-direct builds of nimbus-server require dist to exist beforehand.",
            index_path.display()
        )));
    }

    Ok(())
}
