#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::{rc::Rc, sync::OnceLock};

use crate::error::Result;
use crate::limits::RuntimeCompatibilityTarget;
use crate::runtime::bootstrap::{
    bootstrap_script_provenance_inputs, extension_transpiler_for_target, install_bootstrap,
    install_missing_deno_extension_state, snapshot_extensions,
};

use super::embedder::{
    Extension, ExtensionFileSource, ExtensionFileSourceCode, ExtensionSourceProvider,
    JsRuntimeForSnapshot, ModuleCodeString, ModuleName, RuntimeOptions,
};

type ResidualLazySources = &'static [(&'static str, &'static str)];
type PackagedRuntimeExtensionSources = &'static [(&'static str, &'static str)];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum V8RuntimeConstructionMode {
    Unsnapshotted,
    StartupSnapshot,
}

impl V8RuntimeConstructionMode {
    pub(crate) fn for_compatibility_target(target: RuntimeCompatibilityTarget) -> Self {
        debug_assert!(
            target != RuntimeCompatibilityTarget::BunJsc,
            "Bun/JSC must not enter the V8 construction-mode selector"
        );
        // Option A: only NodeFull keeps a startup snapshot. The NodeFull RO-heap anchor
        // installs NodeFull's superset RO heap into the shared cage FIRST, so a WebStandard
        // SNAPSHOT would deserialize against it and crash (Unknown external reference / SIGBUS
        // — confirmed: with the anchor armed, a snapshotted WebStandard build aborts; with it
        // disabled it passes). Non-Node V8 targets are therefore built UNSNAPSHOTTED and ride
        // the anchor's superset RO builtins (proven correct by
        // reachable_fix_unsnapshotted_weblean_against_nodefull_anchor_ro_intrinsics_correct).
        if target.is_node() {
            Self::StartupSnapshot
        } else {
            Self::Unsnapshotted
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Unsnapshotted => "unsnapshotted",
            Self::StartupSnapshot => "startup_snapshot",
        }
    }

    pub(crate) fn uses_startup_snapshot(self) -> bool {
        matches!(self, Self::StartupSnapshot)
    }
}

pub(crate) struct V8StartupSnapshot {
    bytes: &'static [u8],
    residual_lazy_js_sources: ResidualLazySources,
    residual_lazy_esm_sources: ResidualLazySources,
}

impl V8StartupSnapshot {
    fn new(
        bytes: Box<[u8]>,
        residual_lazy_js_sources: ResidualLazySources,
        residual_lazy_esm_sources: ResidualLazySources,
    ) -> Self {
        // deno_core currently accepts startup snapshots as &'static [u8]. The
        // worker pool keeps a single bootstrap snapshot for its own lifetime,
        // so leaking one buffer per compatibility target matches the pool's lifetime
        // and avoids unsound lifetime extension tricks. Snapshot companion source
        // tables follow the same process-lifetime contract.
        Self {
            bytes: Box::leak(bytes),
            residual_lazy_js_sources,
            residual_lazy_esm_sources,
        }
    }

    pub(crate) fn as_startup_snapshot(&self) -> &'static [u8] {
        self.bytes
    }

    pub(crate) fn residual_lazy_js_sources(&self) -> ResidualLazySources {
        self.residual_lazy_js_sources
    }

    pub(crate) fn residual_lazy_esm_sources(&self) -> ResidualLazySources {
        self.residual_lazy_esm_sources
    }
}

#[cfg(test)]
static V8_BOOTSTRAP_SNAPSHOT_BUILDS: AtomicUsize = AtomicUsize::new(0);
pub(crate) fn create_v8_startup_snapshot(
    compatibility_target: RuntimeCompatibilityTarget,
    service_extension_enabled: bool,
) -> Result<V8StartupSnapshot> {
    // Anchor floor (Option A): the SnapshotCreator below ALSO aliases the cage's shared RO
    // heap (see the lock comment), so a snapshot BUILD can be the first cage installer just
    // like a deserialize. Guard it with the same fail-closed floor as
    // create_runtime_with_bootstrap_state so a snapshot build before the NodeFull anchor
    // installs is caught, rather than silently installing a non-superset heap first. No-op
    // unless the anchor system is in use; the anchor's OWN snapshot build is exempt via the
    // IN_ANCHOR_BUILD thread-local.
    crate::runtime::driver::anchor::assert_anchor_floor();
    // Serialize snapshot BUILD against cold isolate CREATION and DISPOSAL on the
    // one process-global re-entrant lock owned by deno_core: the SnapshotCreator
    // here and every restored isolate alias the default IsolateGroup's read-only
    // heap on a single (shared) cage, and build/create/dispose are its only
    // unguarded writers. Re-entrant, so the build's own isolate disposal (via
    // create_blob) nesting under this guard does not self-deadlock.
    let _shared_heap_guard = deno_core::shared_ro_heap_serialize_lock().lock();

    #[cfg(test)]
    V8_BOOTSTRAP_SNAPSHOT_BUILDS.fetch_add(1, Ordering::Relaxed);

    // The bootstrap sources run here too, so keep them snapshot-safe. In
    // particular, post-bootstrap cleanup like `delete globalThis.Deno` must
    // stay in the separate finalize step for ordinary runtimes until the fork
    // offers an explicit snapshot-safe replacement.
    let extensions = snapshot_extensions(compatibility_target, service_extension_enabled);
    let (residual_lazy_js_sources, residual_lazy_esm_sources) =
        collect_startup_snapshot_extension_sources(compatibility_target, &extensions)?;
    let extension_source_provider = packaged_runtime_extension_source_provider(&extensions)?;
    let mut runtime = JsRuntimeForSnapshot::new(RuntimeOptions {
        extensions,
        extension_transpiler: extension_transpiler_for_target(compatibility_target),
        extension_source_provider,
        create_params: Some(super::attach_cppgc_heap(Default::default())),
        ..Default::default()
    });
    {
        let op_state = runtime.op_state();
        install_missing_deno_extension_state(&mut op_state.borrow_mut());
    }
    if compatibility_target.is_node() {
        let isolate = runtime.v8_isolate();
        crate::backends::v8::embedder::v8::scope!(scope, isolate);
        let template =
            deno_node::init_global_template(scope, deno_node::ContextInitMode::ForSnapshot);
        let context = deno_node::create_v8_context(
            scope,
            template,
            deno_node::ContextInitMode::ForSnapshot,
            std::ptr::null_mut(),
        );
        assert_eq!(scope.add_context(context), deno_node::VM_CONTEXT_INDEX);
    }
    install_bootstrap(&mut runtime)?;
    Ok(V8StartupSnapshot::new(
        runtime.snapshot(),
        residual_lazy_js_sources,
        residual_lazy_esm_sources,
    ))
}

// ── Compile-time embeddable NodeFull(Node22) anchor snapshot ────────────────────────────────────
//
// Building the NodeFull snapshot at runtime costs ~4.18s (debug). Armed lazily by the anchor it
// lands inside the first request and blows per-request timeouts; the serving path must instead
// DESERIALIZE a pre-built snapshot (~17ms). The snapshot cannot be produced by build.rs (the build
// reaches `crate::` internals and build.rs cannot depend on the crate it builds), so a builder
// BINARY (a normal consumer of this crate) calls `build_embeddable_node22_snapshot_blob`, writes
// the blob to a committed file, and the lib `include_bytes!`es it and reconstructs the snapshot via
// `v8_startup_snapshot_from_embedded_blob`. The serialized bytes ARE the runtime-built snapshot, so
// the installed read-only heap is byte-identical (cage-critical). Blob format (LE,
// length-prefixed): the content provenance, the portable provenance, the V8 startup bytes, the two
// residual lazy (name, source) source tables, then the packaged build-only extension source table.

fn write_blob_len(out: &mut Vec<u8>, n: usize) {
    out.extend_from_slice(&(n as u64).to_le_bytes());
}

fn write_blob_bytes(out: &mut Vec<u8>, bytes: &[u8]) {
    write_blob_len(out, bytes.len());
    out.extend_from_slice(bytes);
}

fn write_blob_table(out: &mut Vec<u8>, table: ResidualLazySources) {
    write_blob_len(out, table.len());
    for (name, source) in table {
        write_blob_bytes(out, name.as_bytes());
        write_blob_bytes(out, source.as_bytes());
    }
}

struct EmbeddedSnapshotBlobReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl EmbeddedSnapshotBlobReader<'_> {
    fn read_len(&mut self) -> Result<usize> {
        let end = self
            .pos
            .checked_add(8)
            .filter(|end| *end <= self.data.len());
        let end = end.ok_or_else(|| {
            crate::error::NimbusRuntimeError::Contract(
                "embedded snapshot blob truncated (length prefix)".to_string(),
            )
        })?;
        let value = u64::from_le_bytes(self.data[self.pos..end].try_into().unwrap());
        self.pos = end;
        usize::try_from(value).map_err(|_| {
            crate::error::NimbusRuntimeError::Contract(
                "embedded snapshot blob length exceeds the platform limit".to_string(),
            )
        })
    }

    fn read_bytes_owned(&mut self) -> Result<Vec<u8>> {
        let len = self.read_len()?;
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.data.len());
        let end = end.ok_or_else(|| {
            crate::error::NimbusRuntimeError::Contract(
                "embedded snapshot blob truncated (body)".to_string(),
            )
        })?;
        let value = self.data[self.pos..end].to_vec();
        self.pos = end;
        Ok(value)
    }

    fn skip_bytes(&mut self) -> Result<()> {
        let len = self.read_len()?;
        let end = self
            .pos
            .checked_add(len)
            .filter(|end| *end <= self.data.len());
        self.pos = end.ok_or_else(|| {
            crate::error::NimbusRuntimeError::Contract(
                "embedded snapshot blob truncated (body)".to_string(),
            )
        })?;
        Ok(())
    }

    fn read_static_str(&mut self) -> Result<&'static str> {
        let bytes = self.read_bytes_owned()?;
        let text = String::from_utf8(bytes).map_err(|error| {
            crate::error::NimbusRuntimeError::Contract(format!(
                "embedded snapshot blob has non-UTF-8 source: {error}"
            ))
        })?;
        // Process-lifetime leak, matching V8StartupSnapshot's &'static source-table contract: one
        // leak per process at the bootstrap-snapshot OnceLock init.
        Ok(Box::leak(text.into_boxed_str()))
    }

    fn read_table(&mut self) -> Result<ResidualLazySources> {
        let count = self.read_len()?;
        let max_pairs = self.data.len().saturating_sub(self.pos) / 16;
        if count > max_pairs {
            return Err(crate::error::NimbusRuntimeError::Contract(format!(
                "embedded snapshot blob table count {count} exceeds the remaining body"
            )));
        }
        let mut pairs: Vec<(&'static str, &'static str)> = Vec::with_capacity(count);
        for _ in 0..count {
            let name = self.read_static_str()?;
            let source = self.read_static_str()?;
            pairs.push((name, source));
        }
        Ok(Box::leak(pairs.into_boxed_slice()))
    }

    fn skip_table(&mut self) -> Result<()> {
        let count = self.read_len()?;
        let max_pairs = self.data.len().saturating_sub(self.pos) / 16;
        if count > max_pairs {
            return Err(crate::error::NimbusRuntimeError::Contract(format!(
                "embedded snapshot blob table count {count} exceeds the remaining body"
            )));
        }
        for _ in 0..count {
            self.skip_bytes()?;
            self.skip_bytes()?;
        }
        Ok(())
    }

    fn finish(self) -> Result<()> {
        if self.pos == self.data.len() {
            return Ok(());
        }
        Err(crate::error::NimbusRuntimeError::Contract(format!(
            "embedded snapshot blob has {} trailing bytes",
            self.data.len() - self.pos
        )))
    }
}

/// The exact (target, service-extension) the NodeFull RO-heap anchor uses, so the embedded blob
/// matches what the anchor would otherwise build.
fn embeddable_node22_snapshot_target() -> (RuntimeCompatibilityTarget, bool) {
    (
        RuntimeCompatibilityTarget::Node22,
        super::startup_key::RuntimeStartupSnapshotKey::NodeFull.service_extension_enabled(),
    )
}

/// Bump on ANY change to the embedded-snapshot build/serialize logic that is NOT otherwise visible
/// to the provenance hash (e.g. a change to the blob byte layout, or a subtle op-behavior change
/// that alters the bootstrapped heap without changing op names or JS source text). This is the
/// manual catch-all for "changed but not regenerated" inputs the structured hash cannot see.
const EMBEDDED_SNAPSHOT_SCHEMA_VERSION: u64 = 6;

#[derive(Clone, Copy)]
enum SnapshotProvenanceMode {
    Content,
    Portable,
}

fn hash_snapshot_extension_source<H: std::hash::Hasher>(
    file: &ExtensionFileSource,
    mode: SnapshotProvenanceMode,
    hasher: &mut H,
) -> Result<()> {
    use std::hash::Hash;

    file.specifier.hash(hasher);
    #[allow(
        deprecated,
        reason = "the portable identity must recognize build-only sources"
    )]
    match (&file.code, mode) {
        (
            ExtensionFileSourceCode::LoadedFromFsDuringSnapshot(path),
            SnapshotProvenanceMode::Portable,
        ) => {
            // The final binary cannot reopen a Cargo checkout path from the build machine. Hash the
            // path identity instead. Snapshot generation and the final release build use the same
            // checkout, and a Cargo git dependency path includes the exact Deno revision. The eager
            // content provenance below still hashes the file bytes while the build checkout exists.
            1_u8.hash(hasher);
            path.hash(hasher);
        }
        _ => {
            0_u8.hash(hasher);
            file.load()?.as_bytes().hash(hasher);
        }
    }
    Ok(())
}

fn extension_file_sources(extension: &Extension) -> impl Iterator<Item = &ExtensionFileSource> {
    extension
        .js_files
        .iter()
        .chain(extension.esm_files.iter())
        .chain(extension.lazy_loaded_js_files.iter())
        .chain(extension.lazy_loaded_esm_files.iter())
}

/// Domain-separated provenance over everything that determines the NodeFull snapshot's RO heap.
/// The content form reads and hashes all source bytes while the build checkout exists. The release
/// gate compares it before the final binary is accepted. The portable form hashes the build-source
/// path identity for sources that Deno intentionally loads only while it creates a snapshot; Cargo
/// git dependency paths include the exact revision. All other source bytes remain in the hash. A
/// source checkout compares both forms, so an in-place source edit with an unchanged path still
/// rejects a stale blob. A deployed binary can compare the portable form when the build-only files
/// no longer exist. Coverage:
///   - the build TARGET arch + OS (a V8 startup snapshot is platform-specific — a darwin-arm64
///     snapshot deserialized on linux-x86_64 is a hard V8_Fatal; the generated blob is per-platform,
///     so a foreign-target blob must fail validation and never install);
///   - the V8 version (snapshots are V8-version-specific; skew must not install);
///   - the `v8-pointer-compression` feature (release ships pointer-compressed; V8 refuses to
///     deserialize a snapshot built under a different pointer-compression config — installing a
///     mismatched blob is a hard abort, so this MUST gate the cage's single-shared-heap layout);
///   - the exact extension SELECTION (names) for this (target, service-extension);
///   - the OP SURFACE (every op name, in declaration order) bound into the bootstrap context;
///   - the full source TEXT of every extension source used by the snapshot or packaged
///     unsnapshotted-runtime table (Nimbus and Deno bootstrap JS);
///   - the schema constant (build-logic changes not visible in the structured inputs above).
///
/// A byte-for-byte comparison is not valid because V8 puts a random hash seed in the startup
/// snapshot. The two input hashes are deterministic within their contracts. A Rust op change that
/// changes bootstrap behavior without changing an op name or JavaScript source requires a schema
/// version bump.
fn embedded_node22_snapshot_provenance_with(mode: SnapshotProvenanceMode) -> Result<u64> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    EMBEDDED_SNAPSHOT_SCHEMA_VERSION.hash(&mut hasher);
    match mode {
        SnapshotProvenanceMode::Content => "content".hash(&mut hasher),
        SnapshotProvenanceMode::Portable => "portable".hash(&mut hasher),
    }
    // A V8 startup snapshot is platform-specific: the build target's arch + OS must gate it so a
    // blob built for one platform (e.g. the committed darwin-arm64 one) can NEVER install on another
    // (e.g. linux-x86_64 CI), which would be a hard V8_Fatal. `std::env::consts` reflects the build
    // TARGET (the binary's compiled-for platform), identical at build time (builder bin) and runtime.
    std::env::consts::ARCH.hash(&mut hasher);
    std::env::consts::OS.hash(&mut hasher);
    super::embedder::v8::V8::get_version().hash(&mut hasher);
    cfg!(feature = "v8-pointer-compression").hash(&mut hasher);
    let (target, service_extension_enabled) = embeddable_node22_snapshot_target();
    (target as u32).hash(&mut hasher);
    service_extension_enabled.hash(&mut hasher);
    let extensions = snapshot_extensions(target, service_extension_enabled);
    for extension in &extensions {
        extension.name.hash(&mut hasher);
        for op in extension.ops.iter() {
            op.name.hash(&mut hasher);
        }
        for file in extension_file_sources(extension) {
            hash_snapshot_extension_source(file, mode, &mut hasher)?;
        }
    }
    // The same release artifact also builds WebStandard without a startup snapshot and can build
    // the service-bearing NodeFull snapshot after installation. Hash the union of build-only
    // sources packaged for those construction paths. This is separate from the NodeFull snapshot
    // inputs above because WebStandard has its own bootstrap extension.
    "packaged-runtime-extension-sources".hash(&mut hasher);
    for packaged_target in [
        RuntimeCompatibilityTarget::WebStandardIsolate,
        RuntimeCompatibilityTarget::Node22,
    ] {
        (packaged_target as u32).hash(&mut hasher);
        for extension in snapshot_extensions(packaged_target, true) {
            extension.name.hash(&mut hasher);
            for file in
                extension_file_sources(&extension).filter(|file| !file.is_runtime_loadable())
            {
                hash_snapshot_extension_source(file, mode, &mut hasher)?;
            }
        }
    }
    // The nimbus bootstrap scripts run DURING snapshot creation (see
    // build_node22_startup_snapshot -> install_bootstrap), so their
    // definitions are baked into the blob heap exactly like extension JS. A
    // blob built from older script text must mismatch and fall back to a
    // runtime build, or snapshot-backed NodeFull isolates keep serving stale
    // bootstrap definitions the binary no longer contains.
    for (name, source) in bootstrap_script_provenance_inputs() {
        name.hash(&mut hasher);
        source.as_bytes().hash(&mut hasher);
    }
    Ok(hasher.finish())
}

fn embedded_node22_snapshot_provenance() -> Result<u64> {
    embedded_node22_snapshot_provenance_with(SnapshotProvenanceMode::Content)
}

fn embedded_node22_snapshot_portable_provenance() -> Result<u64> {
    embedded_node22_snapshot_provenance_with(SnapshotProvenanceMode::Portable)
}

/// Build the NodeFull(Node22) anchor snapshot and serialize its content provenance, portable
/// provenance, V8 bytes, two residual lazy source tables, and the build-only extension source
/// union into a self-describing blob for compile-time embedding. Invoked by the snapshot builder
/// binary, not on the serving path.
pub fn build_embeddable_node22_snapshot_blob() -> Result<Vec<u8>> {
    let (target, service_extension_enabled) = embeddable_node22_snapshot_target();
    // Compute provenance over the SAME inputs the build will consume, BEFORE building.
    let provenance = embedded_node22_snapshot_provenance()?;
    let portable_provenance = embedded_node22_snapshot_portable_provenance()?;
    let snapshot = create_v8_startup_snapshot(target, service_extension_enabled)?;
    let packaged_runtime_extension_sources = collect_packaged_runtime_extension_sources()?;
    let mut out = Vec::new();
    out.extend_from_slice(&provenance.to_le_bytes());
    out.extend_from_slice(&portable_provenance.to_le_bytes());
    write_blob_bytes(&mut out, snapshot.as_startup_snapshot());
    write_blob_table(&mut out, snapshot.residual_lazy_js_sources());
    write_blob_table(&mut out, snapshot.residual_lazy_esm_sources());
    write_blob_table(&mut out, packaged_runtime_extension_sources);
    Ok(out)
}

fn embedded_blob_provenance_at(blob: &[u8], offset: usize, name: &str) -> Result<u64> {
    let end = offset.checked_add(8).ok_or_else(|| {
        crate::error::NimbusRuntimeError::Contract(
            "embedded snapshot blob provenance offset overflowed".to_string(),
        )
    })?;
    let slice = blob.get(offset..end).ok_or_else(|| {
        crate::error::NimbusRuntimeError::Contract(format!(
            "embedded snapshot blob truncated ({name} provenance header)"
        ))
    })?;
    Ok(u64::from_le_bytes(slice.try_into().unwrap()))
}

/// The content provenance hash stored in the leading 8 bytes of an embedded blob.
fn embedded_blob_provenance(blob: &[u8]) -> Result<u64> {
    embedded_blob_provenance_at(blob, 0, "content")
}

/// The runtime-portable provenance hash stored after the content provenance.
fn embedded_blob_portable_provenance(blob: &[u8]) -> Result<u64> {
    embedded_blob_provenance_at(blob, 8, "portable")
}

/// Reconstruct a `V8StartupSnapshot` from an embedded blob (the serving path: deserialize instead
/// of building ~4.18s). The bytes are the runtime-built snapshot, so the installed RO heap is
/// identical — cage-critical. This function only parses; the build gate validates both provenance
/// values, and the serving path validates the portable value before it calls this function.
pub(crate) fn v8_startup_snapshot_from_embedded_blob(blob: &[u8]) -> Result<V8StartupSnapshot> {
    let mut reader = EmbeddedSnapshotBlobReader {
        data: blob,
        pos: 16,
    };
    let bytes = reader.read_bytes_owned()?.into_boxed_slice();
    let residual_lazy_js_sources = reader.read_table()?;
    let residual_lazy_esm_sources = reader.read_table()?;
    reader.skip_table()?;
    reader.finish()?;
    Ok(V8StartupSnapshot::new(
        bytes,
        residual_lazy_js_sources,
        residual_lazy_esm_sources,
    ))
}

fn packaged_runtime_extension_sources_from_embedded_blob(
    blob: &[u8],
) -> Result<PackagedRuntimeExtensionSources> {
    let mut reader = EmbeddedSnapshotBlobReader {
        data: blob,
        pos: 16,
    };
    reader.skip_bytes()?;
    reader.skip_table()?;
    reader.skip_table()?;
    let sources = reader.read_table()?;
    reader.finish()?;
    Ok(sources)
}

fn embedded_blob_matches_current_provenance(blob: &[u8]) -> bool {
    let Some(stored_portable) = embedded_blob_portable_provenance(blob).ok() else {
        return false;
    };
    let Some(current_portable) = embedded_node22_snapshot_portable_provenance().ok() else {
        return false;
    };
    if stored_portable != current_portable {
        #[cfg(test)]
        eprintln!(
            "nimbus-runtime: the test-only snapshot extensions do not match the embedded \
             production NodeFull anchor; building the test snapshot at runtime."
        );
        #[cfg(not(test))]
        eprintln!(
            "nimbus-runtime: embedded NodeFull anchor snapshot is STALE (portable provenance \
             {stored_portable:016x} != current {current_portable:016x}); rejecting the embedded \
             fast path. Regenerate via `make build-node22-anchor-snapshot` before packaging."
        );
        return false;
    }

    // Preserve the source-checkout stale-blob guard. Deployed artifacts do not contain Deno files
    // marked `LoadedFromFsDuringSnapshot`, so failure to read those build-only paths is the expected
    // signal to rely on the portable identity that the eager release gate already validated.
    if let Ok(current_content) = embedded_node22_snapshot_provenance() {
        let Some(stored_content) = embedded_blob_provenance(blob).ok() else {
            return false;
        };
        if stored_content != current_content {
            #[cfg(not(test))]
            eprintln!(
                "nimbus-runtime: embedded NodeFull anchor snapshot is STALE (content provenance \
                 {stored_content:016x} != current {current_content:016x}); rejecting the embedded \
                 fast path. Regenerate via `make build-node22-anchor-snapshot` before packaging."
            );
            return false;
        }
    }
    true
}

/// Guarded loader for the embedded NodeFull snapshot. It always validates runtime-safe provenance.
/// When build-only extension sources remain accessible, it also validates their content provenance
/// so an in-place source edit cannot reuse a stale blob. A deployed binary skips that second check
/// only when those source files are unavailable. On a mismatch or parse error, the caller uses its
/// existing runtime-build fallback. Release packaging must prevent that fallback because Deno
/// sources marked `LoadedFromFsDuringSnapshot` are not available on a deployed host.
pub(crate) fn try_embedded_node22_anchor_snapshot(blob: &[u8]) -> Option<V8StartupSnapshot> {
    embedded_blob_matches_current_provenance(blob).then_some(())?;
    v8_startup_snapshot_from_embedded_blob(blob).ok()
}

/// The generated NodeFull(Node22) anchor snapshot blob (regenerated by the
/// `build_node22_anchor_snapshot` binary). Two gitignored blobs exist, one per pointer-compression
/// config, because a V8 startup snapshot is NOT portable across the `v8-pointer-compression`
/// feature (release ships pointer-compressed; dev/test runs feature-off). The matching blob is
/// selected here at compile time so BOTH configs HIT the embedded fast path; the provenance guard
/// (which hashes the pc feature) is the defense-in-depth backstop if the wrong file is ever placed
/// at a path. A fresh checkout has an empty placeholder (written by build.rs), which fails the
/// provenance guard. Source-checkout development can build the snapshot at runtime; release
/// packaging must generate and check the blob before it builds the deployed binary.
#[cfg(feature = "v8-pointer-compression")]
pub(crate) static EMBEDDED_NODE22_ANCHOR_SNAPSHOT: &[u8] =
    include_bytes!("node22_anchor_snapshot.pc.bin");
#[cfg(not(feature = "v8-pointer-compression"))]
pub(crate) static EMBEDDED_NODE22_ANCHOR_SNAPSHOT: &[u8] =
    include_bytes!("node22_anchor_snapshot.bin");

fn collect_packaged_runtime_extension_sources() -> Result<PackagedRuntimeExtensionSources> {
    let mut sources = Vec::new();
    for target in [
        RuntimeCompatibilityTarget::WebStandardIsolate,
        RuntimeCompatibilityTarget::Node22,
    ] {
        for extension in snapshot_extensions(target, true) {
            for file in
                extension_file_sources(&extension).filter(|file| !file.is_runtime_loadable())
            {
                sources.push((file.specifier, file.load()?.to_string()));
            }
        }
    }
    leak_extension_sources(sources, "packaged runtime")
}

fn packaged_runtime_extension_sources() -> Option<PackagedRuntimeExtensionSources> {
    static SOURCES: OnceLock<Option<PackagedRuntimeExtensionSources>> = OnceLock::new();
    *SOURCES.get_or_init(|| {
        embedded_blob_matches_current_provenance(EMBEDDED_NODE22_ANCHOR_SNAPSHOT)
            .then(|| {
                packaged_runtime_extension_sources_from_embedded_blob(
                    EMBEDDED_NODE22_ANCHOR_SNAPSHOT,
                )
                .ok()
            })
            .flatten()
    })
}

fn find_packaged_runtime_extension_source(
    sources: PackagedRuntimeExtensionSources,
    specifier: &str,
) -> Option<&'static str> {
    let index = sources
        .binary_search_by_key(&specifier, |(candidate, _)| *candidate)
        .ok()?;
    Some(sources[index].1)
}

pub(crate) fn packaged_runtime_extension_source_provider(
    extensions: &[Extension],
) -> Result<Option<Rc<ExtensionSourceProvider>>> {
    let Some(sources) = packaged_runtime_extension_sources() else {
        // A source checkout can load the declared paths directly. The release gate requires a
        // valid embedded table before it builds a deployable artifact.
        return Ok(None);
    };
    for extension in extensions {
        for file in extension_file_sources(extension).filter(|file| !file.is_runtime_loadable()) {
            if find_packaged_runtime_extension_source(sources, file.specifier).is_none() {
                return Err(crate::error::NimbusRuntimeError::Contract(format!(
                    "embedded runtime extension source table is missing {}",
                    file.specifier
                )));
            }
        }
    }
    Ok(Some(Rc::new(move |source| {
        find_packaged_runtime_extension_source(sources, source.specifier)
            .map(|source| source.to_string().into())
    })))
}

fn collect_startup_snapshot_extension_sources(
    compatibility_target: RuntimeCompatibilityTarget,
    extensions: &[Extension],
) -> Result<(ResidualLazySources, ResidualLazySources)> {
    let mut residual_lazy_js_sources = Vec::new();
    let mut residual_lazy_esm_sources = Vec::new();

    for extension in extensions {
        for file in &*extension.lazy_loaded_js_files {
            if !file.is_runtime_loadable() {
                residual_lazy_js_sources.push(transpile_snapshot_extension_source(
                    compatibility_target,
                    file.specifier,
                    file.load()?,
                )?);
            }
        }
        for file in &*extension.lazy_loaded_esm_files {
            if !file.is_runtime_loadable() {
                residual_lazy_esm_sources.push(transpile_snapshot_extension_source(
                    compatibility_target,
                    file.specifier,
                    file.load()?,
                )?);
            }
        }
    }

    Ok((
        leak_extension_sources(residual_lazy_js_sources, "residual lazy JS")?,
        leak_extension_sources(residual_lazy_esm_sources, "residual lazy ESM")?,
    ))
}

fn transpile_snapshot_extension_source(
    compatibility_target: RuntimeCompatibilityTarget,
    specifier: &'static str,
    source: ModuleCodeString,
) -> Result<(&'static str, String)> {
    let source = if let Some(transpiler) = extension_transpiler_for_target(compatibility_target) {
        let (source, _) =
            transpiler(ModuleName::from_static(specifier), source).map_err(|error| {
                crate::error::NimbusRuntimeError::JavaScript(format!(
                    "failed to transpile residual extension source {specifier}: {error}"
                ))
            })?;
        source
    } else {
        source
    };
    Ok((specifier, source.to_string()))
}

fn leak_extension_sources(
    mut sources: Vec<(&'static str, String)>,
    source_kind: &str,
) -> Result<ResidualLazySources> {
    sources.sort_by_key(|(specifier, _)| *specifier);
    let mut deduped = Vec::<(&'static str, String)>::new();
    for (specifier, source) in sources {
        if let Some((last_specifier, last_source)) = deduped.last()
            && *last_specifier == specifier
        {
            if last_source != &source {
                return Err(crate::error::NimbusRuntimeError::JavaScript(format!(
                    "conflicting {source_kind} extension source for {specifier}"
                )));
            }
            continue;
        }
        deduped.push((specifier, source));
    }

    let sources = deduped
        .into_iter()
        .map(|(specifier, source)| {
            (
                specifier,
                Box::leak(source.into_boxed_str()) as &'static str,
            )
        })
        .collect::<Vec<_>>();
    Ok(Box::leak(sources.into_boxed_slice()))
}

#[cfg(test)]
pub(crate) fn v8_bootstrap_snapshot_build_count_for_test() -> usize {
    V8_BOOTSTRAP_SNAPSHOT_BUILDS.load(Ordering::Relaxed)
}

/// Validate the generated embedded NodeFull(Node22) anchor blob for THIS build config, returning
/// `Err(message)` on any staleness or corruption. Confirms: (1) the blob is non-empty (not a fresh
/// placeholder); (2) its stored content and portable provenance equal the values recomputed from
/// the current binary's inputs (V8 version, pointer-compression feature, extension selection, op
/// surface, bootstrap JS source, schema version); (3) the packaged runtime source table exactly
/// matches the current source union; (4) the blob deserializes structurally. This is the staleness
/// gate the serving path relies on, run eagerly so a mismatch fails CI loudly rather than silently
/// degrading to a ~4.18s runtime build at first request. It MUST run in a
/// NON-`cfg(test)` build — under `cfg(test)` `snapshot_extensions` includes a test-only extension,
/// so the recomputed provenance legitimately differs from the generated production blob. Invoked by
/// `build_node22_anchor_snapshot --check` (per pointer-compression config) in CI. A byte-for-byte
/// compare is deliberately NOT used: V8 bakes a random hash-seed into the snapshot, so the bytes are
/// not reproducible across builds (see `embedded_node22_snapshot_provenance`).
pub fn check_generated_embedded_anchor_snapshot(
    generated: &[u8],
) -> std::result::Result<(), String> {
    let pc = cfg!(feature = "v8-pointer-compression");
    if generated.is_empty() {
        return Err(format!(
            "generated embedded anchor snapshot is an EMPTY placeholder \
             (v8-pointer-compression={pc}); generate it with `make build-node22-anchor-snapshot`",
        ));
    }
    let stored = embedded_blob_provenance(generated)
        .map_err(|error| format!("reading generated blob provenance: {error}"))?;
    let current = embedded_node22_snapshot_provenance()
        .map_err(|error| format!("computing current provenance: {error}"))?;
    if stored != current {
        return Err(format!(
            "generated embedded anchor snapshot is STALE (v8-pointer-compression={pc}): stored \
             content provenance {stored:016x} != current {current:016x}; regenerate via \
             `make build-node22-anchor-snapshot`.",
        ));
    }
    let stored_portable = embedded_blob_portable_provenance(generated)
        .map_err(|error| format!("reading generated blob portable provenance: {error}"))?;
    let current_portable = embedded_node22_snapshot_portable_provenance()
        .map_err(|error| format!("computing current portable provenance: {error}"))?;
    if stored_portable != current_portable {
        return Err(format!(
            "generated embedded anchor snapshot is not portable for this build \
             (v8-pointer-compression={pc}): stored provenance {stored_portable:016x} != current \
             {current_portable:016x}; regenerate via `make build-node22-anchor-snapshot`.",
        ));
    }
    let stored_runtime_sources =
        packaged_runtime_extension_sources_from_embedded_blob(generated)
            .map_err(|error| format!("reading packaged runtime extension sources: {error}"))?;
    let current_runtime_sources = collect_packaged_runtime_extension_sources()
        .map_err(|error| format!("collecting current runtime extension sources: {error}"))?;
    if stored_runtime_sources != current_runtime_sources {
        return Err(format!(
            "generated embedded anchor snapshot has a stale packaged runtime extension source \
             table (v8-pointer-compression={pc}); regenerate via `make \
             build-node22-anchor-snapshot`."
        ));
    }
    v8_startup_snapshot_from_embedded_blob(generated)
        .map_err(|error| format!("generated embedded anchor snapshot fails to parse: {error}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn minimal_snapshot_blob() -> Vec<u8> {
        let mut blob = 0_u64.to_le_bytes().to_vec();
        blob.extend_from_slice(&0_u64.to_le_bytes());
        write_blob_bytes(&mut blob, b"snapshot");
        write_blob_table(&mut blob, &[]);
        write_blob_table(&mut blob, &[]);
        write_blob_table(&mut blob, &[]);
        blob
    }

    #[test]
    fn embedded_snapshot_blob_parser_accepts_the_current_layout() {
        let snapshot = v8_startup_snapshot_from_embedded_blob(&minimal_snapshot_blob())
            .expect("current embedded snapshot layout should parse");

        assert_eq!(snapshot.as_startup_snapshot(), b"snapshot");
        assert!(snapshot.residual_lazy_js_sources().is_empty());
        assert!(snapshot.residual_lazy_esm_sources().is_empty());
    }

    #[test]
    fn embedded_snapshot_blob_exposes_packaged_runtime_sources() {
        let mut blob = 0_u64.to_le_bytes().to_vec();
        blob.extend_from_slice(&0_u64.to_le_bytes());
        write_blob_bytes(&mut blob, b"snapshot");
        write_blob_table(&mut blob, &[]);
        write_blob_table(&mut blob, &[]);
        write_blob_table(
            &mut blob,
            &[("ext:nimbus/packaged.js", "globalThis.packaged = true;")],
        );

        let sources = packaged_runtime_extension_sources_from_embedded_blob(&blob)
            .expect("current embedded source-table layout should parse");

        assert_eq!(
            sources,
            &[("ext:nimbus/packaged.js", "globalThis.packaged = true;")]
        );
    }

    #[test]
    fn generated_blob_packages_the_supported_runtime_source_union() {
        let stored =
            packaged_runtime_extension_sources_from_embedded_blob(EMBEDDED_NODE22_ANCHOR_SNAPSHOT)
                .expect("generated embedded blob should contain the packaged runtime source table");
        let current = collect_packaged_runtime_extension_sources()
            .expect("supported runtime source union should load from the source checkout");

        assert_eq!(stored, current);
        assert!(!stored.is_empty());
    }

    #[test]
    fn embedded_snapshot_blob_parser_rejects_trailing_legacy_tables() {
        let mut blob = minimal_snapshot_blob();
        write_blob_table(&mut blob, &[]);

        let error = match v8_startup_snapshot_from_embedded_blob(&blob) {
            Ok(_) => panic!("legacy replay tables must not be ignored"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("trailing bytes"));
    }

    #[test]
    fn embedded_snapshot_blob_parser_rejects_impossible_table_count() {
        let mut blob = 0_u64.to_le_bytes().to_vec();
        blob.extend_from_slice(&0_u64.to_le_bytes());
        write_blob_bytes(&mut blob, b"snapshot");
        write_blob_len(&mut blob, usize::MAX);

        let error = match v8_startup_snapshot_from_embedded_blob(&blob) {
            Ok(_) => panic!("an impossible table count must fail before allocation"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("exceeds the remaining body"));
    }

    #[test]
    fn portable_snapshot_provenance_does_not_open_a_build_only_path() {
        let source = ExtensionFileSource::loaded_during_snapshot(
            "ext:nimbus/missing.js",
            "/nimbus-build-only-path-that-must-not-be-opened/missing.js",
        );
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        hash_snapshot_extension_source(&source, SnapshotProvenanceMode::Portable, &mut hasher)
            .expect("portable provenance must not open a build-only source path");

        assert_ne!(std::hash::Hasher::finish(&hasher), 0);
    }

    #[test]
    fn content_snapshot_provenance_requires_a_build_only_source() {
        let source = ExtensionFileSource::loaded_during_snapshot(
            "ext:nimbus/missing.js",
            "/nimbus-build-only-path-that-must-not-be-opened/missing.js",
        );
        let mut hasher = std::collections::hash_map::DefaultHasher::new();

        let error =
            hash_snapshot_extension_source(&source, SnapshotProvenanceMode::Content, &mut hasher)
                .expect_err(
                    "content provenance must read a build-only source while the checkout exists",
                );

        assert!(error.to_string().contains("No such file or directory"));
    }

    #[test]
    fn embedded_snapshot_blob_rejects_a_truncated_portable_header() {
        let blob = 0_u64.to_le_bytes();

        let error = embedded_blob_portable_provenance(&blob)
            .expect_err("a missing portable provenance header must fail");

        assert!(error.to_string().contains("portable provenance header"));
    }
}
