#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};

use deno_core::{JsRuntime, JsRuntimeForSnapshot, RuntimeOptions};

use crate::affinity::{RuntimeAffinityKey, runtime_affinity_key};
use crate::context::RuntimeInvocationContext;
use crate::error::Result;
use crate::limits::{RuntimeExecutionModel, RuntimePoolKind};
use crate::runtime::{NeovexRuntime, RuntimeBundle};

use super::{install_bootstrap, runtime_extension};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeConstructionMode {
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

pub(crate) struct RuntimeWorkerIsolatePool {
    warmed: bool,
    retained_runtimes: Vec<RetainedRuntimeEntry>,
    next_retained_sequence: u64,
}

pub(crate) struct ReusableRuntime {
    pub(crate) runtime: JsRuntime,
    pub(crate) retained_reuse_count: usize,
    pub(crate) construction_mode: RuntimeConstructionMode,
}

impl ReusableRuntime {
    pub(crate) fn fresh(runtime: JsRuntime, construction_mode: RuntimeConstructionMode) -> Self {
        Self {
            runtime,
            retained_reuse_count: 0,
            construction_mode,
        }
    }
}

struct RetainedRuntimeEntry {
    runtime: JsRuntime,
    affinity_key: Option<RuntimeAffinityKey>,
    last_used_sequence: u64,
    retained_reuse_count: usize,
    construction_mode: RuntimeConstructionMode,
}

impl RuntimeWorkerIsolatePool {
    pub(crate) fn new() -> Self {
        Self {
            warmed: false,
            retained_runtimes: Vec::new(),
            next_retained_sequence: 1,
        }
    }

    #[cfg(test)]
    pub(crate) fn take_runtime(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
    ) -> Result<ReusableRuntime> {
        self.take_runtime_with_options(runtime_owner, bundle, false)
    }

    #[cfg(test)]
    pub(crate) fn take_runtime_with_options(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        use_locker: bool,
    ) -> Result<ReusableRuntime> {
        self.take_runtime_with_options_for_invocation(runtime_owner, bundle, None, use_locker)
    }

    pub(crate) fn take_runtime_for_invocation(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
    ) -> Result<ReusableRuntime> {
        self.take_runtime_with_options_for_invocation(runtime_owner, bundle, context, false)
    }

    pub(crate) fn take_runtime_with_options_for_invocation(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        use_locker: bool,
    ) -> Result<ReusableRuntime> {
        match runtime_owner.policy.limits().runtime_pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {}
            RuntimePoolKind::RetainedJsRuntimePool => {
                let affinity_key = runtime_affinity_key(
                    runtime_owner.policy.limits().routing_affinity,
                    context,
                    bundle,
                );
                if let Some(runtime) =
                    self.take_retained_runtime_for_invocation(runtime_owner, bundle, &affinity_key)?
                {
                    self.warmed = true;
                    return Ok(runtime);
                }

                runtime_owner.policy.metrics().record_isolate_pool_miss();
                let construction_mode = runtime_owner.retained_runtime_construction_mode();
                // The retained-runtime path normally rebuilds the main realm
                // through the deno_core fork's fresh-main-realm API and keeps
                // the retained runtime on the same unsnapshotted construction
                // model so reset/reuse stays on one consistent bootstrap path.
                // Tests can opt into startup-snapshot construction explicitly
                // so we can investigate retained snapshot reuse without
                // changing the production contract.
                let runtime = if construction_mode.uses_startup_snapshot() {
                    let snapshot = runtime_owner.bootstrap_snapshot()?;
                    runtime_owner.create_runtime(bundle, Some(snapshot), use_locker)?
                } else {
                    runtime_owner.create_runtime(bundle, None, use_locker)?
                };
                if construction_mode.uses_startup_snapshot() {
                    tracing::debug!(
                        bundle = %bundle.entrypoint().display(),
                        affinity_key = ?affinity_key,
                        retained_pool_entries = self.retained_runtimes.len(),
                        use_locker,
                        construction_mode = construction_mode.as_str(),
                        "building retained runtime from startup snapshot on cold miss"
                    );
                }
                self.warmed = true;
                return Ok(ReusableRuntime::fresh(runtime, construction_mode));
            }
        }
        let snapshot = runtime_owner.bootstrap_snapshot()?;
        if self.warmed {
            runtime_owner.policy.metrics().record_isolate_pool_hit();
            runtime_owner
                .create_runtime(bundle, Some(snapshot), use_locker)
                .map(|runtime| {
                    ReusableRuntime::fresh(runtime, RuntimeConstructionMode::StartupSnapshot)
                })
        } else {
            runtime_owner.policy.metrics().record_isolate_pool_miss();
            let runtime = runtime_owner.create_runtime(bundle, Some(snapshot), use_locker)?;
            self.warmed = true;
            Ok(ReusableRuntime::fresh(
                runtime,
                RuntimeConstructionMode::StartupSnapshot,
            ))
        }
    }

    pub(crate) fn return_runtime_for_invocation(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        runtime: ReusableRuntime,
    ) {
        let affinity_key = runtime_affinity_key(
            runtime_owner.policy.limits().routing_affinity,
            context,
            bundle,
        );
        self.return_runtime_with_affinity(runtime_owner, runtime, affinity_key);
    }

    fn return_runtime_with_affinity(
        &mut self,
        runtime_owner: &NeovexRuntime,
        mut runtime: ReusableRuntime,
        affinity_key: Option<RuntimeAffinityKey>,
    ) {
        match runtime_owner.policy.limits().runtime_pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {}
            RuntimePoolKind::RetainedJsRuntimePool => {
                if matches!(
                    runtime_owner.policy.limits().execution_model,
                    RuntimeExecutionModel::CooperativeLocker
                ) && runtime.runtime.is_v8_lock_held()
                {
                    runtime.runtime.release_v8_lock();
                }
                if runtime.retained_reuse_count
                    >= runtime_owner.policy.limits().max_retained_runtime_reuses
                {
                    runtime_owner
                        .policy
                        .metrics()
                        .record_retained_runtime_pool_retirement();
                    return;
                }
                let last_used_sequence = self.next_retained_sequence();
                if runtime.construction_mode.uses_startup_snapshot() {
                    tracing::debug!(
                        affinity_key = ?affinity_key,
                        retained_reuse_count = runtime.retained_reuse_count,
                        retained_pool_entries_before = self.retained_runtimes.len(),
                        construction_mode = runtime.construction_mode.as_str(),
                        "returning snapshot-seeded retained runtime to worker-local pool"
                    );
                }
                self.retained_runtimes.push(RetainedRuntimeEntry {
                    runtime: runtime.runtime,
                    affinity_key,
                    last_used_sequence,
                    retained_reuse_count: runtime.retained_reuse_count,
                    construction_mode: runtime.construction_mode,
                });
                runtime_owner
                    .policy
                    .metrics()
                    .increment_retained_runtime_pool_entries();
                self.enforce_retained_runtime_bounds(runtime_owner);
            }
        }
    }

    fn take_retained_runtime_for_invocation(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        affinity_key: &Option<RuntimeAffinityKey>,
    ) -> Result<Option<ReusableRuntime>> {
        while let Some(index) = self.matching_retained_runtime_index(affinity_key.as_ref()) {
            if let Some(runtime) = self.try_reuse_retained_runtime(runtime_owner, bundle, index)? {
                return Ok(Some(runtime));
            }
        }

        let should_reuse_lru = affinity_key.is_none()
            || self.retained_runtimes.len() >= self.max_retained_runtimes_per_worker(runtime_owner);
        if should_reuse_lru {
            while let Some(index) = self.lru_retained_runtime_index() {
                if let Some(runtime) =
                    self.try_reuse_retained_runtime(runtime_owner, bundle, index)?
                {
                    return Ok(Some(runtime));
                }
            }
        }

        Ok(None)
    }

    fn try_reuse_retained_runtime(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        index: usize,
    ) -> Result<Option<ReusableRuntime>> {
        let mut entry = self.remove_retained_runtime(index, runtime_owner);
        if entry.construction_mode.uses_startup_snapshot() {
            tracing::debug!(
                bundle = %bundle.entrypoint().display(),
                affinity_key = ?entry.affinity_key,
                retained_reuse_count = entry.retained_reuse_count,
                retained_pool_entries_after_take = self.retained_runtimes.len(),
                construction_mode = entry.construction_mode.as_str(),
                "reusing snapshot-seeded retained runtime from worker-local pool"
            );
        }
        match runtime_owner.reset_retained_runtime(
            &mut entry.runtime,
            bundle,
            entry.construction_mode,
        ) {
            Ok(()) => {
                runtime_owner.policy.metrics().record_isolate_pool_hit();
                Ok(Some(ReusableRuntime {
                    runtime: entry.runtime,
                    retained_reuse_count: entry.retained_reuse_count.saturating_add(1),
                    construction_mode: entry.construction_mode,
                }))
            }
            Err(error) => {
                if entry.construction_mode.uses_startup_snapshot() {
                    tracing::warn!(
                        bundle = %bundle.entrypoint().display(),
                        affinity_key = ?entry.affinity_key,
                        retained_reuse_count = entry.retained_reuse_count,
                        construction_mode = entry.construction_mode.as_str(),
                        error = %error,
                        "reset failed for snapshot-seeded retained runtime; replacing runtime"
                    );
                }
                runtime_owner
                    .policy
                    .metrics()
                    .record_isolate_pool_replacement();
                Ok(None)
            }
        }
    }

    fn matching_retained_runtime_index(
        &self,
        affinity_key: Option<&RuntimeAffinityKey>,
    ) -> Option<usize> {
        self.retained_runtimes
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.affinity_key.as_ref() == affinity_key)
            .max_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index)
    }

    fn lru_retained_runtime_index(&self) -> Option<usize> {
        self.retained_runtimes
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index)
    }

    fn next_retained_sequence(&mut self) -> u64 {
        let sequence = self.next_retained_sequence;
        self.next_retained_sequence = self.next_retained_sequence.saturating_add(1);
        sequence
    }

    fn enforce_retained_runtime_bounds(&mut self, runtime_owner: &NeovexRuntime) {
        let distinct_affinity_keys = self
            .retained_runtimes
            .iter()
            .map(|entry| entry.affinity_key.clone())
            .collect::<Vec<_>>();
        for affinity_key in distinct_affinity_keys {
            self.enforce_retained_runtime_affinity_limit(runtime_owner, affinity_key.as_ref());
        }

        while self.retained_runtimes.len() > self.max_retained_runtimes_per_worker(runtime_owner) {
            if self.remove_lru_matching_retained_runtime(|_| true, runtime_owner) {
                runtime_owner
                    .policy
                    .metrics()
                    .record_isolate_pool_replacement();
                runtime_owner
                    .policy
                    .metrics()
                    .record_retained_runtime_pool_eviction();
            } else {
                break;
            }
        }
    }

    fn enforce_retained_runtime_affinity_limit(
        &mut self,
        runtime_owner: &NeovexRuntime,
        affinity_key: Option<&RuntimeAffinityKey>,
    ) {
        let limit = match affinity_key {
            Some(_) => runtime_owner
                .policy
                .limits()
                .max_retained_runtimes_per_affinity_key_per_worker
                .min(self.max_retained_runtimes_per_worker(runtime_owner)),
            None => 1,
        };
        while self
            .retained_runtimes
            .iter()
            .filter(|entry| entry.affinity_key.as_ref() == affinity_key)
            .count()
            > limit
        {
            if self.remove_lru_matching_retained_runtime(
                |entry| entry.affinity_key.as_ref() == affinity_key,
                runtime_owner,
            ) {
                runtime_owner
                    .policy
                    .metrics()
                    .record_isolate_pool_replacement();
                runtime_owner
                    .policy
                    .metrics()
                    .record_retained_runtime_pool_eviction();
            } else {
                break;
            }
        }
    }

    fn remove_lru_matching_retained_runtime(
        &mut self,
        predicate: impl Fn(&RetainedRuntimeEntry) -> bool,
        runtime_owner: &NeovexRuntime,
    ) -> bool {
        let Some(index) = self
            .retained_runtimes
            .iter()
            .enumerate()
            .filter(|(_, entry)| predicate(entry))
            .min_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index)
        else {
            return false;
        };
        self.remove_retained_runtime(index, runtime_owner);
        true
    }

    fn remove_retained_runtime(
        &mut self,
        index: usize,
        runtime_owner: &NeovexRuntime,
    ) -> RetainedRuntimeEntry {
        let entry = self.retained_runtimes.swap_remove(index);
        runtime_owner
            .policy
            .metrics()
            .decrement_retained_runtime_pool_entries();
        entry
    }

    fn max_retained_runtimes_per_worker(&self, runtime_owner: &NeovexRuntime) -> usize {
        match runtime_owner.policy.limits().execution_model {
            RuntimeExecutionModel::CooperativeLocker => {
                runtime_owner
                    .policy
                    .limits()
                    .max_retained_runtimes_per_worker
            }
            RuntimeExecutionModel::RunToCompletion => 1,
        }
    }

    #[cfg(test)]
    pub(crate) fn retained_runtime_count_for_test(&self) -> usize {
        self.retained_runtimes.len()
    }

    #[cfg(test)]
    pub(crate) fn retained_runtime_affinity_keys_for_test(
        &self,
    ) -> Vec<Option<RuntimeAffinityKey>> {
        self.retained_runtimes
            .iter()
            .map(|entry| entry.affinity_key.clone())
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn retained_runtime_construction_modes_for_test(
        &self,
    ) -> Vec<RuntimeConstructionMode> {
        self.retained_runtimes
            .iter()
            .map(|entry| entry.construction_mode)
            .collect()
    }
}

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
