#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::Result;
use crate::limits::RuntimeCompatibilityTarget;
use crate::runtime::bootstrap::{
    extension_transpiler_for_target, install_bootstrap, install_missing_deno_extension_state,
    snapshot_extensions,
};

use super::embedder::{
    Extension, JsRuntimeForSnapshot, ModuleCodeString, ModuleName, RuntimeOptions,
};

type ResidualLazySources = &'static [(&'static str, &'static str)];

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
    extension_replay_js_sources: ResidualLazySources,
    extension_replay_esm_sources: ResidualLazySources,
}

impl V8StartupSnapshot {
    fn new(
        bytes: Box<[u8]>,
        residual_lazy_js_sources: ResidualLazySources,
        residual_lazy_esm_sources: ResidualLazySources,
        extension_replay_js_sources: ResidualLazySources,
        extension_replay_esm_sources: ResidualLazySources,
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
            extension_replay_js_sources,
            extension_replay_esm_sources,
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

    pub(crate) fn extension_replay_js_sources(&self) -> ResidualLazySources {
        self.extension_replay_js_sources
    }

    pub(crate) fn extension_replay_esm_sources(&self) -> ResidualLazySources {
        self.extension_replay_esm_sources
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
    let (
        residual_lazy_js_sources,
        residual_lazy_esm_sources,
        extension_replay_js_sources,
        extension_replay_esm_sources,
    ) = collect_startup_snapshot_extension_sources(compatibility_target, &extensions)?;
    let mut runtime = JsRuntimeForSnapshot::new(RuntimeOptions {
        extensions,
        extension_transpiler: extension_transpiler_for_target(compatibility_target),
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
        extension_replay_js_sources,
        extension_replay_esm_sources,
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
// the installed read-only heap is byte-identical (cage-critical). Blob format (LE, length-prefixed):
// the V8 startup bytes, then the four residual/replay (name, source) source tables.

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
        Ok(value as usize)
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
        let mut pairs: Vec<(&'static str, &'static str)> = Vec::with_capacity(count);
        for _ in 0..count {
            let name = self.read_static_str()?;
            let source = self.read_static_str()?;
            pairs.push((name, source));
        }
        Ok(Box::leak(pairs.into_boxed_slice()))
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
const EMBEDDED_SNAPSHOT_SCHEMA_VERSION: u64 = 3;

/// Provenance hash over EVERYTHING that determines the NodeFull snapshot's RO heap, computed
/// IDENTICALLY at build time (in the blob) and at runtime (here). The embedded blob is used ONLY
/// when this equals the blob's stored provenance; on ANY mismatch the serving path falls back to a
/// runtime build (slow-but-correct), NEVER installs a stale snapshot (which would silently
/// reinstall the cross-profile cage collision baked into the binary). Coverage:
///   - the build TARGET arch + OS (a V8 startup snapshot is platform-specific — a darwin-arm64
///     snapshot deserialized on linux-x86_64 is a hard V8_Fatal; the committed blob is per-platform,
///     so a foreign-target blob MUST mismatch and fall back, never install);
///   - the V8 version (snapshots are V8-version-specific; skew must not install);
///   - the `v8-pointer-compression` feature (release ships pointer-compressed; V8 refuses to
///     deserialize a snapshot built under a different pointer-compression config — installing a
///     mismatched blob is a hard abort, so this MUST gate the cage's single-shared-heap layout);
///   - the exact extension SELECTION (names) for this (target, service-extension);
///   - the OP SURFACE (every op name, in declaration order) bound into the bootstrap context;
///   - the full source TEXT of every extension js/esm file (nimbus AND deno bootstrap JS);
///   - the schema constant (build-logic changes not visible in the structured inputs above).
///
/// NOTE: a byte-for-byte compare of the snapshot is deliberately NOT used as a backstop — V8 bakes
/// a random hash-seed into the startup snapshot, so two builds from identical inputs differ in the
/// snapshot bytes (empirically at offset ~24). The hash above is over INPUTS, which ARE
/// deterministic, so it is the correct staleness gate. The residual hole — a Rust op change that
/// alters bootstrap behavior without changing any op name or JS source — is covered by bumping
/// `EMBEDDED_SNAPSHOT_SCHEMA_VERSION`.
fn embedded_node22_snapshot_provenance() -> Result<u64> {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    EMBEDDED_SNAPSHOT_SCHEMA_VERSION.hash(&mut hasher);
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
        for file in extension.esm_files.iter().chain(extension.js_files.iter()) {
            file.specifier.hash(&mut hasher);
            file.load()?.as_bytes().hash(&mut hasher);
        }
    }
    Ok(hasher.finish())
}

/// Build the NodeFull(Node22) anchor snapshot and serialize it — leading provenance hash, then the
/// V8 bytes, then the four residual/replay source tables — into a self-describing blob for
/// compile-time embedding. Invoked by the snapshot builder binary, NOT on the serving path.
pub fn build_embeddable_node22_snapshot_blob() -> Result<Vec<u8>> {
    let (target, service_extension_enabled) = embeddable_node22_snapshot_target();
    // Compute provenance over the SAME inputs the build will consume, BEFORE building.
    let provenance = embedded_node22_snapshot_provenance()?;
    let snapshot = create_v8_startup_snapshot(target, service_extension_enabled)?;
    let mut out = Vec::new();
    out.extend_from_slice(&provenance.to_le_bytes());
    write_blob_bytes(&mut out, snapshot.as_startup_snapshot());
    write_blob_table(&mut out, snapshot.residual_lazy_js_sources());
    write_blob_table(&mut out, snapshot.residual_lazy_esm_sources());
    write_blob_table(&mut out, snapshot.extension_replay_js_sources());
    write_blob_table(&mut out, snapshot.extension_replay_esm_sources());
    Ok(out)
}

/// The provenance hash stored in the leading 8 bytes of an embedded blob.
fn embedded_blob_provenance(blob: &[u8]) -> Result<u64> {
    let slice = blob.get(0..8).ok_or_else(|| {
        crate::error::NimbusRuntimeError::Contract(
            "embedded snapshot blob truncated (provenance header)".to_string(),
        )
    })?;
    Ok(u64::from_le_bytes(slice.try_into().unwrap()))
}

/// Reconstruct a `V8StartupSnapshot` from an embedded blob (the serving path: deserialize instead
/// of building ~4.18s). The bytes are the runtime-built snapshot, so the installed RO heap is
/// identical — cage-critical. Callers MUST first confirm `embedded_blob_provenance` matches
/// `embedded_node22_snapshot_provenance`; this only parses.
pub(crate) fn v8_startup_snapshot_from_embedded_blob(blob: &[u8]) -> Result<V8StartupSnapshot> {
    let mut reader = EmbeddedSnapshotBlobReader { data: blob, pos: 8 };
    let bytes = reader.read_bytes_owned()?.into_boxed_slice();
    let residual_lazy_js_sources = reader.read_table()?;
    let residual_lazy_esm_sources = reader.read_table()?;
    let extension_replay_js_sources = reader.read_table()?;
    let extension_replay_esm_sources = reader.read_table()?;
    Ok(V8StartupSnapshot::new(
        bytes,
        residual_lazy_js_sources,
        residual_lazy_esm_sources,
        extension_replay_js_sources,
        extension_replay_esm_sources,
    ))
}

/// Fail-safe loader for the embedded NodeFull snapshot. Returns `Some` ONLY when the embedded blob's
/// provenance matches the current binary's provenance AND it parses cleanly. On ANY doubt — stale
/// provenance, truncation, parse error — returns `None`, and the caller MUST fall back to a runtime
/// build. The embedded path is the optimization; the runtime build is the correctness floor.
pub(crate) fn try_embedded_node22_anchor_snapshot(blob: &[u8]) -> Option<V8StartupSnapshot> {
    let stored = embedded_blob_provenance(blob).ok()?;
    let current = embedded_node22_snapshot_provenance().ok()?;
    if stored != current {
        eprintln!(
            "nimbus-runtime: embedded NodeFull anchor snapshot is STALE (provenance {stored:016x} \
             != current {current:016x}); falling back to a runtime build. Regenerate via \
             `make build-node22-anchor-snapshot`."
        );
        return None;
    }
    v8_startup_snapshot_from_embedded_blob(blob).ok()
}

/// The committed NodeFull(Node22) anchor snapshot blob (regenerated by the
/// `build_node22_anchor_snapshot` binary). TWO blobs are committed, one per pointer-compression
/// config, because a V8 startup snapshot is NOT portable across the `v8-pointer-compression`
/// feature (release ships pointer-compressed; dev/test runs feature-off). The matching blob is
/// selected here at compile time so BOTH configs HIT the embedded fast path; the provenance guard
/// (which hashes the pc feature) is the defense-in-depth backstop if the wrong file is ever placed
/// at a path. A fresh checkout / pre-generation has an EMPTY placeholder (written by build.rs),
/// which fails the provenance guard and falls back to a runtime build.
#[cfg(feature = "v8-pointer-compression")]
pub(crate) static EMBEDDED_NODE22_ANCHOR_SNAPSHOT: &[u8] =
    include_bytes!("node22_anchor_snapshot.pc.bin");
#[cfg(not(feature = "v8-pointer-compression"))]
pub(crate) static EMBEDDED_NODE22_ANCHOR_SNAPSHOT: &[u8] =
    include_bytes!("node22_anchor_snapshot.bin");

fn collect_startup_snapshot_extension_sources(
    compatibility_target: RuntimeCompatibilityTarget,
    extensions: &[Extension],
) -> Result<(
    ResidualLazySources,
    ResidualLazySources,
    ResidualLazySources,
    ResidualLazySources,
)> {
    let mut residual_lazy_js_sources = Vec::new();
    let mut residual_lazy_esm_sources = Vec::new();
    let mut extension_replay_js_sources = Vec::new();
    let mut extension_replay_esm_sources = Vec::new();

    for extension in extensions {
        for file in &*extension.js_files {
            if !file.is_runtime_loadable() {
                extension_replay_js_sources.push(transpile_snapshot_extension_source(
                    compatibility_target,
                    file.specifier,
                    file.load()?,
                )?);
            }
        }
        for file in &*extension.esm_files {
            if !file.is_runtime_loadable() {
                extension_replay_esm_sources.push(transpile_snapshot_extension_source(
                    compatibility_target,
                    file.specifier,
                    file.load()?,
                )?);
            }
        }
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
        leak_snapshot_extension_sources(residual_lazy_js_sources)?,
        leak_snapshot_extension_sources(residual_lazy_esm_sources)?,
        leak_snapshot_extension_sources(extension_replay_js_sources)?,
        leak_snapshot_extension_sources(extension_replay_esm_sources)?,
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

fn leak_snapshot_extension_sources(
    mut sources: Vec<(&'static str, String)>,
) -> Result<ResidualLazySources> {
    sources.sort_by_key(|(specifier, _)| *specifier);
    let mut deduped = Vec::<(&'static str, String)>::new();
    for (specifier, source) in sources {
        if let Some((last_specifier, last_source)) = deduped.last()
            && *last_specifier == specifier
        {
            if last_source != &source {
                return Err(crate::error::NimbusRuntimeError::JavaScript(format!(
                    "conflicting residual extension source for {specifier}"
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

/// Validate the committed embedded NodeFull(Node22) anchor blob for THIS build config, returning
/// `Err(message)` on any staleness or corruption. Confirms: (1) the blob is non-empty (not a fresh
/// placeholder); (2) its stored provenance equals the provenance recomputed from the current
/// binary's inputs (V8 version, pointer-compression feature, extension selection, op surface,
/// bootstrap JS source, schema version); (3) the blob deserializes structurally. This is the
/// staleness gate the serving path relies on, run eagerly so a mismatch fails CI loudly rather than
/// silently degrading to a ~4.18s runtime build at first request. It MUST run in a NON-`cfg(test)`
/// build — under `cfg(test)` `snapshot_extensions` includes a test-only extension, so the recomputed
/// provenance legitimately differs from the committed production blob. Invoked by
/// `build_node22_anchor_snapshot --check` (per pointer-compression config) in CI. A byte-for-byte
/// compare is deliberately NOT used: V8 bakes a random hash-seed into the snapshot, so the bytes are
/// not reproducible across builds (see `embedded_node22_snapshot_provenance`).
pub fn check_committed_embedded_anchor_snapshot(
    committed: &[u8],
) -> std::result::Result<(), String> {
    let pc = cfg!(feature = "v8-pointer-compression");
    if committed.is_empty() {
        return Err(format!(
            "committed embedded anchor snapshot is an EMPTY placeholder \
             (v8-pointer-compression={pc}); generate it with `make build-node22-anchor-snapshot`",
        ));
    }
    let stored = embedded_blob_provenance(committed)
        .map_err(|error| format!("reading committed blob provenance: {error}"))?;
    let current = embedded_node22_snapshot_provenance()
        .map_err(|error| format!("computing current provenance: {error}"))?;
    if stored != current {
        return Err(format!(
            "committed embedded anchor snapshot is STALE (v8-pointer-compression={pc}): stored \
             provenance {stored:016x} != current {current:016x}; the serving path would fall back \
             to a ~4.18s runtime build. Regenerate via `make build-node22-anchor-snapshot`.",
        ));
    }
    v8_startup_snapshot_from_embedded_blob(committed)
        .map_err(|error| format!("committed embedded anchor snapshot fails to parse: {error}"))?;
    Ok(())
}
