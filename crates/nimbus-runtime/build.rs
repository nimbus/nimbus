use std::env;
use std::fs;
use std::path::Path;

const BUN_EMBED_INVOKE_SYMBOL: &str = "nimbus_bun_embed_invoke_program_wrapper_json";

fn main() {
    println!("cargo:rustc-check-cfg=cfg(nimbus_bun_jsc_linked_ffi)");
    println!("cargo:rerun-if-env-changed=NIMBUS_BUN_EMBED_LINK_ARGS");

    let Ok(manifest_path) = env::var("NIMBUS_BUN_EMBED_LINK_ARGS") else {
        return;
    };
    if manifest_path.trim().is_empty() {
        return;
    }

    let manifest = Path::new(&manifest_path);
    let contents = fs::read_to_string(manifest).unwrap_or_else(|error| {
        panic!(
            "failed to read NIMBUS_BUN_EMBED_LINK_ARGS manifest {}: {error}",
            manifest.display()
        )
    });

    println!("cargo:rustc-cfg=nimbus_bun_jsc_linked_ffi");
    println!("cargo:rerun-if-changed={}", manifest.display());
    for line in contents.lines() {
        let arg = line.trim();
        if arg.is_empty() || arg.starts_with('#') {
            continue;
        }
        reject_unsafe_bun_link_arg(arg, manifest);
        if arg.ends_with("libbun_embed_probe.a") {
            emit_required_static_archive_symbol();
        }
        println!("cargo:rustc-link-arg={arg}");
    }
    emit_cxx_runtime_link_args();
}

fn emit_required_static_archive_symbol() {
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-arg=-Wl,-u,_{BUN_EMBED_INVOKE_SYMBOL}");
    } else {
        println!("cargo:rustc-link-arg=-Wl,-u,{BUN_EMBED_INVOKE_SYMBOL}");
    }
}

fn reject_unsafe_bun_link_arg(arg: &str, manifest: &Path) {
    if arg.contains("--allow-multiple-definition") || arg.contains("muldefs") {
        panic!(
            "unsafe Bun/JSC link argument `{arg}` in {}; BJA4L forbids duplicate-symbol link workarounds",
            manifest.display()
        );
    }
}

fn emit_cxx_runtime_link_args() {
    if cfg!(target_os = "macos") {
        println!("cargo:rustc-link-arg=-lc++");
    } else if cfg!(target_os = "linux") {
        println!("cargo:rustc-link-arg=-lstdc++");
    }
}
