use crate::RuntimeInvocationContext;
use crate::RuntimeOwnerId;
use crate::error::Result;
use crate::execution_plan::{RuntimePoolAuthorityFacts, RuntimePoolAuthorityKey};
use crate::limits::RuntimePolicy;
use crate::retained_state::{OwnerPartitionedPool, RetainedCheckout, RuntimeReuseAuthority};
use crate::runtime::RuntimeBundle;

use super::host_linker::InvocationHostState;

const DEFAULT_RETAINED_STORE_POOL_CAPACITY: usize = 2;
const DEFAULT_RETAINED_STORE_RETIRE_AFTER_INVOCATIONS: usize = 4;

pub(crate) struct ReusableStore {
    checkout: RetainedCheckout<ReusableStoreState, WasmtimeStoreAuthorityKey>,
}

struct ReusableStoreState {
    store: wasmtime::Store<InvocationHostState>,
    completed_invocations: usize,
}

impl ReusableStore {
    pub(crate) fn new(
        store: wasmtime::Store<InvocationHostState>,
        authority: RuntimeReuseAuthority<WasmtimeStoreAuthorityKey>,
    ) -> Self {
        Self {
            checkout: RetainedCheckout::fresh(
                ReusableStoreState {
                    store,
                    completed_invocations: 0,
                },
                authority,
            ),
        }
    }

    pub(crate) fn store_mut(&mut self) -> &mut wasmtime::Store<InvocationHostState> {
        &mut self.checkout.value_mut().store
    }

    fn finish_invocation(mut self) -> Self {
        let state = self.checkout.value_mut();
        state.completed_invocations = state.completed_invocations.saturating_add(1);
        self
    }

    fn completed_invocations(&self) -> usize {
        self.checkout.value().completed_invocations
    }

    #[cfg(test)]
    fn authority(&self) -> &RuntimeReuseAuthority<WasmtimeStoreAuthorityKey> {
        self.checkout.authority()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WasmtimeStoreAuthorityKey {
    pool_authority: RuntimePoolAuthorityKey,
    actual_bundle_sha256: String,
}

impl WasmtimeStoreAuthorityKey {
    pub(crate) fn for_invocation(
        policy: &RuntimePolicy,
        bundle: &RuntimeBundle,
        context: &RuntimeInvocationContext,
    ) -> Result<RuntimeReuseAuthority<Self>> {
        let owner_lease = context.runtime_owner_lease().ok_or_else(|| {
            crate::error::NimbusRuntimeError::Contract(
                "mutable Wasmtime Store retention requires a runtime owner lease".to_string(),
            )
        })?;
        RuntimeReuseAuthority::new_with_deployment(
            owner_lease.clone(),
            context.deployment_authority_lease().cloned(),
            Self {
                pool_authority: RuntimePoolAuthorityKey::exact(
                    RuntimePoolAuthorityFacts::for_profileless_retained_state(
                        policy,
                        bundle,
                        "wasmtime_component_store",
                    )?,
                ),
                actual_bundle_sha256: RuntimeBundle::compute_sha256_for_path(bundle.entrypoint())?,
            },
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WasmtimeStorePoolStats {
    pub hits: usize,
    pub misses: usize,
    pub authority_mismatches: usize,
    pub evictions: usize,
    pub retirements: usize,
    pub revoked_discards: usize,
}

pub(crate) struct WasmtimeStorePool {
    retained: OwnerPartitionedPool<ReusableStoreState, WasmtimeStoreAuthorityKey>,
    retire_after_invocations: usize,
    max_reuse_retirements: usize,
}

impl WasmtimeStorePool {
    pub(crate) fn new() -> Self {
        Self {
            retained: OwnerPartitionedPool::new(DEFAULT_RETAINED_STORE_POOL_CAPACITY, 1),
            retire_after_invocations: DEFAULT_RETAINED_STORE_RETIRE_AFTER_INVOCATIONS,
            max_reuse_retirements: 0,
        }
    }

    pub(crate) fn take(
        &mut self,
        authority: &RuntimeReuseAuthority<WasmtimeStoreAuthorityKey>,
    ) -> Result<Option<ReusableStore>> {
        Ok(self
            .retained
            .checkout(authority)?
            .map(|checkout| ReusableStore { checkout }))
    }

    pub(crate) fn return_store(&mut self, store: ReusableStore) {
        let store = store.finish_invocation();
        if store.completed_invocations() >= self.retire_after_invocations {
            self.max_reuse_retirements = self.max_reuse_retirements.saturating_add(1);
            return;
        }
        self.retained.retain(store.checkout);
    }

    pub(crate) fn retire_owner(&mut self, owner_id: &RuntimeOwnerId) -> usize {
        self.retained.retire_owner(owner_id)
    }

    pub(crate) fn retire_deployment_authority(
        &mut self,
        authority_id: &crate::RuntimeDeploymentAuthorityId,
    ) -> usize {
        self.retained.retire_deployment_authority(authority_id)
    }

    pub(crate) fn stats(&self) -> WasmtimeStorePoolStats {
        let common = self.retained.stats();
        WasmtimeStorePoolStats {
            hits: common.hits,
            misses: common.misses,
            authority_mismatches: common.owner_mismatch_denials,
            evictions: common.evictions,
            retirements: common
                .retirements
                .saturating_add(self.max_reuse_retirements),
            revoked_discards: common.revoked_discards,
        }
    }

    #[cfg(test)]
    pub(crate) fn stats_for_test(&self) -> WasmtimeStorePoolStats {
        self.stats()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::Value;

    use crate::host::{HostBridge, HostCallRequest};
    use crate::limits::{RuntimeLimits, RuntimePolicy};
    use crate::runtime::{InvocationKind, InvocationRequest, RuntimeBundle};

    use super::*;

    struct NoopHost;

    impl HostBridge for NoopHost {
        fn call(&self, _request: HostCallRequest) -> crate::Result<Value> {
            Ok(Value::Null)
        }
    }

    #[test]
    fn wasmtime_store_pool_reuses_matching_authority_and_denies_mismatch() {
        let engine = super::super::host_linker::create_wasmtime_component_engine()
            .expect("engine should build");
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("component.wat");
        std::fs::write(&bundle_path, "(component)").expect("component should write");
        let bundle = RuntimeBundle::wasm_component(&bundle_path);
        let policy =
            RuntimePolicy::new(RuntimeLimits::application_wasm_component_retained_store_pool());
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "wasm:handler".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };
        let owner_a = crate::RuntimeOwnerId::tenant(
            "tenant-a",
            std::num::NonZeroU64::new(1).expect("fixture incarnation is positive"),
            Some("tenant-a"),
        )
        .expect("owner should build");
        let owner_b = crate::RuntimeOwnerId::tenant(
            "tenant-b",
            std::num::NonZeroU64::new(1).expect("fixture incarnation is positive"),
            Some("tenant-b"),
        )
        .expect("owner should build");
        let (lease_a, _) = crate::RuntimeOwnerLeaseIssuer.issue(owner_a);
        let (lease_b, _) = crate::RuntimeOwnerLeaseIssuer.issue(owner_b);
        let context_a = crate::RuntimeInvocationContext::top_level_for_tenant_with_owner(
            &request,
            "tenant-a",
            lease_a.clone(),
        );
        let context_b = crate::RuntimeInvocationContext::top_level_for_tenant_with_owner(
            &request, "tenant-b", lease_b,
        );
        let authority_a = WasmtimeStoreAuthorityKey::for_invocation(&policy, &bundle, &context_a)
            .expect("authority should build");
        let authority_b = WasmtimeStoreAuthorityKey::for_invocation(&policy, &bundle, &context_b)
            .expect("authority should build");
        let mut store = wasmtime::Store::new(
            &engine,
            InvocationHostState::new_for_policy(Arc::new(NoopHost), context_a, None, &policy),
        );
        store.limiter(|state| state.resource_limiter());
        let mut pool = WasmtimeStorePool::new();

        pool.return_store(ReusableStore::new(store, authority_a.clone()));
        assert!(
            pool.take(&authority_b)
                .expect("mismatched checkout should be admitted")
                .is_none(),
            "authority mismatch denial must not return a retained Store"
        );
        let reused = pool
            .take(&authority_a)
            .expect("matching checkout should be admitted")
            .expect("matching authority should reuse retained Store");

        assert_eq!(reused.authority(), &authority_a);
        assert_eq!(pool.stats_for_test().authority_mismatches, 1);
        assert_eq!(pool.stats_for_test().hits, 1);
    }

    #[test]
    fn wasmtime_retained_store_authority_requires_active_owner_incarnation() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("component.wat");
        std::fs::write(&bundle_path, "(component)").expect("component should write");
        let bundle = RuntimeBundle::wasm_component(&bundle_path);
        let policy =
            RuntimePolicy::new(RuntimeLimits::application_wasm_component_retained_store_pool());
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "wasm:handler".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };
        let ownerless = crate::RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
        let missing = WasmtimeStoreAuthorityKey::for_invocation(&policy, &bundle, &ownerless)
            .expect_err("ownerless retained Store admission must fail closed");
        assert!(
            missing
                .to_string()
                .contains("requires a runtime owner lease")
        );

        let owner = crate::RuntimeOwnerId::tenant(
            "tenant-a",
            std::num::NonZeroU64::new(1).expect("fixture incarnation is positive"),
            Some("tenant-a"),
        )
        .expect("owner should build");
        let (lease, revocation) = crate::RuntimeOwnerLeaseIssuer.issue(owner);
        let revoked = crate::RuntimeInvocationContext::top_level_for_tenant_with_owner(
            &request, "tenant-a", lease,
        );
        assert!(revocation.revoke());
        let revoked_error = WasmtimeStoreAuthorityKey::for_invocation(&policy, &bundle, &revoked)
            .expect_err("revoked retained Store admission must fail closed");
        assert!(revoked_error.to_string().contains("revoked"));
    }

    #[test]
    fn wasmtime_authority_distinguishes_subject_and_incarnation_independently() {
        let tempdir = tempfile::tempdir().expect("tempdir should build");
        let bundle_path = tempdir.path().join("component.wat");
        std::fs::write(&bundle_path, "(component)").expect("component should write");
        let bundle = RuntimeBundle::wasm_component(&bundle_path);
        let policy =
            RuntimePolicy::new(RuntimeLimits::application_wasm_component_retained_store_pool());
        let request = InvocationRequest {
            kind: InvocationKind::Query,
            function_name: "wasm:handler".to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
            services: Default::default(),
        };
        let context = |subject: &str, incarnation: u64| {
            let owner = crate::RuntimeOwnerId::tenant(
                subject,
                std::num::NonZeroU64::new(incarnation).expect("fixture incarnation is positive"),
                Some("shared-label"),
            )
            .expect("owner should build");
            let (lease, _) = crate::RuntimeOwnerLeaseIssuer.issue(owner);
            crate::RuntimeInvocationContext::top_level_for_tenant_with_owner(
                &request,
                "shared-label",
                lease,
            )
        };

        let first =
            WasmtimeStoreAuthorityKey::for_invocation(&policy, &bundle, &context("tenant-a", 1))
                .expect("authority should build");
        let other_subject =
            WasmtimeStoreAuthorityKey::for_invocation(&policy, &bundle, &context("tenant-b", 1))
                .expect("authority should build");
        let other_incarnation =
            WasmtimeStoreAuthorityKey::for_invocation(&policy, &bundle, &context("tenant-a", 2))
                .expect("authority should build");

        assert_ne!(first, other_subject);
        assert_ne!(first, other_incarnation);
    }
}
