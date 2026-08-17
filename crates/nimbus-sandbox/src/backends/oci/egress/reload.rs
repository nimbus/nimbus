//! Durable desired generation and exact PEP reload-attempt reconciliation.

use std::num::NonZeroU64;
#[cfg(test)]
use std::sync::atomic::{AtomicU64, Ordering};

use nimbus_core::TenantId;
use nimbus_egress::CompiledEgressPolicy;
use nimbus_proxy::{PolicyReloadAttempt, PolicyReloadObservation, PolicyReloadReceipt};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

#[cfg(test)]
use super::egress_proxy_error;
use super::{EgressProxyAssignment, EgressProxyRegistry};

/// Container-manifest state for the desired egress policy and its latest
/// provider-effect attempt.
///
/// The policy bytes remain in `SandboxSpec::egress`. This state gives those
/// bytes a monotonic desired generation and persists one attempt generation
/// before the PEP is touched. `Applying` therefore survives a lost provider
/// acknowledgement and can be reconciled by exact PEP inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EgressPolicyReloadState {
    desired_generation: NonZeroU64,
    latest_attempt_generation: u64,
    phase: EgressPolicyReloadPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum EgressPolicyReloadPhase {
    Stable,
    Applying,
}

impl EgressPolicyReloadState {
    pub(crate) const fn initial() -> Self {
        Self {
            desired_generation: NonZeroU64::MIN,
            latest_attempt_generation: 0,
            phase: EgressPolicyReloadPhase::Stable,
        }
    }

    #[cfg(test)]
    pub(crate) const fn desired_generation(&self) -> NonZeroU64 {
        self.desired_generation
    }

    #[cfg(test)]
    pub(crate) const fn latest_attempt_generation(&self) -> u64 {
        self.latest_attempt_generation
    }

    pub(crate) const fn is_applying(&self) -> bool {
        matches!(self.phase, EgressPolicyReloadPhase::Applying)
    }

    pub(crate) fn active_attempt(&self) -> Result<Option<PolicyReloadAttempt>> {
        if self.latest_attempt_generation == 0 {
            return Ok(None);
        }
        let attempt_generation =
            NonZeroU64::new(self.latest_attempt_generation).ok_or_else(|| {
                SandboxError::OperationFailed {
                    message: "active egress policy reload attempt generation is zero".to_owned(),
                }
            })?;
        Ok(Some(PolicyReloadAttempt::new(
            self.desired_generation,
            attempt_generation,
        )))
    }

    pub(crate) fn begin(&mut self) -> Result<PolicyReloadAttempt> {
        if self.is_applying() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "egress policy desired generation {} still has applying attempt generation {}",
                    self.desired_generation, self.latest_attempt_generation
                ),
            });
        }
        let desired_generation = self
            .desired_generation
            .get()
            .checked_add(1)
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "egress policy desired generation exhausted".to_owned(),
            })?;
        let attempt_generation =
            self.latest_attempt_generation
                .checked_add(1)
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: "egress policy reload attempt generation exhausted".to_owned(),
                })?;
        self.desired_generation = NonZeroU64::new(desired_generation)
            .expect("incrementing a nonzero value stays nonzero");
        self.latest_attempt_generation = attempt_generation;
        self.phase = EgressPolicyReloadPhase::Applying;
        Ok(PolicyReloadAttempt::new(
            self.desired_generation,
            NonZeroU64::new(attempt_generation)
                .expect("the first and every subsequent attempt is nonzero"),
        ))
    }

    pub(crate) fn pending_attempt(&self) -> Result<Option<PolicyReloadAttempt>> {
        if !self.is_applying() {
            return Ok(None);
        }
        let attempt_generation =
            NonZeroU64::new(self.latest_attempt_generation).ok_or_else(|| {
                SandboxError::OperationFailed {
                    message: format!(
                        "egress policy desired generation {} has an invalid zero applying attempt",
                        self.desired_generation
                    ),
                }
            })?;
        Ok(Some(PolicyReloadAttempt::new(
            self.desired_generation,
            attempt_generation,
        )))
    }

    pub(crate) fn complete(&mut self, receipt: PolicyReloadReceipt) -> Result<()> {
        let expected = self
            .pending_attempt()?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "cannot complete an egress policy reload with no applying attempt"
                    .to_owned(),
            })?;
        if receipt.attempt() != expected {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "egress policy reload receipt {:?} does not match durable applying attempt \
                     {expected:?}",
                    receipt.attempt()
                ),
            });
        }
        self.phase = EgressPolicyReloadPhase::Stable;
        Ok(())
    }
}

impl EgressProxyRegistry {
    /// Legacy unfenced hot reload retained for direct PEP tests.
    #[cfg(test)]
    pub(crate) fn reload(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        compiled: CompiledEgressPolicy,
    ) -> Result<()> {
        static TEST_RELOAD_GENERATION: AtomicU64 = AtomicU64::new(2);
        let generation = TEST_RELOAD_GENERATION.fetch_add(1, Ordering::Relaxed);
        let attempt = PolicyReloadAttempt::new(
            NonZeroU64::new(generation).expect("test reload generation must be nonzero"),
            NonZeroU64::new(generation).expect("test reload attempt must be nonzero"),
        );
        let workload_id = Self::workload_id(tenant_id, id)?;
        self.engine
            .with_pep(&workload_id, |pep| {
                pep.reload_policy_for_attempt(compiled, attempt)
            })
            .map_err(egress_proxy_error)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!("egress proxy for sandbox {id} is not running"),
            })?
            .map_err(egress_proxy_error)?;
        Ok(())
    }

    /// Inspect first, apply only when the exact durable attempt is not active,
    /// then re-inspect before returning acknowledgement to the manifest owner.
    #[cfg(test)]
    pub(crate) fn reconcile_reload(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        compiled: CompiledEgressPolicy,
        attempt: PolicyReloadAttempt,
    ) -> Result<PolicyReloadReceipt> {
        if let PolicyReloadObservation::Exact(receipt) =
            self.inspect_reload(tenant_id, id, &compiled, attempt)?
        {
            return Ok(receipt);
        }

        let workload_id = Self::workload_id(tenant_id, id)?;
        self.engine
            .with_pep(&workload_id, |pep| {
                pep.reload_policy_for_attempt(compiled.clone(), attempt)
            })
            .map_err(egress_proxy_error)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "egress proxy for sandbox {id} disappeared while reconciling reload \
                     attempt {attempt:?}"
                ),
            })?
            .map_err(egress_proxy_error)?;

        match self.inspect_reload(tenant_id, id, &compiled, attempt)? {
            PolicyReloadObservation::Exact(receipt) => Ok(receipt),
            observation => Err(SandboxError::OperationFailed {
                message: format!(
                    "egress proxy for sandbox {id} did not expose exact reload attempt \
                     {attempt:?} after acknowledgement; observed {observation:?}"
                ),
            }),
        }
    }

    /// Reconcile an exact durable reload attempt only while the registered PEP
    /// and its complete listener lifecycle attachment remain authenticated.
    pub(crate) fn reconcile_authenticated_reload(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        assignment: Option<&EgressProxyAssignment>,
        compiled: CompiledEgressPolicy,
        attempt: PolicyReloadAttempt,
    ) -> Result<PolicyReloadReceipt> {
        if let PolicyReloadObservation::Exact(receipt) =
            self.inspect_authenticated_reload(tenant_id, id, assignment, &compiled, attempt)?
        {
            return Ok(receipt);
        }

        self.with_authenticated_reload_attachment(tenant_id, id, assignment, &compiled, |pep| {
            pep.reload_policy_for_attempt(compiled.clone(), attempt)
        })?
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "egress proxy for sandbox {id} disappeared while reconciling authenticated \
                 reload attempt {attempt:?}"
            ),
        })?;

        match self.inspect_authenticated_reload(tenant_id, id, assignment, &compiled, attempt)? {
            PolicyReloadObservation::Exact(receipt) => Ok(receipt),
            observation => Err(SandboxError::OperationFailed {
                message: format!(
                    "egress proxy for sandbox {id} did not expose exact authenticated reload \
                     attempt {attempt:?} after acknowledgement; observed {observation:?}"
                ),
            }),
        }
    }

    fn inspect_authenticated_reload(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        assignment: Option<&EgressProxyAssignment>,
        compiled: &CompiledEgressPolicy,
        attempt: PolicyReloadAttempt,
    ) -> Result<PolicyReloadObservation> {
        self.with_authenticated_reload_attachment(tenant_id, id, assignment, compiled, |pep| {
            pep.inspect_policy_reload(compiled, attempt)
        })?
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "egress proxy for sandbox {id} is not running while inspecting authenticated \
                 reload attempt {attempt:?}"
            ),
        })
    }

    #[cfg(test)]
    fn inspect_reload(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        compiled: &CompiledEgressPolicy,
        attempt: PolicyReloadAttempt,
    ) -> Result<PolicyReloadObservation> {
        let workload_id = Self::workload_id(tenant_id, id)?;
        self.engine
            .with_pep(&workload_id, |pep| {
                pep.inspect_policy_reload(compiled, attempt)
            })
            .map_err(egress_proxy_error)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "egress proxy for sandbox {id} is not running while inspecting reload \
                     attempt {attempt:?}"
                ),
            })?
            .map_err(egress_proxy_error)
    }
}
