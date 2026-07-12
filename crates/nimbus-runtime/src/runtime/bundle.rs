use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backends::v8::V8RuntimeConstructionMode;
use crate::backends::v8::embedder::ModuleSpecifier;
use crate::error::{NimbusRuntimeError, Result};
use crate::limits::{
    RuntimeBackendKind, RuntimeBackendLifecyclePolicy, RuntimeBackendLockdownProfile,
    RuntimeBackendTrustTier, RuntimeBundleContentKind, RuntimeCompatibilityTarget,
    RuntimeExecutionModel, RuntimeGuestSemantics, RuntimeJavaScriptEvaluationFormat,
    RuntimeLanguage, RuntimeLimits, RuntimeMemoryEnforcement, RuntimeMode,
    RuntimeNodeFullRealmReusePolicy, RuntimePoolKind, RuntimePreset, RuntimeProfile,
    RuntimeRoutingAffinity,
};
use crate::module_loader::BundleModuleCodeCache;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeComponentWorld {
    #[default]
    NimbusFunction,
    NimbusAgent,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeBundleWasmComponentContent {
    target_world: RuntimeComponentWorld,
    precompiled_sha256: Option<String>,
}

impl RuntimeBundleWasmComponentContent {
    pub fn target_world(&self) -> RuntimeComponentWorld {
        self.target_world
    }

    pub fn precompiled_sha256(&self) -> Option<&str> {
        self.precompiled_sha256.as_deref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RuntimeBundleContent {
    JavaScript,
    WasmComponent(RuntimeBundleWasmComponentContent),
}

impl RuntimeBundleContent {
    pub fn content_kind(&self) -> RuntimeBundleContentKind {
        match self {
            Self::JavaScript => RuntimeBundleContentKind::JavaScript,
            Self::WasmComponent(_) => RuntimeBundleContentKind::WasmComponent,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeBundleIdentity {
    tenant_label: Option<String>,
    content_kind: RuntimeBundleContentKind,
    target_world: Option<RuntimeComponentWorld>,
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

    pub fn target_world(&self) -> Option<RuntimeComponentWorld> {
        self.target_world
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
    content: RuntimeBundleContent,
    entrypoint_kind: RuntimeBundleEntrypointKind,
    entrypoint: PathBuf,
    canonical_entrypoint: Option<PathBuf>,
    canonical_module_root: Option<PathBuf>,
    module_specifier: std::result::Result<ModuleSpecifier, String>,
    expected_sha256: Option<String>,
    identity: RuntimeBundleIdentity,
    module_code_caches: Mutex<HashMap<RuntimeBundleEngineCacheKey, Arc<BundleModuleCodeCache>>>,
    // A genuine per-deploy nonce, set once by the loader at bundle
    // registration (see `with_deploy_nonce`). Mixed into the import-time seed
    // so two distinct deploys reseed differently even when their content and
    // entrypoint mtime are byte-identical. Absent for hand-rolled/legacy
    // bundles, which keep the content+mtime seed.
    deploy_nonce: std::sync::OnceLock<String>,
    deploy_stamp: std::sync::OnceLock<RuntimeBundleDeployStamp>,
}

/// Deploy-stable identity for guest determinism semantics: the timestamp the
/// bundle was deployed (entrypoint mtime — deploys rewrite the bundle file —
/// falling back to first-observation time) and a content-derived seed (the
/// bundle SHA-256). Both survive server restarts for the same deployed bundle,
/// so import-time `Date.now()` / `Math.random()` values and
/// `performance.timeOrigin` stay stable across runs per the Convex default
/// runtime contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeBundleDeployStamp {
    pub timestamp_ms: u64,
    pub seed_hex: String,
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
    guest_semantics: RuntimeGuestSemantics,
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
            guest_semantics: limits.guest_semantics,
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
            RuntimeBundleContent::JavaScript,
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
            RuntimeBundleContent::JavaScript,
            RuntimeBundleEntrypointKind::Main,
            Some(normalize_sha256(expected_sha256.as_ref())?),
            None,
            None,
        ))
    }

    pub fn wasm_component(entrypoint: impl AsRef<Path>) -> Self {
        Self::wasm_component_for_world(entrypoint, RuntimeComponentWorld::NimbusFunction)
    }

    pub fn wasm_component_for_world(
        entrypoint: impl AsRef<Path>,
        target_world: RuntimeComponentWorld,
    ) -> Self {
        Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            RuntimeBundleContent::WasmComponent(RuntimeBundleWasmComponentContent {
                target_world,
                precompiled_sha256: None,
            }),
            RuntimeBundleEntrypointKind::Main,
            None,
            None,
            None,
        )
    }

    pub fn wasm_component_with_expected_sha256(
        entrypoint: impl AsRef<Path>,
        expected_sha256: impl AsRef<str>,
    ) -> Result<Self> {
        Self::wasm_component_for_world_with_expected_sha256(
            entrypoint,
            RuntimeComponentWorld::NimbusFunction,
            expected_sha256,
        )
    }

    pub fn wasm_component_for_world_with_expected_sha256(
        entrypoint: impl AsRef<Path>,
        target_world: RuntimeComponentWorld,
        expected_sha256: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            RuntimeBundleContent::WasmComponent(RuntimeBundleWasmComponentContent {
                target_world,
                precompiled_sha256: None,
            }),
            RuntimeBundleEntrypointKind::Main,
            Some(normalize_sha256(expected_sha256.as_ref())?),
            None,
            None,
        ))
    }

    pub fn wasm_component_with_precompiled_sha256(
        entrypoint: impl AsRef<Path>,
        target_world: RuntimeComponentWorld,
        expected_sha256: impl AsRef<str>,
        precompiled_sha256: impl AsRef<str>,
    ) -> Result<Self> {
        Ok(Self::from_parts(
            entrypoint.as_ref().to_path_buf(),
            RuntimeBundleContent::WasmComponent(RuntimeBundleWasmComponentContent {
                target_world,
                precompiled_sha256: Some(normalize_sha256(precompiled_sha256.as_ref())?),
            }),
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
            RuntimeBundleContent::JavaScript,
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
            RuntimeBundleContent::JavaScript,
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
            RuntimeBundleContent::JavaScript,
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
            RuntimeBundleContent::JavaScript,
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
        self.shared.content.content_kind()
    }

    pub fn content(&self) -> &RuntimeBundleContent {
        &self.shared.content
    }

    pub fn target_world(&self) -> Option<RuntimeComponentWorld> {
        self.shared.identity.target_world()
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

    pub fn verify_precompiled_component_integrity(
        &self,
        precompiled_path: impl AsRef<Path>,
    ) -> Result<()> {
        let RuntimeBundleContent::WasmComponent(content) = self.content() else {
            return Err(NimbusRuntimeError::Contract(format!(
                "runtime bundle `{}` is not a WASM component bundle",
                self.entrypoint().display()
            )));
        };
        let Some(expected_sha256) = content.precompiled_sha256() else {
            return Err(NimbusRuntimeError::Contract(format!(
                "WASM component bundle `{}` has no precompiled component hash",
                self.entrypoint().display()
            )));
        };
        let actual_sha256 = Self::compute_sha256_for_path(precompiled_path.as_ref())?;
        if actual_sha256 == expected_sha256 {
            return Ok(());
        }
        Err(NimbusRuntimeError::BundleIntegrityMismatch(format!(
            "{} precompiled component (expected {}, got {})",
            precompiled_path.as_ref().display(),
            expected_sha256,
            actual_sha256
        )))
    }

    fn from_parts(
        entrypoint: PathBuf,
        content: RuntimeBundleContent,
        entrypoint_kind: RuntimeBundleEntrypointKind,
        expected_sha256: Option<String>,
        tenant_label: Option<String>,
        explicit_module_root: Option<PathBuf>,
    ) -> Self {
        let content_kind = content.content_kind();
        let target_world = match &content {
            RuntimeBundleContent::JavaScript => None,
            RuntimeBundleContent::WasmComponent(content) => Some(content.target_world()),
        };
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
            target_world,
            entrypoint_kind,
            entrypoint: canonical_entrypoint
                .clone()
                .unwrap_or_else(|| entrypoint.clone()),
            expected_sha256: expected_sha256.clone(),
        };
        Self {
            shared: Arc::new(RuntimeBundleShared {
                content,
                entrypoint_kind,
                entrypoint,
                canonical_entrypoint,
                canonical_module_root,
                module_specifier,
                expected_sha256,
                identity,
                module_code_caches: Mutex::new(HashMap::new()),
                deploy_nonce: std::sync::OnceLock::new(),
                deploy_stamp: std::sync::OnceLock::new(),
            }),
        }
    }

    /// Attach a genuine per-deploy nonce, captured by the loader at bundle
    /// registration and persisted with the bundle provenance. The nonce is
    /// mixed into the import-time seed (see [`Self::deploy_stamp`]) so two
    /// distinct deploys establish different import-time random streams even
    /// when their content and entrypoint mtime are byte-identical — the mtime
    /// is no longer the freshness signal. Set exactly once (the first deploy
    /// nonce wins); it must be attached before the first `deploy_stamp` read
    /// (i.e. before the first invocation), which the loader guarantees.
    pub fn with_deploy_nonce(self, deploy_nonce: impl Into<String>) -> Self {
        let _ = self.shared.deploy_nonce.set(deploy_nonce.into());
        self
    }

    /// The bundle's deploy stamp (see [`RuntimeBundleDeployStamp`]). Computed
    /// once per bundle handle and cached; reading the file for the seed hash
    /// only happens when no expected SHA-256 was recorded.
    pub(crate) fn deploy_stamp(&self) -> RuntimeBundleDeployStamp {
        self.shared
            .deploy_stamp
            .get_or_init(|| {
                let entrypoint = self
                    .shared
                    .canonical_entrypoint
                    .as_deref()
                    .unwrap_or(&self.shared.entrypoint);
                let timestamp_ms = std::fs::metadata(entrypoint)
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or_else(|_| std::time::SystemTime::now())
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis().min(u64::MAX as u128) as u64)
                    .unwrap_or(0);
                let content_hash = match &self.shared.expected_sha256 {
                    Some(sha256) => sha256.clone(),
                    None => Self::compute_sha256_for_path(entrypoint).unwrap_or_else(|_| {
                        compute_sha256_hex(entrypoint.to_string_lossy().as_bytes())
                    }),
                };
                // The import-time random stream is a most-recent-DEPLOYMENT
                // value, not a content property: a redeploy must reseed it even
                // when the content is byte-identical. Freshness comes from the
                // per-deploy nonce the loader captured at registration, NOT the
                // entrypoint mtime — so two deploys that preserve the mtime, or
                // land in the same millisecond, still reseed differently. The
                // mtime remains the deploy timestamp guest code observes
                // (`Date.now()` / `performance.timeOrigin`), where same-instant
                // deploys legitimately share a value. Legacy bundles with no
                // nonce keep the content+mtime seed.
                let seed_hex = match self.shared.deploy_nonce.get() {
                    Some(nonce) => compute_sha256_hex(
                        format!("{content_hash}:{timestamp_ms}:{nonce}").as_bytes(),
                    ),
                    None => compute_sha256_hex(format!("{content_hash}:{timestamp_ms}").as_bytes()),
                };
                RuntimeBundleDeployStamp {
                    timestamp_ms,
                    seed_hex,
                }
            })
            .clone()
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

#[cfg(test)]
mod deploy_stamp_tests {
    use super::*;

    fn write_bundle(dir: &std::path::Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(
            &path,
            b"globalThis.__nimbusInvoke = async () => ({}); export {};",
        )
        .expect("bundle write should succeed");
        path
    }

    #[test]
    fn identical_content_and_mtime_reseed_differently_per_deploy_nonce() {
        // Two deploys of byte-identical source with an identical entrypoint
        // mtime: the mtime-based seed collided (M2). With a genuine per-deploy
        // nonce the import-time seed must differ, while the deploy timestamp
        // (which guest Date.now()/timeOrigin observes) legitimately stays equal.
        let dir = tempfile::tempdir().expect("tempdir should create");
        let path = write_bundle(dir.path(), "bundle.mjs");

        let first = RuntimeBundle::new(&path).with_deploy_nonce("deploy-nonce-a");
        let second = RuntimeBundle::new(&path).with_deploy_nonce("deploy-nonce-b");

        let first_stamp = first.deploy_stamp();
        let second_stamp = second.deploy_stamp();

        assert_ne!(
            first_stamp.seed_hex, second_stamp.seed_hex,
            "distinct per-deploy nonces must reseed the import stream even for identical content+mtime",
        );
        assert_eq!(
            first_stamp.timestamp_ms, second_stamp.timestamp_ms,
            "the deploy timestamp stays the entrypoint mtime and is unaffected by the nonce",
        );
    }

    #[test]
    fn same_deploy_nonce_is_stable_across_reconstruction() {
        // Per-invocation / cross-restart determinism: the same deployed
        // artifact (same content, mtime, and persisted nonce) must reseed
        // identically no matter how many times the bundle handle is rebuilt.
        let dir = tempfile::tempdir().expect("tempdir should create");
        let path = write_bundle(dir.path(), "bundle.mjs");

        let first = RuntimeBundle::new(&path).with_deploy_nonce("stable-nonce");
        let second = RuntimeBundle::new(&path).with_deploy_nonce("stable-nonce");

        assert_eq!(
            first.deploy_stamp().seed_hex,
            second.deploy_stamp().seed_hex,
            "an identical persisted nonce must keep the import seed stable across reconstruction",
        );
    }

    #[test]
    fn missing_nonce_preserves_legacy_content_and_mtime_seed() {
        // Bundles with no nonce (hand-rolled/legacy) keep the historical seed
        // so their behavior is unchanged by this fix.
        let dir = tempfile::tempdir().expect("tempdir should create");
        let path = write_bundle(dir.path(), "bundle.mjs");

        let bundle = RuntimeBundle::new(&path);
        let stamp = bundle.deploy_stamp();

        let content_hash =
            RuntimeBundle::compute_sha256_for_path(&path).expect("content hash should compute");
        let expected =
            compute_sha256_hex(format!("{content_hash}:{}", stamp.timestamp_ms).as_bytes());
        assert_eq!(
            stamp.seed_hex, expected,
            "a bundle without a deploy nonce must keep the content+mtime seed",
        );
    }
}
