#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use deno_core::{JsRuntimeForSnapshot, RuntimeOptions};

use crate::error::Result;

use super::super::{install_bootstrap, runtime_extension};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeConstructionMode {
    #[allow(dead_code)] // Used only in test code after RetainedJsRuntimePool removal
    Unsnapshotted,
    StartupSnapshot,
}

impl RuntimeConstructionMode {
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

pub(crate) struct RuntimeStartupSnapshot {
    bytes: &'static [u8],
}

impl RuntimeStartupSnapshot {
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
static RUNTIME_BOOTSTRAP_SNAPSHOT_BUILDS: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn create_bootstrap_snapshot() -> Result<RuntimeStartupSnapshot> {
    #[cfg(test)]
    RUNTIME_BOOTSTRAP_SNAPSHOT_BUILDS.fetch_add(1, Ordering::Relaxed);

    // BOOTSTRAP_SOURCE runs here too, so keep it snapshot-safe. In particular,
    // post-bootstrap cleanup like `delete globalThis.Deno` must stay in the
    // separate finalize step for ordinary runtimes until the fork offers an
    // explicit snapshot-safe replacement.
    let mut runtime = JsRuntimeForSnapshot::new(RuntimeOptions {
        extensions: vec![runtime_extension()],
        ..Default::default()
    });
    install_bootstrap(&mut runtime)?;
    Ok(RuntimeStartupSnapshot::new(runtime.snapshot()))
}

#[cfg(test)]
pub(crate) fn bootstrap_snapshot_build_count_for_test() -> usize {
    RUNTIME_BOOTSTRAP_SNAPSHOT_BUILDS.load(Ordering::Relaxed)
}
