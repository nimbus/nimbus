use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::backends::v8::V8RuntimeConstructionMode;
use crate::backends::v8::embedder::ModuleSpecifier;
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::{
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeBundleContentKind, RuntimeCompatibilityTarget,
    RuntimeExecutionModel, RuntimeJavaScriptEvaluationFormat, RuntimeLanguage, RuntimeLimits,
    RuntimeMemoryEnforcement, RuntimeMode, RuntimeNodeFullRealmReusePolicy, RuntimePoolKind,
    RuntimePreset, RuntimeProfile, RuntimeRoutingAffinity,
};
use crate::module_loader::BundleModuleCodeCache;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeBundleIdentity {
    tenant_label: Option<String>,
    content_kind: RuntimeBundleContentKind,
    entrypoint_kind: RuntimeBundleEntrypointKind,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RuntimeBundleEntrypointKind {
    Main,
    Side,
}

#[derive(Debug)]
struct RuntimeBundleShared {
    content_kind: RuntimeBundleContentKind,
    entrypoint_kind: RuntimeBundleEntrypointKind,
    entrypoint: PathBuf,
    canonical_entrypoint: Option<PathBuf>,
    canonical_module_root: Option<PathBuf>,
    module_specifier: std::result::Result<ModuleSpecifier, String>,
    expected_sha256: Option<String>,
    identity: RuntimeBundleIdentity,
    module_code_caches: Mutex<HashMap<RuntimeBundleEngineCacheKey, Arc<BundleModuleCodeCache>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RuntimeBundleEngineCacheKey {
    backend_kind: RuntimeBackendKind,
    backend_trust_tier: RuntimeBackendTrustTier,
    backend_lockdown_profile: RuntimeBackendLockdownProfile,
    backend_lifecycle_policy: RuntimeBackendLifecyclePolicy,
    content_kind: RuntimeBundleContentKind,
    javascript_evaluation_format: RuntimeJavaScriptEvaluationFormat,
    compatibility_target: RuntimeCompatibilityTarget,
    runtime_profile: Option<RuntimeProfile>,
    node_conditions: Vec<String>,
    execution_model: RuntimeExecutionModel,
    mode: RuntimeMode,
    language: RuntimeLanguage,
    preset: RuntimePreset,
    runtime_pool_kind: RuntimePoolKind,
    node_full_realm_reuse_policy: RuntimeNodeFullRealmReusePolicy,
    memory_enforcement: RuntimeMemoryEnforcement,
    routing_affinity: RuntimeRoutingAffinity,
    max_heap_mb: usize,
    initial_heap_mb: usize,
    execution_timeout: std::time::Duration,
    system_timeout: std::time::Duration,
    max_nested_runtime_invocations: usize,
    construction_mode: V8RuntimeConstructionMode,
    service_extension_enabled: bool,
    read_grants: Vec<String>,
    write_grants: Vec<String>,
    net_connect_grants: Vec<String>,
    net_listen_grants: Vec<String>,
    env_read_grants: Vec<String>,
    env_write_grants: Vec<String>,
    secret_grants: Vec<String>,
    identity_grants: Vec<String>,
    exact_service_grants: Vec<String>,
    run_grants: Vec<String>,
    sys_grants: Vec<String>,
    ffi_grants: Vec<String>,
    worker_grants: Vec<String>,
    tool_grants: Vec<String>,
}

impl RuntimeBundleEngineCacheKey {
    fn for_limits(limits: &RuntimeLimits, construction_mode: V8RuntimeConstructionMode) -> Self {
        let service_extension_enabled =
            limits.service_capability_enabled && limits.grants.has_service_grants();
        Self {
            backend_kind: limits.backend_kind,
            backend_trust_tier: limits.backend_trust_tier,
            backend_lockdown_profile: limits.backend_lockdown_profile,
            backend_lifecycle_policy: limits.backend_lifecycle_policy,
            content_kind: limits.bundle_content_kind,
            javascript_evaluation_format: limits.javascript_evaluation_format,
            compatibility_target: limits.compatibility_target,
            runtime_profile: RuntimeProfile::for_limits(limits),
            node_conditions: limits.node_conditions.clone(),
            execution_model: limits.execution_model,
            mode: limits.mode,
            language: limits.language,
            preset: limits.preset,
            runtime_pool_kind: limits.runtime_pool_kind,
            node_full_realm_reuse_policy: limits.node_full_realm_reuse_policy,
            memory_enforcement: limits.memory_enforcement,
            routing_affinity: limits.routing_affinity,
            max_heap_mb: limits.max_heap_mb,
            initial_heap_mb: limits.initial_heap_mb,
            execution_timeout: limits.execution_timeout,
            system_timeout: limits.system_timeout,
            max_nested_runtime_invocations: limits.max_nested_runtime_invocations,
            construction_mode,
            service_extension_enabled,
            read_grants: sorted_deduped(&limits.grants.read),
            write_grants: sorted_deduped(&limits.grants.write),
            net_connect_grants: sorted_deduped(&limits.grants.net_connect),
            net_listen_grants: sorted_deduped(&limits.grants.net_listen),
            env_read_grants: sorted_deduped(&limits.grants.env_read),
            env_write_grants: sorted_deduped(&limits.grants.env_write),
            secret_grants: sorted_deduped(&limits.grants.secret),
            identity_grants: sorted_deduped(&limits.grants.identity),
            exact_service_grants: if service_extension_enabled {
                limits.grants.sorted_service_grants()
            } else {
                Vec::new()
            },
            run_grants: sorted_deduped(&limits.grants.run),
            sys_grants: sorted_deduped(&limits.grants.sys),
            ffi_grants: sorted_deduped(&limits.grants.ffi),
            worker_grants: sorted_deduped(&limits.grants.worker),
            tool_grants: sorted_deduped(&limits.grants.tool),
        }
    }
}

fn sorted_deduped(values: &[String]) -> Vec<String> {
    let mut values = values.to_vec();
    values.sort();
    values.dedup();
    values
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
            RuntimeBundleEntrypointKind::Main,
            None,
            None,
            None,
        )
    }

    /// A path-shaped bundle for the startup RO-heap anchor. The anchor CONSTRUCTS a NodeFull
    /// isolate (to install the shared cage read-only heap) but NEVER evaluates its
    /// entrypoint, and construction only touches the entrypoint's parent directory, not the
    /// file. So no file is written: the entrypoint lives under the system temp dir (which
    /// exists on every platform) but is never read. This keeps the anchor off the
    /// startup-path dependency on a *writable* filesystem.
    pub(crate) fn virtual_anchor() -> Self {
        Self::new(std::env::temp_dir().join("nimbus-nodefull-anchor.virtual.mjs"))
    }

    pub fn with_expected_sha256(
        entrypoint: impl AsRef<Path>,
        expected_sha256: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            RuntimeBundleContentKind::JavaScript,
            RuntimeBundleEntrypointKind::Main,
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
            RuntimeBundleEntrypointKind::Main,
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
            RuntimeBundleEntrypointKind::Main,
            None,
            None,
            Some(module_root.as_ref().to_path_buf()),
        )
    }

    pub(crate) fn with_side_entrypoint_and_module_root(
        entrypoint: impl AsRef<Path>,
        module_root: impl AsRef<Path>,
    ) -> Self {
        Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            RuntimeBundleContentKind::JavaScript,
            RuntimeBundleEntrypointKind::Side,
            None,
            None,
            Some(module_root.as_ref().to_path_buf()),
        )
    }

    #[cfg(test)]
    pub(crate) fn with_side_entrypoint(entrypoint: impl AsRef<Path>) -> Self {
        Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            RuntimeBundleContentKind::JavaScript,
            RuntimeBundleEntrypointKind::Side,
            None,
            None,
            None,
        )
    }

    pub fn entrypoint(&self) -> &Path {
        &self.shared.entrypoint
    }

    pub fn content_kind(&self) -> RuntimeBundleContentKind {
        self.shared.content_kind
    }

    pub(crate) fn entrypoint_kind(&self) -> RuntimeBundleEntrypointKind {
        self.shared.entrypoint_kind
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
        entrypoint_kind: RuntimeBundleEntrypointKind,
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
            entrypoint_kind,
            entrypoint: canonical_entrypoint
                .clone()
                .unwrap_or_else(|| entrypoint.clone()),
            expected_sha256: expected_sha256.clone(),
        };
        Self {
            shared: Arc::new(RuntimeBundleShared {
                content_kind,
                entrypoint_kind,
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

    pub(crate) fn module_code_cache(
        &self,
        limits: &RuntimeLimits,
        construction_mode: V8RuntimeConstructionMode,
    ) -> Arc<BundleModuleCodeCache> {
        let key = RuntimeBundleEngineCacheKey::for_limits(limits, construction_mode);
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
