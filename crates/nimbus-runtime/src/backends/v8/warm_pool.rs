use crate::affinity::{RuntimeAffinityKey, runtime_affinity_key};
use crate::context::RuntimeInvocationContext;
use crate::error::Result;
use crate::limits::{RuntimeLimits, RuntimeMemoryPressureLevel, RuntimePoolKind};
use crate::runtime::realm_lease::{RuntimeRealmLeaseController, RuntimeRealmLeaseRetentionPolicy};
use crate::runtime::{NimbusRuntime, RuntimeBundle, RuntimeBundleIdentity};
use crate::runtime_capabilities::RuntimePermissionProfile;

#[cfg(test)]
use super::RuntimeReuseLifecycleState;
use super::{
    RuntimeReuseLifecycle, WarmPoolMemoryPressureEviction, WarmRuntimeBoundaryMaintenance,
    WarmRuntimeCondemnationReason, WarmRuntimeRetentionDecision,
    embedder::JsRuntime,
    prepare_warm_runtime_for_retention, retained_entry_eviction_count_for_pressure,
    startup::V8RuntimeConstructionMode,
};

pub(crate) struct V8WorkerRuntimePool {
    warmed: bool,
    warm_pool: Vec<WarmPoolEntry>,
    next_warm_sequence: u64,
}

pub(crate) struct WarmPoolEntry {
    pub(crate) runtime: JsRuntime,
    pub(crate) partition_key: RuntimePoolPartitionKey,
    pub(crate) reuse_count: usize,
    pub(crate) last_used_sequence: u64,
    pub(crate) construction_mode: V8RuntimeConstructionMode,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) boundary_maintenance: WarmRuntimeBoundaryMaintenance,
    lifecycle: RuntimeReuseLifecycle,
    realm_lease_controller: RuntimeRealmLeaseController,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RuntimePoolPartitionKey {
    bundle_identity: RuntimeBundleIdentity,
    affinity_key: Option<RuntimeAffinityKey>,
    runtime_limits: RuntimeLimits,
    permission_profile: RuntimePermissionProfile,
    construction_mode: V8RuntimeConstructionMode,
    exact_service_grants: Vec<String>,
}

impl RuntimePoolPartitionKey {
    fn for_invocation(
        runtime_owner: &NimbusRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        construction_mode: V8RuntimeConstructionMode,
    ) -> Self {
        let runtime_limits = runtime_owner.policy().limits().clone();
        let permission_profile = RuntimePermissionProfile::for_limits(&runtime_limits);
        Self {
            bundle_identity: bundle.identity().clone(),
            affinity_key: runtime_affinity_key(runtime_limits.routing_affinity, context, bundle),
            exact_service_grants: runtime_limits.grants.sorted_service_grants(),
            permission_profile,
            runtime_limits,
            construction_mode,
        }
    }

    fn matches_exact(&self, other: &Self) -> bool {
        self == other
    }

    fn matches_reusable_entry(&self, other: &Self) -> bool {
        self.matches_exact(other)
    }
}

pub(crate) struct ReusableV8Runtime {
    pub(crate) runtime: JsRuntime,
    pub(crate) warm_reuse_count: usize,
    pub(crate) construction_mode: V8RuntimeConstructionMode,
    pub(crate) lifecycle: RuntimeReuseLifecycle,
    pub(crate) realm_lease_controller: RuntimeRealmLeaseController,
}

impl ReusableV8Runtime {
    pub(crate) fn fresh(runtime: JsRuntime, construction_mode: V8RuntimeConstructionMode) -> Self {
        Self {
            runtime,
            warm_reuse_count: 0,
            construction_mode,
            lifecycle: RuntimeReuseLifecycle::bootstrapped_and_leased(),
            realm_lease_controller: RuntimeRealmLeaseController::new(
                RuntimeRealmLeaseRetentionPolicy::default(),
            ),
        }
    }
}

impl V8WorkerRuntimePool {
    pub(crate) fn new() -> Self {
        // Harden the NodeFull-anchor ordering convention into an enforced invariant at the pool
        // boundary: `V8RuntimeBackendFactory::create` arms + BLOCKS on the anchor before building
        // this pool, so by here the superset RO heap is installed. Fail closed if a future reorder
        // ever constructs the pool first. No-op unless the anchor system is in use.
        crate::runtime::driver::anchor::assert_anchor_floor();
        Self {
            warmed: false,
            warm_pool: Vec::new(),
            next_warm_sequence: 1,
        }
    }

    #[cfg(test)]
    pub(crate) fn take_runtime(
        &mut self,
        runtime_owner: &NimbusRuntime,
        bundle: &RuntimeBundle,
    ) -> Result<ReusableV8Runtime> {
        self.take_runtime_with_options(runtime_owner, bundle, false)
    }

    #[cfg(test)]
    pub(crate) fn take_runtime_with_options(
        &mut self,
        runtime_owner: &NimbusRuntime,
        bundle: &RuntimeBundle,
        use_locker: bool,
    ) -> Result<ReusableV8Runtime> {
        self.take_runtime_with_options_for_invocation(runtime_owner, bundle, None, use_locker)
    }

    pub(crate) fn take_runtime_for_invocation(
        &mut self,
        runtime_owner: &NimbusRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
    ) -> Result<ReusableV8Runtime> {
        self.take_runtime_with_options_for_invocation(runtime_owner, bundle, context, false)
    }

    pub(crate) fn take_runtime_with_options_for_invocation(
        &mut self,
        runtime_owner: &NimbusRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        use_locker: bool,
    ) -> Result<ReusableV8Runtime> {
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            runtime_owner.policy().limits().compatibility_target,
        );
        match runtime_owner.policy().limits().runtime_pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {}
            RuntimePoolKind::WarmPool | RuntimePoolKind::WarmContextRecycle => {
                let partition_key = RuntimePoolPartitionKey::for_invocation(
                    runtime_owner,
                    bundle,
                    context,
                    construction_mode,
                );
                if let Some(entry) = self.take_warm_pool_entry(&partition_key) {
                    let WarmPoolEntry {
                        runtime,
                        reuse_count,
                        construction_mode,
                        mut lifecycle,
                        realm_lease_controller,
                        ..
                    } = entry;
                    lifecycle.mark_leased();
                    runtime_owner.policy().metrics().record_warm_pool_hit();
                    record_profiled_runtime_pool_hit(runtime_owner);
                    runtime_owner
                        .policy()
                        .metrics()
                        .decrement_retained_runtime_pool_entries();
                    self.warmed = true;
                    return Ok(ReusableV8Runtime {
                        runtime,
                        warm_reuse_count: reuse_count,
                        construction_mode,
                        lifecycle,
                        realm_lease_controller,
                    });
                }

                // Cold miss: build a fresh runtime
                runtime_owner.policy().metrics().record_warm_pool_miss();
                record_profiled_runtime_pool_miss(runtime_owner);
                let runtime =
                    runtime_owner.create_runtime_for_mode(bundle, use_locker, construction_mode)?;
                self.warmed = true;
                return Ok(ReusableV8Runtime::fresh(runtime, construction_mode));
            }
            RuntimePoolKind::BunJscTrustedRetained | RuntimePoolKind::BunJscFreshDiscard => {
                unreachable!("Bun/JSC pool kinds are rejected before V8 runtime invocation")
            }
        }
        if self.warmed {
            record_profiled_runtime_pool_hit(runtime_owner);
            create_reusable_runtime_for_mode(runtime_owner, bundle, use_locker, construction_mode)
        } else {
            record_profiled_runtime_pool_miss(runtime_owner);
            let runtime =
                runtime_owner.create_runtime_for_mode(bundle, use_locker, construction_mode)?;
            self.warmed = true;
            Ok(ReusableV8Runtime::fresh(runtime, construction_mode))
        }
    }

    pub(crate) fn return_runtime_for_invocation(
        &mut self,
        runtime_owner: &NimbusRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        runtime: ReusableV8Runtime,
    ) {
        let partition_key = RuntimePoolPartitionKey::for_invocation(
            runtime_owner,
            bundle,
            context,
            runtime.construction_mode,
        );
        self.return_runtime_with_partition(runtime_owner, runtime, partition_key);
    }

    fn return_runtime_with_partition(
        &mut self,
        runtime_owner: &NimbusRuntime,
        mut runtime: ReusableV8Runtime,
        partition_key: RuntimePoolPartitionKey,
    ) {
        match runtime_owner.policy().limits().runtime_pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {}
            RuntimePoolKind::WarmPool | RuntimePoolKind::WarmContextRecycle => {
                runtime.lifecycle.mark_draining();
                let boundary_maintenance = match prepare_warm_runtime_for_retention(
                    &mut runtime,
                    runtime_owner.policy().limits(),
                ) {
                    WarmRuntimeRetentionDecision::Retain(boundary_maintenance) => {
                        runtime.lifecycle.mark_clean_return();
                        boundary_maintenance
                    }
                    WarmRuntimeRetentionDecision::Condemn(reason) => {
                        if warm_runtime_condemnation_is_dirty_discard(reason) {
                            runtime.lifecycle.mark_dirty_discard();
                        } else {
                            runtime.lifecycle.mark_condemned();
                        }
                        record_warm_runtime_condemnation(runtime_owner, reason);
                        return;
                    }
                };
                let last_used_sequence = self.next_warm_sequence();
                self.warm_pool.push(WarmPoolEntry {
                    runtime: runtime.runtime,
                    partition_key,
                    reuse_count: runtime.warm_reuse_count,
                    last_used_sequence,
                    construction_mode: runtime.construction_mode,
                    boundary_maintenance,
                    lifecycle: runtime.lifecycle,
                    realm_lease_controller: runtime.realm_lease_controller,
                });
                runtime_owner
                    .policy()
                    .metrics()
                    .increment_retained_runtime_pool_entries();
                self.enforce_warm_pool_bounds(runtime_owner);
            }
            RuntimePoolKind::BunJscTrustedRetained | RuntimePoolKind::BunJscFreshDiscard => {
                unreachable!("Bun/JSC pool kinds are rejected before V8 runtime invocation")
            }
        }
    }

    fn take_warm_pool_entry(
        &mut self,
        partition_key: &RuntimePoolPartitionKey,
    ) -> Option<WarmPoolEntry> {
        // Reuse only exact bundle identity + affinity + capability partition
        // matches. Context recycling builds on the same authority boundary; a
        // retained runtime must not cross tenant/function/script affinity just
        // because the bundle and normalized limits match.
        let reusable_index = self
            .warm_pool
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.partition_key.matches_reusable_entry(partition_key))
            .max_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index);

        reusable_index.map(|index| self.warm_pool.swap_remove(index))
    }

    fn next_warm_sequence(&mut self) -> u64 {
        let sequence = self.next_warm_sequence;
        self.next_warm_sequence = self.next_warm_sequence.saturating_add(1);
        sequence
    }

    fn enforce_warm_pool_bounds(&mut self, runtime_owner: &NimbusRuntime) {
        let max_entries = runtime_owner
            .policy()
            .limits()
            .max_warm_pool_entries_per_worker;
        while self.warm_pool.len() > max_entries {
            if !self.evict_lru_entry(runtime_owner) {
                break;
            }
        }
    }

    #[allow(dead_code)]
    pub(crate) fn apply_memory_pressure(
        &mut self,
        runtime_owner: &NimbusRuntime,
        pressure: RuntimeMemoryPressureLevel,
    ) -> WarmPoolMemoryPressureEviction {
        let target_evictions =
            retained_entry_eviction_count_for_pressure(pressure, self.warm_pool.len());
        let mut evicted_entries = 0;
        for _ in 0..target_evictions {
            if self.evict_lru_entry(runtime_owner) {
                evicted_entries += 1;
            } else {
                break;
            }
        }

        WarmPoolMemoryPressureEviction {
            pressure,
            evicted_entries,
            retained_entries: self.warm_pool.len(),
        }
    }

    fn evict_lru_entry(&mut self, runtime_owner: &NimbusRuntime) -> bool {
        let Some(index) = self
            .warm_pool
            .iter()
            .enumerate()
            .min_by_key(|(_, entry)| entry.last_used_sequence)
            .map(|(index, _)| index)
        else {
            return false;
        };
        self.warm_pool.swap_remove(index);
        runtime_owner
            .policy()
            .metrics()
            .record_retained_runtime_pool_eviction();
        runtime_owner
            .policy()
            .metrics()
            .decrement_retained_runtime_pool_entries();
        true
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn warm_pool_count_for_test(&self) -> usize {
        self.warm_pool.len()
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn last_boundary_maintenance_for_test(
        &self,
    ) -> Option<WarmRuntimeBoundaryMaintenance> {
        self.warm_pool
            .iter()
            .max_by_key(|entry| entry.last_used_sequence)
            .map(|entry| entry.boundary_maintenance)
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn last_lifecycle_state_for_test(&self) -> Option<RuntimeReuseLifecycleState> {
        self.warm_pool
            .iter()
            .max_by_key(|entry| entry.last_used_sequence)
            .map(|entry| entry.lifecycle.state())
    }

    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn last_lifecycle_history_for_test(&self) -> Option<&[RuntimeReuseLifecycleState]> {
        self.warm_pool
            .iter()
            .max_by_key(|entry| entry.last_used_sequence)
            .map(|entry| entry.lifecycle.history())
    }
}

fn record_warm_runtime_condemnation(
    runtime_owner: &NimbusRuntime,
    reason: WarmRuntimeCondemnationReason,
) {
    if matches!(
        reason,
        WarmRuntimeCondemnationReason::EventLoopNotQuiescent { .. }
            | WarmRuntimeCondemnationReason::RequestStateResetFailed { .. }
    ) {
        runtime_owner
            .policy()
            .metrics()
            .record_warm_pool_discard_unquiesced();
    } else {
        runtime_owner
            .policy()
            .metrics()
            .record_warm_pool_retirement();
        runtime_owner
            .policy()
            .metrics()
            .record_retained_runtime_pool_retirement();
    }
}

fn record_profiled_runtime_pool_hit(runtime_owner: &NimbusRuntime) {
    let policy = runtime_owner.policy();
    let metrics = policy.metrics();
    metrics.record_runtime_pool_hit();
    metrics.record_profile_runtime_pool_hit(policy.runtime_profile());
}

fn record_profiled_runtime_pool_miss(runtime_owner: &NimbusRuntime) {
    let policy = runtime_owner.policy();
    let metrics = policy.metrics();
    metrics.record_runtime_pool_miss();
    metrics.record_profile_runtime_pool_miss(policy.runtime_profile());
}

fn warm_runtime_condemnation_is_dirty_discard(reason: WarmRuntimeCondemnationReason) -> bool {
    !matches!(
        reason,
        WarmRuntimeCondemnationReason::MaxWarmReusesExceeded { .. }
    )
}

fn create_reusable_runtime_for_mode(
    runtime_owner: &NimbusRuntime,
    bundle: &RuntimeBundle,
    use_locker: bool,
    construction_mode: V8RuntimeConstructionMode,
) -> Result<ReusableV8Runtime> {
    runtime_owner
        .create_runtime_for_mode(bundle, use_locker, construction_mode)
        .map(|runtime| ReusableV8Runtime::fresh(runtime, construction_mode))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::Value;

    use crate::context::RuntimeInvocationContext;
    use crate::host::{HostBridge, HostCallRequest};
    use crate::limits::{RuntimeLimits, RuntimePolicy, RuntimePoolKind, RuntimeRoutingAffinity};
    use crate::runtime::{InvocationKind, InvocationRequest};

    use super::*;

    struct RejectHost;

    impl HostBridge for RejectHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            Err(crate::error::NimbusRuntimeError::Contract(format!(
                "warm-pool partition-key test host should not execute {}",
                request.operation
            )))
        }
    }

    fn invocation_request(function_name: &str) -> InvocationRequest {
        InvocationRequest {
            kind: InvocationKind::Query,
            function_name: function_name.to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        }
    }

    fn runtime_owner_for_limits(limits: RuntimeLimits) -> NimbusRuntime {
        NimbusRuntime::with_policy(Arc::new(RejectHost), Arc::new(RuntimePolicy::new(limits)))
    }

    fn context_recycle_limits() -> RuntimeLimits {
        RuntimeLimits {
            runtime_pool_kind: RuntimePoolKind::WarmContextRecycle,
            ..RuntimeLimits::application_web_standard()
        }
    }

    fn context_recycle_restricted_limits() -> RuntimeLimits {
        RuntimeLimits {
            runtime_pool_kind: RuntimePoolKind::WarmContextRecycle,
            ..RuntimeLimits::restricted_code()
        }
    }

    #[test]
    fn reusable_partition_key_preserves_tenant_affinity_for_unscoped_bundle() {
        let runtime_owner = runtime_owner_for_limits(RuntimeLimits::application_web_standard());
        let bundle = RuntimeBundle::new("/tmp/nimbus-pir2-unscoped-bundle.mjs");
        let request = invocation_request("messages:list");
        let tenant_a = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
        let tenant_b = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-b");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            runtime_owner.policy().limits().compatibility_target,
        );

        let tenant_a_key = RuntimePoolPartitionKey::for_invocation(
            &runtime_owner,
            &bundle,
            Some(&tenant_a),
            construction_mode,
        );
        let tenant_b_key = RuntimePoolPartitionKey::for_invocation(
            &runtime_owner,
            &bundle,
            Some(&tenant_b),
            construction_mode,
        );

        assert_ne!(tenant_a_key, tenant_b_key);
        assert!(
            !tenant_a_key.matches_reusable_entry(&tenant_b_key),
            "same unscoped bundle and limits must not cross tenant affinity"
        );
    }

    #[test]
    fn reusable_partition_key_preserves_function_affinity_for_unscoped_bundle() {
        let mut limits = RuntimeLimits::application_web_standard();
        limits.routing_affinity = RuntimeRoutingAffinity::Function;
        let runtime_owner = runtime_owner_for_limits(limits);
        let bundle = RuntimeBundle::new("/tmp/nimbus-pir2-unscoped-bundle.mjs");
        let list_request = invocation_request("messages:list");
        let send_request = invocation_request("messages:send");
        let list_context =
            RuntimeInvocationContext::top_level_for_tenant(&list_request, "tenant-a");
        let send_context =
            RuntimeInvocationContext::top_level_for_tenant(&send_request, "tenant-a");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            runtime_owner.policy().limits().compatibility_target,
        );

        let list_key = RuntimePoolPartitionKey::for_invocation(
            &runtime_owner,
            &bundle,
            Some(&list_context),
            construction_mode,
        );
        let send_key = RuntimePoolPartitionKey::for_invocation(
            &runtime_owner,
            &bundle,
            Some(&send_context),
            construction_mode,
        );

        assert_ne!(list_key, send_key);
        assert!(
            !list_key.matches_reusable_entry(&send_key),
            "same unscoped bundle and limits must not cross function affinity"
        );
    }

    #[test]
    fn context_recycle_partition_key_preserves_exact_service_grants() {
        let mut db_limits = context_recycle_limits();
        db_limits.grants.service = vec!["db".to_string()];
        let db_runtime_owner = runtime_owner_for_limits(db_limits);

        let mut cache_limits = context_recycle_limits();
        cache_limits.grants.service = vec!["cache".to_string()];
        let cache_runtime_owner = runtime_owner_for_limits(cache_limits);

        let bundle = RuntimeBundle::new("/tmp/nimbus-pir2-context-recycle-grants.mjs");
        let request = invocation_request("messages:list");
        let context = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            db_runtime_owner.policy().limits().compatibility_target,
        );

        let db_key = RuntimePoolPartitionKey::for_invocation(
            &db_runtime_owner,
            &bundle,
            Some(&context),
            construction_mode,
        );
        let cache_key = RuntimePoolPartitionKey::for_invocation(
            &cache_runtime_owner,
            &bundle,
            Some(&context),
            construction_mode,
        );

        assert_ne!(db_key, cache_key);
        assert!(
            !db_key.matches_reusable_entry(&cache_key),
            "WarmContextRecycle must partition retained runtimes by exact service grants"
        );
    }

    #[test]
    fn context_recycle_partition_key_rejects_authority_dimension_fuzz_cases() {
        let base_owner = runtime_owner_for_limits(context_recycle_limits());
        let base_bundle = RuntimeBundle::new("/tmp/nimbus-pir2-context-recycle-base.mjs");
        let alt_bundle = RuntimeBundle::new("/tmp/nimbus-pir2-context-recycle-alt.mjs");
        let list_request = invocation_request("messages:list");
        let send_request = invocation_request("messages:send");
        let tenant_a = RuntimeInvocationContext::top_level_for_tenant(&list_request, "tenant-a");
        let tenant_b = RuntimeInvocationContext::top_level_for_tenant(&list_request, "tenant-b");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            base_owner.policy().limits().compatibility_target,
        );
        let base_key = RuntimePoolPartitionKey::for_invocation(
            &base_owner,
            &base_bundle,
            Some(&tenant_a),
            construction_mode,
        );

        let mut function_limits = context_recycle_limits();
        function_limits.routing_affinity = RuntimeRoutingAffinity::Function;
        let function_owner = runtime_owner_for_limits(function_limits);
        let function_list_context =
            RuntimeInvocationContext::top_level_for_tenant(&list_request, "tenant-a");
        let function_send_context =
            RuntimeInvocationContext::top_level_for_tenant(&send_request, "tenant-a");
        let function_list_key = RuntimePoolPartitionKey::for_invocation(
            &function_owner,
            &base_bundle,
            Some(&function_list_context),
            construction_mode,
        );
        let function_send_key = RuntimePoolPartitionKey::for_invocation(
            &function_owner,
            &base_bundle,
            Some(&function_send_context),
            construction_mode,
        );

        let mut script_limits = context_recycle_limits();
        script_limits.routing_affinity = RuntimeRoutingAffinity::Script;
        let script_owner = runtime_owner_for_limits(script_limits);
        let script_base_key = RuntimePoolPartitionKey::for_invocation(
            &script_owner,
            &base_bundle,
            Some(&tenant_a),
            construction_mode,
        );
        let script_alt_key = RuntimePoolPartitionKey::for_invocation(
            &script_owner,
            &alt_bundle,
            Some(&tenant_a),
            construction_mode,
        );

        let restricted_owner = runtime_owner_for_limits(context_recycle_restricted_limits());
        let restricted_key = RuntimePoolPartitionKey::for_invocation(
            &restricted_owner,
            &base_bundle,
            Some(&tenant_a),
            construction_mode,
        );

        let mut db_limits = context_recycle_limits();
        db_limits.grants.service = vec!["db".to_string()];
        let db_owner = runtime_owner_for_limits(db_limits);
        let db_key = RuntimePoolPartitionKey::for_invocation(
            &db_owner,
            &base_bundle,
            Some(&tenant_a),
            construction_mode,
        );
        let mut cache_limits = context_recycle_limits();
        cache_limits.grants.service = vec!["cache".to_string()];
        let cache_owner = runtime_owner_for_limits(cache_limits);
        let cache_key = RuntimePoolPartitionKey::for_invocation(
            &cache_owner,
            &base_bundle,
            Some(&tenant_a),
            construction_mode,
        );

        let unsnapshotted_key = RuntimePoolPartitionKey::for_invocation(
            &base_owner,
            &base_bundle,
            Some(&tenant_a),
            V8RuntimeConstructionMode::Unsnapshotted,
        );

        let cases = [
            ("tenant", base_key.clone(), {
                RuntimePoolPartitionKey::for_invocation(
                    &base_owner,
                    &base_bundle,
                    Some(&tenant_b),
                    construction_mode,
                )
            }),
            ("function", function_list_key, function_send_key),
            ("script", script_base_key, script_alt_key),
            ("permission_profile", base_key.clone(), restricted_key),
            ("exact_service_grants", db_key, cache_key),
            ("construction_mode", base_key, unsnapshotted_key),
        ];

        for (dimension, left, right) in cases {
            assert_ne!(
                left, right,
                "{dimension}: fuzz case should produce distinct partition keys"
            );
            assert!(
                !left.matches_reusable_entry(&right),
                "{dimension}: WarmContextRecycle must not reuse across authority dimensions"
            );
        }
    }
}
