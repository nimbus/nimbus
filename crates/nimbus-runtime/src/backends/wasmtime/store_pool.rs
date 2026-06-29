use std::collections::VecDeque;
use std::path::PathBuf;

use crate::RuntimeInvocationContext;
use crate::error::Result;
use crate::limits::{RuntimeBackendLifecyclePolicy, RuntimePolicy, RuntimePoolKind};
use crate::runtime::{RuntimeBundle, RuntimeComponentWorld};

use super::host_linker::InvocationHostState;

const DEFAULT_RETAINED_STORE_POOL_CAPACITY: usize = 2;
const DEFAULT_RETAINED_STORE_RETIRE_AFTER_INVOCATIONS: usize = 4;

pub(crate) struct ReusableStore {
    store: wasmtime::Store<InvocationHostState>,
    authority: WasmtimeStoreAuthorityKey,
    completed_invocations: usize,
}

impl ReusableStore {
    pub(crate) fn new(
        store: wasmtime::Store<InvocationHostState>,
        authority: WasmtimeStoreAuthorityKey,
    ) -> Self {
        Self {
            store,
            authority,
            completed_invocations: 0,
        }
    }

    pub(crate) fn store_mut(&mut self) -> &mut wasmtime::Store<InvocationHostState> {
        &mut self.store
    }

    fn finish_invocation(mut self) -> Self {
        self.completed_invocations = self.completed_invocations.saturating_add(1);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WasmtimeStoreAuthorityKey {
    bundle_sha256: String,
    target_world: Option<RuntimeComponentWorld>,
    entrypoint: PathBuf,
    tenant_label: Option<String>,
    max_heap_mb: usize,
    initial_heap_mb: usize,
    runtime_pool_kind: RuntimePoolKind,
    lifecycle_policy: RuntimeBackendLifecyclePolicy,
}

impl WasmtimeStoreAuthorityKey {
    pub(crate) fn for_invocation(
        policy: &RuntimePolicy,
        bundle: &RuntimeBundle,
        context: &RuntimeInvocationContext,
    ) -> Result<Self> {
        Ok(Self {
            bundle_sha256: RuntimeBundle::compute_sha256_for_path(bundle.entrypoint())?,
            target_world: bundle.target_world(),
            entrypoint: bundle.entrypoint().to_path_buf(),
            tenant_label: context.tenant_label.clone(),
            max_heap_mb: policy.limits().max_heap_mb,
            initial_heap_mb: policy.limits().initial_heap_mb,
            runtime_pool_kind: policy.limits().runtime_pool_kind,
            lifecycle_policy: policy.limits().backend_lifecycle_policy,
        })
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WasmtimeStorePoolStats {
    pub hits: usize,
    pub misses: usize,
    pub authority_mismatches: usize,
    pub evictions: usize,
    pub retirements: usize,
}

pub(crate) struct WasmtimeStorePool {
    retained: VecDeque<ReusableStore>,
    capacity: usize,
    retire_after_invocations: usize,
    stats: WasmtimeStorePoolStats,
}

impl WasmtimeStorePool {
    pub(crate) fn new() -> Self {
        Self {
            retained: VecDeque::new(),
            capacity: DEFAULT_RETAINED_STORE_POOL_CAPACITY,
            retire_after_invocations: DEFAULT_RETAINED_STORE_RETIRE_AFTER_INVOCATIONS,
            stats: WasmtimeStorePoolStats::default(),
        }
    }

    pub(crate) fn take(&mut self, authority: &WasmtimeStoreAuthorityKey) -> Option<ReusableStore> {
        let Some(position) = self
            .retained
            .iter()
            .position(|store| store.authority == *authority)
        else {
            if !self.retained.is_empty() {
                self.stats.authority_mismatches = self.stats.authority_mismatches.saturating_add(1);
            }
            self.stats.misses = self.stats.misses.saturating_add(1);
            return None;
        };
        self.stats.hits = self.stats.hits.saturating_add(1);
        self.retained.remove(position)
    }

    pub(crate) fn return_store(&mut self, store: ReusableStore) {
        let store = store.finish_invocation();
        if store.completed_invocations >= self.retire_after_invocations {
            self.stats.retirements = self.stats.retirements.saturating_add(1);
            return;
        }
        self.retained.push_back(store);
        while self.retained.len() > self.capacity {
            self.retained.pop_front();
            self.stats.evictions = self.stats.evictions.saturating_add(1);
        }
    }

    pub(crate) fn stats(&self) -> WasmtimeStorePoolStats {
        self.stats
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
        let context_a = crate::RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-a");
        let context_b = crate::RuntimeInvocationContext::top_level_for_tenant(&request, "tenant-b");
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
            pool.take(&authority_b).is_none(),
            "authority mismatch denial must not return a retained Store"
        );
        let reused = pool
            .take(&authority_a)
            .expect("matching authority should reuse retained Store");

        assert_eq!(reused.authority, authority_a);
        assert_eq!(pool.stats_for_test().authority_mismatches, 1);
        assert_eq!(pool.stats_for_test().hits, 1);
    }
}
