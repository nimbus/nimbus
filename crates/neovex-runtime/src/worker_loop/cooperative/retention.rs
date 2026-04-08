use crate::limits::RuntimePoolKind;
use crate::runtime::ReusableRuntime;

use super::CooperativeWorkerLoop;

impl CooperativeWorkerLoop {
    pub(super) fn retain_or_defer_runtime_drop(
        &mut self,
        runtime_owner: &crate::runtime::NeovexRuntime,
        bundle: &crate::runtime::RuntimeBundle,
        context: &crate::RuntimeInvocationContext,
        runtime: ReusableRuntime,
    ) {
        match self.policy.limits().runtime_pool_kind {
            RuntimePoolKind::WarmPool => {
                let mut runtime = runtime;
                if runtime.runtime.reset_request_state().is_err() {
                    self.policy.metrics().record_warm_pool_discard_unquiesced();
                    return;
                }
                runtime.retained_reuse_count = runtime.retained_reuse_count.saturating_add(1);
                self.isolate_pool.return_runtime_for_invocation(
                    runtime_owner,
                    bundle,
                    Some(context),
                    runtime,
                );
            }
            RuntimePoolKind::StartupSnapshotCache => {
                self.deferred_runtime_drops.push(runtime.runtime);
            }
        }
    }

    pub(super) fn drain_deferred_runtime_drops_if_idle(&mut self) {
        if !self.scheduler.is_idle() || self.deferred_runtime_drops.is_empty() {
            return;
        }

        self.deferred_runtime_drops.clear();
    }
}
