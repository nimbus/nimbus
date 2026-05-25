use std::env;

fn main() {
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
}
