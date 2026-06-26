#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::Result;
use crate::limits::RuntimeCompatibilityTarget;
use crate::runtime::bootstrap::{
    extension_transpiler_for_target, install_bootstrap, snapshot_extensions,
};

use super::embedder::{
    Extension, JsRuntimeForSnapshot, ModuleCodeString, ModuleName, RuntimeOptions,
};

type ResidualLazySources = &'static [(&'static str, &'static str)];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum V8RuntimeConstructionMode {
    #[allow(dead_code)] // Used only in test helpers
    Unsnapshotted,
    StartupSnapshot,
}

impl V8RuntimeConstructionMode {
    pub(crate) fn for_compatibility_target(target: RuntimeCompatibilityTarget) -> Self {
        debug_assert!(
            target != RuntimeCompatibilityTarget::BunJsc,
            "Bun/JSC must not enter the V8 construction-mode selector"
        );
        Self::StartupSnapshot
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
        ..Default::default()
    });
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
