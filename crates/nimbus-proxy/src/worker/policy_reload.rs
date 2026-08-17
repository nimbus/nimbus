//! Exact policy-reload effect receipts for one running workload PEP.

use nimbus_egress::CompiledEgressPolicy;

use crate::error::{EgressProxyError, Result};
use crate::policy_state::{
    PolicyReloadAttempt, PolicyReloadObservation, PolicyReloadReceipt, WorkloadPepPolicyEvidence,
};

use super::WorkloadPep;

impl WorkloadPep {
    /// Inspect policy bytes, reload identity, and readiness under one lock.
    pub fn inspect_policy_evidence(
        &self,
        policy: &CompiledEgressPolicy,
    ) -> Result<WorkloadPepPolicyEvidence> {
        let guard = self
            .policy_state
            .read()
            .map_err(|_| EgressProxyError::OperationFailed {
                message: "egress proxy policy lock is poisoned".to_owned(),
            })?;
        let (audit_healthy, worker_live) = self.health.snapshot();
        Ok(guard.policy_evidence(policy, audit_healthy, worker_live))
    }

    /// Apply one caller-persisted reload attempt idempotently.
    ///
    /// Replaying the exact attempt and policy returns the original receipt.
    /// Reusing an attempt for different policy bytes or replaying a stale
    /// generation fails closed.
    pub fn reload_policy_for_attempt(
        &self,
        policy: CompiledEgressPolicy,
        attempt: PolicyReloadAttempt,
    ) -> Result<PolicyReloadReceipt> {
        self.health.with_ready_control_effect(|| {
            let mut guard =
                self.policy_state
                    .write()
                    .map_err(|_| EgressProxyError::OperationFailed {
                        message: "egress proxy policy lock is poisoned".to_owned(),
                    })?;
            guard
                .reload_for_attempt(policy, attempt)
                .map_err(|message| EgressProxyError::OperationFailed { message })
        })
    }

    /// Inspect whether the exact durable attempt and policy are active.
    pub fn inspect_policy_reload(
        &self,
        policy: &CompiledEgressPolicy,
        attempt: PolicyReloadAttempt,
    ) -> Result<PolicyReloadObservation> {
        let guard = self
            .policy_state
            .read()
            .map_err(|_| EgressProxyError::OperationFailed {
                message: "egress proxy policy lock is poisoned".to_owned(),
            })?;
        Ok(guard.observe_reload(policy, attempt))
    }
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroU64;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::worker::WorkloadPepConfig;

    fn next_attempt() -> PolicyReloadAttempt {
        PolicyReloadAttempt::new(
            NonZeroU64::new(2).expect("test desired generation is nonzero"),
            NonZeroU64::new(2).expect("test attempt generation is nonzero"),
        )
    }

    #[test]
    fn reload_rejects_stopped_worker_without_policy_mutation() {
        let policy = CompiledEgressPolicy::deny_all();
        let mut pep = WorkloadPep::start(WorkloadPepConfig::new(policy.clone()))
            .expect("test PEP should start");
        pep.shutdown()
            .expect("worker should acknowledge explicit listener shutdown");
        let attempt = next_attempt();

        let error = pep
            .reload_policy_for_attempt(policy.clone(), attempt)
            .expect_err("a stopped PEP must reject a policy reload effect");

        assert!(
            error.to_string().contains("worker is not live"),
            "stopped-worker rejection must retain an exact diagnostic: {error}"
        );
        assert_eq!(
            pep.inspect_policy_reload(&policy, attempt)
                .expect("retained policy should remain inspectable"),
            PolicyReloadObservation::Untagged,
            "rejected reload must not mutate the retained active policy"
        );
    }

    #[test]
    fn reload_rejects_unhealthy_audit_without_policy_mutation() {
        let audit_healthy = Arc::new(AtomicBool::new(true));
        let policy = CompiledEgressPolicy::deny_all();
        let pep = WorkloadPep::start(
            WorkloadPepConfig::new(policy.clone())
                .with_audit_health_probe(Arc::clone(&audit_healthy)),
        )
        .expect("test PEP should start");
        audit_healthy.store(false, Ordering::SeqCst);
        let attempt = next_attempt();

        let error = pep
            .reload_policy_for_attempt(policy.clone(), attempt)
            .expect_err("an audit-unhealthy PEP must reject a policy reload effect");

        assert!(
            error.to_string().contains("audit is not healthy"),
            "audit-health rejection must retain an exact diagnostic: {error}"
        );
        assert_eq!(
            pep.inspect_policy_reload(&policy, attempt)
                .expect("retained policy should remain inspectable"),
            PolicyReloadObservation::Untagged,
            "rejected reload must not mutate the retained active policy"
        );
    }
}
