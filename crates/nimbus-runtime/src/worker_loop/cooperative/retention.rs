use crate::runtime::RuntimeHost;

use super::CooperativeWorkerLoop;
use super::backend::CooperativeBackendDriver;

impl<D: CooperativeBackendDriver> CooperativeWorkerLoop<D> {
    pub(super) fn retain_or_defer_runtime_drop(
        &mut self,
        host: &RuntimeHost,
        bundle: &crate::runtime::RuntimeBundle,
        context: &crate::RuntimeInvocationContext,
        reusable_runtime: <D::Slot as super::backend::CooperativeBackendSlot>::ReusableRuntime,
    ) {
        self.driver.retain_reusable_runtime(
            self.policy.clone(),
            host,
            bundle,
            context,
            reusable_runtime,
        );
    }

    pub(super) fn drain_deferred_v8_runtime_drops_if_idle(&mut self) {
        self.driver.idle_maintenance(self.scheduler.is_idle());
    }
}
