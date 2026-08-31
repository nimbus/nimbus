use std::future::Future;
use std::pin::Pin;
use std::sync::OnceLock;
use std::time::Instant;

use crate::backends::{RuntimeBackend, RuntimeBackendFactory, RuntimeBackendInvocation};
use crate::error::Result;
use crate::execution_plan::RuntimeExecutionPlan;
use crate::runtime::RuntimeInvocationExecution;

pub(crate) mod embedder;
mod lifecycle;
mod startup;
mod startup_key;
mod warm_pool;

use self::embedder::{JsRuntime, v8};

pub(crate) use self::lifecycle::{
    RuntimeReuseLifecycle, WarmRuntimeBoundaryMaintenance, WarmRuntimeCondemnationReason,
    WarmRuntimeRetentionDecision, prepare_warm_runtime_for_retention,
};
#[cfg(test)]
pub(crate) use self::lifecycle::{
    RuntimeReuseLifecycleState, WarmPoolMemoryPressureEviction,
    heap_carryover_limit_bytes_for_test, retained_entry_eviction_count_for_pressure,
    retained_entry_eviction_count_for_pressure_for_test,
};
#[cfg(test)]
pub(crate) use self::startup::v8_bootstrap_snapshot_build_count_for_test;
pub(crate) use self::startup::{
    EMBEDDED_NODE22_ANCHOR_SNAPSHOT, V8RuntimeConstructionMode, V8StartupSnapshot,
    create_v8_startup_snapshot, try_embedded_node22_anchor_snapshot,
};
pub use self::startup::{
    build_embeddable_node22_snapshot_blob, check_generated_embedded_anchor_snapshot,
};
pub(crate) use self::startup_key::RuntimeStartupSnapshotKey;
pub(crate) use self::warm_pool::{ReusableV8Runtime, V8WorkerRuntimePool};

pub(crate) fn attach_cppgc_heap(create_params: v8::CreateParams) -> v8::CreateParams {
    let heap = v8::cppgc::Heap::create(cppgc_platform(), v8::cppgc::HeapCreateParams::default());
    create_params.cpp_heap(heap)
}

fn cppgc_platform() -> v8::SharedRef<v8::Platform> {
    static CPPGC_PLATFORM: OnceLock<v8::SharedRef<v8::Platform>> = OnceLock::new();
    CPPGC_PLATFORM
        .get_or_init(|| {
            let thread_pool_size = std::cmp::min(
                std::thread::available_parallelism()
                    .map(|n| n.get() as u32)
                    .unwrap_or(4),
                4,
            );
            let platform = v8::new_default_platform(thread_pool_size, false).make_shared();
            v8::cppgc::initialize_process(platform.clone());
            platform
        })
        .clone()
}

#[derive(Debug, Default)]
pub(crate) struct V8RuntimeBackendFactory;

impl RuntimeBackendFactory for V8RuntimeBackendFactory {
    fn create(&self) -> Box<dyn RuntimeBackend> {
        // Force NodeFull-first: arm + BLOCK on the NodeFull RO-heap anchor before this
        // backend's pool exists or serves, so the cage RO heap is NodeFull's superset and a
        // WebStandard-first install is unreachable (Option A crash fix).
        crate::runtime::driver::anchor::enable_and_arm_nodefull_anchor();
        Box::new(V8RuntimeBackend {
            v8_runtime_pool: V8WorkerRuntimePool::new(),
        })
    }
}

struct V8RuntimeBackend {
    v8_runtime_pool: V8WorkerRuntimePool,
}

impl RuntimeBackend for V8RuntimeBackend {
    fn invoke<'a>(
        &'a mut self,
        invocation: RuntimeBackendInvocation,
    ) -> Pin<Box<dyn Future<Output = Result<serde_json::Value>> + 'a>> {
        let RuntimeBackendInvocation {
            watchdog,
            host,
            policy,
            bundle,
            request,
            context,
            cancellation,
            permit,
        } = invocation;
        Box::pin(async move {
            let execution_plan_started_at = Instant::now();
            let execution_plan =
                RuntimeExecutionPlan::for_invocation(policy.as_ref(), &request, &context);
            policy
                .metrics()
                .record_execution_plan_build(execution_plan_started_at.elapsed());
            let runtime = host.runtime_with_policy(policy);
            runtime
                .invoke_bundle_unmanaged(
                    Some(&mut self.v8_runtime_pool),
                    RuntimeInvocationExecution {
                        watchdog,
                        bundle,
                        request,
                        context,
                        execution_plan,
                        external_cancellation: cancellation,
                        response_ready_tx: None,
                        permit,
                    },
                )
                .await
        })
    }

    fn retire_owner(&mut self, owner_id: &crate::RuntimeOwnerId) -> usize {
        self.v8_runtime_pool.retire_owner(owner_id)
    }

    fn retire_deployment_authority(
        &mut self,
        authority_id: &crate::RuntimeDeploymentAuthorityId,
    ) -> usize {
        self.v8_runtime_pool
            .retire_deployment_authority(authority_id)
    }
}

#[derive(Default)]
pub(crate) struct DeferredV8RuntimeDropQueue {
    pending: Vec<JsRuntime>,
}

impl DeferredV8RuntimeDropQueue {
    pub(crate) fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub(crate) fn defer(&mut self, runtime: JsRuntime) {
        self.pending.push(runtime);
    }

    pub(crate) fn drain_if_idle(&mut self, worker_is_idle: bool) {
        if !worker_is_idle || self.pending.is_empty() {
            return;
        }

        self.pending.clear();
    }

    pub(crate) fn clear(&mut self) {
        self.pending.clear();
    }

    #[cfg(test)]
    pub(crate) fn pending_len_for_test(&self) -> usize {
        self.pending.len()
    }
}
