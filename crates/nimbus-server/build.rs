use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    let protoc = protoc_bin_vendored::protoc_bin_path()
        .expect("vendored protoc should be available for Firestore codegen");
    // The Firebase adapter vendors the audited Firestore proto tree under
    // `crates/nimbus-server/proto/google/...` so upgrades remain explicit.
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }

    tonic_build::configure()
        .build_client(true)
        .build_server(true)
        .generate_default_stubs(true)
        .include_file("firebase_grpc.rs")
        .compile_protos(&["proto/google/firestore/v1/firestore.proto"], &["proto"])?;

    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR should be set for build scripts");
    strip_generated_doc_comments(Path::new(&out_dir))?;

    ensure_ui_assets()?;

    Ok(())
}

fn ensure_ui_assets() -> io::Result<()> {
    let manifest = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR should be set"),
    );
    let dist_dir = manifest.join("../../packages/nimbus-ui/dist");
    let index_path = dist_dir.join("index.html");
    let profile = std::env::var("PROFILE").unwrap_or_default();

    println!("cargo:rerun-if-changed={}", dist_dir.display());

    if !index_path.exists() {
        if profile == "release" {
            return Err(io::Error::other(format!(
                "release build requires nimbus-ui dist; run `make build-ui` first (missing {})",
                index_path.display()
            )));
        }
        fs::create_dir_all(&dist_dir)?;
        let stub = "<!doctype html><html><head><meta charset=\"utf-8\"><title>Nimbus UI</title></head><body><main><h1>Nimbus UI</h1><p>Run <code>make build-ui</code> to populate this stub.</p></main></body></html>";
        fs::write(&index_path, stub)?;
    }

    Ok(())
}

fn strip_generated_doc_comments(out_dir: &Path) -> io::Result<()> {
    for entry in fs::read_dir(out_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("rs") {
            continue;
        }

        let source = fs::read_to_string(&path)?;
        let stripped = source
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !trimmed.starts_with("///") && !trimmed.starts_with("//!")
            })
            .collect::<Vec<_>>()
            .join("\n");
        if stripped != source {
            fs::write(path, stripped)?;
        }
    }
    Ok(())
}
