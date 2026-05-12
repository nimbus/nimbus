#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::error::Result;
use crate::limits::RuntimeCompatibilityTarget;
use crate::runtime::bootstrap::{
    extension_transpiler_for_target, install_bootstrap, snapshot_extensions,
};

use super::embedder::{JsRuntimeForSnapshot, RuntimeOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum V8RuntimeConstructionMode {
    #[allow(dead_code)] // Used only in test helpers
    Unsnapshotted,
    StartupSnapshot,
}

impl V8RuntimeConstructionMode {
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
}

impl V8StartupSnapshot {
    fn new(bytes: Box<[u8]>) -> Self {
        // deno_core currently accepts startup snapshots as &'static [u8]. The
        // worker pool keeps a single bootstrap snapshot for its own lifetime,
        // so leaking one buffer per worker matches the pool's lifetime and
        // avoids unsound lifetime extension tricks.
        Self {
            bytes: Box::leak(bytes),
        }
    }

    pub(crate) fn as_startup_snapshot(&self) -> &'static [u8] {
        self.bytes
    }
}

#[cfg(test)]
static V8_BOOTSTRAP_SNAPSHOT_BUILDS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn create_v8_startup_snapshot(
    compatibility_target: RuntimeCompatibilityTarget,
) -> Result<V8StartupSnapshot> {
    #[cfg(test)]
    V8_BOOTSTRAP_SNAPSHOT_BUILDS.fetch_add(1, Ordering::Relaxed);

    // BOOTSTRAP_SOURCE runs here too, so keep it snapshot-safe. In particular,
    // post-bootstrap cleanup like `delete globalThis.Deno` must stay in the
    // separate finalize step for ordinary runtimes until the fork offers an
    // explicit snapshot-safe replacement.
    let mut runtime = JsRuntimeForSnapshot::new(RuntimeOptions {
        extensions: snapshot_extensions(compatibility_target),
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
    Ok(V8StartupSnapshot::new(runtime.snapshot()))
}

#[cfg(test)]
pub(crate) fn v8_bootstrap_snapshot_build_count_for_test() -> usize {
    V8_BOOTSTRAP_SNAPSHOT_BUILDS.load(Ordering::Relaxed)
}
