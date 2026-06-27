//! Regenerates (or `--check`s) the embedded NodeFull(Node22) anchor-snapshot blob. This is a normal
//! CONSUMER of `nimbus-runtime` (depends on the lib; no build-script cycle) AND a NON-`cfg(test)`
//! build, so it computes the same snapshot/provenance the serving path does — unlike a unit test,
//! whose `cfg(test)` snapshot includes a test-only extension. Run via `make
//! build-node22-anchor-snapshot` (writes both configs) or `make verify-node22-anchor-snapshot`
//! (`--check` both configs). The output/compare path is chosen by THIS binary's own
//! pointer-compression cfg, so it always matches the blob it builds (`.pc.bin` = pointer-compressed
//! = release; `.bin` = feature-off = dev/test). Regenerate whenever the bootstrap / extension set /
//! deno-fork pin / op surface changes.

fn anchor_snapshot_path() -> String {
    // Select the path matching the config this binary was compiled under.
    let filename = if cfg!(feature = "v8-pointer-compression") {
        "node22_anchor_snapshot.pc.bin"
    } else {
        "node22_anchor_snapshot.bin"
    };
    format!("{}/src/backends/v8/{filename}", env!("CARGO_MANIFEST_DIR"))
}

fn main() {
    let check = std::env::args().any(|arg| arg == "--check");
    let path = anchor_snapshot_path();
    let pc = cfg!(feature = "v8-pointer-compression");

    if check {
        // Validate the committed blob against the current binary's provenance (V8 version,
        // pointer-compression feature, extension selection, op surface, bootstrap JS, schema) and
        // confirm it parses. NOT a byte-compare: V8 snapshots embed a random hash-seed and are not
        // byte-reproducible.
        let committed = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("read committed blob {path}: {error}"));
        match nimbus_runtime::check_committed_embedded_anchor_snapshot(&committed) {
            Ok(()) => eprintln!(
                "OK: committed embedded anchor snapshot is live for this build \
                 (provenance match + parses, v8-pointer-compression={pc}) at {path}"
            ),
            Err(message) => {
                eprintln!("STALE embedded anchor snapshot: {message}");
                std::process::exit(1);
            }
        }
        return;
    }

    let blob = nimbus_runtime::build_embeddable_node22_snapshot_blob()
        .expect("build NodeFull(Node22) anchor snapshot blob");
    std::fs::write(&path, &blob).unwrap_or_else(|error| panic!("write {path}: {error}"));
    eprintln!(
        "wrote {} bytes to {path} (v8-pointer-compression={pc})",
        blob.len()
    );
}
