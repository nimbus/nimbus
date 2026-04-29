use crate::affinity::{RuntimeAffinityKey, runtime_affinity_key};
use crate::context::RuntimeInvocationContext;
use crate::error::Result;
use crate::limits::{RuntimeCompatibilityTarget, RuntimePoolKind};
use crate::runtime::{NeovexRuntime, RuntimeBundle, RuntimeBundleIdentity};

use super::{embedder::JsRuntime, startup::V8RuntimeConstructionMode};

pub(crate) struct V8WorkerRuntimePool {
    warmed: bool,
    warm_pool: Vec<WarmPoolEntry>,
    next_warm_sequence: u64,
}

pub(crate) struct WarmPoolEntry {
    pub(crate) runtime: JsRuntime,
    pub(crate) bundle_identity: RuntimeBundleIdentity,
    pub(crate) affinity_key: Option<RuntimeAffinityKey>,
    pub(crate) reuse_count: usize,
    pub(crate) last_used_sequence: u64,
    pub(crate) construction_mode: V8RuntimeConstructionMode,
}

pub(crate) struct ReusableV8Runtime {
    pub(crate) runtime: JsRuntime,
    pub(crate) warm_reuse_count: usize,
    pub(crate) construction_mode: V8RuntimeConstructionMode,
}

impl ReusableV8Runtime {
    pub(crate) fn fresh(runtime: JsRuntime, construction_mode: V8RuntimeConstructionMode) -> Self {
        Self {
            runtime,
            warm_reuse_count: 0,
            construction_mode,
        }
    }
}

impl V8WorkerRuntimePool {
    pub(crate) fn new() -> Self {
        Self {
            warmed: false,
            warm_pool: Vec::new(),
            next_warm_sequence: 1,
        }
    }

    #[cfg(test)]
    pub(crate) fn take_runtime(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
    ) -> Result<ReusableV8Runtime> {
        self.take_runtime_with_options(runtime_owner, bundle, false)
    }

    #[cfg(test)]
    pub(crate) fn take_runtime_with_options(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        use_locker: bool,
    ) -> Result<ReusableV8Runtime> {
        self.take_runtime_with_options_for_invocation(runtime_owner, bundle, None, use_locker)
    }

    pub(crate) fn take_runtime_for_invocation(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
    ) -> Result<ReusableV8Runtime> {
        self.take_runtime_with_options_for_invocation(runtime_owner, bundle, context, false)
    }

    pub(crate) fn take_runtime_with_options_for_invocation(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        use_locker: bool,
    ) -> Result<ReusableV8Runtime> {
        let use_startup_snapshot = !matches!(
            runtime_owner.policy().limits().compatibility_target,
            RuntimeCompatibilityTarget::Node22
        );
        match runtime_owner.policy().limits().runtime_pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {}
            RuntimePoolKind::WarmPool => {
                let affinity_key = runtime_affinity_key(
                    runtime_owner.policy().limits().routing_affinity,
                    context,
                    bundle,
                );
                let bundle_identity = bundle.identity().clone();
                if let Some(entry) =
                    self.take_warm_pool_entry(&bundle_identity, affinity_key.as_ref())
                {
                    runtime_owner.policy().metrics().record_warm_pool_hit();
                    runtime_owner.policy().metrics().record_runtime_pool_hit();
                    self.warmed = true;
                    return Ok(ReusableV8Runtime {
                        runtime: entry.runtime,
                        warm_reuse_count: entry.reuse_count,
                        construction_mode: entry.construction_mode,
                    });
                }

                // Cold miss: build a fresh runtime
                runtime_owner.policy().metrics().record_warm_pool_miss();
                runtime_owner.policy().metrics().record_runtime_pool_miss();
                let runtime = if use_startup_snapshot {
                    let snapshot = runtime_owner.bootstrap_snapshot()?;
                    runtime_owner.create_runtime(bundle, Some(snapshot), use_locker)?
                } else {
                    // Proper Node22 snapshotting requires a Deno-style module
                    // evaluation bootstrap. Until that lands, keep the target
                    // honest by constructing live runtimes directly.
                    runtime_owner.create_runtime(bundle, None, use_locker)?
                };
                self.warmed = true;
                return Ok(ReusableV8Runtime::fresh(
                    runtime,
                    if use_startup_snapshot {
                        V8RuntimeConstructionMode::StartupSnapshot
                    } else {
                        V8RuntimeConstructionMode::Unsnapshotted
                    },
                ));
            }
        }
        if self.warmed {
            runtime_owner.policy().metrics().record_runtime_pool_hit();
            if use_startup_snapshot {
                let snapshot = runtime_owner.bootstrap_snapshot()?;
                runtime_owner
                    .create_runtime(bundle, Some(snapshot), use_locker)
                    .map(|runtime| {
                        ReusableV8Runtime::fresh(
                            runtime,
                            V8RuntimeConstructionMode::StartupSnapshot,
                        )
                    })
            } else {
                runtime_owner
                    .create_runtime(bundle, None, use_locker)
                    .map(|runtime| {
                        ReusableV8Runtime::fresh(runtime, V8RuntimeConstructionMode::Unsnapshotted)
                    })
            }
        } else {
            runtime_owner.policy().metrics().record_runtime_pool_miss();
            let runtime = if use_startup_snapshot {
                let snapshot = runtime_owner.bootstrap_snapshot()?;
                runtime_owner.create_runtime(bundle, Some(snapshot), use_locker)?
            } else {
                runtime_owner.create_runtime(bundle, None, use_locker)?
            };
            self.warmed = true;
            Ok(ReusableV8Runtime::fresh(
                runtime,
                if use_startup_snapshot {
                    V8RuntimeConstructionMode::StartupSnapshot
                } else {
                    V8RuntimeConstructionMode::Unsnapshotted
                },
            ))
        }
    }

    pub(crate) fn return_runtime_for_invocation(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        runtime: ReusableV8Runtime,
    ) {
        let affinity_key = runtime_affinity_key(
            runtime_owner.policy().limits().routing_affinity,
            context,
            bundle,
        );
        self.return_runtime_with_affinity(runtime_owner, bundle, runtime, affinity_key);
    }

    fn return_runtime_with_affinity(
        &mut self,
        runtime_owner: &NeovexRuntime,
        bundle: &RuntimeBundle,
        mut runtime: ReusableV8Runtime,
        affinity_key: Option<RuntimeAffinityKey>,
    ) {
        match runtime_owner.policy().limits().runtime_pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {}
            RuntimePoolKind::WarmPool => {
                if runtime.runtime.is_v8_lock_held() {
                    runtime.runtime.release_v8_lock();
                }
                if runtime.warm_reuse_count >= runtime_owner.policy().limits().max_warm_reuses {
                    runtime_owner
                        .policy()
                        .metrics()
                        .record_warm_pool_retirement();
                    return;
                }
                let last_used_sequence = self.next_warm_sequence();
                self.warm_pool.push(WarmPoolEntry {
                    runtime: runtime.runtime,
                    bundle_identity: bundle.identity().clone(),
                    affinity_key,
                    reuse_count: runtime.warm_reuse_count,
                    last_used_sequence,
                    construction_mode: runtime.construction_mode,
                });
                runtime_owner
                    .policy()
                    .metrics()
                    .increment_retained_runtime_pool_entries();
                self.enforce_warm_pool_bounds(runtime_owner);
            }
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
            .policy()
            .limits()
            .max_warm_pool_entries_per_worker;
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
                    .policy()
                    .metrics()
                    .record_retained_runtime_pool_eviction();
                runtime_owner
                    .policy()
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
}
