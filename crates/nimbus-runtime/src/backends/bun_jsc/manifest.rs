use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::limits::{
    RuntimeExecutionAdapterArtifactDiagnostics, RuntimeExecutionAdapterArtifactSource,
    RuntimeExecutionAdapterArtifactStatus, RuntimeExecutionAdapterManifestArtifact,
};

use super::contract::{
    BUN_JSC_ADAPTER_ABI_NAME, BUN_JSC_ADAPTER_ABI_VERSION, BUN_JSC_ADAPTER_KIND,
    BUN_JSC_ADAPTER_MANIFEST_ENV, BUN_JSC_ADAPTER_MANIFEST_FILE, BUN_JSC_ADAPTER_README_FILE,
    BUN_JSC_ADAPTER_SCHEMA_VERSION, BUN_JSC_LIFECYCLE, BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT,
    BUN_JSC_MEMORY_ENFORCEMENT, BUN_JSC_SHARED_LIBRARY_ENV, current_platform,
    current_target_triple, expected_artifact_contract, install_hint,
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
    sbom: String,
    slsa: String,
    checksum_file: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ResolvedBunJscAdapterLibrary {
    pub(crate) path: PathBuf,
    pub(crate) diagnostics: RuntimeExecutionAdapterArtifactDiagnostics,
}

#[derive(Debug, Clone)]
pub(crate) struct BunJscAdapterDiscoveryError {
    message: String,
    diagnostics: Box<RuntimeExecutionAdapterArtifactDiagnostics>,
}

impl BunJscAdapterDiscoveryError {
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    pub(crate) fn diagnostics(&self) -> RuntimeExecutionAdapterArtifactDiagnostics {
        self.diagnostics.as_ref().clone()
    }
}

impl std::fmt::Display for BunJscAdapterDiscoveryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

pub(crate) fn resolve_shared_adapter_library()
-> std::result::Result<ResolvedBunJscAdapterLibrary, BunJscAdapterDiscoveryError> {
    resolve_shared_adapter_library_from_values(
        std::env::var_os(BUN_JSC_SHARED_LIBRARY_ENV),
        std::env::var_os(BUN_JSC_ADAPTER_MANIFEST_ENV),
        &packaged_manifest_paths(),
    )
}

#[cfg(test)]
fn resolve_shared_adapter_library_path_from_values(
    shared_library_env: Option<OsString>,
    manifest_env: Option<OsString>,
    packaged_manifests: &[PathBuf],
) -> std::result::Result<PathBuf, String> {
    resolve_shared_adapter_library_from_values(shared_library_env, manifest_env, packaged_manifests)
        .map(|resolved| resolved.path)
        .map_err(|error| error.message().to_string())
}

fn resolve_shared_adapter_library_from_values(
    shared_library_env: Option<OsString>,
    manifest_env: Option<OsString>,
    packaged_manifests: &[PathBuf],
) -> std::result::Result<ResolvedBunJscAdapterLibrary, BunJscAdapterDiscoveryError> {
    if let Some(path) = env_path(shared_library_env, BUN_JSC_SHARED_LIBRARY_ENV)? {
        return validate_direct_shared_library_path(&path).map_err(|error| {
            discovery_error(
                RuntimeExecutionAdapterArtifactSource::DevelopmentLibraryEnv,
                classify_discovery_error(&error),
                "development_library_invalid",
                error,
            )
        });
    }
    if let Some(path) = env_path(manifest_env, BUN_JSC_ADAPTER_MANIFEST_ENV)? {
        return validate_adapter_manifest_path_with_source(
            &path,
            RuntimeExecutionAdapterArtifactSource::ManifestEnv,
            "manifest_env_verified",
        );
    }

    for manifest_path in packaged_manifests {
        if manifest_path.is_file() {
            return validate_adapter_manifest_path_with_source(
                manifest_path,
                RuntimeExecutionAdapterArtifactSource::PackagedManifest,
                "packaged_manifest_verified",
            );
        }
    }

    Err(discovery_error(
        RuntimeExecutionAdapterArtifactSource::NotFound,
        RuntimeExecutionAdapterArtifactStatus::MissingArtifact,
        "no_adapter_artifact_configured",
        install_hint(),
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

fn env_path(
    value: Option<OsString>,
    name: &str,
) -> std::result::Result<Option<PathBuf>, BunJscAdapterDiscoveryError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() {
        return Err(discovery_error(
            env_source(name),
            RuntimeExecutionAdapterArtifactStatus::MissingArtifact,
            "empty_adapter_path",
            format!("{name} is empty"),
        ));
    }
    Ok(Some(PathBuf::from(value)))
}

fn validate_direct_shared_library_path(
    path: &Path,
) -> std::result::Result<ResolvedBunJscAdapterLibrary, String> {
    if !path.is_file() {
        return Err(format!(
            "{BUN_JSC_SHARED_LIBRARY_ENV} points to {}, which is not a file",
            path.display()
        ));
    }
    let path = path
        .canonicalize()
        .map_err(|error| format!("failed to canonicalize {}: {error}", path.display()))?;
    Ok(ResolvedBunJscAdapterLibrary {
        path,
        diagnostics: RuntimeExecutionAdapterArtifactDiagnostics {
            status: RuntimeExecutionAdapterArtifactStatus::Linked,
            source: RuntimeExecutionAdapterArtifactSource::DevelopmentLibraryEnv,
            reason_code: "development_library_verified".to_string(),
            install_hint: None,
            expected: Some(expected_artifact_contract()),
            manifest: None,
        },
    })
}

#[cfg(test)]
pub(crate) fn validate_adapter_manifest_path(
    manifest_path: &Path,
) -> std::result::Result<PathBuf, String> {
    validate_adapter_manifest_path_with_source(
        manifest_path,
        RuntimeExecutionAdapterArtifactSource::ManifestEnv,
        "manifest_env_verified",
    )
    .map(|resolved| resolved.path)
    .map_err(|error| error.message().to_string())
}

fn validate_adapter_manifest_path_with_source(
    manifest_path: &Path,
    source: RuntimeExecutionAdapterArtifactSource,
    verified_reason_code: &str,
) -> std::result::Result<ResolvedBunJscAdapterLibrary, BunJscAdapterDiscoveryError> {
    match validate_adapter_manifest_path_inner(manifest_path, source, verified_reason_code) {
        Ok(resolved) => Ok(resolved),
        Err(error) => Err(discovery_error(
            source,
            classify_discovery_error(&error),
            classify_discovery_reason_code(&error),
            error,
        )),
    }
}

fn validate_adapter_manifest_path_inner(
    manifest_path: &Path,
    source: RuntimeExecutionAdapterArtifactSource,
    verified_reason_code: &str,
) -> std::result::Result<ResolvedBunJscAdapterLibrary, String> {
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
    validate_adapter_path_safety(&manifest_dir, "Bun/JSC adapter manifest directory", source)?;
    validate_adapter_path_safety(&manifest_path, "Bun/JSC adapter manifest", source)?;

    let manifest_bytes = std::fs::read(&manifest_path)
        .map_err(|error| format!("failed to read {}: {error}", manifest_path.display()))?;
    let manifest = serde_json::from_slice::<BunJscAdapterManifest>(&manifest_bytes)
        .map_err(|error| format!("invalid Bun/JSC adapter manifest JSON: {error}"))?;
    validate_manifest_contract(&manifest)?;

    let library_component = single_relative_component("library", &manifest.library)?;
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
    validate_adapter_path_safety(&library_path, "Bun/JSC adapter library", source)?;

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
    validate_manifest_provenance_files(
        &manifest,
        &manifest_dir,
        &manifest_bytes,
        &library_path,
        &actual_sha256,
        source,
    )?;

    Ok(ResolvedBunJscAdapterLibrary {
        path: library_path,
        diagnostics: RuntimeExecutionAdapterArtifactDiagnostics {
            status: RuntimeExecutionAdapterArtifactStatus::Linked,
            source,
            reason_code: verified_reason_code.to_string(),
            install_hint: None,
            expected: Some(expected_artifact_contract()),
            manifest: Some(manifest_diagnostics(&manifest, &actual_sha256)?),
        },
    })
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
    let provenance = manifest
        .provenance
        .as_ref()
        .ok_or_else(|| "Bun/JSC adapter manifest provenance must be present".to_string())?;
    expect_non_empty("provenance.checksum_file", &provenance.checksum_file)?;
    expect_non_empty("provenance.sbom", &provenance.sbom)?;
    expect_non_empty("provenance.slsa", &provenance.slsa)?;
    if manifest.schema_version != BUN_JSC_ADAPTER_SCHEMA_VERSION {
        return Err(format!(
            "Bun/JSC adapter manifest schema_version must be {}, got {}",
            BUN_JSC_ADAPTER_SCHEMA_VERSION, manifest.schema_version
        ));
    }
    Ok(())
}

fn manifest_diagnostics(
    manifest: &BunJscAdapterManifest,
    library_sha256: &str,
) -> std::result::Result<RuntimeExecutionAdapterManifestArtifact, String> {
    let provenance = manifest
        .provenance
        .as_ref()
        .ok_or_else(|| "Bun/JSC adapter manifest provenance must be present".to_string())?;
    Ok(RuntimeExecutionAdapterManifestArtifact {
        adapter_version: manifest.adapter_version.clone(),
        nimbus_version: manifest.nimbus_version.clone(),
        source_repository: manifest.bun_source_repository.clone(),
        source_ref: manifest.bun_source_ref.clone(),
        source_revision: manifest.bun_source_revision.clone(),
        target_triple: manifest.target_triple.clone(),
        platform: manifest.platform.clone(),
        library_file: manifest.library.clone(),
        library_sha256: library_sha256.to_string(),
        abi_name: manifest.abi.name.clone(),
        abi_version: manifest.abi.version,
        checksum_file: provenance.checksum_file.clone(),
        sbom: provenance.sbom.clone(),
        slsa: provenance.slsa.clone(),
    })
}

pub(crate) fn load_error_diagnostics(
    mut diagnostics: RuntimeExecutionAdapterArtifactDiagnostics,
    reason_code: impl Into<String>,
    message: impl Into<String>,
) -> BunJscAdapterDiscoveryError {
    diagnostics.status = RuntimeExecutionAdapterArtifactStatus::LoadFailed;
    diagnostics.reason_code = reason_code.into();
    discovery_error_from_diagnostics(diagnostics, message)
}

fn discovery_error(
    source: RuntimeExecutionAdapterArtifactSource,
    status: RuntimeExecutionAdapterArtifactStatus,
    reason_code: impl Into<String>,
    message: impl Into<String>,
) -> BunJscAdapterDiscoveryError {
    discovery_error_from_diagnostics(
        RuntimeExecutionAdapterArtifactDiagnostics {
            status,
            source,
            reason_code: reason_code.into(),
            install_hint: Some(install_hint()),
            expected: Some(expected_artifact_contract()),
            manifest: None,
        },
        message,
    )
}

fn discovery_error_from_diagnostics(
    diagnostics: RuntimeExecutionAdapterArtifactDiagnostics,
    message: impl Into<String>,
) -> BunJscAdapterDiscoveryError {
    BunJscAdapterDiscoveryError {
        message: message.into(),
        diagnostics: Box::new(diagnostics),
    }
}

fn env_source(name: &str) -> RuntimeExecutionAdapterArtifactSource {
    if name == BUN_JSC_SHARED_LIBRARY_ENV {
        RuntimeExecutionAdapterArtifactSource::DevelopmentLibraryEnv
    } else {
        RuntimeExecutionAdapterArtifactSource::ManifestEnv
    }
}

fn classify_discovery_error(error: &str) -> RuntimeExecutionAdapterArtifactStatus {
    if error.contains("checksum mismatch") || error.contains("checksum file must contain") {
        RuntimeExecutionAdapterArtifactStatus::ChecksumMismatch
    } else if error.contains("target_triple")
        || error.contains("platform")
        || current_target_triple() == "unsupported"
    {
        RuntimeExecutionAdapterArtifactStatus::UnsupportedPlatform
    } else if error.contains("not a file")
        || error.contains("does not exist")
        || error.contains("failed to canonicalize")
    {
        RuntimeExecutionAdapterArtifactStatus::MissingArtifact
    } else {
        RuntimeExecutionAdapterArtifactStatus::InvalidManifest
    }
}

fn classify_discovery_reason_code(error: &str) -> &'static str {
    if error.contains("checksum mismatch") || error.contains("checksum file must contain") {
        "checksum_mismatch"
    } else if error.contains("target_triple")
        || error.contains("platform")
        || current_target_triple() == "unsupported"
    {
        "unsupported_platform"
    } else if error.contains("not a file")
        || error.contains("does not exist")
        || error.contains("failed to canonicalize")
    {
        "missing_artifact"
    } else {
        "invalid_manifest"
    }
}

fn validate_manifest_provenance_files(
    manifest: &BunJscAdapterManifest,
    manifest_dir: &Path,
    manifest_bytes: &[u8],
    library_path: &Path,
    library_sha256: &str,
    source: RuntimeExecutionAdapterArtifactSource,
) -> std::result::Result<(), String> {
    let provenance = manifest
        .provenance
        .as_ref()
        .ok_or_else(|| "Bun/JSC adapter manifest provenance must be present".to_string())?;
    let checksum_component =
        single_relative_component("provenance.checksum_file", &provenance.checksum_file)?;
    let checksum_path = manifest_dir.join(checksum_component);
    if !checksum_path.is_file() {
        return Err(format!(
            "Bun/JSC adapter manifest provenance.checksum_file {} does not exist beside the manifest",
            checksum_path.display()
        ));
    }
    let checksum_path = checksum_path.canonicalize().map_err(|error| {
        format!(
            "failed to canonicalize Bun/JSC adapter checksum file {}: {error}",
            checksum_path.display()
        )
    })?;
    if !checksum_path.starts_with(manifest_dir) {
        return Err(format!(
            "Bun/JSC adapter checksum file {} escapes manifest directory {}",
            checksum_path.display(),
            manifest_dir.display()
        ));
    }
    validate_adapter_path_safety(&checksum_path, "Bun/JSC adapter checksum file", source)?;
    let checksums = std::fs::read_to_string(&checksum_path).map_err(|error| {
        format!(
            "failed to read Bun/JSC adapter checksum file {}: {error}",
            checksum_path.display()
        )
    })?;

    verify_checksum_manifest_entry(&checksums, &manifest.library, library_sha256)?;
    verify_checksum_manifest_entry(
        &checksums,
        BUN_JSC_ADAPTER_MANIFEST_FILE,
        &compute_sha256_hex(manifest_bytes),
    )?;

    for (field, file_name) in [
        ("provenance.sbom", &provenance.sbom),
        ("provenance.slsa", &provenance.slsa),
    ] {
        if file_name == &manifest.library
            || file_name == BUN_JSC_ADAPTER_MANIFEST_FILE
            || file_name == BUN_JSC_ADAPTER_README_FILE
            || file_name == &provenance.checksum_file
        {
            return Err(format!(
                "Bun/JSC adapter manifest {field} must not collide with required archive file {file_name:?}"
            ));
        }
        let evidence_component = single_relative_component(field, file_name)?;
        let evidence_path = manifest_dir.join(evidence_component);
        if !evidence_path.is_file() {
            return Err(format!(
                "Bun/JSC adapter manifest {field} {} does not exist beside the manifest",
                evidence_path.display()
            ));
        }
        let evidence_path = evidence_path.canonicalize().map_err(|error| {
            format!(
                "failed to canonicalize Bun/JSC adapter {field} {}: {error}",
                evidence_path.display()
            )
        })?;
        if !evidence_path.starts_with(manifest_dir) {
            return Err(format!(
                "Bun/JSC adapter {field} {} escapes manifest directory {}",
                evidence_path.display(),
                manifest_dir.display()
            ));
        }
        validate_adapter_path_safety(&evidence_path, "Bun/JSC adapter provenance file", source)?;
        verify_checksum_manifest_entry(
            &checksums,
            file_name,
            &compute_sha256_for_path(&evidence_path)?,
        )?;
    }

    if !library_path.starts_with(manifest_dir) {
        return Err(format!(
            "Bun/JSC adapter library {} escapes manifest directory {}",
            library_path.display(),
            manifest_dir.display()
        ));
    }

    Ok(())
}

fn verify_checksum_manifest_entry(
    checksums: &str,
    file_name: &str,
    expected_sha256: &str,
) -> std::result::Result<(), String> {
    for line in checksums.lines() {
        let mut parts = line.split_whitespace();
        let Some(digest) = parts.next() else {
            continue;
        };
        let Some(subject) = parts.next() else {
            continue;
        };
        if subject == file_name && digest.eq_ignore_ascii_case(expected_sha256) {
            return Ok(());
        }
    }
    Err(format!(
        "Bun/JSC adapter checksum file must contain SHA-256 {expected_sha256} for {file_name}"
    ))
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

fn single_relative_component<'a>(
    field: &str,
    value: &'a str,
) -> std::result::Result<&'a Path, String> {
    let path = Path::new(value);
    let mut components = path.components();
    match (components.next(), components.next()) {
        (Some(Component::Normal(_)), None) => Ok(path),
        _ => Err(format!(
            "Bun/JSC adapter manifest {field} must be a single relative filename, got {value:?}"
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

#[cfg(unix)]
fn validate_adapter_path_safety(
    path: &Path,
    label: &str,
    source: RuntimeExecutionAdapterArtifactSource,
) -> std::result::Result<(), String> {
    validate_unix_mode(path, label)?;
    if source == RuntimeExecutionAdapterArtifactSource::PackagedManifest {
        validate_packaged_manifest_trust_chain(path, label)?;
    }
    Ok(())
}

#[cfg(unix)]
fn validate_unix_mode(path: &Path, label: &str) -> std::result::Result<(), String> {
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
fn validate_adapter_path_safety(
    _path: &Path,
    _label: &str,
    _source: RuntimeExecutionAdapterArtifactSource,
) -> std::result::Result<(), String> {
    Ok(())
}

#[cfg(all(unix, target_os = "linux"))]
fn validate_packaged_manifest_trust_chain(
    path: &Path,
    label: &str,
) -> std::result::Result<(), String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    for ancestor in packaged_manifest_trust_chain(path)? {
        let metadata = std::fs::symlink_metadata(&ancestor).map_err(|error| {
            format!(
                "failed to read packaged Bun/JSC adapter path metadata for {}: {error}",
                ancestor.display()
            )
        })?;
        if metadata.uid() != 0 {
            return Err(format!(
                "{label} {} is under non-root-owned packaged path {}; Linux packaged Bun/JSC adapter paths must be root-owned",
                path.display(),
                ancestor.display()
            ));
        }
        if !metadata.file_type().is_symlink() && metadata.permissions().mode() & 0o022 != 0 {
            return Err(format!(
                "{label} {} is under unsafe packaged path {} with permissions {:o}; group/other writable packaged Bun/JSC adapter paths are rejected",
                path.display(),
                ancestor.display(),
                metadata.permissions().mode() & 0o777
            ));
        }
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
fn validate_packaged_manifest_trust_chain(
    path: &Path,
    label: &str,
) -> std::result::Result<(), String> {
    use std::os::unix::fs::PermissionsExt;

    for ancestor in packaged_manifest_trust_chain(path)? {
        let metadata = std::fs::symlink_metadata(&ancestor).map_err(|error| {
            format!(
                "failed to read packaged Bun/JSC adapter path metadata for {}: {error}",
                ancestor.display()
            )
        })?;
        if !metadata.file_type().is_symlink() && metadata.permissions().mode() & 0o022 != 0 {
            return Err(format!(
                "{label} {} is under unsafe packaged path {} with permissions {:o}; group/other writable packaged Bun/JSC adapter paths are rejected",
                path.display(),
                ancestor.display(),
                metadata.permissions().mode() & 0o777
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn packaged_manifest_trust_chain(path: &Path) -> std::result::Result<Vec<PathBuf>, String> {
    let mut ancestors: Vec<PathBuf> = path.ancestors().map(Path::to_path_buf).collect();
    ancestors.reverse();
    if ancestors.is_empty() {
        return Err(format!(
            "Bun/JSC packaged adapter path {} has no ancestor chain",
            path.display()
        ));
    }
    Ok(ancestors)
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::backends::bun_jsc::contract::shared_library_basename;

    fn write_stub_library(dir: &Path) -> (PathBuf, String) {
        let library_path = dir.join(shared_library_basename());
        std::fs::write(&library_path, b"stub Bun/JSC shared adapter")
            .expect("stub library should be written");
        set_safe_file_permissions(&library_path, 0o755);
        let sha256 = compute_sha256_for_path(&library_path).expect("stub sha256 should compute");
        (library_path, sha256)
    }

    fn manifest_json(library_sha256: &str) -> Value {
        let contract = BUN_JSC_LINKED_ADAPTER_SOURCE_CONTRACT;
        json!({
            "schema_version": 1,
            "kind": "nimbus.bun_jsc.adapter",
            "adapter_version": "v0.1.0-bun-proof-main-20260525",
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

    fn secure_tempdir() -> tempfile::TempDir {
        let temp_dir = tempfile::tempdir().expect("temp dir should be created");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(temp_dir.path())
                .expect("temp dir metadata should load")
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(temp_dir.path(), permissions)
                .expect("temp dir permissions should be tightened");
        }
        temp_dir
    }

    fn set_safe_file_permissions(path: &Path, mode: u32) {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(path)
                .expect("fixture metadata should load")
                .permissions();
            permissions.set_mode(mode);
            std::fs::set_permissions(path, permissions)
                .expect("fixture permissions should be tightened");
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            let _ = mode;
        }
    }

    fn write_manifest(dir: &Path, manifest: &Value) -> PathBuf {
        let manifest_path = dir.join(BUN_JSC_ADAPTER_MANIFEST_FILE);
        std::fs::write(
            &manifest_path,
            serde_json::to_vec_pretty(manifest).expect("manifest should serialize"),
        )
        .expect("manifest should be written");
        set_safe_file_permissions(&manifest_path, 0o644);
        if let Some(provenance) = manifest.get("provenance").and_then(Value::as_object) {
            for field in ["sbom", "slsa"] {
                if let Some(file_name) = provenance.get(field).and_then(Value::as_str)
                    && !file_name.contains('/')
                    && !file_name.contains("..")
                {
                    let contents = match field {
                        "sbom" => br#"{"bomFormat":"CycloneDX","components":[]}"#.as_slice(),
                        "slsa" => br#"{"_type":"https://in-toto.io/Statement/v1","predicateType":"https://slsa.dev/provenance/v1"}"#.as_slice(),
                        _ => unreachable!(),
                    };
                    let evidence_path = dir.join(file_name);
                    std::fs::write(&evidence_path, contents)
                        .expect("provenance evidence should be written");
                    set_safe_file_permissions(&evidence_path, 0o644);
                }
            }
            if let Some(checksum_file) = provenance.get("checksum_file").and_then(Value::as_str)
                && !checksum_file.contains('/')
                && !checksum_file.contains("..")
            {
                let mut checksums = String::new();
                for file_name in [
                    shared_library_basename(),
                    BUN_JSC_ADAPTER_MANIFEST_FILE,
                    provenance.get("sbom").and_then(Value::as_str).unwrap_or(""),
                    provenance.get("slsa").and_then(Value::as_str).unwrap_or(""),
                ] {
                    if file_name.is_empty() {
                        continue;
                    }
                    let path = dir.join(file_name);
                    if path.is_file() {
                        let sha256 = compute_sha256_for_path(&path)
                            .expect("checksum should compute for manifest fixture");
                        checksums.push_str(&format!("{sha256}  {file_name}\n"));
                    }
                }
                let checksum_path = dir.join(checksum_file);
                std::fs::write(&checksum_path, checksums)
                    .expect("checksum manifest should be written");
                set_safe_file_permissions(&checksum_path, 0o644);
            }
        }
        manifest_path
    }

    #[test]
    fn valid_packaged_manifest_resolves_canonical_library_path() {
        let temp_dir = secure_tempdir();
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
        let temp_dir = secure_tempdir();
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
        let temp_dir = secure_tempdir();
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
    fn manifest_override_reports_sanitized_artifact_diagnostics() {
        let temp_dir = secure_tempdir();
        let (library_path, sha256) = write_stub_library(temp_dir.path());
        let manifest_path = write_manifest(temp_dir.path(), &manifest_json(&sha256));

        let resolved = resolve_shared_adapter_library_from_values(
            None,
            Some(manifest_path.into_os_string()),
            &[],
        )
        .expect("manifest override should resolve with diagnostics");

        assert_eq!(
            resolved.path,
            library_path
                .canonicalize()
                .expect("stub library should canonicalize")
        );
        assert_eq!(
            resolved.diagnostics.status,
            RuntimeExecutionAdapterArtifactStatus::Linked
        );
        assert_eq!(
            resolved.diagnostics.source,
            RuntimeExecutionAdapterArtifactSource::ManifestEnv
        );
        assert_eq!(resolved.diagnostics.reason_code, "manifest_env_verified");
        let manifest = resolved
            .diagnostics
            .manifest
            .as_ref()
            .expect("verified manifest metadata should be exposed");
        assert_eq!(manifest.source_ref, "nimbus-bun-jsc-proof-main-20260708");
        assert_eq!(manifest.library_file, shared_library_basename());
        assert!(
            !serde_json::to_string(&resolved.diagnostics)
                .expect("diagnostics should serialize")
                .contains(temp_dir.path().to_string_lossy().as_ref()),
            "serialized diagnostics must not expose absolute temp paths"
        );
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn discovery_uses_first_existing_packaged_manifest() {
        let temp_dir = secure_tempdir();
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

    #[cfg(target_os = "linux")]
    #[test]
    fn packaged_manifest_from_non_root_test_root_is_rejected() {
        let temp_dir = secure_tempdir();
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let manifest_path = write_manifest(temp_dir.path(), &manifest_json(&sha256));

        let error = resolve_shared_adapter_library_from_values(None, None, &[manifest_path])
            .expect_err("Linux packaged manifests must come from root-owned package paths");

        assert!(
            error
                .message()
                .contains("Linux packaged Bun/JSC adapter paths must be root-owned")
                || error.message().contains("unsafe packaged path"),
            "unexpected error: {error}"
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
    fn discovery_without_env_or_package_reports_missing_artifact_diagnostics() {
        let error = resolve_shared_adapter_library_from_values(None, None, &[])
            .expect_err("missing adapter should return diagnostics");

        let diagnostics = error.diagnostics();
        assert_eq!(
            diagnostics.status,
            RuntimeExecutionAdapterArtifactStatus::MissingArtifact
        );
        assert_eq!(
            diagnostics.source,
            RuntimeExecutionAdapterArtifactSource::NotFound
        );
        assert_eq!(diagnostics.reason_code, "no_adapter_artifact_configured");
        assert!(diagnostics.manifest.is_none());
        assert_eq!(
            diagnostics
                .expected
                .expect("expected contract should be present")
                .source_ref,
            "nimbus-bun-jsc-proof-main-20260708"
        );
    }

    #[test]
    fn manifest_rejects_wrong_bun_revision() {
        let temp_dir = secure_tempdir();
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
        let temp_dir = secure_tempdir();
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
        let temp_dir = secure_tempdir();
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
        let temp_dir = secure_tempdir();
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
    fn manifest_env_checksum_mismatch_reports_sanitized_artifact_diagnostics() {
        let temp_dir = secure_tempdir();
        let (_library_path, _sha256) = write_stub_library(temp_dir.path());
        let manifest_path = write_manifest(temp_dir.path(), &manifest_json(&"0".repeat(64)));

        let error = resolve_shared_adapter_library_from_values(
            None,
            Some(manifest_path.into_os_string()),
            &[],
        )
        .expect_err("bad manifest override should return diagnostics");

        let diagnostics = error.diagnostics();
        assert_eq!(
            diagnostics.status,
            RuntimeExecutionAdapterArtifactStatus::ChecksumMismatch
        );
        assert_eq!(
            diagnostics.source,
            RuntimeExecutionAdapterArtifactSource::ManifestEnv
        );
        assert_eq!(diagnostics.reason_code, "checksum_mismatch");
        assert!(diagnostics.manifest.is_none());
        assert!(
            !serde_json::to_string(&diagnostics)
                .expect("diagnostics should serialize")
                .contains(temp_dir.path().to_string_lossy().as_ref()),
            "serialized diagnostics must not expose absolute temp paths"
        );
    }

    #[test]
    fn manifest_requires_provenance_evidence() {
        let temp_dir = secure_tempdir();
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let mut manifest = manifest_json(&sha256);
        manifest
            .as_object_mut()
            .expect("manifest should be object")
            .remove("provenance");
        let manifest_path = write_manifest(temp_dir.path(), &manifest);

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("missing provenance evidence should be rejected");

        assert!(
            error.contains("provenance must be present"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_rejects_missing_provenance_file() {
        let temp_dir = secure_tempdir();
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let manifest = manifest_json(&sha256);
        let manifest_path = write_manifest(temp_dir.path(), &manifest);
        std::fs::remove_file(temp_dir.path().join("nimbus-bun-jsc-adapter.sbom.cdx.json"))
            .expect("fixture SBOM should be removed");

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("missing provenance file should be rejected");

        assert!(
            error.contains("provenance.sbom") && error.contains("does not exist"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_rejects_provenance_checksum_mismatch() {
        let temp_dir = secure_tempdir();
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let manifest = manifest_json(&sha256);
        let manifest_path = write_manifest(temp_dir.path(), &manifest);
        std::fs::write(
            temp_dir.path().join("nimbus-bun-jsc-adapter.intoto.jsonl"),
            b"tampered",
        )
        .expect("fixture provenance should be tampered");

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("provenance checksum mismatch should be rejected");

        assert!(
            error.contains("checksum file must contain SHA-256")
                && error.contains("nimbus-bun-jsc-adapter.intoto.jsonl"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_rejects_unsafe_provenance_paths() {
        let temp_dir = secure_tempdir();
        let (_library_path, sha256) = write_stub_library(temp_dir.path());
        let mut manifest = manifest_json(&sha256);
        manifest["provenance"]["slsa"] = json!("../adapter.intoto.jsonl");
        let manifest_path = write_manifest(temp_dir.path(), &manifest);

        let error = validate_adapter_manifest_path(&manifest_path)
            .expect_err("unsafe provenance path should be rejected");

        assert!(
            error.contains("provenance.slsa") && error.contains("single relative filename"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn manifest_rejects_unsupported_memory_and_lifecycle_policy() {
        let temp_dir = secure_tempdir();
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
        let temp_dir = secure_tempdir();
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
        let temp_dir = secure_tempdir();
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

        let temp_dir = secure_tempdir();
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
