//! Linearizable worker and audit health for control-plane effects.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use crate::error::{EgressProxyError, Result};

/// Sticky process-lifetime health shared by the request path and control
/// plane.
///
/// The transition gate makes a health-qualified control effect linearizable:
/// once a worker-stop or audit-failure transition starts, no later effect can
/// observe the prior healthy state, and neither transition can cross an effect
/// that already authenticated both conditions.
pub(crate) struct WorkloadPepHealth {
    audit_healthy: Arc<AtomicBool>,
    worker_live: AtomicBool,
    transition_gate: RwLock<()>,
}

impl WorkloadPepHealth {
    pub(crate) fn new(audit_healthy: Arc<AtomicBool>) -> Self {
        Self {
            audit_healthy,
            worker_live: AtomicBool::new(true),
            transition_gate: RwLock::new(()),
        }
    }

    pub(crate) fn snapshot(&self) -> (bool, bool) {
        let _gate = self
            .transition_gate
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (
            self.audit_healthy.load(Ordering::SeqCst),
            self.worker_live.load(Ordering::SeqCst),
        )
    }

    pub(crate) fn audit_is_healthy(&self) -> bool {
        self.snapshot().0
    }

    pub(crate) fn mark_audit_unhealthy(&self) {
        let _gate = self
            .transition_gate
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.audit_healthy.store(false, Ordering::SeqCst);
    }

    pub(crate) fn mark_worker_stopped(&self) {
        let _gate = self
            .transition_gate
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        self.worker_live.store(false, Ordering::SeqCst);
    }

    pub(crate) fn with_ready_control_effect<R>(
        &self,
        effect: impl FnOnce() -> Result<R>,
    ) -> Result<R> {
        let _gate = self
            .transition_gate
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !self.worker_live.load(Ordering::SeqCst) {
            return Err(EgressProxyError::OperationFailed {
                message: "egress proxy worker is not live; control effect rejected".to_owned(),
            });
        }
        if !self.audit_healthy.load(Ordering::SeqCst) {
            return Err(EgressProxyError::OperationFailed {
                message: "egress proxy decision audit is not healthy; control effect rejected"
                    .to_owned(),
            });
        }
        effect()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use super::*;

    fn assert_transition_waits_for_control_effect(
        transition: fn(&WorkloadPepHealth),
        expected_rejection: &str,
    ) {
        let health = Arc::new(WorkloadPepHealth::new(Arc::new(AtomicBool::new(true))));
        let (effect_entered_tx, effect_entered_rx) = mpsc::channel();
        let (release_effect_tx, release_effect_rx) = mpsc::channel();
        let effect_health = Arc::clone(&health);
        let effect = thread::spawn(move || {
            effect_health.with_ready_control_effect(|| {
                effect_entered_tx
                    .send(())
                    .expect("test should observe the authenticated effect");
                release_effect_rx
                    .recv_timeout(Duration::from_secs(1))
                    .expect("test must release the authenticated effect");
                Ok(())
            })
        });
        effect_entered_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("control effect should enter under the shared gate");

        let (transition_done_tx, transition_done_rx) = mpsc::channel();
        let transition_health = Arc::clone(&health);
        let transition_thread = thread::spawn(move || {
            transition(&transition_health);
            transition_done_tx
                .send(())
                .expect("test should observe the completed health transition");
        });
        assert!(
            transition_done_rx
                .recv_timeout(Duration::from_millis(50))
                .is_err(),
            "health transition must not cross an authenticated control effect"
        );

        release_effect_tx
            .send(())
            .expect("test should release the control effect");
        effect
            .join()
            .expect("control-effect thread should not panic")
            .expect("the pre-transition control effect should succeed");
        transition_done_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("health transition should complete after the effect releases");
        transition_thread
            .join()
            .expect("health-transition thread should not panic");

        let error = health
            .with_ready_control_effect(|| Ok(()))
            .expect_err("a post-transition control effect must fail closed");
        assert!(
            error.to_string().contains(expected_rejection),
            "post-transition rejection must identify the failed health fence: {error}"
        );
    }

    #[test]
    fn audit_failure_cannot_cross_authenticated_control_effect() {
        assert_transition_waits_for_control_effect(
            WorkloadPepHealth::mark_audit_unhealthy,
            "audit is not healthy",
        );
    }

    #[test]
    fn worker_stop_cannot_cross_authenticated_control_effect() {
        assert_transition_waits_for_control_effect(
            WorkloadPepHealth::mark_worker_stopped,
            "worker is not live",
        );
    }
}
