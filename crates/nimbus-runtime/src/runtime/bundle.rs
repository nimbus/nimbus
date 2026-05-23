use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::backends::v8::embedder::ModuleSpecifier;
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::{
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeBundleContentKind, RuntimeCompatibilityTarget,
    RuntimeJavaScriptEvaluationFormat, RuntimeLimits,
};
use crate::module_loader::BundleModuleCodeCache;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeBundleIdentity {
    tenant_label: Option<String>,
    content_kind: RuntimeBundleContentKind,
    entrypoint: PathBuf,
    expected_sha256: Option<String>,
}

impl RuntimeBundleIdentity {
    pub fn tenant_label(&self) -> Option<&str> {
        self.tenant_label.as_deref()
    }

    pub fn content_kind(&self) -> RuntimeBundleContentKind {
        self.content_kind
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
    content_kind: RuntimeBundleContentKind,
    entrypoint: PathBuf,
    canonical_entrypoint: Option<PathBuf>,
    canonical_module_root: Option<PathBuf>,
    module_specifier: std::result::Result<ModuleSpecifier, String>,
    expected_sha256: Option<String>,
    identity: RuntimeBundleIdentity,
    module_code_caches: Mutex<HashMap<RuntimeBundleEngineCacheKey, Arc<BundleModuleCodeCache>>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct RuntimeBundleEngineCacheKey {
    backend_kind: RuntimeBackendKind,
    backend_trust_tier: RuntimeBackendTrustTier,
    backend_lockdown_profile: RuntimeBackendLockdownProfile,
    backend_lifecycle_policy: RuntimeBackendLifecyclePolicy,
    content_kind: RuntimeBundleContentKind,
    javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat,
    compatibility_target: RuntimeCompatibilityTarget,
}

impl RuntimeBundleEngineCacheKey {
    fn for_limits(limits: &RuntimeLimits) -> Self {
        Self {
            backend_kind: limits.backend_kind,
            backend_trust_tier: limits.backend_trust_tier,
            backend_lockdown_profile: limits.backend_lockdown_profile,
            backend_lifecycle_policy: limits.backend_lifecycle_policy,
            content_kind: limits.bundle_content_kind,
            javascript_evaluation_format: limits.javascript_evaluation_format,
            compatibility_target: limits.compatibility_target,
        }
    }
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
        Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            RuntimeBundleContentKind::JavaScript,
            None,
            None,
            None,
        )
    }

    pub fn with_expected_sha256(
        entrypoint: impl AsRef<Path>,
        expected_sha256: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            RuntimeBundleContentKind::JavaScript,
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
            RuntimeBundleContentKind::JavaScript,
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
            RuntimeBundleContentKind::JavaScript,
            None,
            None,
            Some(module_root.as_ref().to_path_buf()),
        )
    }

    pub fn entrypoint(&self) -> &Path {
        &self.shared.entrypoint
    }

    pub fn content_kind(&self) -> RuntimeBundleContentKind {
        self.shared.content_kind
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
        content_kind: RuntimeBundleContentKind,
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
            content_kind,
            entrypoint: canonical_entrypoint
                .clone()
                .unwrap_or_else(|| entrypoint.clone()),
            expected_sha256: expected_sha256.clone(),
        };
        Self {
            shared: Arc::new(RuntimeBundleShared {
                content_kind,
                entrypoint,
                canonical_entrypoint,
                canonical_module_root,
                module_specifier,
                expected_sha256,
                identity,
                module_code_caches: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub(crate) fn module_code_cache(&self, limits: &RuntimeLimits) -> Arc<BundleModuleCodeCache> {
        let key = RuntimeBundleEngineCacheKey::for_limits(limits);
        let mut caches = self
            .shared
            .module_code_caches
            .lock()
            .expect("bundle module code cache lock should not be poisoned");
        caches
            .entry(key)
            .or_insert_with(|| Arc::new(BundleModuleCodeCache::new()))
            .clone()
    }

    #[cfg(test)]
    pub(crate) fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.shared, &other.shared)
    }

    #[cfg(test)]
    pub(crate) fn module_code_cache_entry_count(&self) -> usize {
        self.shared
            .module_code_caches
            .lock()
            .expect("bundle module code cache lock should not be poisoned")
            .values()
            .map(|cache| cache.entry_count())
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn module_code_cache_write_count(&self) -> usize {
        self.shared
            .module_code_caches
            .lock()
            .expect("bundle module code cache lock should not be poisoned")
            .values()
            .map(|cache| cache.write_count())
            .sum()
    }

    #[cfg(test)]
    pub(crate) fn module_code_cache_partition_count(&self) -> usize {
        self.shared
            .module_code_caches
            .lock()
            .expect("bundle module code cache lock should not be poisoned")
            .len()
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
