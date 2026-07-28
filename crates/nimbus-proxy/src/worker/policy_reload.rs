//! Exact policy-reload effect receipts for one running workload PEP.

use nimbus_egress::CompiledEgressPolicy;

use crate::error::{EgressProxyError, Result};
use crate::policy_state::{PolicyReloadAttempt, PolicyReloadObservation, PolicyReloadReceipt};

use super::WorkloadPep;

impl WorkloadPep {
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
        let mut guard =
            self.policy_state
                .write()
                .map_err(|_| EgressProxyError::OperationFailed {
                    message: "egress proxy policy lock is poisoned".to_owned(),
                })?;
        guard
            .reload_for_attempt(policy, attempt)
            .map_err(|message| EgressProxyError::OperationFailed { message })
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
