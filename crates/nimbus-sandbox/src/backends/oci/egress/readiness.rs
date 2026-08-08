//! Exact PEP dependency authentication for launch and observed readiness.

use std::net::SocketAddr;

use nimbus_core::TenantId;
use nimbus_egress::{CompiledEgressPolicy, EgressPolicy};
use nimbus_network::{PortLeaseId, PortLeaseLifetime};
use nimbus_proxy::{
    EgressProxyError, PolicyGeneration, PolicyReloadAttempt, WorkloadPep, WorkloadPepPolicyEvidence,
};

use crate::backends::oci::port_lease::{
    ExpectedListenerAuthority, OciPortProvider, provider_binding, require_active_listener_binding,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::{
    EgressPolicyReloadState, EgressProxyAssignment, EgressProxyRegistry, egress_proxy_error,
};

/// Exact authenticated PEP dependency suitable for portable evidence lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AuthenticatedPepReadiness {
    port_lease_id: PortLeaseId,
    lifetime: PortLeaseLifetime,
    policy_generation: PolicyGeneration,
}

/// Stable fail-closed reason no current PEP evidence can be emitted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EgressReadinessFailure {
    MissingAssignment,
    ReloadApplying,
    MissingRegistration,
    AttachmentMismatch,
    ListenerAddressMismatch,
    ListenerAuthorityRejected(String),
    LifetimeMismatch,
    WorkerStopped,
    AuditUnhealthy,
    MissingPolicy,
    PolicyMismatch,
    ReloadAttemptMismatch {
        expected: Option<PolicyReloadAttempt>,
        observed: Option<PolicyReloadAttempt>,
    },
}

/// Exact ready evidence or one named fail-closed reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EgressReadinessState {
    Ready(AuthenticatedPepReadiness),
    NotReady(EgressReadinessFailure),
}

/// Whether reload reconciliation has one exact live lifecycle attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EgressReloadAttachmentState {
    Authenticated,
    MissingRegistration,
}

impl EgressReadinessState {
    #[cfg(test)]
    pub(crate) const fn is_missing_registration(&self) -> bool {
        matches!(
            self,
            Self::NotReady(EgressReadinessFailure::MissingRegistration)
        )
    }
}

struct LifecycleSnapshot {
    local_addr: SocketAddr,
    attachment_matches: bool,
    lifetime: Option<PortLeaseLifetime>,
    policy: WorkloadPepPolicyEvidence,
}

struct AuthenticatedLifecycleSnapshot {
    port_lease_id: PortLeaseId,
    lifetime: PortLeaseLifetime,
    policy_generation: PolicyGeneration,
    policy: WorkloadPepPolicyEvidence,
}

impl EgressProxyRegistry {
    fn authenticate_registered_lifecycle(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        assignment: &EgressProxyAssignment,
        compiled: &CompiledEgressPolicy,
        pep: &WorkloadPep,
        artifacts: &super::RegisteredArtifacts,
    ) -> Result<std::result::Result<AuthenticatedLifecycleSnapshot, EgressReadinessFailure>> {
        let lifetime = artifacts
            .lifetime
            .as_ref()
            .filter(|lifetime| lifetime.request() == &assignment.port_lease)
            .map(|lifetime| lifetime.lifetime());
        let attachment_matches = artifacts.port_lease.as_ref() == Some(&assignment.port_lease)
            && lifetime.is_some()
            && artifacts.cleanup.is_none();
        let snapshot = LifecycleSnapshot {
            local_addr: pep.local_addr(),
            attachment_matches,
            lifetime,
            policy: pep
                .inspect_policy_evidence(compiled)
                .map_err(egress_proxy_error)?,
        };

        if !snapshot.attachment_matches {
            return Ok(Err(EgressReadinessFailure::AttachmentMismatch));
        }
        let expected_addr = assignment.bind_addr()?;
        if snapshot.local_addr != expected_addr {
            return Ok(Err(EgressReadinessFailure::ListenerAddressMismatch));
        }
        let authority_record = match artifacts.plan_members.as_deref() {
            Some(plan_members) => self
                .port_authority()?
                .inspect_plan_member(plan_members, &assignment.port_lease)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "planned egress PEP lease {} rejected its stored membership witness: {error}",
                        assignment.port_lease.lease_id()
                    ),
                })
                .and_then(|record| {
                    let expected = provider_binding(
                        &assignment.port_lease,
                        expected_addr,
                        OciPortProvider::EgressPep,
                    )?;
                    if record.phase() == nimbus_network::PortLeasePhase::Active
                        && record.binding() == Some(&expected)
                    {
                        Ok(record)
                    } else {
                        Err(SandboxError::OperationFailed {
                            message: format!(
                                "planned egress PEP lease {} lacks its exact Active provider binding",
                                assignment.port_lease.lease_id()
                            ),
                        })
                    }
                }),
            None => require_active_listener_binding(
                self.port_authority()?,
                ExpectedListenerAuthority::egress_pep(tenant_id, id, expected_addr)?,
                &assignment.port_lease,
                expected_addr,
                OciPortProvider::EgressPep,
            ),
        };
        let record = match authority_record {
            Ok(record) => record,
            Err(error) => {
                return Ok(Err(EgressReadinessFailure::ListenerAuthorityRejected(
                    error.to_string(),
                )));
            }
        };
        let lifetime = snapshot
            .lifetime
            .expect("an authenticated attachment snapshot retains its lifetime");
        if record.active_lifetime() != Some(lifetime) {
            return Ok(Err(EgressReadinessFailure::LifetimeMismatch));
        }

        let readiness = snapshot.policy.readiness();
        if !readiness.worker_live() {
            return Ok(Err(EgressReadinessFailure::WorkerStopped));
        }
        if !readiness.audit_healthy() {
            return Ok(Err(EgressReadinessFailure::AuditUnhealthy));
        }
        let Some(policy_generation) = readiness.policy_generation() else {
            return Ok(Err(EgressReadinessFailure::MissingPolicy));
        };
        if !readiness.is_ready() {
            return Ok(Err(EgressReadinessFailure::MissingPolicy));
        }

        Ok(Ok(AuthenticatedLifecycleSnapshot {
            port_lease_id: record.request().lease_id().clone(),
            lifetime,
            policy_generation,
            policy: snapshot.policy,
        }))
    }

    fn authenticated_lifecycle_snapshot(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        assignment: &EgressProxyAssignment,
        compiled: &CompiledEgressPolicy,
    ) -> Result<std::result::Result<AuthenticatedLifecycleSnapshot, EgressReadinessFailure>> {
        let workload_id = Self::workload_id(tenant_id, id)?;
        match self
            .engine
            .with_pep_and_attachment(&workload_id, |pep, artifacts| {
                self.authenticate_registered_lifecycle(
                    tenant_id, id, assignment, compiled, pep, artifacts,
                )
            })
            .map_err(egress_proxy_error)?
        {
            Some(snapshot) => snapshot,
            None => Ok(Err(EgressReadinessFailure::MissingRegistration)),
        }
    }

    /// Run one reload inspection or effect while the exact lifecycle
    /// attachment remains authenticated under the workload-local lock.
    pub(crate) fn with_authenticated_reload_attachment<R>(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        assignment: Option<&EgressProxyAssignment>,
        compiled: &CompiledEgressPolicy,
        operation: impl FnOnce(&WorkloadPep) -> std::result::Result<R, EgressProxyError>,
    ) -> Result<Option<R>> {
        let Some(assignment) = assignment else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "egress reload for sandbox {id} has no durable listener assignment"
                ),
            });
        };
        let workload_id = Self::workload_id(tenant_id, id)?;
        let result = self
            .engine
            .with_pep_and_attachment(&workload_id, |pep, artifacts| {
                match self.authenticate_registered_lifecycle(
                    tenant_id, id, assignment, compiled, pep, artifacts,
                )? {
                    Ok(_) => operation(pep).map_err(egress_proxy_error),
                    Err(reason) => Err(SandboxError::OperationFailed {
                        message: format!(
                            "egress reload for sandbox {id} rejected unauthenticated listener \
                             attachment: {reason:?}"
                        ),
                    }),
                }
            })
            .map_err(egress_proxy_error)?;
        result.transpose()
    }

    /// Authenticate lifecycle authority before a durable policy reload effect.
    ///
    /// Exact current attachment and provider evidence is required. Active
    /// policy bytes and reload-attempt identity are deliberately not required
    /// here because reconciling those stale fields is the effect this seam
    /// authorizes.
    pub(crate) fn authenticated_reload_attachment(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        assignment: Option<&EgressProxyAssignment>,
        compiled: &CompiledEgressPolicy,
    ) -> Result<EgressReloadAttachmentState> {
        match self.with_authenticated_reload_attachment(
            tenant_id,
            id,
            assignment,
            compiled,
            |_| Ok(()),
        )? {
            Some(()) => Ok(EgressReloadAttachmentState::Authenticated),
            None => Ok(EgressReloadAttachmentState::MissingRegistration),
        }
    }

    /// Authenticate the desired assignment, durable listener, retained process
    /// lifetime, live worker, audit health, policy bytes, and reload attempt.
    pub(crate) fn authenticated_readiness(
        &self,
        tenant_id: &TenantId,
        id: &SandboxId,
        assignment: Option<&EgressProxyAssignment>,
        policy: &EgressPolicy,
        reload: Option<&EgressPolicyReloadState>,
    ) -> Result<EgressReadinessState> {
        let Some(assignment) = assignment else {
            return Ok(EgressReadinessState::NotReady(
                EgressReadinessFailure::MissingAssignment,
            ));
        };
        if reload.is_some_and(EgressPolicyReloadState::is_applying) {
            return Ok(EgressReadinessState::NotReady(
                EgressReadinessFailure::ReloadApplying,
            ));
        }
        let expected_attempt = reload
            .map(EgressPolicyReloadState::active_attempt)
            .transpose()?
            .flatten();
        let compiled = policy
            .compile()
            .map_err(|message| SandboxError::InvalidSpec { message })?;
        let snapshot =
            match self.authenticated_lifecycle_snapshot(tenant_id, id, assignment, &compiled)? {
                Ok(snapshot) => snapshot,
                Err(reason) => return Ok(EgressReadinessState::NotReady(reason)),
            };
        if !snapshot.policy.policy_matches() {
            return Ok(EgressReadinessState::NotReady(
                EgressReadinessFailure::PolicyMismatch,
            ));
        }
        if snapshot.policy.reload_attempt() != expected_attempt {
            return Ok(EgressReadinessState::NotReady(
                EgressReadinessFailure::ReloadAttemptMismatch {
                    expected: expected_attempt,
                    observed: snapshot.policy.reload_attempt(),
                },
            ));
        }

        Ok(EgressReadinessState::Ready(AuthenticatedPepReadiness {
            port_lease_id: snapshot.port_lease_id,
            lifetime: snapshot.lifetime,
            policy_generation: snapshot.policy_generation,
        }))
    }
}
