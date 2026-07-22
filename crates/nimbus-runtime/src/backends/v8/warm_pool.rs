use crate::affinity::{RuntimeReuseLocalityKey, runtime_reuse_locality_key};
use crate::context::RuntimeInvocationContext;
use crate::error::Result;
use crate::execution_plan::{RuntimePoolAuthorityFacts, RuntimePoolAuthorityKey};
#[cfg(test)]
use crate::limits::RuntimeMemoryPressureLevel;
use crate::limits::RuntimePoolKind;
use crate::retained_state::{
    OwnerPartitionedPool, RetainedCheckout, RuntimeOwnerLease, RuntimeReuseAuthority,
};
use crate::runtime::realm_lease::{RuntimeRealmLeaseController, RuntimeRealmLeaseRetentionPolicy};
use crate::runtime::{NimbusRuntime, RuntimeBundle};

use super::{
    RuntimeReuseLifecycle, WarmRuntimeBoundaryMaintenance, WarmRuntimeCondemnationReason,
    WarmRuntimeRetentionDecision, embedder::JsRuntime, prepare_warm_runtime_for_retention,
    startup::V8RuntimeConstructionMode,
};
#[cfg(test)]
use super::{
    RuntimeReuseLifecycleState, WarmPoolMemoryPressureEviction,
    retained_entry_eviction_count_for_pressure,
};

pub(crate) struct V8WorkerRuntimePool {
    warmed: bool,
    warm_pool: OwnerPartitionedPool<WarmPoolEntry, V8RetainedAuthorityKey>,
}

pub(crate) struct WarmPoolEntry {
    pub(crate) runtime: JsRuntime,
    pub(crate) reuse_count: usize,
    pub(crate) construction_mode: V8RuntimeConstructionMode,
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) boundary_maintenance: WarmRuntimeBoundaryMaintenance,
    lifecycle: RuntimeReuseLifecycle,
    realm_lease_controller: RuntimeRealmLeaseController,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct V8RetainedAuthorityKey {
    pool_authority: RuntimePoolAuthorityKey,
    reuse_locality_key: Option<RuntimeReuseLocalityKey>,
}

impl V8RetainedAuthorityKey {
    fn for_invocation(
        runtime_instance: &NimbusRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        construction_mode: V8RuntimeConstructionMode,
    ) -> Result<Self> {
        let policy = runtime_instance.policy();
        let runtime_profile = policy.runtime_profile().ok_or_else(|| {
            crate::error::NimbusRuntimeError::Contract(
                "mutable V8 retention requires an admitted runtime profile".to_string(),
            )
        })?;
        let runtime_limits = policy.limits();
        Ok(Self {
            pool_authority: RuntimePoolAuthorityKey::exact(
                RuntimePoolAuthorityFacts::for_realm_reuse(
                    runtime_profile,
                    policy.as_ref(),
                    bundle,
                    construction_mode,
                )?,
            ),
            reuse_locality_key: runtime_reuse_locality_key(
                runtime_limits.routing_affinity,
                context,
                bundle,
            )
            .map_err(|error| crate::error::NimbusRuntimeError::Contract(error.to_string()))?,
        })
    }
}

pub(crate) struct ReusableV8Runtime {
    pub(crate) runtime: JsRuntime,
    pub(crate) warm_reuse_count: usize,
    pub(crate) construction_mode: V8RuntimeConstructionMode,
    pub(crate) lifecycle: RuntimeReuseLifecycle,
    pub(crate) realm_lease_controller: RuntimeRealmLeaseController,
    pub(crate) owner_lease: Option<RuntimeOwnerLease>,
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
            owner_lease: None,
        }
    }

    fn fresh_for_owner(
        runtime: JsRuntime,
        construction_mode: V8RuntimeConstructionMode,
        owner_lease: RuntimeOwnerLease,
    ) -> Self {
        let mut reusable = Self::fresh(runtime, construction_mode);
        reusable.owner_lease = Some(owner_lease);
        reusable
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
            warm_pool: OwnerPartitionedPool::new(usize::MAX, usize::MAX),
        }
    }

    pub(crate) fn retire_owner(&mut self, owner_id: &crate::RuntimeOwnerId) -> usize {
        self.warm_pool.retire_owner(owner_id)
    }

    pub(crate) fn retire_deployment_authority(
        &mut self,
        authority_id: &crate::RuntimeDeploymentAuthorityId,
    ) -> usize {
        self.warm_pool.retire_deployment_authority(authority_id)
    }

    #[cfg(test)]
    pub(crate) fn take_runtime(
        &mut self,
        runtime_instance: &NimbusRuntime,
        bundle: &RuntimeBundle,
    ) -> Result<ReusableV8Runtime> {
        self.take_runtime_with_options(runtime_instance, bundle, false)
    }

    #[cfg(test)]
    pub(crate) fn take_runtime_with_options(
        &mut self,
        runtime_instance: &NimbusRuntime,
        bundle: &RuntimeBundle,
        use_locker: bool,
    ) -> Result<ReusableV8Runtime> {
        self.take_runtime_with_options_for_invocation(runtime_instance, bundle, None, use_locker)
    }

    pub(crate) fn take_runtime_for_invocation(
        &mut self,
        runtime_instance: &NimbusRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
    ) -> Result<ReusableV8Runtime> {
        self.take_runtime_with_options_for_invocation(runtime_instance, bundle, context, false)
    }

    pub(crate) fn take_runtime_with_options_for_invocation(
        &mut self,
        runtime_instance: &NimbusRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        use_locker: bool,
    ) -> Result<ReusableV8Runtime> {
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            runtime_instance.policy().limits().compatibility_target,
        );
        match runtime_instance.policy().limits().runtime_pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {}
            RuntimePoolKind::WarmPool | RuntimePoolKind::WarmContextRecycle => {
                let owner_lease = required_runtime_owner_lease(context)?.clone();
                let authority_key = V8RetainedAuthorityKey::for_invocation(
                    runtime_instance,
                    bundle,
                    context,
                    construction_mode,
                )?;
                let authority = RuntimeReuseAuthority::new_with_deployment(
                    owner_lease.clone(),
                    context
                        .and_then(RuntimeInvocationContext::deployment_authority_lease)
                        .cloned(),
                    authority_key,
                )?;
                let before_checkout = self.warm_pool.stats();
                let checkout = self.warm_pool.checkout(&authority)?;
                self.record_owner_checkout(runtime_instance, before_checkout, checkout.is_some());
                if let Some(checkout) = checkout {
                    let (entry, retained_authority) = checkout.into_parts();
                    let WarmPoolEntry {
                        runtime,
                        reuse_count,
                        construction_mode,
                        mut lifecycle,
                        realm_lease_controller,
                        ..
                    } = entry;
                    lifecycle.mark_leased();
                    runtime_instance.policy().metrics().record_warm_pool_hit();
                    record_profiled_runtime_pool_hit(runtime_instance);
                    runtime_instance
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
                        owner_lease: Some(retained_authority.owner_lease().clone()),
                    });
                }

                // Cold miss: build a fresh runtime
                runtime_instance.policy().metrics().record_warm_pool_miss();
                record_profiled_runtime_pool_miss(runtime_instance);
                let runtime = runtime_instance.create_runtime_for_mode(
                    bundle,
                    use_locker,
                    construction_mode,
                )?;
                self.warmed = true;
                return Ok(ReusableV8Runtime::fresh_for_owner(
                    runtime,
                    construction_mode,
                    owner_lease,
                ));
            }
            RuntimePoolKind::BunJscTrustedRetained
            | RuntimePoolKind::BunJscFreshDiscard
            | RuntimePoolKind::PrecompiledModuleCache
            | RuntimePoolKind::RetainedStorePool => {
                unreachable!("non-V8 pool kinds are rejected before V8 runtime invocation")
            }
        }
        if self.warmed {
            record_profiled_runtime_pool_hit(runtime_instance);
            create_reusable_runtime_for_mode(
                runtime_instance,
                bundle,
                use_locker,
                construction_mode,
            )
        } else {
            record_profiled_runtime_pool_miss(runtime_instance);
            let runtime =
                runtime_instance.create_runtime_for_mode(bundle, use_locker, construction_mode)?;
            self.warmed = true;
            Ok(ReusableV8Runtime::fresh(runtime, construction_mode))
        }
    }

    pub(crate) fn return_runtime_for_invocation(
        &mut self,
        runtime_instance: &NimbusRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        runtime: ReusableV8Runtime,
    ) {
        let Ok(authority_key) = V8RetainedAuthorityKey::for_invocation(
            runtime_instance,
            bundle,
            context,
            runtime.construction_mode,
        ) else {
            return;
        };
        self.return_runtime_with_authority(runtime_instance, runtime, authority_key, context);
    }

    fn return_runtime_with_authority(
        &mut self,
        runtime_instance: &NimbusRuntime,
        mut runtime: ReusableV8Runtime,
        authority_key: V8RetainedAuthorityKey,
        context: Option<&RuntimeInvocationContext>,
    ) {
        match runtime_instance.policy().limits().runtime_pool_kind {
            RuntimePoolKind::StartupSnapshotCache => {}
            RuntimePoolKind::WarmPool | RuntimePoolKind::WarmContextRecycle => {
                let Some(owner_lease) = runtime.owner_lease.take() else {
                    runtime.lifecycle.mark_condemned();
                    return;
                };
                let Some(invocation_owner_lease) =
                    context.and_then(RuntimeInvocationContext::runtime_owner_lease)
                else {
                    runtime.lifecycle.mark_condemned();
                    return;
                };
                if owner_lease.is_revoked() || invocation_owner_lease.is_revoked() {
                    runtime.lifecycle.mark_condemned();
                    runtime_instance
                        .policy()
                        .metrics()
                        .record_retained_owner_return_after_revoke_discard();
                    return;
                }
                if owner_lease != *invocation_owner_lease {
                    runtime.lifecycle.mark_condemned();
                    runtime_instance
                        .policy()
                        .metrics()
                        .record_retained_owner_mismatch_denial();
                    return;
                }
                let deployment_authority = context
                    .and_then(RuntimeInvocationContext::deployment_authority_lease)
                    .cloned();
                if deployment_authority
                    .as_ref()
                    .is_some_and(crate::RuntimeDeploymentAuthorityLease::is_revoked)
                {
                    runtime.lifecycle.mark_condemned();
                    runtime_instance
                        .policy()
                        .metrics()
                        .record_retained_owner_return_after_revoke_discard();
                    return;
                }
                let Ok(authority) = RuntimeReuseAuthority::new_with_deployment(
                    owner_lease,
                    deployment_authority,
                    authority_key,
                ) else {
                    runtime.lifecycle.mark_condemned();
                    return;
                };
                runtime.lifecycle.mark_draining();
                let boundary_maintenance = match prepare_warm_runtime_for_retention(
                    &mut runtime,
                    runtime_instance.policy().limits(),
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
                        record_warm_runtime_condemnation(runtime_instance, reason);
                        return;
                    }
                };
                let global_capacity = runtime_instance
                    .policy()
                    .limits()
                    .max_warm_pool_entries_per_worker;
                let per_owner_capacity = derived_per_owner_capacity(global_capacity);
                let before = self.warm_pool.stats();
                self.warm_pool
                    .set_capacities(global_capacity, per_owner_capacity);
                let rejected = self.warm_pool.retain(RetainedCheckout::fresh(
                    WarmPoolEntry {
                        runtime: runtime.runtime,
                        reuse_count: runtime.warm_reuse_count,
                        construction_mode: runtime.construction_mode,
                        boundary_maintenance,
                        lifecycle: runtime.lifecycle,
                        realm_lease_controller: runtime.realm_lease_controller,
                    },
                    authority,
                ));
                self.record_pool_deltas(runtime_instance, before);
                if rejected.is_some() {
                    return;
                }
                runtime_instance
                    .policy()
                    .metrics()
                    .increment_retained_runtime_pool_entries();
            }
            RuntimePoolKind::BunJscTrustedRetained
            | RuntimePoolKind::BunJscFreshDiscard
            | RuntimePoolKind::PrecompiledModuleCache
            | RuntimePoolKind::RetainedStorePool => {
                unreachable!("non-V8 pool kinds are rejected before V8 runtime invocation")
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn apply_memory_pressure(
        &mut self,
        runtime_instance: &NimbusRuntime,
        pressure: RuntimeMemoryPressureLevel,
    ) -> WarmPoolMemoryPressureEviction {
        let target_evictions =
            retained_entry_eviction_count_for_pressure(pressure, self.warm_pool.len());
        let before = self.warm_pool.stats();
        let evicted_entries = self.warm_pool.evict_global_lru(target_evictions);
        self.record_pool_deltas(runtime_instance, before);

        WarmPoolMemoryPressureEviction {
            pressure,
            evicted_entries,
            retained_entries: self.warm_pool.len(),
        }
    }

    fn record_owner_checkout(
        &self,
        runtime_instance: &NimbusRuntime,
        before: crate::retained_state::OwnerPartitionedPoolStats,
        hit: bool,
    ) {
        let metrics = runtime_instance.policy().metrics();
        metrics.record_retained_owner_checkout_result(hit);
        let after = self.warm_pool.stats();
        for _ in 0..after
            .owner_mismatch_denials
            .saturating_sub(before.owner_mismatch_denials)
        {
            metrics.record_retained_owner_mismatch_denial();
        }
        for _ in 0..after
            .revoked_discards
            .saturating_sub(before.revoked_discards)
        {
            metrics.record_retained_owner_return_after_revoke_discard();
        }
    }

    fn record_pool_deltas(
        &self,
        runtime_instance: &NimbusRuntime,
        before: crate::retained_state::OwnerPartitionedPoolStats,
    ) {
        let after = self.warm_pool.stats();
        for _ in 0..after.evictions.saturating_sub(before.evictions) {
            runtime_instance
                .policy()
                .metrics()
                .record_retained_runtime_pool_eviction();
            runtime_instance
                .policy()
                .metrics()
                .decrement_retained_runtime_pool_entries();
        }
        for _ in 0..after
            .revoked_discards
            .saturating_sub(before.revoked_discards)
        {
            runtime_instance
                .policy()
                .metrics()
                .record_retained_owner_return_after_revoke_discard();
        }
    }

    #[cfg(test)]
    pub(crate) fn warm_pool_count_for_test(&self) -> usize {
        self.warm_pool.len()
    }

    #[cfg(test)]
    pub(crate) fn last_boundary_maintenance_for_test(
        &self,
    ) -> Option<WarmRuntimeBoundaryMaintenance> {
        self.warm_pool
            .most_recent()
            .map(|entry| entry.boundary_maintenance)
    }

    #[cfg(test)]
    pub(crate) fn last_lifecycle_state_for_test(&self) -> Option<RuntimeReuseLifecycleState> {
        self.warm_pool
            .most_recent()
            .map(|entry| entry.lifecycle.state())
    }

    #[cfg(test)]
    pub(crate) fn last_lifecycle_history_for_test(&self) -> Option<&[RuntimeReuseLifecycleState]> {
        self.warm_pool
            .most_recent()
            .map(|entry| entry.lifecycle.history())
    }
}

fn derived_per_owner_capacity(global_capacity: usize) -> usize {
    global_capacity.saturating_sub(1).max(1)
}

fn required_runtime_owner_lease(
    context: Option<&RuntimeInvocationContext>,
) -> Result<&RuntimeOwnerLease> {
    let lease = context
        .and_then(RuntimeInvocationContext::runtime_owner_lease)
        .ok_or_else(|| {
            crate::error::NimbusRuntimeError::Contract(
                "mutable V8 runtime retention requires a runtime owner lease".to_string(),
            )
        })?;
    lease.ensure_active()?;
    Ok(lease)
}

fn record_warm_runtime_condemnation(
    runtime_instance: &NimbusRuntime,
    reason: WarmRuntimeCondemnationReason,
) {
    if matches!(
        reason,
        WarmRuntimeCondemnationReason::EventLoopNotQuiescent { .. }
            | WarmRuntimeCondemnationReason::RequestStateResetFailed { .. }
    ) {
        runtime_instance
            .policy()
            .metrics()
            .record_warm_pool_discard_unquiesced();
    } else {
        runtime_instance
            .policy()
            .metrics()
            .record_warm_pool_retirement();
        runtime_instance
            .policy()
            .metrics()
            .record_retained_runtime_pool_retirement();
    }
}

fn record_profiled_runtime_pool_hit(runtime_instance: &NimbusRuntime) {
    let policy = runtime_instance.policy();
    let metrics = policy.metrics();
    metrics.record_runtime_pool_hit();
    metrics.record_profile_runtime_pool_hit(policy.runtime_profile());
}

fn record_profiled_runtime_pool_miss(runtime_instance: &NimbusRuntime) {
    let policy = runtime_instance.policy();
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
    runtime_instance: &NimbusRuntime,
    bundle: &RuntimeBundle,
    use_locker: bool,
    construction_mode: V8RuntimeConstructionMode,
) -> Result<ReusableV8Runtime> {
    runtime_instance
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

    fn tenant_context(
        request: &InvocationRequest,
        stable_subject: &str,
        incarnation: u64,
        audit_label: &str,
    ) -> RuntimeInvocationContext {
        let owner_id = crate::RuntimeOwnerId::tenant(
            stable_subject,
            std::num::NonZeroU64::new(incarnation).expect("fixture incarnation must be positive"),
            Some(audit_label),
        )
        .expect("fixture owner should build");
        let (owner_lease, _) = crate::RuntimeOwnerLeaseIssuer.issue(owner_id);
        RuntimeInvocationContext::top_level_for_tenant_with_owner(request, audit_label, owner_lease)
    }

    fn runtime_owner_for_limits(limits: RuntimeLimits) -> NimbusRuntime {
        NimbusRuntime::with_policy(
            Arc::new(RejectHost),
            Arc::new(RuntimePolicy::new(limits)),
            crate::RuntimeEgressPosture::CoarsePermissions,
        )
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

    fn partition_key(
        runtime_instance: &NimbusRuntime,
        bundle: &RuntimeBundle,
        context: Option<&RuntimeInvocationContext>,
        construction_mode: V8RuntimeConstructionMode,
    ) -> RuntimeReuseAuthority<V8RetainedAuthorityKey> {
        let owner_lease = required_runtime_owner_lease(context)
            .expect("valid tenant-scoped owner lease should exist")
            .clone();
        let authority_key = V8RetainedAuthorityKey::for_invocation(
            runtime_instance,
            bundle,
            context,
            construction_mode,
        )
        .expect("valid retained authority key should build");
        RuntimeReuseAuthority::new(owner_lease, authority_key)
            .expect("valid tenant-scoped reuse authority should build")
    }

    #[test]
    fn reusable_partition_key_preserves_tenant_affinity_for_unscoped_bundle() {
        let runtime_instance = runtime_owner_for_limits(RuntimeLimits::application_web_standard());
        let bundle = RuntimeBundle::new("/tmp/nimbus-pir2-unscoped-bundle.mjs");
        let request = invocation_request("messages:list");
        let tenant_a = tenant_context(&request, "tenant-a", 1, "tenant-a");
        let tenant_b = tenant_context(&request, "tenant-b", 1, "tenant-b");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            runtime_instance.policy().limits().compatibility_target,
        );

        let tenant_a_key = partition_key(
            &runtime_instance,
            &bundle,
            Some(&tenant_a),
            construction_mode,
        );
        let tenant_b_key = partition_key(
            &runtime_instance,
            &bundle,
            Some(&tenant_b),
            construction_mode,
        );

        assert_ne!(tenant_a_key, tenant_b_key);
        assert!(
            !tenant_a_key.matches_exact(&tenant_b_key),
            "same unscoped bundle and limits must not cross tenant affinity"
        );
    }

    #[test]
    fn reusable_partition_key_preserves_function_affinity_for_unscoped_bundle() {
        let mut limits = RuntimeLimits::application_web_standard();
        limits.routing_affinity = RuntimeRoutingAffinity::Function;
        let runtime_instance = runtime_owner_for_limits(limits);
        let bundle = RuntimeBundle::new("/tmp/nimbus-pir2-unscoped-bundle.mjs");
        let list_request = invocation_request("messages:list");
        let send_request = invocation_request("messages:send");
        let list_context = tenant_context(&list_request, "tenant-a", 1, "tenant-a");
        let send_context = tenant_context(&send_request, "tenant-a", 1, "tenant-a");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            runtime_instance.policy().limits().compatibility_target,
        );

        let list_key = partition_key(
            &runtime_instance,
            &bundle,
            Some(&list_context),
            construction_mode,
        );
        let send_key = partition_key(
            &runtime_instance,
            &bundle,
            Some(&send_context),
            construction_mode,
        );

        assert_ne!(list_key, send_key);
        assert!(
            !list_key.matches_exact(&send_key),
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
        let context = tenant_context(&request, "tenant-a", 1, "tenant-a");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            db_runtime_owner.policy().limits().compatibility_target,
        );

        let db_key = partition_key(
            &db_runtime_owner,
            &bundle,
            Some(&context),
            construction_mode,
        );
        let cache_key = partition_key(
            &cache_runtime_owner,
            &bundle,
            Some(&context),
            construction_mode,
        );

        assert_ne!(db_key, cache_key);
        assert!(
            !db_key.matches_exact(&cache_key),
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
        let tenant_a = tenant_context(&list_request, "tenant-a", 1, "tenant-a");
        let tenant_b = tenant_context(&list_request, "tenant-b", 1, "tenant-b");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            base_owner.policy().limits().compatibility_target,
        );
        let base_key = partition_key(
            &base_owner,
            &base_bundle,
            Some(&tenant_a),
            construction_mode,
        );

        let mut function_limits = context_recycle_limits();
        function_limits.routing_affinity = RuntimeRoutingAffinity::Function;
        let function_owner = runtime_owner_for_limits(function_limits);
        let function_list_context = tenant_context(&list_request, "tenant-a", 1, "tenant-a");
        let function_send_context = tenant_context(&send_request, "tenant-a", 1, "tenant-a");
        let function_list_key = partition_key(
            &function_owner,
            &base_bundle,
            Some(&function_list_context),
            construction_mode,
        );
        let function_send_key = partition_key(
            &function_owner,
            &base_bundle,
            Some(&function_send_context),
            construction_mode,
        );

        let mut script_limits = context_recycle_limits();
        script_limits.routing_affinity = RuntimeRoutingAffinity::Script;
        let script_owner = runtime_owner_for_limits(script_limits);
        let script_base_key = partition_key(
            &script_owner,
            &base_bundle,
            Some(&tenant_a),
            construction_mode,
        );
        let script_alt_key = partition_key(
            &script_owner,
            &alt_bundle,
            Some(&tenant_a),
            construction_mode,
        );

        let restricted_owner = runtime_owner_for_limits(context_recycle_restricted_limits());
        let restricted_key = partition_key(
            &restricted_owner,
            &base_bundle,
            Some(&tenant_a),
            construction_mode,
        );

        let mut db_limits = context_recycle_limits();
        db_limits.grants.service = vec!["db".to_string()];
        let db_owner = runtime_owner_for_limits(db_limits);
        let db_key = partition_key(&db_owner, &base_bundle, Some(&tenant_a), construction_mode);
        let mut cache_limits = context_recycle_limits();
        cache_limits.grants.service = vec!["cache".to_string()];
        let cache_owner = runtime_owner_for_limits(cache_limits);
        let cache_key = partition_key(
            &cache_owner,
            &base_bundle,
            Some(&tenant_a),
            construction_mode,
        );

        let unsnapshotted_key = partition_key(
            &base_owner,
            &base_bundle,
            Some(&tenant_a),
            V8RuntimeConstructionMode::Unsnapshotted,
        );
        let snapshotted_key = partition_key(
            &base_owner,
            &base_bundle,
            Some(&tenant_a),
            V8RuntimeConstructionMode::StartupSnapshot,
        );

        // Production-logic half (kept, not dropped): the cage fix builds non-Node profiles
        // UNSNAPSHOTTED — they must never deserialize the shared RO heap — so WebStandard's
        // production-SELECTED construction mode IS Unsnapshotted. The original case asserted the
        // opposite implicitly (base_key's selected mode != Unsnapshotted), which was a real,
        // now-stale fact; assert the current production reality explicitly so a regression in the
        // selection is still caught here.
        assert_eq!(
            V8RuntimeConstructionMode::for_compatibility_target(
                base_owner.policy().limits().compatibility_target
            ),
            V8RuntimeConstructionMode::Unsnapshotted,
            "WebStandard must select Unsnapshotted (cage fix: non-Node never deserializes the \
             shared RO heap)"
        );

        let cases = [
            ("tenant", base_key.clone(), {
                partition_key(
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
            // Capability half: the partition key incorporates construction_mode, so two runtimes
            // built with DIFFERENT modes never share a recycled context. Not tautological — this
            // drops to equal (and fails) if the key ever stops keying on mode.
            ("construction_mode", snapshotted_key, unsnapshotted_key),
        ];

        for (dimension, left, right) in cases {
            assert_ne!(
                left, right,
                "{dimension}: fuzz case should produce distinct partition keys"
            );
            assert!(
                !left.matches_exact(&right),
                "{dimension}: WarmContextRecycle must not reuse across authority dimensions"
            );
        }
    }

    #[test]
    fn owner_authority_is_mandatory_when_routing_affinity_is_none() {
        let mut limits = RuntimeLimits::application_web_standard();
        limits.routing_affinity = RuntimeRoutingAffinity::None;
        let runtime = runtime_owner_for_limits(limits);
        let bundle = RuntimeBundle::new("/tmp/nimbus-rti-none-affinity.mjs");
        let request = invocation_request("messages:list");
        let tenant_a = tenant_context(&request, "tenant-a", 1, "shared-label");
        let tenant_b = tenant_context(&request, "tenant-b", 1, "shared-label");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            runtime.policy().limits().compatibility_target,
        );

        let key_a = partition_key(&runtime, &bundle, Some(&tenant_a), construction_mode);
        let key_b = partition_key(&runtime, &bundle, Some(&tenant_b), construction_mode);

        assert_ne!(key_a, key_b);
        assert!(!key_a.matches_exact(&key_b));
    }

    #[test]
    fn owner_authority_is_mandatory_for_unscoped_script_affinity() {
        let mut limits = RuntimeLimits::application_web_standard();
        limits.routing_affinity = RuntimeRoutingAffinity::Script;
        let runtime = runtime_owner_for_limits(limits);
        let bundle = RuntimeBundle::new("/tmp/nimbus-rti-unscoped-script.mjs");
        let request = invocation_request("messages:list");
        let tenant_a = tenant_context(&request, "tenant-a", 1, "tenant-a");
        let tenant_b = tenant_context(&request, "tenant-b", 1, "tenant-b");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            runtime.policy().limits().compatibility_target,
        );

        let key_a = partition_key(&runtime, &bundle, Some(&tenant_a), construction_mode);
        let key_b = partition_key(&runtime, &bundle, Some(&tenant_b), construction_mode);

        assert_ne!(key_a, key_b);
        assert!(!key_a.matches_exact(&key_b));
    }

    #[test]
    fn owner_authority_distinguishes_recreated_subject_incarnations() {
        let runtime = runtime_owner_for_limits(RuntimeLimits::application_web_standard());
        let bundle = RuntimeBundle::new("/tmp/nimbus-rti-recreated-tenant.mjs");
        let request = invocation_request("messages:list");
        let original = tenant_context(&request, "tenant-a", 1, "tenant-a");
        let recreated = tenant_context(&request, "tenant-a", 2, "tenant-a");
        let construction_mode = V8RuntimeConstructionMode::for_compatibility_target(
            runtime.policy().limits().compatibility_target,
        );

        let original_key = partition_key(&runtime, &bundle, Some(&original), construction_mode);
        let recreated_key = partition_key(&runtime, &bundle, Some(&recreated), construction_mode);

        assert_ne!(original_key, recreated_key);
        assert!(!original_key.matches_exact(&recreated_key));
    }

    #[test]
    fn mutable_v8_partition_rejects_missing_and_revoked_owners() {
        let request = invocation_request("messages:list");
        let ownerless = RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
        let missing = required_runtime_owner_lease(Some(&ownerless))
            .expect_err("ownerless mutable V8 retention must fail closed");
        assert!(
            missing
                .to_string()
                .contains("requires a runtime owner lease")
        );

        let owner_id = crate::RuntimeOwnerId::tenant(
            "tenant-a",
            std::num::NonZeroU64::new(1).expect("fixture incarnation must be positive"),
            Some("tenant-a"),
        )
        .expect("owner should build");
        let (lease, revocation) = crate::RuntimeOwnerLeaseIssuer.issue(owner_id);
        let revoked_context =
            RuntimeInvocationContext::top_level_for_tenant_with_owner(&request, "tenant-a", lease);
        assert!(revocation.revoke());

        let revoked = required_runtime_owner_lease(Some(&revoked_context))
            .expect_err("revoked mutable V8 retention must fail closed");
        assert!(revoked.to_string().contains("revoked"));
    }
}
