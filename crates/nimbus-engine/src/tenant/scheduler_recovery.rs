use std::sync::atomic::{AtomicBool, Ordering};

use super::TenantRuntime;

/// One-shot intent to recover orphaned scheduler claims after opening durable
/// tenant state.
///
/// Opening a provider runtime must not acquire sequence authority: another
/// healthy Nimbus process may own the committer lease. The intent is consumed
/// only inside the runtime's serialized committer immediately before its first
/// scheduler write. A failed recovery restores the intent so lease contention,
/// cancellation, or provider errors remain retryable without a second runtime
/// load.
#[derive(Default)]
pub(super) struct SchedulerRecoveryIntent {
    pending: AtomicBool,
}

impl SchedulerRecoveryIntent {
    fn mark_pending(&self) {
        self.pending.store(true, Ordering::Release);
    }

    fn begin(&self) -> bool {
        self.pending.swap(false, Ordering::AcqRel)
    }

    fn restore(&self) {
        self.pending.store(true, Ordering::Release);
    }
}

impl TenantRuntime {
    pub(crate) fn mark_scheduler_recovery_pending(&self) {
        self.scheduler_recovery.mark_pending();
    }

    pub(super) fn begin_scheduler_recovery(&self) -> bool {
        self.scheduler_recovery.begin()
    }

    pub(super) fn restore_scheduler_recovery(&self) {
        self.scheduler_recovery.restore();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheduler_recovery_intent_is_one_shot_and_restorable() {
        let intent = SchedulerRecoveryIntent::default();
        assert!(!intent.begin());

        intent.mark_pending();
        assert!(intent.begin());
        assert!(!intent.begin());

        intent.restore();
        assert!(intent.begin());
        assert!(!intent.begin());
    }
}
