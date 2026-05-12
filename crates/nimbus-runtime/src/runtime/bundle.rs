use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};

use crate::backends::v8::embedder::ModuleSpecifier;
use crate::error::{NimbusRuntimeError, Result};
use crate::module_loader::BundleModuleCodeCache;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeBundleIdentity {
    tenant_label: Option<String>,
    entrypoint: PathBuf,
    expected_sha256: Option<String>,
}

impl RuntimeBundleIdentity {
    pub fn tenant_label(&self) -> Option<&str> {
        self.tenant_label.as_deref()
    }

    pub fn entrypoint(&self) -> &Path {
        &self.entrypoint
    }

    pub fn expected_sha256(&self) -> Option<&str> {
        self.expected_sha256.as_deref()
    }
}

#[derive(Debug)]
struct RuntimeBundleShared {
    entrypoint: PathBuf,
    canonical_entrypoint: Option<PathBuf>,
    canonical_module_root: Option<PathBuf>,
    module_specifier: std::result::Result<ModuleSpecifier, String>,
    expected_sha256: Option<String>,
    identity: RuntimeBundleIdentity,
    module_code_cache: Arc<BundleModuleCodeCache>,
}

#[derive(Debug, Clone)]
pub struct RuntimeBundle {
    shared: Arc<RuntimeBundleShared>,
}

impl PartialEq for RuntimeBundle {
    fn eq(&self, other: &Self) -> bool {
        self.shared.identity == other.shared.identity
    }
}

impl Eq for RuntimeBundle {}

impl RuntimeBundle {
    pub fn new(entrypoint: impl AsRef<Path>) -> Self {
        Self::from_parts(entrypoint.as_ref().to_path_buf(), None, None, None)
    }

    pub fn with_expected_sha256(
        entrypoint: impl AsRef<Path>,
        expected_sha256: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            Some(normalize_sha256(expected_sha256.as_ref())?),
            None,
            None,
        ))
    }

    pub fn for_tenant(
        entrypoint: impl AsRef<Path>,
        expected_sha256: impl AsRef<str>,
        tenant_label: impl Into<String>,
    ) -> Result<Self> {
        Ok(Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            Some(normalize_sha256(expected_sha256.as_ref())?),
            Some(tenant_label.into()),
            None,
        ))
    }

    pub(crate) fn with_module_root(
        entrypoint: impl AsRef<Path>,
        module_root: impl AsRef<Path>,
    ) -> Self {
        Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            None,
            None,
            Some(module_root.as_ref().to_path_buf()),
        )
    }

    pub fn entrypoint(&self) -> &Path {
        &self.shared.entrypoint
    }

    pub fn canonical_entrypoint(&self) -> Option<&Path> {
        self.shared.canonical_entrypoint.as_deref()
    }

    pub fn identity(&self) -> &RuntimeBundleIdentity {
        &self.shared.identity
    }

    pub fn compute_sha256_for_path(path: impl AsRef<Path>) -> Result<String> {
        let bytes = std::fs::read(path)?;
        Ok(compute_sha256_hex(&bytes))
    }

    pub(crate) fn module_specifier(&self) -> Result<ModuleSpecifier> {
        self.shared
            .module_specifier
            .clone()
            .map_err(NimbusRuntimeError::Contract)
    }

    pub(crate) fn module_root(&self) -> Result<PathBuf> {
        if let Some(root) = &self.shared.canonical_module_root {
            return Ok(root.clone());
        }
        self.entrypoint()
            .parent()
            .ok_or_else(|| {
                NimbusRuntimeError::Contract(format!(
                    "bundle entrypoint does not have a parent directory: {}",
                    self.entrypoint().display()
                ))
            })?
            .canonicalize()
            .map_err(NimbusRuntimeError::from)
    }

    pub(crate) fn verify_integrity(&self) -> Result<()> {
        // Stable bundle identity is only for pooling, metrics, and provenance bookkeeping.
        // Path-backed bundles remain mutable, so every invocation must re-hash bundle contents.
        let Some(expected_sha256) = &self.shared.expected_sha256 else {
            return Ok(());
        };
        let actual_sha256 = Self::compute_sha256_for_path(self.entrypoint())?;
        if &actual_sha256 == expected_sha256 {
            return Ok(());
        }
        Err(NimbusRuntimeError::BundleIntegrityMismatch(format!(
            "{} (expected {}, got {})",
            self.entrypoint().display(),
            expected_sha256,
            actual_sha256
        )))
    }

    fn from_parts(
        entrypoint: PathBuf,
        expected_sha256: Option<String>,
        tenant_label: Option<String>,
        explicit_module_root: Option<PathBuf>,
    ) -> Self {
        let canonical_entrypoint = entrypoint.canonicalize().ok();
        let module_specifier_path = canonical_entrypoint
            .clone()
            .unwrap_or_else(|| entrypoint.clone());
        let module_specifier =
            ModuleSpecifier::from_file_path(&module_specifier_path).map_err(|_| {
                format!(
                    "bundle entrypoint is not a valid file URL: {}",
                    entrypoint.display()
                )
            });
        let canonical_module_root = explicit_module_root
            .map(|path| path.canonicalize().unwrap_or(path))
            .or_else(|| {
                canonical_entrypoint
                    .as_ref()
                    .and_then(|path| path.parent().map(Path::to_path_buf))
                    .or_else(|| {
                        entrypoint
                            .parent()
                            .and_then(|path| path.canonicalize().ok())
                    })
            });
        let identity = RuntimeBundleIdentity {
            tenant_label,
            entrypoint: canonical_entrypoint
                .clone()
                .unwrap_or_else(|| entrypoint.clone()),
            expected_sha256: expected_sha256.clone(),
        };
        Self {
            shared: Arc::new(RuntimeBundleShared {
                entrypoint,
                canonical_entrypoint,
                canonical_module_root,
                module_specifier,
                expected_sha256,
                identity,
                module_code_cache: Arc::new(BundleModuleCodeCache::new()),
            }),
        }
    }

    pub(crate) fn module_code_cache(&self) -> Arc<BundleModuleCodeCache> {
        self.shared.module_code_cache.clone()
    }

    #[cfg(test)]
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    #[cfg(test)]
    pub(crate) fn module_code_cache_entry_count(&self) -> usize {
        self.shared.module_code_cache.entry_count()
    }

    #[cfg(test)]
    pub(crate) fn module_code_cache_write_count(&self) -> usize {
        self.shared.module_code_cache.write_count()
    }
}

fn normalize_sha256(value: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.len() != 64 || !trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(NimbusRuntimeError::Contract(format!(
            "bundle sha256 must be a 64-character hex string, got {trimmed:?}"
        )));
    }
    Ok(trimmed.to_ascii_lowercase())
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
