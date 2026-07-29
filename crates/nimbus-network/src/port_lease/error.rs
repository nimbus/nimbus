//! Public port-lease diagnostics and internal transition-error mapping.

use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};
use std::num::NonZeroU16;

use nimbus_core::TenantId;

use super::{
    PortBindingMismatch, PortLeaseFenceMismatch, PortLeaseOperation, PortLeaseOperationError,
    PortLeasePhase, PortRange,
};
use crate::{
    NetworkPlanId, NetworkProviderId, NetworkResourceId, NetworkStateStoreError,
    NetworkStateTransactionError, PortLeaseId,
};

/// Durable authority or lifecycle rejection.
#[derive(Debug)]
pub enum PortLeaseError {
    /// The shared store could not be safely read or committed.
    Store(NetworkStateStoreError),
    /// The checksum-valid durable authority violates lease invariants.
    CorruptAuthority { reason: String },
    /// No durable lease has this stable identity.
    NotFound { lease_id: PortLeaseId },
    /// One lease ID was reused with different immutable reservation identity.
    IdentityConflict { lease_id: PortLeaseId },
    /// A provider-managed publication batch member omitted durable plan identity.
    PlanRequired { lease_id: PortLeaseId },
    /// A caller did not present the complete immutable member set of one plan.
    PlanMembershipConflict { plan_id: NetworkPlanId },
    /// A tenant-published request omitted tenant attribution.
    TenantAttributionRequired { lease_id: PortLeaseId },
    /// Publication intent does not match the request's accounting class.
    InvalidPublicationAccounting { lease_id: PortLeaseId },
    /// A metered request fell outside the caller-supplied tenant decision.
    TenantLimitScopeMismatch {
        expected_tenant_id: TenantId,
        request_lease_id: PortLeaseId,
        actual_tenant_id: TenantId,
    },
    /// New published requests would exceed the atomic tenant limit.
    TenantPublishedPortLimitExceeded {
        tenant_id: TenantId,
        current_live: usize,
        additional: usize,
        maximum: usize,
    },
    /// Another non-terminal lease already fences the requested host port.
    PortConflict {
        conflicting_port: NonZeroU16,
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        existing_lease_id: PortLeaseId,
        existing_owner_id: NetworkResourceId,
        existing_phase: PortLeasePhase,
    },
    /// Every port in an overlapping requested range is fenced.
    PortRangeExhausted {
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        requested_range: PortRange,
    },
    /// A generation/epoch/owner request did not match durable authority.
    StaleFence(Box<PortLeaseFenceMismatch>),
    /// Concrete provider evidence does not satisfy the durable request.
    BindingMismatch {
        lease_id: PortLeaseId,
        mismatch: PortBindingMismatch,
    },
    /// A second adoption supplied different provider evidence.
    BindingConflict { lease_id: PortLeaseId },
    /// A second failed-bind report supplied different provider evidence.
    BindFailureConflict { lease_id: PortLeaseId },
    /// Another attempt owns the durable pre-bind claim.
    BindClaimConflict { lease_id: PortLeaseId },
    /// Another coordinator owns never-bound compensation authority.
    ReservationClaimConflict { lease_id: PortLeaseId },
    /// A live process still owns one exact launch-reservation lifetime.
    ReservationLifetimeOwnerLive { provider_id: NetworkProviderId },
    /// A live process still owns the exact lease-lifetime lock.
    LifetimeOwnerLive { lease_id: PortLeaseId },
    /// A different lifetime generation already owns recovery authority.
    LifetimeConflict { lease_id: PortLeaseId },
    /// The monotonic owner-lifetime generation cannot advance safely.
    LifetimeGenerationExhausted { lease_id: PortLeaseId },
    /// A lifetime guard does not match the durable lease generation.
    LifetimeMismatch { lease_id: PortLeaseId },
    /// Process-death release was requested for a provider-managed effect.
    LifetimeScopeMismatch { lease_id: PortLeaseId },
    /// The requested operation is not legal from the durable phase.
    InvalidTransition {
        lease_id: PortLeaseId,
        phase: PortLeasePhase,
        operation: PortLeaseOperation,
    },
}

impl PortLeaseError {
    pub(super) fn from_transaction(
        error: NetworkStateTransactionError<PortLeaseOperationError>,
    ) -> Self {
        match error {
            NetworkStateTransactionError::Store(error) => Self::Store(error),
            NetworkStateTransactionError::Operation(error) => error.into(),
        }
    }
}

impl From<PortLeaseOperationError> for PortLeaseError {
    fn from(error: PortLeaseOperationError) -> Self {
        match error {
            PortLeaseOperationError::CorruptAuthority { reason } => {
                Self::CorruptAuthority { reason }
            }
            PortLeaseOperationError::NotFound { lease_id } => Self::NotFound { lease_id },
            PortLeaseOperationError::IdentityConflict { lease_id } => {
                Self::IdentityConflict { lease_id }
            }
            PortLeaseOperationError::PlanRequired { lease_id } => Self::PlanRequired { lease_id },
            PortLeaseOperationError::PlanMembershipConflict { plan_id } => {
                Self::PlanMembershipConflict { plan_id }
            }
            PortLeaseOperationError::TenantAttributionRequired { lease_id } => {
                Self::TenantAttributionRequired { lease_id }
            }
            PortLeaseOperationError::InvalidPublicationAccounting { lease_id } => {
                Self::InvalidPublicationAccounting { lease_id }
            }
            PortLeaseOperationError::TenantLimitScopeMismatch {
                expected_tenant_id,
                request_lease_id,
                actual_tenant_id,
            } => Self::TenantLimitScopeMismatch {
                expected_tenant_id,
                request_lease_id,
                actual_tenant_id,
            },
            PortLeaseOperationError::TenantPublishedPortLimitExceeded {
                tenant_id,
                current_live,
                additional,
                maximum,
            } => Self::TenantPublishedPortLimitExceeded {
                tenant_id,
                current_live,
                additional,
                maximum,
            },
            PortLeaseOperationError::PortConflict {
                conflicting_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase,
            } => Self::PortConflict {
                conflicting_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase,
            },
            PortLeaseOperationError::PortRangeExhausted {
                requested_lease_id,
                requested_owner_id,
                requested_range,
            } => Self::PortRangeExhausted {
                requested_lease_id,
                requested_owner_id,
                requested_range,
            },
            PortLeaseOperationError::StaleFence(mismatch) => Self::StaleFence(mismatch),
            PortLeaseOperationError::BindingMismatch { lease_id, mismatch } => {
                Self::BindingMismatch { lease_id, mismatch }
            }
            PortLeaseOperationError::BindingConflict { lease_id } => {
                Self::BindingConflict { lease_id }
            }
            PortLeaseOperationError::BindFailureConflict { lease_id } => {
                Self::BindFailureConflict { lease_id }
            }
            PortLeaseOperationError::BindClaimConflict { lease_id } => {
                Self::BindClaimConflict { lease_id }
            }
            PortLeaseOperationError::ReservationClaimConflict { lease_id } => {
                Self::ReservationClaimConflict { lease_id }
            }
            PortLeaseOperationError::LifetimeConflict { lease_id } => {
                Self::LifetimeConflict { lease_id }
            }
            PortLeaseOperationError::LifetimeGenerationExhausted { lease_id } => {
                Self::LifetimeGenerationExhausted { lease_id }
            }
            PortLeaseOperationError::LifetimeMismatch { lease_id } => {
                Self::LifetimeMismatch { lease_id }
            }
            PortLeaseOperationError::LifetimeScopeMismatch { lease_id } => {
                Self::LifetimeScopeMismatch { lease_id }
            }
            PortLeaseOperationError::InvalidTransition {
                lease_id,
                phase,
                operation,
            } => Self::InvalidTransition {
                lease_id,
                phase,
                operation,
            },
        }
    }
}

impl Display for PortLeaseError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Store(error) => Display::fmt(error, formatter),
            Self::CorruptAuthority { reason } => {
                write!(formatter, "port lease authority is corrupt: {reason}")
            }
            Self::NotFound { lease_id } => {
                write!(formatter, "port lease {lease_id} does not exist")
            }
            Self::IdentityConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} was reused with different immutable reservation identity"
            ),
            Self::PlanRequired { lease_id } => write!(
                formatter,
                "provider-managed port lease {lease_id} requires durable network plan identity"
            ),
            Self::PlanMembershipConflict { plan_id } => write!(
                formatter,
                "network plan {plan_id} requires its complete immutable durable member set"
            ),
            Self::TenantAttributionRequired { lease_id } => write!(
                formatter,
                "tenant-published port lease {lease_id} requires tenant attribution"
            ),
            Self::InvalidPublicationAccounting { lease_id } => write!(
                formatter,
                "port lease {lease_id} publication intent does not match its accounting class"
            ),
            Self::TenantLimitScopeMismatch {
                expected_tenant_id,
                request_lease_id,
                actual_tenant_id,
            } => write!(
                formatter,
                "tenant-published port lease {request_lease_id} belongs to tenant \
                 {actual_tenant_id}, outside the supplied limit for tenant {expected_tenant_id}"
            ),
            Self::TenantPublishedPortLimitExceeded {
                tenant_id,
                current_live,
                additional,
                maximum,
            } => write!(
                formatter,
                "published port quota exceeded for tenant {tenant_id}: {current_live} live plus \
                 {additional} new leases exceeds limit {maximum}"
            ),
            Self::PortConflict {
                conflicting_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase,
            } => write!(
                formatter,
                "port {} requested by lease {} owner {:?} conflicts with lease {} owner {:?} in \
                 phase {:?}",
                conflicting_port,
                requested_lease_id,
                requested_owner_id,
                existing_lease_id,
                existing_owner_id,
                existing_phase
            ),
            Self::PortRangeExhausted {
                requested_lease_id,
                requested_owner_id,
                requested_range,
            } => write!(
                formatter,
                "port range {}..={} requested by lease {} owner {:?} has no free slot in its \
                 overlap domain",
                requested_range.start(),
                requested_range.end(),
                requested_lease_id,
                requested_owner_id
            ),
            Self::StaleFence(mismatch) => write!(
                formatter,
                "port lease {} rejected stale or divergent fence: expected owner {:?} plan {:?} \
                 tenant {:?} accounting {:?} publication {:?} binding {:?} generation {} epoch {}, \
                 candidate owner {:?} plan {:?} tenant {:?} accounting {:?} publication {:?} \
                 binding {:?} generation {} epoch {}",
                mismatch.expected.lease_id,
                mismatch.expected.owner_id,
                mismatch.expected.plan_id,
                mismatch.expected.tenant_id,
                mismatch.expected.accounting,
                mismatch.expected.publication,
                mismatch.expected.binding,
                mismatch.expected.generation.as_u64(),
                mismatch.expected.lease_epoch.as_u64(),
                mismatch.candidate.owner_id,
                mismatch.candidate.plan_id,
                mismatch.candidate.tenant_id,
                mismatch.candidate.accounting,
                mismatch.candidate.publication,
                mismatch.candidate.binding,
                mismatch.candidate.generation.as_u64(),
                mismatch.candidate.lease_epoch.as_u64()
            ),
            Self::BindingMismatch { lease_id, mismatch } => {
                write!(
                    formatter,
                    "port lease {lease_id} rejected provider evidence: {mismatch}"
                )
            }
            Self::BindingConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} already has different adopted provider evidence"
            ),
            Self::BindFailureConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} already has different failed-bind evidence"
            ),
            Self::BindClaimConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} is owned by a different in-flight provider bind attempt"
            ),
            Self::ReservationClaimConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} is owned by a different launch reservation coordinator"
            ),
            Self::ReservationLifetimeOwnerLive { provider_id } => write!(
                formatter,
                "launch reservation for provider {provider_id} still has a live process owner"
            ),
            Self::LifetimeOwnerLive { lease_id } => write!(
                formatter,
                "port lease {lease_id} still has a live process-lifetime owner"
            ),
            Self::LifetimeConflict { lease_id } => write!(
                formatter,
                "port lease {lease_id} is owned by a different process-lifetime generation"
            ),
            Self::LifetimeGenerationExhausted { lease_id } => write!(
                formatter,
                "port lease {lease_id} exhausted its monotonic process-lifetime generation"
            ),
            Self::LifetimeMismatch { lease_id } => write!(
                formatter,
                "port lease {lease_id} rejected stale or substituted process-lifetime evidence"
            ),
            Self::LifetimeScopeMismatch { lease_id } => write!(
                formatter,
                "port lease {lease_id} requires exact provider absence; process death alone is \
                 insufficient"
            ),
            Self::InvalidTransition {
                lease_id,
                phase,
                operation,
            } => write!(
                formatter,
                "port lease {lease_id} cannot {operation} from phase {phase:?}"
            ),
        }
    }
}

impl StdError for PortLeaseError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Store(error) => Some(error),
            Self::BindingMismatch { mismatch, .. } => Some(mismatch),
            _ => None,
        }
    }
}
