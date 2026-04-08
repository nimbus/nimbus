use deno_core::JsRuntime;

use crate::affinity::{RuntimeAffinityKey, runtime_affinity_key};
use crate::context::RuntimeInvocationContext;
use crate::error::Result;
use crate::limits::{RuntimeExecutionModel, RuntimePoolKind};
use crate::runtime::bundle::RuntimeBundleIdentity;
use crate::runtime::{NeovexRuntime, RuntimeBundle};

use super::startup::RuntimeConstructionMode;

pub(crate) struct RuntimeWorkerIsolatePool {
    warmed: bool,
    retained_runtimes: Vec<RetainedRuntimeEntry>,
    warm_pool: Vec<WarmPoolEntry>,
    next_retained_sequence: u64,
    next_warm_sequence: u64,
}

pub(crate) struct WarmPoolEntry {
    pub(crate) runtime: JsRuntime,
    pub(crate) bundle_identity: RuntimeBundleIdentity,
    pub(crate) affinity_key: Option<RuntimeAffinityKey>,
    pub(crate) reuse_count: usize,
    pub(crate) last_used_sequence: u64,
    pub(crate) construction_mode: RuntimeConstructionMode,
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
            warm_pool: Vec::new(),
            next_retained_sequence: 1,
            next_warm_sequence: 1,
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
            RuntimePoolKind::WarmModulePool => {
                let affinity_key = runtime_affinity_key(
                    runtime_owner.policy.limits().routing_affinity,
                    context,
                    bundle,
                );
                let bundle_identity = bundle.bundle_identity().clone();
                if let Some(entry) =
                    self.take_warm_pool_entry(&bundle_identity, affinity_key.as_ref())
                {
                    runtime_owner.policy.metrics().record_warm_pool_hit();
                    runtime_owner.policy.metrics().record_isolate_pool_hit();
                    self.warmed = true;
                    return Ok(ReusableRuntime {
                        runtime: entry.runtime,
                        retained_reuse_count: entry.reuse_count,
                        construction_mode: entry.construction_mode,
                    });
                }

                // Cold miss: build a fresh runtime
                runtime_owner.policy.metrics().record_warm_pool_miss();
                runtime_owner.policy.metrics().record_isolate_pool_miss();
                let snapshot = runtime_owner.bootstrap_snapshot()?;
                let runtime = runtime_owner.create_runtime(bundle, Some(snapshot), use_locker)?;
                self.warmed = true;
                return Ok(ReusableRuntime::fresh(
                    runtime,
                    RuntimeConstructionMode::StartupSnapshot,
                ));
            }
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
        self.return_runtime_with_affinity(runtime_owner, bundle, runtime, affinity_key);
    }

    fn return_runtime_with_affinity(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        mut runtime: ReusableRuntime,
        affinity_key: Option<RuntimeAffinityKey>,
    ) {
        match runtime_owner.policy.limits().runtime_pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {}
            RuntimePoolKind::WarmModulePool => {
                if runtime.runtime.is_v8_lock_held() {
                    runtime.runtime.release_v8_lock();
                }
                if runtime.retained_reuse_count
                    >= runtime_owner.policy.limits().max_warm_module_reuses
                {
                    runtime_owner.policy.metrics().record_warm_pool_retirement();
                    return;
                }
                let last_used_sequence = self.next_warm_sequence();
                self.warm_pool.push(WarmPoolEntry {
                    runtime: runtime.runtime,
                    bundle_identity: bundle.bundle_identity().clone(),
                    affinity_key,
                    reuse_count: runtime.retained_reuse_count,
                    last_used_sequence,
                    construction_mode: runtime.construction_mode,
                });
                runtime_owner
                    .policy
                    .metrics()
                    .increment_retained_runtime_pool_entries();
                self.enforce_warm_pool_bounds(runtime_owner);
            }
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
                tracing::warn!(
                    bundle = %bundle.entrypoint().display(),
                    affinity_key = ?entry.affinity_key,
                    retained_reuse_count = entry.retained_reuse_count,
                    construction_mode = entry.construction_mode.as_str(),
                    error = %error,
                    "reset failed for retained runtime; replacing runtime"
                );
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

    fn take_warm_pool_entry(
        &mut self,
        bundle_identity: &RuntimeBundleIdentity,
        affinity_key: Option<&RuntimeAffinityKey>,
    ) -> Option<WarmPoolEntry> {
        // Prefer exact bundle identity + affinity match (most recently used).
        let exact_index = self
            .warm_pool
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                &entry.bundle_identity == bundle_identity
                    && entry.affinity_key.as_ref() == affinity_key
            })
            .max_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index);

        if let Some(index) = exact_index {
            return Some(self.warm_pool.swap_remove(index));
        }

        // Fall back to bundle identity match with any affinity.
        let bundle_index = self
            .warm_pool
            .iter()
            .enumerate()
            .filter(|(_, entry)| &entry.bundle_identity == bundle_identity)
            .max_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index);

        bundle_index.map(|index| self.warm_pool.swap_remove(index))
    }

    fn next_warm_sequence(&mut self) -> u64 {
        let sequence = self.next_warm_sequence;
        self.next_warm_sequence = self.next_warm_sequence.saturating_add(1);
        sequence
    }

    fn enforce_warm_pool_bounds(&mut self, runtime_owner: &NeovexRuntime) {
        let max_entries = runtime_owner
            .policy
            .limits()
            .max_warm_module_pool_entries_per_worker;
        while self.warm_pool.len() > max_entries {
            // Evict LRU
            if let Some(index) = self
                .warm_pool
                .iter()
                .enumerate()
                .min_by_key(|(_, entry)| entry.last_used_sequence)
                .map(|(index, _)| index)
            {
                self.warm_pool.swap_remove(index);
                runtime_owner
                    .policy
                    .metrics()
                    .record_retained_runtime_pool_eviction();
                runtime_owner
                    .policy
                    .metrics()
                    .decrement_retained_runtime_pool_entries();
            } else {
                break;
            }
        }
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn warm_pool_count_for_test(&self) -> usize {
        self.warm_pool.len()
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
