use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::Value;
use tokio::sync::oneshot;

use crate::RuntimeInvocationContext;
use crate::backends::v8::{DeferredV8RuntimeDropQueue, ReusableV8Runtime, V8WorkerRuntimePool};
use crate::error::Result;
use crate::execution_plan::RuntimeExecutionPlan;
use crate::executor::{SharedInvocationPermit, WorkerActivitySignal};
use crate::host::HostCallCancellation;
use crate::limits::{RuntimePolicy, RuntimePoolKind};
use crate::runtime::{
    CooperativeLockerRuntimeSlot, CooperativeRuntimeSlotPoll, CooperativeRuntimeSlotStart,
    InvocationRequest, RuntimeBundle, RuntimeHost, RuntimeInvocationExecution,
};
use crate::watchdog::WatchdogTimer;

pub(super) trait CooperativeBackendSlot: 'static {
    type ReusableRuntime;

    fn poll_once<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<CooperativeRuntimeSlotPoll>> + 'a>>;

    fn finish_with_runtime<'a>(
        self,
    ) -> Pin<Box<dyn Future<Output = (Result<Value>, Option<Self::ReusableRuntime>)> + 'a>>
    where
        Self: Sized;

    fn finish_with_result_and_runtime<'a>(
        self,
        result: Result<Value>,
    ) -> Pin<Box<dyn Future<Output = (Result<Value>, Option<Self::ReusableRuntime>)> + 'a>>
    where
        Self: Sized;

    fn is_ready_to_resume(&self) -> bool;
}

pub(super) struct CooperativeBackendInvocationStart {
    pub(super) watchdog: WatchdogTimer,
    pub(super) host: RuntimeHost,
    pub(super) policy: Arc<RuntimePolicy>,
    pub(super) bundle: RuntimeBundle,
    pub(super) request: InvocationRequest,
    pub(super) context: RuntimeInvocationContext,
    pub(super) execution_plan: RuntimeExecutionPlan,
    pub(super) cancellation: Option<HostCallCancellation>,
    pub(super) response_ready_tx: Option<oneshot::Sender<Value>>,
    pub(super) permit: SharedInvocationPermit,
    pub(super) activity_signal: Arc<WorkerActivitySignal>,
}

pub(super) trait CooperativeBackendDriver: 'static {
    type Slot: CooperativeBackendSlot;

    fn start_slot<'a>(
        &'a mut self,
        start: CooperativeBackendInvocationStart,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Slot>> + 'a>>;

    fn invoke_direct<'a>(
        &'a mut self,
        start: CooperativeBackendInvocationStart,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>>;

    fn retain_reusable_runtime(
        &mut self,
        policy: Arc<RuntimePolicy>,
        host: &RuntimeHost,
        bundle: &RuntimeBundle,
        context: &RuntimeInvocationContext,
        reusable_runtime: <Self::Slot as CooperativeBackendSlot>::ReusableRuntime,
    );

    fn idle_maintenance(&mut self, worker_is_idle: bool);

    fn clear_retained(&mut self);
}

pub(super) struct V8LockerDriver {
    v8_runtime_pool: V8WorkerRuntimePool,
    deferred_v8_runtime_drops: DeferredV8RuntimeDropQueue,
}

impl V8LockerDriver {
    pub(super) fn new() -> Self {
        Self {
            v8_runtime_pool: V8WorkerRuntimePool::new(),
            deferred_v8_runtime_drops: DeferredV8RuntimeDropQueue::new(),
        }
    }
}

impl CooperativeBackendSlot for CooperativeLockerRuntimeSlot {
    type ReusableRuntime = ReusableV8Runtime;

    fn poll_once<'a>(
        &'a mut self,
    ) -> Pin<Box<dyn Future<Output = Result<CooperativeRuntimeSlotPoll>> + 'a>> {
        Box::pin(CooperativeLockerRuntimeSlot::poll_once(self))
    }

    fn finish_with_runtime<'a>(
        self,
    ) -> Pin<Box<dyn Future<Output = (Result<Value>, Option<Self::ReusableRuntime>)> + 'a>>
    where
        Self: Sized,
    {
        Box::pin(CooperativeLockerRuntimeSlot::finish_with_runtime(self))
    }

    fn finish_with_result_and_runtime<'a>(
        self,
        result: Result<Value>,
    ) -> Pin<Box<dyn Future<Output = (Result<Value>, Option<Self::ReusableRuntime>)> + 'a>>
    where
        Self: Sized,
    {
        Box::pin(CooperativeLockerRuntimeSlot::finish_with_result_and_runtime(self, result))
    }

    fn is_ready_to_resume(&self) -> bool {
        CooperativeLockerRuntimeSlot::is_ready_to_resume(self)
    }
}

impl CooperativeBackendDriver for V8LockerDriver {
    type Slot = CooperativeLockerRuntimeSlot;

    fn start_slot<'a>(
        &'a mut self,
        start: CooperativeBackendInvocationStart,
    ) -> Pin<Box<dyn Future<Output = Result<Self::Slot>> + 'a>> {
        Box::pin(async move {
            let runtime = start.host.runtime_with_policy(start.policy);
            runtime
                .start_cooperative_locker_runtime_slot(
                    &mut self.v8_runtime_pool,
                    CooperativeRuntimeSlotStart {
                        invocation: RuntimeInvocationExecution {
                            watchdog: start.watchdog,
                            bundle: start.bundle,
                            request: start.request,
                            context: start.context,
                            execution_plan: start.execution_plan,
                            external_cancellation: start.cancellation,
                            response_ready_tx: start.response_ready_tx,
                            permit: start.permit,
                        },
                        activity_signal: start.activity_signal,
                    },
                )
                .await
        })
    }

    fn invoke_direct<'a>(
        &'a mut self,
        start: CooperativeBackendInvocationStart,
    ) -> Pin<Box<dyn Future<Output = Result<Value>> + 'a>> {
        Box::pin(async move {
            let runtime = start.host.runtime_with_policy(start.policy);
            runtime
                .invoke_bundle_unmanaged(
                    Some(&mut self.v8_runtime_pool),
                    RuntimeInvocationExecution {
                        watchdog: start.watchdog,
                        bundle: start.bundle,
                        request: start.request,
                        context: start.context,
                        execution_plan: start.execution_plan,
                        external_cancellation: start.cancellation,
                        response_ready_tx: start.response_ready_tx,
                        permit: start.permit,
                    },
                )
                .await
        })
    }

    fn retain_reusable_runtime(
        &mut self,
        policy: Arc<RuntimePolicy>,
        host: &RuntimeHost,
        bundle: &RuntimeBundle,
        context: &RuntimeInvocationContext,
        mut reusable_runtime: ReusableV8Runtime,
    ) {
        match policy.limits().runtime_pool_kind {
            RuntimePoolKind::WarmPool | RuntimePoolKind::WarmContextRecycle => {
                reusable_runtime.warm_reuse_count =
                    reusable_runtime.warm_reuse_count.saturating_add(1);
                let runtime_owner = host.runtime_with_policy(policy);
                self.v8_runtime_pool.return_runtime_for_invocation(
                    &runtime_owner,
                    bundle,
                    Some(context),
                    reusable_runtime,
                );
            }
            RuntimePoolKind::StartupSnapshotCache => {
                self.deferred_v8_runtime_drops
                    .defer(reusable_runtime.runtime);
            }
            RuntimePoolKind::BunJscTrustedRetained | RuntimePoolKind::BunJscFreshDiscard => {
                unreachable!("Bun/JSC pool kinds are rejected before V8 runtime invocation")
            }
        }
    }

    fn idle_maintenance(&mut self, worker_is_idle: bool) {
        self.deferred_v8_runtime_drops.drain_if_idle(worker_is_idle);
    }

    fn clear_retained(&mut self) {
        self.deferred_v8_runtime_drops.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_driver_v8_locker_driver_starts_with_no_deferred_drops() {
        let mut driver = V8LockerDriver::new();

        assert_eq!(driver.deferred_v8_runtime_drops.pending_len_for_test(), 0);
        driver.idle_maintenance(true);
        assert_eq!(driver.deferred_v8_runtime_drops.pending_len_for_test(), 0);
        driver.clear_retained();
        assert_eq!(driver.deferred_v8_runtime_drops.pending_len_for_test(), 0);
    }

    #[test]
    fn backend_slot_trait_exposes_v8_reusable_runtime_type() {
        let reusable_type = std::any::type_name::<
            <CooperativeLockerRuntimeSlot as CooperativeBackendSlot>::ReusableRuntime,
        >();

        assert!(reusable_type.ends_with("ReusableV8Runtime"));
    }
}
