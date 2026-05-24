use std::env;

fn main() {
    println!("cargo:rustc-check-cfg=cfg(nimbus_bun_jsc_shared_adapter)");
    println!("cargo:rerun-if-env-changed=NIMBUS_BUN_EMBED_SHARED_LIBRARY");

    if env::var_os("NIMBUS_BUN_EMBED_SHARED_LIBRARY").is_some_and(|value| !value.is_empty()) {
        println!("cargo:rustc-cfg=nimbus_bun_jsc_shared_adapter");
    }
}
