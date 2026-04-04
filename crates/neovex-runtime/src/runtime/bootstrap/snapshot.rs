#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use deno_core::{JsRuntime, JsRuntimeForSnapshot, RuntimeOptions};

use crate::error::Result;
use crate::runtime::{NeovexRuntime, RuntimeBundle};

use super::{install_bootstrap, runtime_extension};

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

pub(crate) struct RuntimeWorkerIsolatePool {
    warmed: bool,
}

impl RuntimeWorkerIsolatePool {
    pub(crate) fn new() -> Self {
        Self { warmed: false }
    }

    pub(crate) fn take_runtime(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
    ) -> Result<JsRuntime> {
        let snapshot = runtime_owner.bootstrap_snapshot()?;
        if self.warmed {
            runtime_owner.policy.metrics().record_isolate_pool_hit();
            runtime_owner.create_runtime_from_snapshot(bundle, snapshot)
        } else {
            runtime_owner.policy.metrics().record_isolate_pool_miss();
            let runtime = runtime_owner.create_runtime_from_snapshot(bundle, snapshot)?;
            self.warmed = true;
            Ok(runtime)
        }
    }

    pub(crate) fn record_replacement(&self, runtime_owner: &NeovexRuntime) {
        runtime_owner
            .policy
            .metrics()
            .record_isolate_pool_replacement();
    }
}

pub(crate) fn create_bootstrap_snapshot() -> Result<RuntimeStartupSnapshot> {
    #[cfg(test)]
    RUNTIME_BOOTSTRAP_SNAPSHOT_BUILDS.fetch_add(1, Ordering::Relaxed);

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
