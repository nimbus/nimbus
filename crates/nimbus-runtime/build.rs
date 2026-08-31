use std::env;

/// Fence the shared-path `librusty_v8.a` hazard. rusty_v8 copies EVERY prebuilt variant
/// (pointer-compression and non-pc) to the SAME `target/<profile>/gn_out/obj/librusty_v8.a`,
/// last-writer-wins. Interleaving a feature-off build after the feature-on v8 build (whose script
/// cargo then caches) leaves the non-pc V8 in that shared file, and a feature-on link silently picks
/// it up — running without pointer compression or the shared cage (the Rust bindings are
/// pc-agnostic, so it links cleanly and fails silently rather than loudly). When this crate is built
/// with `v8-pointer-compression`, assert the most-recently-run v8 build fetched a `ptrcomp` variant,
/// and re-run whenever the linked `.a` changes so an interleaved overwrite is caught on the next
/// build. Degrades to a no-op (never a false failure) if the cargo build layout can't be read.
fn guard_pointer_compression_v8_link() {
    if env::var_os("CARGO_FEATURE_V8_POINTER_COMPRESSION").is_none() {
        return;
    }
    let Ok(out_dir) = env::var("OUT_DIR") else {
        return;
    };
    // OUT_DIR = target/<profile>/build/nimbus-runtime-<hash>/out
    let Some(profile_dir) = std::path::Path::new(&out_dir).ancestors().nth(3) else {
        return;
    };
    let gn_archive = profile_dir.join("gn_out/obj/librusty_v8.a");
    // Re-run this guard whenever the linked archive is overwritten (e.g. by an interleaved
    // feature-off build) so the mislink is caught on the next feature-on build.
    println!("cargo:rerun-if-changed={}", gn_archive.display());

    // The v8 build script logs the prebuilt variant it fetched, e.g.
    // "static lib URL: .../librusty_v8_ptrcomp_simdutf_release_<target>.a.gz". Find the most
    // recently run v8 build output and read its variant — that is what occupies the shared archive.
    let build_dir = profile_dir.join("build");
    let mut latest: Option<(std::time::SystemTime, String)> = None;
    if let Ok(entries) = std::fs::read_dir(&build_dir) {
        for entry in entries.flatten() {
            if !entry.file_name().to_string_lossy().starts_with("v8-") {
                continue;
            }
            let output = entry.path().join("output");
            let (Ok(meta), Ok(content)) =
                (std::fs::metadata(&output), std::fs::read_to_string(&output))
            else {
                continue;
            };
            let Ok(mtime) = meta.modified() else {
                continue;
            };
            let Some(line) = content
                .lines()
                .find(|l| l.contains("librusty_v8_") && l.contains("_release"))
            else {
                continue;
            };
            if latest.as_ref().is_none_or(|(t, _)| mtime > *t) {
                latest = Some((mtime, line.trim().to_string()));
            }
        }
    }
    if let Some((_, variant_line)) = latest {
        assert!(
            variant_line.contains("ptrcomp"),
            "nimbus-runtime is being built with `v8-pointer-compression`, but the shared prebuilt \
             V8 at {} was last written by a NON-pointer-compression v8 build (most recent v8 build \
             output: `{}`). rusty_v8 copies every variant to that same path (last-writer-wins), so \
             an interleaved feature-off build leaves the wrong V8 in place and this feature-on \
             binary would silently link it (no pointer compression, no shared cage). Run `cargo \
             clean` (or build feature-on under a separate CARGO_TARGET_DIR) and rebuild.",
            gn_archive.display(),
            variant_line,
        );
    }
}

fn main() {
    guard_pointer_compression_v8_link();
    println!("cargo:rustc-check-cfg=cfg(nimbus_bun_jsc_shared_adapter)");
    println!("cargo:rerun-if-env-changed=NIMBUS_BUN_EMBED_SHARED_LIBRARY");
    println!("cargo:rerun-if-env-changed=NIMBUS_BUN_JSC_ADAPTER_MANIFEST");

    let has_shared_library =
        env::var_os("NIMBUS_BUN_EMBED_SHARED_LIBRARY").is_some_and(|value| !value.is_empty());
    let has_adapter_manifest =
        env::var_os("NIMBUS_BUN_JSC_ADAPTER_MANIFEST").is_some_and(|value| !value.is_empty());
    if has_shared_library || has_adapter_manifest {
        println!("cargo:rustc-cfg=nimbus_bun_jsc_shared_adapter");
    }

    // The lib `include_bytes!`es one of two embedded NodeFull(Node22) anchor snapshots, selected by
    // the `v8-pointer-compression` cfg (`.pc.bin` = pointer-compressed = release; `.bin` = feature
    // off = dev/test). The real blobs are produced by `make build-node22-anchor-snapshot` (which runs
    // the builder binary once per config) and remain gitignored. A fresh checkout or bootstrap build
    // compiles for EITHER config, write an EMPTY placeholder for BOTH paths when absent — an empty
    // blob fails the provenance guard at runtime and falls back to a runtime build (slow-but-correct)
    // until the blob is generated. Never overwrite a real blob.
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    for filename in [
        "node22_anchor_snapshot.bin",
        "node22_anchor_snapshot.pc.bin",
    ] {
        let snapshot_path = format!("{manifest_dir}/src/backends/v8/{filename}");
        println!("cargo:rerun-if-changed=src/backends/v8/{filename}");
        if !std::path::Path::new(&snapshot_path).exists() {
            std::fs::write(&snapshot_path, []).expect("write placeholder anchor snapshot");
        }
    }
}
