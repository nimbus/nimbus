//! Port-lease transition names and operation-local diagnostics.

use std::fmt::{self, Display, Formatter};
use std::num::NonZeroU16;

use nimbus_core::TenantId;

use super::{PortLeasePhase, PortLeaseRequest};
use crate::{NetworkResourceId, PortBindingMismatch, PortLeaseId, PortRange};

/// Expected and rejected immutable requests carried by a stale-fence error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortLeaseFenceMismatch {
    pub(super) expected: PortLeaseRequest,
    pub(super) candidate: PortLeaseRequest,
}

impl PortLeaseFenceMismatch {
    /// Current durable lease identity and fence.
    pub fn expected(&self) -> &PortLeaseRequest {
        &self.expected
    }

    /// Rejected stale or divergent lease identity and fence.
    pub fn candidate(&self) -> &PortLeaseRequest {
        &self.candidate
    }
}

/// Named operation used by invalid-transition diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortLeaseOperation {
    /// Verify a complete never-bound batch before any provider effect.
    VerifyReservationClaim,
    /// Release a batch that has never reached provider adoption.
    ReleaseReservedWithoutEffect,
    /// Claim exclusive ownership of a Nimbus provider bind attempt.
    ClaimBind,
    /// Relinquish a claimed attempt after proving no provider effect.
    AbandonBindClaimWithoutEffect,
    /// Record a concrete provider binding.
    Adopt,
    /// Record a confirmed no-effect provider bind failure.
    RecordBindFailureWithoutEffect,
    /// Mark an adopted binding active.
    Activate,
    /// Begin one process-lifetime generation before a provider effect.
    BeginLifetime,
    /// Retain an ambiguous effect under an exclusive recovery generation.
    MarkCleanupPending,
    /// Transfer an exact surviving provider-managed binding to a new owner.
    ReclaimProviderManagedBinding,
    /// Release a process-bound effect after its exact owner lifetime died.
    ReleaseAfterOwnerDeath,
    /// Retain a process-bound slot for rebind after its exact owner died.
    PrepareRebindAfterOwnerDeath,
    /// Clear an exact stopped binding while retaining its numeric fence.
    PrepareRebindAfterConfirmedStop,
    /// Fence new use.
    Withdraw,
    /// Confirm terminal release.
    Release,
    /// Release an exact restart-retained reservation after confirmed stop.
    ReleaseAfterConfirmedStop,
}

impl Display for PortLeaseOperation {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::VerifyReservationClaim => "verify never-bound reservation coordinator",
            Self::ReleaseReservedWithoutEffect => "release reserved batch without provider effect",
            Self::ClaimBind => "claim provider bind attempt",
            Self::AbandonBindClaimWithoutEffect => "abandon bind claim without provider effect",
            Self::Adopt => "adopt",
            Self::RecordBindFailureWithoutEffect => "record bind failure without provider effect",
            Self::Activate => "activate",
            Self::BeginLifetime => "begin process-lifetime generation",
            Self::MarkCleanupPending => "mark cleanup pending",
            Self::ReclaimProviderManagedBinding => "reclaim surviving provider-managed binding",
            Self::ReleaseAfterOwnerDeath => "release after process-owner death",
            Self::PrepareRebindAfterOwnerDeath => "prepare rebind after process-owner death",
            Self::PrepareRebindAfterConfirmedStop => "prepare rebind after confirmed provider stop",
            Self::Withdraw => "withdraw",
            Self::Release => "release",
            Self::ReleaseAfterConfirmedStop => "release after confirmed provider stop",
        })
    }
}

#[derive(Debug)]
pub(super) enum PortLeaseOperationError {
    CorruptAuthority {
        reason: String,
    },
    NotFound {
        lease_id: PortLeaseId,
    },
    IdentityConflict {
        lease_id: PortLeaseId,
    },
    TenantAttributionRequired {
        lease_id: PortLeaseId,
    },
    InvalidPublicationAccounting {
        lease_id: PortLeaseId,
    },
    TenantLimitScopeMismatch {
        expected_tenant_id: TenantId,
        request_lease_id: PortLeaseId,
        actual_tenant_id: TenantId,
    },
    TenantPublishedPortLimitExceeded {
        tenant_id: TenantId,
        current_live: usize,
        additional: usize,
        maximum: usize,
    },
    PortConflict {
        conflicting_port: NonZeroU16,
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        existing_lease_id: PortLeaseId,
        existing_owner_id: NetworkResourceId,
        existing_phase: PortLeasePhase,
    },
    PortRangeExhausted {
        requested_lease_id: PortLeaseId,
        requested_owner_id: NetworkResourceId,
        requested_range: PortRange,
    },
    StaleFence(Box<PortLeaseFenceMismatch>),
    BindingMismatch {
        lease_id: PortLeaseId,
        mismatch: PortBindingMismatch,
    },
    BindingConflict {
        lease_id: PortLeaseId,
    },
    BindFailureConflict {
        lease_id: PortLeaseId,
    },
    BindClaimConflict {
        lease_id: PortLeaseId,
    },
    ReservationClaimConflict {
        lease_id: PortLeaseId,
    },
    LifetimeConflict {
        lease_id: PortLeaseId,
    },
    LifetimeGenerationExhausted {
        lease_id: PortLeaseId,
    },
    LifetimeMismatch {
        lease_id: PortLeaseId,
    },
    LifetimeScopeMismatch {
        lease_id: PortLeaseId,
    },
    InvalidTransition {
        lease_id: PortLeaseId,
        phase: PortLeasePhase,
        operation: PortLeaseOperation,
    },
}
