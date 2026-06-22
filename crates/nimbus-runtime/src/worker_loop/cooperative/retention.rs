use crate::backends::v8::ReusableV8Runtime;
use crate::limits::RuntimePoolKind;
use crate::runtime::RuntimeHost;

use super::CooperativeWorkerLoop;

impl CooperativeWorkerLoop {
    pub(super) fn retain_or_defer_runtime_drop(
        &mut self,
        host: &RuntimeHost,
        bundle: &crate::runtime::RuntimeBundle,
        context: &crate::RuntimeInvocationContext,
        mut runtime: ReusableV8Runtime,
    ) {
        match self.policy.limits().runtime_pool_kind {
            RuntimePoolKind::WarmPool | RuntimePoolKind::WarmContextRecycle => {
                runtime.warm_reuse_count = runtime.warm_reuse_count.saturating_add(1);
                let runtime_owner = host.runtime_with_policy(self.policy.clone());
                self.v8_runtime_pool.return_runtime_for_invocation(
                    &runtime_owner,
                    bundle,
                    Some(context),
                    runtime,
                );
            }
            RuntimePoolKind::StartupSnapshotCache => {
                self.deferred_v8_runtime_drops.defer(runtime.runtime);
            }
            RuntimePoolKind::BunJscTrustedRetained | RuntimePoolKind::BunJscFreshDiscard => {
                unreachable!("Bun/JSC pool kinds are rejected before V8 runtime invocation")
            }
        }
    }

    pub(super) fn drain_deferred_v8_runtime_drops_if_idle(&mut self) {
        self.deferred_v8_runtime_drops
            .drain_if_idle(self.scheduler.is_idle());
    }
}
