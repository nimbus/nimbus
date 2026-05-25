use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

pub(crate) const BUN_JSC_SHARED_LIBRARY_ENV: &str = "NIMBUS_BUN_EMBED_SHARED_LIBRARY";
pub(crate) const BUN_JSC_ADAPTER_MANIFEST_ENV: &str = "NIMBUS_BUN_JSC_ADAPTER_MANIFEST";
const BUN_JSC_ADAPTER_MANIFEST_FILE: &str = "nimbus-bun-jsc-adapter.json";
const BUN_JSC_ADAPTER_KIND: &str = "nimbus.bun_jsc.adapter";
const BUN_JSC_ADAPTER_SCHEMA_VERSION: u32 = 1;
const BUN_JSC_ADAPTER_ABI_NAME: &str = "nimbus-bun-jsc-embedder";
const BUN_JSC_ADAPTER_ABI_VERSION: u32 = 1;
const BUN_JSC_MEMORY_ENFORCEMENT: &str = "outer_quota_required";
const BUN_JSC_LIFECYCLE: &str = "fresh_discard";

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
        source_ref: "bun-v1.4.0-nimbus.5",
        git_revision: "ad0e1d2bbc6690651e04f10eaf1dcdf8a6c0de57",
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

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BunJscAdapterManifest {
    schema_version: u32,
    kind: String,
    adapter_version: String,
    nimbus_version: String,
    bun_source_repository: String,
    bun_source_ref: String,
    bun_source_revision: String,
    target_triple: String,
    platform: String,
    library: String,
    library_sha256: String,
    abi: BunJscAdapterAbiManifest,
    memory_enforcement: String,
    lifecycle: String,
    provenance: Option<BunJscAdapterProvenanceManifest>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BunJscAdapterAbiManifest {
    name: String,
    version: u32,
    required_exports: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BunJscAdapterProvenanceManifest {
    sbom: Option<String>,
    slsa: Option<String>,
    checksum_file: String,
}

pub(crate) fn resolve_shared_adapter_library_path() -> std::result::Result<PathBuf, String> {
    resolve_shared_adapter_library_path_from_values(
        std::env::var_os(BUN_JSC_SHARED_LIBRARY_ENV),
        std::env::var_os(BUN_JSC_ADAPTER_MANIFEST_ENV),
        &packaged_manifest_paths(),
    )
}

fn resolve_shared_adapter_library_path_from_values(
    shared_library_env: Option<OsString>,
    manifest_env: Option<OsString>,
    packaged_manifests: &[PathBuf],
) -> std::result::Result<PathBuf, String> {
    if let Some(path) = env_path(shared_library_env, BUN_JSC_SHARED_LIBRARY_ENV)? {
        return validate_direct_shared_library_path(&path);
    }
    if let Some(path) = env_path(manifest_env, BUN_JSC_ADAPTER_MANIFEST_ENV)? {
        return validate_adapter_manifest_path(&path);
    }

    for manifest_path in packaged_manifests {
        if manifest_path.is_file() {
            return validate_adapter_manifest_path(manifest_path);
        }
    }

    Err(format!(
        "set {BUN_JSC_SHARED_LIBRARY_ENV} to libnimbus_bun_jsc_embedder.so/dylib for a development proof, set {BUN_JSC_ADAPTER_MANIFEST_ENV} to {BUN_JSC_ADAPTER_MANIFEST_FILE}, or install the optional nimbus-bun-jsc-adapter package"
    ))
}

fn packaged_manifest_paths() -> Vec<PathBuf> {
    vec![
        PathBuf::from(format!(
            "/usr/libexec/nimbus/runtime/bun-jsc/current/{BUN_JSC_ADAPTER_MANIFEST_FILE}"
        )),
        PathBuf::from(format!(
            "/opt/homebrew/opt/nimbus/libexec/runtime/bun-jsc/current/{BUN_JSC_ADAPTER_MANIFEST_FILE}"
        )),
        PathBuf::from(format!(
            "/usr/local/opt/nimbus/libexec/runtime/bun-jsc/current/{BUN_JSC_ADAPTER_MANIFEST_FILE}"
        )),
    ]
}

fn env_path(value: Option<OsString>, name: &str) -> std::result::Result<Option<PathBuf>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(format!("{name} is empty"));
    }
    Ok(Some(PathBuf::from(value)))
}

fn validate_direct_shared_library_path(path: &Path) -> std::result::Result<PathBuf, String> {
    if !path.is_file() {
        return Err(format!(
            "{BUN_JSC_SHARED_LIBRARY_ENV} points to {}, which is not a file",
            path.display()
        ));
    }
    path.canonicalize()
        .map_err(|error| format!("failed to canonicalize {}: {error}", path.display()))
}

pub(crate) fn validate_adapter_manifest_path(
    manifest_path: &Path,
) -> std::result::Result<PathBuf, String> {
    if !manifest_path.is_file() {
        return Err(format!(
            "{BUN_JSC_ADAPTER_MANIFEST_ENV} points to {}, which is not a file",
            manifest_path.display()
        ));
    }
    let manifest_path = manifest_path.canonicalize().map_err(|error| {
        format!(
            "failed to canonicalize {}: {error}",
            manifest_path.display()
        )
    })?;
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| {
            format!(
                "Bun/JSC adapter manifest has no parent directory: {}",
                manifest_path.display()
            )
        })?
        .to_path_buf();
    validate_packaged_path_safety(&manifest_dir, "Bun/JSC adapter manifest directory")?;
    validate_packaged_path_safety(&manifest_path, "Bun/JSC adapter manifest")?;

    let manifest_bytes = std::fs::read(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest = serde_json::from_slice::<BunJscAdapterManifest>(&manifest_bytes)
        .map_err(|error| format!("invalid Bun/JSC adapter manifest JSON: {error}"))?;
    validate_manifest_contract(&manifest)?;

    let library_component = single_relative_component(&manifest.library)?;
    let library_path = manifest_dir.join(library_component);
    if !library_path.is_file() {
        return Err(format!(
            "Bun/JSC adapter manifest library {} does not exist beside {}",
            library_path.display(),
            manifest_path.display()
        ));
    }
    let library_path = library_path
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize {}: {error}", library_path.display()))?;
    if !library_path.starts_with(&manifest_dir) {
        return Err(format!(
            "Bun/JSC adapter library {} escapes manifest directory {}",
            library_path.display(),
            manifest_dir.display()
        ));
    }
    validate_packaged_path_safety(&library_path, "Bun/JSC adapter library")?;

    let actual_sha256 = compute_sha256_for_path(&library_path)?;
    let expected_sha256 = normalize_sha256(&manifest.library_sha256, "library_sha256")?;
    if actual_sha256 != expected_sha256 {
        return Err(format!(
            "Bun/JSC adapter library checksum mismatch for {}: expected {}, got {}",
            library_path.display(),
            expected_sha256,
            actual_sha256
        ));
    }

    Ok(library_path)
}

fn validate_manifest_contract(manifest: &BunJscAdapterManifest) -> std::result::Result<(), String> {
    let contract = BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT;
    expect_string("kind", &manifest.kind, BUN_JSC_ADAPTER_KIND)?;
    expect_non_empty("adapter_version", &manifest.adapter_version)?;
    expect_non_empty("nimbus_version", &manifest.nimbus_version)?;
    expect_string(
        "bun_source_repository",
        &manifest.bun_source_repository,
        contract.repository,
    )?;
    expect_string(
        "bun_source_ref",
        &manifest.bun_source_ref,
        contract.source_ref,
    )?;
    expect_string(
        "bun_source_revision",
        &manifest.bun_source_revision,
        contract.git_revision,
    )?;
    expect_string(
        "target_triple",
        &manifest.target_triple,
        current_target_triple(),
    )?;
    expect_string("platform", &manifest.platform, current_platform())?;
    expect_non_empty("library", &manifest.library)?;
    expect_string("abi.name", &manifest.abi.name, BUN_JSC_ADAPTER_ABI_NAME)?;
    if manifest.abi.version != BUN_JSC_ADAPTER_ABI_VERSION {
        return Err(format!(
            "Bun/JSC adapter manifest abi.version must be {}, got {}",
            BUN_JSC_ADAPTER_ABI_VERSION, manifest.abi.version
        ));
    }
    let required_exports: Vec<&str> = manifest
        .abi
        .required_exports
        .iter()
        .map(String::as_str)
        .collect();
    if required_exports != contract.required_exports {
        return Err(format!(
            "Bun/JSC adapter manifest abi.required_exports does not match the Nimbus contract: expected {:?}, got {:?}",
            contract.required_exports, manifest.abi.required_exports
        ));
    }
    expect_string(
        "memory_enforcement",
        &manifest.memory_enforcement,
        BUN_JSC_MEMORY_ENFORCEMENT,
    )?;
    expect_string("lifecycle", &manifest.lifecycle, BUN_JSC_LIFECYCLE)?;
    if let Some(provenance) = &manifest.provenance {
        expect_non_empty("provenance.checksum_file", &provenance.checksum_file)?;
        if let Some(sbom) = &provenance.sbom {
            expect_non_empty("provenance.sbom", sbom)?;
        }
        if let Some(slsa) = &provenance.slsa {
            expect_non_empty("provenance.slsa", slsa)?;
        }
    }
    if manifest.schema_version != BUN_JSC_ADAPTER_SCHEMA_VERSION {
        return Err(format!(
            "Bun/JSC adapter manifest schema_version must be {}, got {}",
            BUN_JSC_ADAPTER_SCHEMA_VERSION, manifest.schema_version
        ));
    }
    Ok(())
}

fn expect_non_empty(name: &str, actual: &str) -> std::result::Result<(), String> {
    if actual.trim().is_empty() {
        return Err(format!("Bun/JSC adapter manifest {name} must be non-empty"));
    }
    Ok(())
}

fn expect_string(name: &str, actual: &str, expected: &str) -> std::result::Result<(), String> {
    if actual == expected {
        return Ok(());
    }
    Err(format!(
        "Bun/JSC adapter manifest {name} must be {expected:?}, got {actual:?}"
    ))
}

fn single_relative_component(value: &str) -> std::result::Result<&Path, String> {
    let path = Path::new(value);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(path),
        _ => Err(format!(
            "Bun/JSC adapter manifest library must be a single relative filename, got {value:?}"
        )),
    }
}

fn normalize_sha256(value: &str, field: &str) -> std::result::Result<String, String> {
    let trimmed = value.trim();
    if trimmed.len() != 64 || !trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "Bun/JSC adapter manifest {field} must be a 64-character SHA-256 hex string, got {trimmed:?}"
        ));
    }
    Ok(trimmed.to_ascii_lowercase())
}

fn compute_sha256_for_path(path: &Path) -> std::result::Result<String, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("failed to read {} for SHA-256: {error}", path.display()))?;
    Ok(compute_sha256_hex(&bytes))
}

fn compute_sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn current_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        platform => platform,
    }
}

fn current_target_triple() -> &'static str {
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

#[cfg(unix)]
fn validate_packaged_path_safety(path: &Path, label: &str) -> std::result::Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("failed to read metadata for {}: {error}", path.display()))?;
    let mode = metadata.permissions().mode();
    if mode & 0o022 != 0 {
        return Err(format!(
            "{label} {} has unsafe permissions {:o}; group/other writable packaged Bun/JSC adapter paths are rejected",
            path.display(),
            mode & 0o777
        ));
    }
    Ok(())
}

#[cfg(not(unix))]
fn validate_packaged_path_safety(_path: &Path, _label: &str) -> std::result::Result<(), String> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn shared_library_basename() -> &'static str {
    "libnimbus_bun_jsc_embedder.dylib"
}

#[cfg(not(target_os = "macos"))]
fn shared_library_basename() -> &'static str {
    "libnimbus_bun_jsc_embedder.so"
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    fn write_stub_library(dir: &Path) -> (PathBuf, String) {
        let library_path = dir.join(shared_library_basename());
        std::fs::write(&library_path, b"stub Bun/JSC shared adapter")
            .expect("stub library should be written");
        let sha256 = compute_sha256_for_path(&library_path).expect("stub sha256 should compute");
        (library_path, sha256)
    }

    fn manifest_json(library_sha256: &str) -> Value {
        let contract = BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT;
        json!({
            "schema_version": 1,
            "kind": "nimbus.bun_jsc.adapter",
            "adapter_version": "v0.1.0-bun-v1.4.0-nimbus.5",
            "nimbus_version": "v0.1.0",
            "bun_source_repository": contract.repository,
            "bun_source_ref": contract.source_ref,
            "bun_source_revision": contract.git_revision,
            "target_triple": current_target_triple(),
            "platform": current_platform(),
            "library": shared_library_basename(),
            "library_sha256": library_sha256,
            "abi": {
                "name": "nimbus-bun-jsc-embedder",
                "version": 1,
                "required_exports": contract.required_exports,
            },
            "memory_enforcement": "outer_quota_required",
            "lifecycle": "fresh_discard",
            "provenance": {
                "sbom": "nimbus-bun-jsc-adapter.sbom.cdx.json",
                "slsa": "nimbus-bun-jsc-adapter.intoto.jsonl",
                "checksum_file": "checksums-sha256.txt"
            }
        })
    }

    fn write_manifest(dir: &Path, manifest: &Value) -> PathBuf {
        let manifest_path = dir.join(BUN_JSC_ADAPTER_MANIFEST_FILE);
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(manifest).expect("manifest should serialize"),
        )
        .expect("manifest should be written");
        manifest_path
    }

    #[test]
    fn valid_packaged_manifest_resolves_canonical_library_path() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (library_path, sha256) = write_stub_library(temp_dir.path());
        let manifest_path = write_manifest(temp_dir.path(), &manifest_json(&sha256));

        let resolved = validate_adapter_manifest_path(&manifest_path)
            .expect("valid manifest should resolve the packaged adapter library");

        assert_eq!(
            resolved,
            library_path
                .canonicalize()
                .expect("stub library should canonicalize")
        );
    }

    #[test]
    fn discovery_prefers_explicit_development_library_override() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (library_path, _sha256) = write_stub_library(temp_dir.path());
        let bad_manifest = temp_dir.path().join("missing.json");

        let resolved = resolve_shared_adapter_library_path_from_values(
            Some(library_path.clone().into_os_string()),
            Some(bad_manifest.into_os_string()),
            &[],
        )
        .expect("development library override should take precedence");

        assert_eq!(
            resolved,
            library_path
                .canonicalize()
                .expect("stub library should canonicalize")
        );
    }

    #[test]
    fn discovery_uses_manifest_override_before_packaged_locations() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (library_path, sha256) = write_stub_library(temp_dir.path());
        let manifest_path = write_manifest(temp_dir.path(), &manifest_json(&sha256));

        let resolved = resolve_shared_adapter_library_path_from_values(
            None,
            Some(manifest_path.into_os_string()),
            &[],
        )
        .expect("manifest override should resolve");

        assert_eq!(
            resolved,
            library_path
                .canonicalize()
                .expect("stub library should canonicalize")
        );
    }

    #[test]
    fn discovery_uses_first_existing_packaged_manifest() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (library_path, sha256) = write_stub_library(temp_dir.path());
        let manifest_path = write_manifest(temp_dir.path(), &manifest_json(&sha256));

        let resolved = resolve_shared_adapter_library_path_from_values(
            None,
            None,
            &[
                temp_dir.path().join("missing.json"),
                manifest_path,
                temp_dir.path().join("also-missing.json"),
            ],
        )
        .expect("packaged manifest should resolve");

        assert_eq!(
            resolved,
            library_path
                .canonicalize()
                .expect("stub library should canonicalize")
        );
    }

    #[test]
    fn discovery_without_env_or_package_returns_install_hint() {
        let error = resolve_shared_adapter_library_path_from_values(None, None, &[])
            .expect_err("missing adapter should remain fail-closed");

        assert!(
            error.contains("install the optional nimbus-bun-jsc-adapter package"),
            "unexpected error: {error}"
        );
        assert!(
            error.contains(BUN_JSC_SHARED_LIBRARY_ENV)
                && error.contains(BUN_JSC_ADAPTER_MANIFEST_ENV),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_rejects_wrong_bun_revision() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let mut manifest = manifest_json(&sha256);
        manifest["bun_source_revision"] = json!("not-the-proven-revision");
        let manifest_path = write_manifest(temp_dir.path(), &manifest);

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("wrong source revision should be rejected");

        assert!(
            error.contains("bun_source_revision"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_rejects_wrong_target_triple() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let mut manifest = manifest_json(&sha256);
        manifest["target_triple"] = json!("wasm32-unknown-unknown");
        let manifest_path = write_manifest(temp_dir.path(), &manifest);

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("wrong target triple should be rejected");

        assert!(error.contains("target_triple"), "unexpected error: {error}");
    }

    #[test]
    fn manifest_rejects_schema_mismatch() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let mut manifest = manifest_json(&sha256);
        manifest["schema_version"] = json!(2);
        let manifest_path = write_manifest(temp_dir.path(), &manifest);

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("unsupported schema should be rejected");

        assert!(
            error.contains("schema_version"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_rejects_checksum_mismatch() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (_library_path, _sha256) = write_stub_library(temp_dir.path());
        let manifest_path = write_manifest(temp_dir.path(), &manifest_json(&"0".repeat(64)));

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("checksum mismatch should be rejected");

        assert!(
            error.contains("checksum mismatch"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_rejects_unsupported_memory_and_lifecycle_policy() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (_library_path, sha256) = write_stub_library(temp_dir.path());

        let mut memory_manifest = manifest_json(&sha256);
        memory_manifest["memory_enforcement"] = json!("v8_isolate_heap_limit");
        let memory_manifest_path = write_manifest(temp_dir.path(), &memory_manifest);
        let memory_error = validate_adapter_manifest_path(&memory_manifest_path)
            .expect_err("unsupported memory policy should be rejected");
        assert!(
            memory_error.contains("memory_enforcement"),
            "unexpected error: {memory_error}"
        );

        let mut lifecycle_manifest = manifest_json(&sha256);
        lifecycle_manifest["lifecycle"] = json!("trusted_retained");
        let lifecycle_manifest_path = write_manifest(temp_dir.path(), &lifecycle_manifest);
        let lifecycle_error = validate_adapter_manifest_path(&lifecycle_manifest_path)
            .expect_err("unsupported lifecycle policy should be rejected");
        assert!(
            lifecycle_error.contains("lifecycle"),
            "unexpected error: {lifecycle_error}"
        );
    }

    #[test]
    fn manifest_rejects_library_paths_that_escape_manifest_directory() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let mut manifest = manifest_json(&sha256);
        manifest["library"] = json!("../libnimbus_bun_jsc_embedder.so");
        let manifest_path = write_manifest(temp_dir.path(), &manifest);

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("escaping library path should be rejected");

        assert!(
            error.contains("single relative filename"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_rejects_unknown_fields() {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let mut manifest = manifest_json(&sha256);
        manifest["unexpected"] = json!("field");
        let manifest_path = write_manifest(temp_dir.path(), &manifest);

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("unknown manifest fields should be rejected");

        assert!(error.contains("unknown field"), "unexpected error: {error}");
    }

    #[cfg(unix)]
    #[test]
    fn manifest_rejects_group_or_other_writable_packaged_directory() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let manifest_path = write_manifest(temp_dir.path(), &manifest_json(&sha256));
        let original_permissions = std::fs::metadata(temp_dir.path())
            .expect("temp dir metadata should load")
            .permissions();
        let mut unsafe_permissions = original_permissions.clone();
        unsafe_permissions.set_mode(0o777);
        std::fs::set_permissions(temp_dir.path(), unsafe_permissions)
            .expect("temp dir permissions should change");

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("group/other writable manifest directories should be rejected");

        std::fs::set_permissions(temp_dir.path(), original_permissions)
            .expect("temp dir permissions should be restored");
        assert!(
            error.contains("unsafe permissions"),
            "unexpected error: {error}"
        );
    }
}
