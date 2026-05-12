use std::fs;
use std::io;
use std::path::Path;

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
