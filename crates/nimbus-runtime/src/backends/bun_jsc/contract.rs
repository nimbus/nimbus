use crate::limits::RuntimeExecutionAdapterExpectedArtifact;
#[cfg(not(feature = "bun-jsc-linked-adapter"))]
use crate::limits::{
    RuntimeExecutionAdapterArtifactDiagnostics, RuntimeExecutionAdapterArtifactSource,
    RuntimeExecutionAdapterArtifactStatus,
};

pub(crate) const BUN_JSC_SHARED_LIBRARY_ENV: &str = "NIMBUS_BUN_EMBED_SHARED_LIBRARY";
pub(crate) const BUN_JSC_ADAPTER_MANIFEST_ENV: &str = "NIMBUS_BUN_JSC_ADAPTER_MANIFEST";
pub(crate) const BUN_JSC_ADAPTER_MANIFEST_FILE: &str = "nimbus-bun-jsc-adapter.json";
pub(crate) const BUN_JSC_ADAPTER_README_FILE: &str = "README.md";
pub(crate) const BUN_JSC_ADAPTER_KIND: &str = "nimbus.bun_jsc.adapter";
pub(crate) const BUN_JSC_ADAPTER_SCHEMA_VERSION: u32 = 1;
pub(crate) const BUN_JSC_ADAPTER_ABI_NAME: &str = "nimbus-bun-jsc-embedder";
pub(crate) const BUN_JSC_ADAPTER_ABI_VERSION: u32 = 1;
pub(crate) const BUN_JSC_MEMORY_ENFORCEMENT: &str = "outer_quota_required";
pub(crate) const BUN_JSC_LIFECYCLE: &str = "fresh_discard";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct BunJscLinkedAdapterSourceContract {
    pub(crate) repository: &'static str,
    pub(crate) source_ref: &'static str,
    pub(crate) git_revision: &'static str,
    pub(crate) proof_target: &'static str,
    pub(crate) simdutf_namespace: &'static str,
    pub(crate) required_exports: &'static [&'static str],
}

pub(crate) const BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT: BunJscLinkedAdapterSourceContract =
    BunJscLinkedAdapterSourceContract {
        repository: "https://github.com/nimbus/bun",
        source_ref: "nimbus-bun-jsc-proof-main-20260901",
        git_revision: "c09efa5c28e550782902d7185ea6eb760fca57df",
        proof_target: "check-bun-embed-shared",
        simdutf_namespace: "nimbus_bun_simdutf",
        required_exports: &[
            "nimbus_bun_embed_probe_construct_and_destroy_vm",
            "nimbus_bun_embed_probe_sync_host_call",
            "nimbus_bun_embed_probe_async_host_call",
            "nimbus_bun_embed_probe_program_bundle_host_calls",
            "nimbus_bun_embed_probe_timeout_and_cancel",
            "nimbus_bun_embed_probe_permission_surface_inventory",
            "nimbus_bun_embed_probe_memory_behavior",
            "nimbus_bun_embed_probe_package_module_policy",
            "nimbus_bun_embed_probe_lifecycle_reuse_stress",
            "nimbus_bun_embed_invoke_program_wrapper_json",
            "nimbus_bun_embed_invoke_program_wrapper_json_with_host_bridge",
        ],
    };

pub(crate) fn expected_artifact_contract() -> RuntimeExecutionAdapterExpectedArtifact {
    let contract = BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT;
    RuntimeExecutionAdapterExpectedArtifact {
        kind: BUN_JSC_ADAPTER_KIND.to_string(),
        schema_version: BUN_JSC_ADAPTER_SCHEMA_VERSION,
        source_repository: contract.repository.to_string(),
        source_ref: contract.source_ref.to_string(),
        source_revision: contract.git_revision.to_string(),
        target_triple: current_target_triple().to_string(),
        platform: current_platform().to_string(),
        manifest_file: BUN_JSC_ADAPTER_MANIFEST_FILE.to_string(),
        library_file: shared_library_basename().to_string(),
        readme_file: BUN_JSC_ADAPTER_README_FILE.to_string(),
        abi_name: BUN_JSC_ADAPTER_ABI_NAME.to_string(),
        abi_version: BUN_JSC_ADAPTER_ABI_VERSION,
        memory_enforcement: BUN_JSC_MEMORY_ENFORCEMENT.to_string(),
        lifecycle: BUN_JSC_LIFECYCLE.to_string(),
        proof_target: contract.proof_target.to_string(),
        simdutf_namespace: contract.simdutf_namespace.to_string(),
        required_export_count: contract.required_exports.len(),
    }
}

#[cfg(not(feature = "bun-jsc-linked-adapter"))]
pub(crate) fn disabled_build_diagnostics() -> RuntimeExecutionAdapterArtifactDiagnostics {
    RuntimeExecutionAdapterArtifactDiagnostics {
        status: RuntimeExecutionAdapterArtifactStatus::NotLinked,
        source: RuntimeExecutionAdapterArtifactSource::BuildFeatureDisabled,
        reason_code: "linked_adapter_feature_disabled".to_string(),
        install_hint: Some(install_hint()),
        expected: Some(expected_artifact_contract()),
        manifest: None,
    }
}

pub(crate) fn install_hint() -> String {
    format!(
        "install the optional nimbus-bun-jsc-adapter package, set {BUN_JSC_ADAPTER_MANIFEST_ENV} to a verified {BUN_JSC_ADAPTER_MANIFEST_FILE}, or set {BUN_JSC_SHARED_LIBRARY_ENV} for a development proof"
    )
}

pub(crate) fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        platform => platform,
    }
}

pub(crate) fn current_target_triple() -> &'static str {
    #[cfg(all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"))]
    {
        "x86_64-unknown-linux-gnu"
    }
    #[cfg(all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"))]
    {
        "aarch64-unknown-linux-gnu"
    }
    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    {
        "aarch64-apple-darwin"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "macos"))]
    {
        "x86_64-apple-darwin"
    }
    #[cfg(all(target_arch = "x86_64", target_os = "windows", target_env = "msvc"))]
    {
        "x86_64-pc-windows-msvc"
    }
    #[cfg(not(any(
        all(target_arch = "x86_64", target_os = "linux", target_env = "gnu"),
        all(target_arch = "aarch64", target_os = "linux", target_env = "gnu"),
        all(target_arch = "aarch64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "macos"),
        all(target_arch = "x86_64", target_os = "windows", target_env = "msvc")
    )))]
    {
        "unsupported"
    }
}

#[cfg(target_os = "macos")]
pub(crate) fn shared_library_basename() -> &'static str {
    "libnimbus_bun_jsc_embedder.dylib"
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn shared_library_basename() -> &'static str {
    "libnimbus_bun_jsc_embedder.so"
}
