use crate::backends::v8::ReusableV8Runtime;
use crate::limits::RuntimePoolKind;

use super::CooperativeWorkerLoop;

impl CooperativeWorkerLoop {
    pub(super) fn retain_or_defer_runtime_drop(
        &mut self,
        runtime_owner: &crate::runtime::NeovexRuntime,
        bundle: &crate::runtime::RuntimeBundle,
        context: &crate::RuntimeInvocationContext,
        mut runtime: ReusableV8Runtime,
    ) {
        match self.policy.limits().runtime_pool_kind {
            RuntimePoolKind::WarmPool => {
                if runtime.runtime.reset_request_state().is_err() {
                    self.policy.metrics().record_warm_pool_discard_unquiesced();
                    return;
                }
                runtime.warm_reuse_count = runtime.warm_reuse_count.saturating_add(1);
                self.v8_runtime_pool.return_runtime_for_invocation(
                    runtime_owner,
                    bundle,
                    Some(context),
                    runtime,
                );
            }
            RuntimePoolKind::StartupSnapshotCache => {
                self.deferred_v8_runtime_drops.defer(runtime.runtime);
            }
        }
    }

    pub(super) fn drain_deferred_v8_runtime_drops_if_idle(&mut self) {
        self.deferred_v8_runtime_drops
            .drain_if_idle(self.scheduler.is_idle());
    }
}
