//! Process-lifetime fencing for crash-safe port-effect reconciliation.
//!
//! The lock proves only whether the Nimbus process generation that owned an
//! effect is still live. Provider-specific inspection remains in the adapter
//! that owns the socket, subprocess, or external service.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use super::plan_batch::{
    authenticate_complete_plan_batch_if_present, authenticate_complete_plan_member,
    authenticate_complete_plan_members, authenticate_scalar_plan_if_present,
};
use super::{
    LocalPortLeaseAuthority, PortBindClaim, PortLeaseBinding, PortLeaseError, PortLeaseOperation,
    PortLeaseOperationError, PortLeasePhase, PortLeaseRecord, PortLeaseRequest, exact_record,
    exact_record_mut, require_reservation_claim,
};
use crate::state_store::{create_dir_all_owner_only, is_lock_contended, open_owner_file};
use crate::{NetworkReservationClaim, PortLeaseId};

mod batch_reservation;
pub use batch_reservation::PortLeaseBatchReservationWithLifetimes;

const LIFETIME_LOCK_DIRECTORY: &str = "port-lease-lifetimes";

/// Monotonic process-owner generation within one stable port lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PortLeaseLifetimeGeneration(u64);

impl PortLeaseLifetimeGeneration {
    /// Return the monotonic generation value.
    pub const fn as_u64(self) -> u64 {
        self.0
    }

    pub(super) const fn from_stored(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }
}

/// Whether process death is sufficient evidence that the effect is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortLeaseEffectScope {
    /// Every descriptor/effect handle is owned by the same process lifetime.
    ProcessBound,
    /// The effect may survive the coordinator and requires provider inspection.
    ProviderManaged,
}

/// Durable process-lifetime generation attached to one lease effect attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortLeaseLifetime {
    generation: PortLeaseLifetimeGeneration,
    effect_scope: PortLeaseEffectScope,
}

impl PortLeaseLifetime {
    /// Monotonic owner generation.
    pub const fn generation(self) -> PortLeaseLifetimeGeneration {
        self.generation
    }

    /// Evidence required after this process generation dies.
    pub const fn effect_scope(self) -> PortLeaseEffectScope {
        self.effect_scope
    }
}

/// Non-cloneable proof that the current process owns one live effect attempt.
pub struct PortLeaseLifetimeGuard {
    request: PortLeaseRequest,
    lifetime: PortLeaseLifetime,
    _lock: LifetimeFileGuard,
}

/// One atomic reservation plus its first live provider-attempt lifetime.
///
/// Direct listener adapters use this result when no higher-level launch
/// coordinator already owns a reservation lifetime. The durable reservation,
/// bind claim, and process lifetime commit in one store transaction.
pub struct PortLeaseReservationWithLifetime {
    record: PortLeaseRecord,
    lifetime: PortLeaseLifetimeGuard,
}

impl PortLeaseReservationWithLifetime {
    /// Durable reservation and selected port.
    pub fn record(&self) -> &PortLeaseRecord {
        &self.record
    }

    /// Split the durable result from its non-cloneable live-owner guard.
    pub fn into_parts(self) -> (PortLeaseRecord, PortLeaseLifetimeGuard) {
        (self.record, self.lifetime)
    }
}

impl fmt::Debug for PortLeaseReservationWithLifetime {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortLeaseReservationWithLifetime")
            .field("record", &self.record)
            .field("lifetime", &self.lifetime)
            .finish()
    }
}

impl PortLeaseLifetimeGuard {
    /// Exact durable lifetime generation held by this process.
    pub const fn lifetime(&self) -> PortLeaseLifetime {
        self.lifetime
    }

    /// Exact immutable lease request fenced by this guard.
    pub fn request(&self) -> &PortLeaseRequest {
        &self.request
    }
}

impl fmt::Debug for PortLeaseLifetimeGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortLeaseLifetimeGuard")
            .field("lease_id", self.request.lease_id())
            .field("lifetime", &self.lifetime)
            .finish_non_exhaustive()
    }
}

/// Exclusive proof that the prior process owner has released its lifetime lock.
pub struct PortLeaseRecoveryGuard {
    request: PortLeaseRequest,
    lifetime: PortLeaseLifetime,
    _lock: LifetimeFileGuard,
}

impl PortLeaseRecoveryGuard {
    /// Dead process-owner generation being reconciled.
    pub const fn lifetime(&self) -> PortLeaseLifetime {
        self.lifetime
    }

    /// Exact immutable lease request fenced by this recovery.
    pub fn request(&self) -> &PortLeaseRequest {
        &self.request
    }
}

impl fmt::Debug for PortLeaseRecoveryGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortLeaseRecoveryGuard")
            .field("lease_id", self.request.lease_id())
            .field("lifetime", &self.lifetime)
            .finish_non_exhaustive()
    }
}

/// Result of nonblocking owner-death inspection.
#[derive(Debug)]
pub enum PortLeaseRecoveryAttempt {
    /// The exact lifetime lock is still owned by a live process.
    LiveOwner(PortLeaseRecord),
    /// The caller exclusively owns reconciliation for the dead generation.
    Acquired(PortLeaseRecoveryGuard),
    /// The exact lease already reached a terminal state.
    Settled(PortLeaseRecord),
}

/// Deterministic result of one explicit process-bound lease reconciliation.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PortLeaseLifetimeReconciliation {
    released: Vec<PortLeaseId>,
    live: Vec<PortLeaseId>,
    provider_managed: Vec<PortLeaseId>,
    missing_lifetime: Vec<PortLeaseId>,
}

impl PortLeaseLifetimeReconciliation {
    /// Dead process-bound leases released by this reconciliation.
    pub fn released(&self) -> &[PortLeaseId] {
        &self.released
    }

    /// Process-bound leases whose lifetime owner is still live.
    pub fn live(&self) -> &[PortLeaseId] {
        &self.live
    }

    /// Provider-managed leases intentionally left for their effect adapter.
    pub fn provider_managed(&self) -> &[PortLeaseId] {
        &self.provider_managed
    }

    /// Nonterminal records that predate or bypass the lifetime contract.
    pub fn missing_lifetime(&self) -> &[PortLeaseId] {
        &self.missing_lifetime
    }
}

#[derive(Debug)]
struct LifetimeFileGuard {
    _file: File,
}

enum LifetimeLockAttempt {
    Acquired(LifetimeFileGuard),
    Contended,
}

fn exact_plan_recoveries<'a>(
    requests: &[PortLeaseRequest],
    recoveries: &'a [PortLeaseRecoveryGuard],
) -> Result<BTreeMap<PortLeaseId, &'a PortLeaseRecoveryGuard>, PortLeaseError> {
    let Some(first_request) = requests.first() else {
        return Err(PortLeaseError::CorruptAuthority {
            reason: "planned recovery transition requires at least one member".to_owned(),
        });
    };
    if requests.len() != recoveries.len() {
        return Err(PortLeaseError::LifetimeMismatch {
            lease_id: first_request.lease_id().clone(),
        });
    }
    let mut distinct_requests = BTreeMap::new();
    for request in requests {
        if distinct_requests
            .insert(request.lease_id().clone(), request)
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: request.lease_id().clone(),
            });
        }
    }
    let mut by_id = BTreeMap::new();
    for recovery in recoveries {
        if by_id
            .insert(recovery.request().lease_id().clone(), recovery)
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: recovery.request().lease_id().clone(),
            });
        }
    }
    for request in requests {
        if by_id
            .get(request.lease_id())
            .is_none_or(|recovery| recovery.request() != request)
        {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
    }
    Ok(by_id)
}

impl LocalPortLeaseAuthority {
    /// Inspect whether an exact planned subset has never reached a provider effect.
    ///
    /// The complete plan witness and reservation claim authenticate the desired
    /// generation. A `false` result preserves ambiguity for any prior claim,
    /// lifetime, adoption, binding, stop receipt, or failure evidence.
    pub fn inspect_plan_members_never_effected(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<bool, PortLeaseError> {
        let witness = plan_members.iter().collect::<Vec<_>>();
        let requested = requests.iter().collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &requested)?;
            let mut distinct = BTreeMap::new();
            for request in requests {
                if let Some(previous) = distinct.insert(request.lease_id().clone(), request)
                    && previous != request
                {
                    return Err(PortLeaseOperationError::IdentityConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                if record.reservation_claim.as_ref() != Some(reservation_claim) {
                    return Err(PortLeaseOperationError::ReservationClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if record.phase != PortLeasePhase::Reserved
                    || record.bind_claim.is_some()
                    || record.adoption_claim.is_some()
                    || record.binding.is_some()
                    || record.confirmed_stopped_binding.is_some()
                    || record.failure.is_some()
                    || record.last_lifetime_generation != 0
                    || record.active_lifetime.is_some()
                {
                    return Ok(false);
                }
            }
            Ok(true)
        })
    }

    /// Release one live-owner effect after the adapter confirms exact absence.
    ///
    /// The non-cloneable guard authenticates the process generation whose
    /// socket or provider effect was stopped. Portable request identity alone
    /// is deliberately insufficient to clear an active lifetime.
    pub fn release_with_lifetime(
        &self,
        request: &PortLeaseRequest,
        lifetime: &PortLeaseLifetimeGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        if lifetime.request != *request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        self.transaction(|state| {
            authenticate_scalar_plan_if_present(state, request)?;
            let record = exact_record_mut(state, request)?;
            match record.phase {
                PortLeasePhase::Withdrawing
                    if record.active_lifetime == Some(lifetime.lifetime) =>
                {
                    record.phase = PortLeasePhase::Released;
                    record.reservation_claim = None;
                    record.confirmed_stopped_binding = None;
                    record.active_lifetime = None;
                }
                PortLeasePhase::Released
                    if record.active_lifetime.is_none()
                        && record.last_lifetime_generation
                            == lifetime.lifetime.generation.as_u64() => {}
                PortLeasePhase::Withdrawing | PortLeasePhase::Released => {
                    return Err(PortLeaseOperationError::LifetimeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase,
                        operation: PortLeaseOperation::Release,
                    });
                }
            }
            Ok(record.clone())
        })
    }

    /// Atomically reserve one direct listener and claim its first lifetime.
    ///
    /// The lifetime lock is held before the durable transaction. A crash can
    /// therefore leave either no new reservation or a reservation carrying
    /// both its exact bind claim and recoverable lifetime generation; it
    /// cannot strand an unowned `Reserved` record between two commits.
    pub fn reserve_and_claim_bind_with_lifetime(
        &self,
        request: PortLeaseRequest,
        claim: PortBindClaim,
        effect_scope: PortLeaseEffectScope,
    ) -> Result<PortLeaseReservationWithLifetime, PortLeaseError> {
        self.transaction(|state| authenticate_scalar_plan_if_present(state, &request))?;
        let lock = match self.try_acquire_lifetime_lock(request.lease_id())? {
            LifetimeLockAttempt::Acquired(lock) => lock,
            LifetimeLockAttempt::Contended => {
                return Err(PortLeaseError::LifetimeOwnerLive {
                    lease_id: request.lease_id().clone(),
                });
            }
        };
        let (record, lifetime) = self.transaction(|state| {
            authenticate_scalar_plan_if_present(state, &request)?;
            state.reserve_request(request.clone(), None)?;
            let record = exact_record_mut(state, &request)?;
            require_reservation_claim(record, None)?;
            if record.phase != PortLeasePhase::Reserved {
                return Err(PortLeaseOperationError::InvalidTransition {
                    lease_id: request.lease_id().clone(),
                    phase: record.phase,
                    operation: PortLeaseOperation::BeginLifetime,
                });
            }
            if record
                .bind_claim
                .as_ref()
                .is_some_and(|current| current != &claim)
            {
                return Err(PortLeaseOperationError::BindClaimConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            if record.active_lifetime.is_some() {
                return Err(PortLeaseOperationError::LifetimeConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            let lifetime = advance_lifetime(record, &request, effect_scope)?;
            record.bind_claim = Some(claim);
            Ok((record.clone(), lifetime))
        })?;
        Ok(PortLeaseReservationWithLifetime {
            record,
            lifetime: PortLeaseLifetimeGuard {
                request,
                lifetime,
                _lock: lock,
            },
        })
    }

    /// Atomically claim a provider attempt and its process-lifetime generation.
    ///
    /// The OS lifetime lock is held before the one durable transaction. A
    /// crash therefore leaves either the untouched reservation or a claimed,
    /// lifetime-fenced attempt that explicit recovery can classify; there is
    /// no claimed-but-unrecoverable midpoint.
    pub fn claim_bind_with_lifetime(
        &self,
        request: &PortLeaseRequest,
        reservation_claim: Option<&NetworkReservationClaim>,
        claim: PortBindClaim,
        effect_scope: PortLeaseEffectScope,
    ) -> Result<PortLeaseLifetimeGuard, PortLeaseError> {
        self.transaction(|state| authenticate_scalar_plan_if_present(state, request))?;
        let lock = match self.try_acquire_lifetime_lock(request.lease_id())? {
            LifetimeLockAttempt::Acquired(lock) => lock,
            LifetimeLockAttempt::Contended => {
                return Err(PortLeaseError::LifetimeOwnerLive {
                    lease_id: request.lease_id().clone(),
                });
            }
        };
        let lifetime = self.transaction(|state| {
            authenticate_scalar_plan_if_present(state, request)?;
            let record = exact_record_mut(state, request)?;
            require_reservation_claim(record, reservation_claim)?;
            if record.phase != PortLeasePhase::Reserved {
                return Err(PortLeaseOperationError::InvalidTransition {
                    lease_id: request.lease_id().clone(),
                    phase: record.phase,
                    operation: PortLeaseOperation::BeginLifetime,
                });
            }
            if record
                .bind_claim
                .as_ref()
                .is_some_and(|current| current != &claim)
            {
                return Err(PortLeaseOperationError::BindClaimConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            if record.active_lifetime.is_some() {
                return Err(PortLeaseOperationError::LifetimeConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            let lifetime = advance_lifetime(record, request, effect_scope)?;
            record.bind_claim = Some(claim);
            Ok(lifetime)
        })?;
        Ok(PortLeaseLifetimeGuard {
            request: request.clone(),
            lifetime,
            _lock: lock,
        })
    }

    /// Claim one planned member while authenticating its complete immutable
    /// plan witness in the same durable transaction.
    ///
    /// Unrelated members remain untouched so independent effect providers can
    /// realize separate listener phases without weakening plan membership.
    pub fn claim_bind_plan_member_with_lifetime(
        &self,
        plan_members: &[PortLeaseRequest],
        request: &PortLeaseRequest,
        reservation_claim: &NetworkReservationClaim,
        claim: PortBindClaim,
        effect_scope: PortLeaseEffectScope,
    ) -> Result<PortLeaseLifetimeGuard, PortLeaseError> {
        let witness = plan_members.iter().collect::<Vec<_>>();
        self.transaction(|state| authenticate_complete_plan_member(state, &witness, request))?;
        let lock = match self.try_acquire_lifetime_lock(request.lease_id())? {
            LifetimeLockAttempt::Acquired(lock) => lock,
            LifetimeLockAttempt::Contended => {
                return Err(PortLeaseError::LifetimeOwnerLive {
                    lease_id: request.lease_id().clone(),
                });
            }
        };
        let lifetime = self.transaction(|state| {
            authenticate_complete_plan_member(state, &witness, request)?;
            let record = exact_record_mut(state, request)?;
            require_reservation_claim(record, Some(reservation_claim))?;
            if record.phase != PortLeasePhase::Reserved {
                return Err(PortLeaseOperationError::InvalidTransition {
                    lease_id: request.lease_id().clone(),
                    phase: record.phase,
                    operation: PortLeaseOperation::BeginLifetime,
                });
            }
            if record
                .bind_claim
                .as_ref()
                .is_some_and(|current| current != &claim)
            {
                return Err(PortLeaseOperationError::BindClaimConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            if record.active_lifetime.is_some() {
                return Err(PortLeaseOperationError::LifetimeConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            let lifetime = advance_lifetime(record, request, effect_scope)?;
            record.bind_claim = Some(claim);
            Ok(lifetime)
        })?;
        Ok(PortLeaseLifetimeGuard {
            request: request.clone(),
            lifetime,
            _lock: lock,
        })
    }

    /// Claim an exact retained plan member for one higher rebind lifetime.
    ///
    /// The confirmed-stop binding is the durable receipt for the prior effect.
    /// A dead claim-only retry advances again under the same exclusive lock;
    /// it never reuses the former process generation.
    pub fn claim_rebind_plan_member_with_lifetime(
        &self,
        plan_members: &[PortLeaseRequest],
        request: &PortLeaseRequest,
        confirmed_stopped_binding: &PortLeaseBinding,
        claim: PortBindClaim,
        effect_scope: PortLeaseEffectScope,
    ) -> Result<PortLeaseLifetimeGuard, PortLeaseError> {
        let witness = plan_members.iter().collect::<Vec<_>>();
        self.transaction(|state| authenticate_complete_plan_member(state, &witness, request))?;
        let lock = match self.try_acquire_lifetime_lock(request.lease_id())? {
            LifetimeLockAttempt::Acquired(lock) => lock,
            LifetimeLockAttempt::Contended => {
                return Err(PortLeaseError::LifetimeOwnerLive {
                    lease_id: request.lease_id().clone(),
                });
            }
        };
        let lifetime = self.transaction(|state| {
            authenticate_complete_plan_member(state, &witness, request)?;
            let record = exact_record_mut(state, request)?;
            if record.phase != PortLeasePhase::Reserved
                || record.reservation_claim.is_some()
                || record.binding.is_some()
                || record.adoption_claim.is_some()
                || record.confirmed_stopped_binding.as_ref() != Some(confirmed_stopped_binding)
                || record.failure.is_some()
            {
                return Err(PortLeaseOperationError::InvalidTransition {
                    lease_id: request.lease_id().clone(),
                    phase: record.phase,
                    operation: PortLeaseOperation::BeginLifetime,
                });
            }
            if let Some(active) = record.active_lifetime {
                if active.effect_scope != effect_scope {
                    return Err(PortLeaseOperationError::LifetimeConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if record.bind_claim.as_ref() != Some(&claim) {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
            } else if record.bind_claim.is_some() {
                return Err(PortLeaseOperationError::BindClaimConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            let lifetime = advance_lifetime(record, request, effect_scope)?;
            record.bind_claim = Some(claim);
            Ok(lifetime)
        })?;
        Ok(PortLeaseLifetimeGuard {
            request: request.clone(),
            lifetime,
            _lock: lock,
        })
    }

    /// Activate the exact retained endpoint under a new provider incarnation.
    ///
    /// The stopped binding authenticates the endpoint and provenance. The new
    /// binding must use that same numeric slot while its provider handle
    /// authenticates the new claim instead of reusing the stopped effect's
    /// opaque incarnation.
    pub fn adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
        &self,
        plan_members: &[PortLeaseRequest],
        request: &PortLeaseRequest,
        confirmed_stopped_binding: &PortLeaseBinding,
        claim: &PortBindClaim,
        binding: PortLeaseBinding,
        lifetime: &PortLeaseLifetimeGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        if lifetime.request != *request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        let witness = plan_members.iter().collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_member(state, &witness, request)?;
            let record = exact_record_mut(state, request)?;
            if record.phase == PortLeasePhase::Active
                && record.reservation_claim.is_none()
                && record.bind_claim.is_none()
                && record.adoption_claim.as_ref() == Some(claim)
                && record.binding.as_ref() == Some(&binding)
                && record.confirmed_stopped_binding.is_none()
                && record.active_lifetime == Some(lifetime.lifetime)
            {
                return Ok(record.clone());
            }
            if binding.endpoint() != confirmed_stopped_binding.endpoint()
                || binding.provenance() != confirmed_stopped_binding.provenance()
                || !binding.provider_registration_matches_claim(claim)
            {
                return Err(PortLeaseOperationError::BindingConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            if let Some(mismatch) = binding.mismatch(request.binding()) {
                return Err(PortLeaseOperationError::BindingMismatch {
                    lease_id: request.lease_id().clone(),
                    mismatch,
                });
            }
            if record.phase != PortLeasePhase::Reserved
                || record.reservation_claim.is_some()
                || record.bind_claim.as_ref() != Some(claim)
                || record.adoption_claim.is_some()
                || record.binding.is_some()
                || record.confirmed_stopped_binding.as_ref() != Some(confirmed_stopped_binding)
                || record.active_lifetime != Some(lifetime.lifetime)
                || record.reserved_port != Some(binding.actual_port())
                || record.failure.is_some()
            {
                return Err(PortLeaseOperationError::InvalidTransition {
                    lease_id: request.lease_id().clone(),
                    phase: record.phase,
                    operation: PortLeaseOperation::Adopt,
                });
            }
            record.phase = PortLeasePhase::Active;
            record.bind_claim = None;
            record.adoption_claim = Some(claim.clone());
            record.binding = Some(binding);
            record.confirmed_stopped_binding = None;
            Ok(record.clone())
        })
    }

    /// Relinquish an exact planned rebind claim after proving no effect.
    pub fn abandon_rebind_plan_member_with_lifetime_without_effect(
        &self,
        plan_members: &[PortLeaseRequest],
        request: &PortLeaseRequest,
        confirmed_stopped_binding: &PortLeaseBinding,
        claim: &PortBindClaim,
        lifetime: &PortLeaseLifetimeGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        if lifetime.request != *request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        let witness = plan_members.iter().collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_member(state, &witness, request)?;
            let record = exact_record_mut(state, request)?;
            let replay = record.phase == PortLeasePhase::Reserved
                && record.reservation_claim.is_none()
                && record.bind_claim.is_none()
                && record.adoption_claim.is_none()
                && record.binding.is_none()
                && record.confirmed_stopped_binding.as_ref() == Some(confirmed_stopped_binding)
                && record.active_lifetime.is_none()
                && record.last_lifetime_generation == lifetime.lifetime.generation.as_u64()
                && record.failure.is_none();
            if replay {
                return Ok(record.clone());
            }
            if record.phase != PortLeasePhase::Reserved
                || record.reservation_claim.is_some()
                || record.bind_claim.as_ref() != Some(claim)
                || record.adoption_claim.is_some()
                || record.binding.is_some()
                || record.confirmed_stopped_binding.as_ref() != Some(confirmed_stopped_binding)
                || record.active_lifetime != Some(lifetime.lifetime)
                || record.failure.is_some()
            {
                return Err(PortLeaseOperationError::InvalidTransition {
                    lease_id: request.lease_id().clone(),
                    phase: record.phase,
                    operation: PortLeaseOperation::AbandonBindClaimWithoutEffect,
                });
            }
            record.bind_claim = None;
            record.active_lifetime = None;
            Ok(record.clone())
        })
    }

    /// Atomically claim a retained provider-owned plan subset for rebind.
    pub fn claim_rebind_plan_members_with_lifetimes(
        &self,
        plan_members: &[PortLeaseRequest],
        claims: &[(PortLeaseRequest, PortBindClaim, PortLeaseBinding)],
        effect_scope: PortLeaseEffectScope,
    ) -> Result<Vec<PortLeaseLifetimeGuard>, PortLeaseError> {
        let Some(first) = claims.first() else {
            return Err(PortLeaseError::CorruptAuthority {
                reason: "planned rebind claim requires at least one member".to_owned(),
            });
        };
        let mut distinct = BTreeMap::new();
        for (request, claim, binding) in claims {
            if distinct
                .insert(request.lease_id().clone(), (request, claim, binding))
                .is_some()
            {
                return Err(PortLeaseError::IdentityConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
        }
        let witness = plan_members.iter().collect::<Vec<_>>();
        let requested = claims
            .iter()
            .map(|(request, _, _)| request)
            .collect::<Vec<_>>();
        self.transaction(|state| authenticate_complete_plan_members(state, &witness, &requested))?;
        let mut locks = BTreeMap::new();
        for lease_id in distinct.keys() {
            let lock = match self.try_acquire_lifetime_lock(lease_id)? {
                LifetimeLockAttempt::Acquired(lock) => lock,
                LifetimeLockAttempt::Contended => {
                    return Err(PortLeaseError::LifetimeOwnerLive {
                        lease_id: lease_id.clone(),
                    });
                }
            };
            locks.insert(lease_id.clone(), lock);
        }
        let lifetimes = self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &requested)?;
            for (request, claim, confirmed) in distinct.values().copied() {
                let record = exact_record(state, request)?;
                if let Some(active) = record.active_lifetime {
                    if active.effect_scope != effect_scope {
                        return Err(PortLeaseOperationError::LifetimeConflict {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    if record.bind_claim.as_ref() != Some(claim) {
                        return Err(PortLeaseOperationError::BindClaimConflict {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                }
                if record.phase != PortLeasePhase::Reserved
                    || record.reservation_claim.is_some()
                    || record.binding.is_some()
                    || record.adoption_claim.is_some()
                    || record.confirmed_stopped_binding.as_ref() != Some(confirmed)
                    || record.failure.is_some()
                    || (record.active_lifetime.is_none() && record.bind_claim.is_some())
                {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::BeginLifetime,
                    });
                }
            }
            let mut lifetimes = BTreeMap::new();
            for (request, claim, _) in distinct.values().copied() {
                let record = exact_record_mut(state, request)?;
                let lifetime = advance_lifetime(record, request, effect_scope)?;
                record.bind_claim = Some(claim.clone());
                lifetimes.insert(request.lease_id().clone(), lifetime);
            }
            Ok(lifetimes)
        })?;
        let _ = first;
        claims
            .iter()
            .map(|(request, _, _)| {
                Ok(PortLeaseLifetimeGuard {
                    request: request.clone(),
                    lifetime: lifetimes[request.lease_id()],
                    _lock: locks
                        .remove(request.lease_id())
                        .expect("every planned rebind member owns one stable lock"),
                })
            })
            .collect()
    }

    /// Atomically activate a retained plan subset after exact-slot rebind.
    ///
    /// Each new binding keeps the retained endpoint and provenance but carries
    /// the new claim's provider incarnation. The stopped incarnation is
    /// evidence for absence, not identity for the replacement effect.
    pub fn adopt_claimed_and_activate_rebind_plan_members_with_lifetimes(
        &self,
        plan_members: &[PortLeaseRequest],
        bindings: &[(PortLeaseRequest, PortBindClaim, PortLeaseBinding)],
        lifetimes: &[PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let claims = bindings
            .iter()
            .map(|(request, claim, _)| (request.clone(), claim.clone()))
            .collect::<Vec<_>>();
        let lifetimes = exact_lifetime_batch(&claims, lifetimes)?;
        let witness = plan_members.iter().collect::<Vec<_>>();
        let requested = bindings
            .iter()
            .map(|(request, _, _)| request)
            .collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &requested)?;
            for (request, claim, binding) in bindings {
                let lifetime = lifetimes[request.lease_id()];
                let record = exact_record(state, request)?;
                let replay = record.phase == PortLeasePhase::Active
                    && record.reservation_claim.is_none()
                    && record.bind_claim.is_none()
                    && record.adoption_claim.as_ref() == Some(claim)
                    && record.binding.as_ref() == Some(binding)
                    && record.confirmed_stopped_binding.is_none()
                    && record.active_lifetime == Some(lifetime);
                if replay {
                    continue;
                }
                if !binding.provider_registration_matches_claim(claim) {
                    return Err(PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if let Some(mismatch) = binding.mismatch(request.binding()) {
                    return Err(PortLeaseOperationError::BindingMismatch {
                        lease_id: request.lease_id().clone(),
                        mismatch,
                    });
                }
                let retained_binding_matches = record
                    .confirmed_stopped_binding
                    .as_ref()
                    .is_some_and(|confirmed| {
                        confirmed.endpoint() == binding.endpoint()
                            && confirmed.provenance() == binding.provenance()
                    });
                if record.phase != PortLeasePhase::Reserved
                    || record.reservation_claim.is_some()
                    || record.bind_claim.as_ref() != Some(claim)
                    || record.adoption_claim.is_some()
                    || record.binding.is_some()
                    || !retained_binding_matches
                    || record.active_lifetime != Some(lifetime)
                    || record.reserved_port != Some(binding.actual_port())
                    || record.failure.is_some()
                {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::Adopt,
                    });
                }
            }
            for (request, claim, binding) in bindings {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::Active {
                    continue;
                }
                record.phase = PortLeasePhase::Active;
                record.bind_claim = None;
                record.adoption_claim = Some(claim.clone());
                record.binding = Some(binding.clone());
                record.confirmed_stopped_binding = None;
            }
            bindings
                .iter()
                .map(|(request, _, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Atomically abandon a retained rebind subset after proving no effect.
    pub fn abandon_rebind_plan_members_with_lifetimes_without_effect(
        &self,
        plan_members: &[PortLeaseRequest],
        bindings: &[(PortLeaseRequest, PortBindClaim, PortLeaseBinding)],
        lifetimes: &[PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let claims = bindings
            .iter()
            .map(|(request, claim, _)| (request.clone(), claim.clone()))
            .collect::<Vec<_>>();
        let lifetimes = exact_lifetime_batch(&claims, lifetimes)?;
        let witness = plan_members.iter().collect::<Vec<_>>();
        let requested = bindings
            .iter()
            .map(|(request, _, _)| request)
            .collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &requested)?;
            for (request, claim, binding) in bindings {
                let lifetime = lifetimes[request.lease_id()];
                let record = exact_record(state, request)?;
                let replay = record.phase == PortLeasePhase::Reserved
                    && record.reservation_claim.is_none()
                    && record.bind_claim.is_none()
                    && record.adoption_claim.is_none()
                    && record.binding.is_none()
                    && record.confirmed_stopped_binding.as_ref() == Some(binding)
                    && record.active_lifetime.is_none()
                    && record.last_lifetime_generation == lifetime.generation.as_u64()
                    && record.failure.is_none();
                if replay {
                    continue;
                }
                if record.phase != PortLeasePhase::Reserved
                    || record.reservation_claim.is_some()
                    || record.bind_claim.as_ref() != Some(claim)
                    || record.adoption_claim.is_some()
                    || record.binding.is_some()
                    || record.confirmed_stopped_binding.as_ref() != Some(binding)
                    || record.active_lifetime != Some(lifetime)
                    || record.failure.is_some()
                {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::AbandonBindClaimWithoutEffect,
                    });
                }
            }
            for (request, _, _) in bindings {
                let record = exact_record_mut(state, request)?;
                if record.active_lifetime.is_some() {
                    record.bind_claim = None;
                    record.active_lifetime = None;
                }
            }
            bindings
                .iter()
                .map(|(request, _, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Atomically claim a complete provider batch and one lifetime per lease.
    ///
    /// Every lifetime file is locked before the durable transaction. Inputs
    /// must be identity-distinct, and locks are acquired in stable lease-ID
    /// order, so a failed contender drops every partial lock without changing
    /// durable authority. The returned guards follow the caller's input order.
    pub fn claim_bind_batch_with_lifetimes(
        &self,
        claims: &[(PortLeaseRequest, PortBindClaim)],
        reservation_claim: Option<&NetworkReservationClaim>,
        effect_scope: PortLeaseEffectScope,
    ) -> Result<Vec<PortLeaseLifetimeGuard>, PortLeaseError> {
        self.claim_bind_batch_with_lifetimes_inner(claims, reservation_claim, effect_scope, None)
    }

    /// Atomically claim a provider-owned subset under one complete plan
    /// witness and one exact reservation coordinator.
    pub fn claim_bind_plan_members_with_lifetimes(
        &self,
        plan_members: &[PortLeaseRequest],
        claims: &[(PortLeaseRequest, PortBindClaim)],
        reservation_claim: &NetworkReservationClaim,
        effect_scope: PortLeaseEffectScope,
    ) -> Result<Vec<PortLeaseLifetimeGuard>, PortLeaseError> {
        self.claim_bind_batch_with_lifetimes_inner(
            claims,
            Some(reservation_claim),
            effect_scope,
            Some(plan_members),
        )
    }

    fn claim_bind_batch_with_lifetimes_inner(
        &self,
        claims: &[(PortLeaseRequest, PortBindClaim)],
        reservation_claim: Option<&NetworkReservationClaim>,
        effect_scope: PortLeaseEffectScope,
        plan_witness: Option<&[PortLeaseRequest]>,
    ) -> Result<Vec<PortLeaseLifetimeGuard>, PortLeaseError> {
        let mut distinct = BTreeMap::<PortLeaseId, (&PortLeaseRequest, &PortBindClaim)>::new();
        for (request, claim) in claims {
            if distinct
                .insert(request.lease_id().clone(), (request, claim))
                .is_some()
            {
                return Err(PortLeaseError::IdentityConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
        }
        let requested = claims
            .iter()
            .map(|(request, _)| request)
            .collect::<Vec<_>>();
        self.transaction(|state| {
            if let Some(plan_witness) = plan_witness {
                let witness = plan_witness.iter().collect::<Vec<_>>();
                authenticate_complete_plan_members(state, &witness, &requested)
            } else {
                authenticate_complete_plan_batch_if_present(state, &requested)
            }
        })?;

        let mut locks = BTreeMap::new();
        for lease_id in distinct.keys() {
            let lock = match self.try_acquire_lifetime_lock(lease_id)? {
                LifetimeLockAttempt::Acquired(lock) => lock,
                LifetimeLockAttempt::Contended => {
                    return Err(PortLeaseError::LifetimeOwnerLive {
                        lease_id: lease_id.clone(),
                    });
                }
            };
            locks.insert(lease_id.clone(), lock);
        }

        let lifetimes = self.transaction(|state| {
            if let Some(plan_witness) = plan_witness {
                let witness = plan_witness.iter().collect::<Vec<_>>();
                authenticate_complete_plan_members(state, &witness, &requested)?;
            } else {
                authenticate_complete_plan_batch_if_present(state, &requested)?;
            }
            for (request, claim) in distinct.values().copied() {
                let record = exact_record(state, request)?;
                require_reservation_claim(record, reservation_claim)?;
                if record.phase != PortLeasePhase::Reserved {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::BeginLifetime,
                    });
                }
                if record
                    .bind_claim
                    .as_ref()
                    .is_some_and(|current| current != claim)
                {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if record.active_lifetime.is_some() {
                    return Err(PortLeaseOperationError::LifetimeConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if record.last_lifetime_generation == u64::MAX {
                    return Err(PortLeaseOperationError::LifetimeGenerationExhausted {
                        lease_id: request.lease_id().clone(),
                    });
                }
            }

            let mut lifetimes = BTreeMap::new();
            for (request, claim) in distinct.values().copied() {
                let record = exact_record_mut(state, request)?;
                let lifetime = advance_lifetime(record, request, effect_scope)?;
                record.bind_claim = Some(claim.clone());
                lifetimes.insert(request.lease_id().clone(), lifetime);
            }
            Ok(lifetimes)
        })?;

        claims
            .iter()
            .map(|(request, _)| {
                let lease_id = request.lease_id();
                Ok(PortLeaseLifetimeGuard {
                    request: request.clone(),
                    lifetime: lifetimes[lease_id],
                    _lock: locks
                        .remove(lease_id)
                        .expect("every claimed lifetime owns one acquired lock"),
                })
            })
            .collect()
    }

    /// Atomically adopt and activate one binding under its exact live guard.
    pub fn adopt_claimed_and_activate_with_lifetime(
        &self,
        request: &PortLeaseRequest,
        reservation_claim: Option<&NetworkReservationClaim>,
        claim: &PortBindClaim,
        binding: super::PortLeaseBinding,
        lifetime: &PortLeaseLifetimeGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        if lifetime.request != *request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        let required_lifetimes = BTreeMap::from([(request.lease_id().clone(), lifetime.lifetime)]);
        self.adopt_claimed_and_activate_batch_inner(
            &[(request.clone(), claim.clone(), binding)],
            reservation_claim,
            Some(&required_lifetimes),
            None,
        )
        .map(|mut records| {
            records
                .pop()
                .expect("one lifetime-authenticated activation returns one record")
        })
    }

    /// Atomically adopt and activate a complete lifetime-fenced binding batch.
    pub fn adopt_claimed_and_activate_batch_with_lifetimes(
        &self,
        bindings: &[(PortLeaseRequest, PortBindClaim, PortLeaseBinding)],
        reservation_claim: Option<&NetworkReservationClaim>,
        lifetimes: &[PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        if bindings.len() != lifetimes.len() {
            let lease_id = bindings
                .first()
                .map(|(request, _, _)| request.lease_id().clone())
                .or_else(|| {
                    lifetimes
                        .first()
                        .map(|lifetime| lifetime.request.lease_id().clone())
                })
                .ok_or_else(|| PortLeaseError::CorruptAuthority {
                    reason: "empty lifetime batch has divergent lengths".to_owned(),
                })?;
            return Err(PortLeaseError::LifetimeMismatch { lease_id });
        }

        let mut required_lifetimes = BTreeMap::new();
        for lifetime in lifetimes {
            if required_lifetimes
                .insert(
                    lifetime.request.lease_id().clone(),
                    (lifetime.request(), lifetime.lifetime()),
                )
                .is_some()
            {
                return Err(PortLeaseError::IdentityConflict {
                    lease_id: lifetime.request.lease_id().clone(),
                });
            }
        }
        for (request, _, _) in bindings {
            let Some((guard_request, _)) = required_lifetimes.get(request.lease_id()) else {
                return Err(PortLeaseError::LifetimeMismatch {
                    lease_id: request.lease_id().clone(),
                });
            };
            if *guard_request != request {
                return Err(PortLeaseError::LifetimeMismatch {
                    lease_id: request.lease_id().clone(),
                });
            }
        }
        let required_lifetimes = required_lifetimes
            .into_iter()
            .map(|(lease_id, (_, lifetime))| (lease_id, lifetime))
            .collect();
        self.adopt_claimed_and_activate_batch_inner(
            bindings,
            reservation_claim,
            Some(&required_lifetimes),
            None,
        )
    }

    /// Adopt and activate one planned member under a complete immutable plan
    /// witness and its exact live process guard.
    pub fn adopt_claimed_and_activate_plan_member_with_lifetime(
        &self,
        plan_members: &[PortLeaseRequest],
        request: &PortLeaseRequest,
        reservation_claim: &NetworkReservationClaim,
        claim: &PortBindClaim,
        binding: super::PortLeaseBinding,
        lifetime: &PortLeaseLifetimeGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        if lifetime.request != *request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        let required_lifetimes = BTreeMap::from([(request.lease_id().clone(), lifetime.lifetime)]);
        self.adopt_claimed_and_activate_batch_inner(
            &[(request.clone(), claim.clone(), binding)],
            Some(reservation_claim),
            Some(&required_lifetimes),
            Some(plan_members),
        )
        .map(|mut records| {
            records
                .pop()
                .expect("one plan-member activation returns one record")
        })
    }

    /// Atomically activate a provider-owned subset while authenticating one
    /// complete immutable plan witness.
    pub fn adopt_claimed_and_activate_plan_members_with_lifetimes(
        &self,
        plan_members: &[PortLeaseRequest],
        bindings: &[(PortLeaseRequest, PortBindClaim, PortLeaseBinding)],
        reservation_claim: &NetworkReservationClaim,
        lifetimes: &[PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let claims = bindings
            .iter()
            .map(|(request, claim, _)| (request.clone(), claim.clone()))
            .collect::<Vec<_>>();
        let required_lifetimes = exact_lifetime_batch(&claims, lifetimes)?;
        self.adopt_claimed_and_activate_batch_inner(
            bindings,
            Some(reservation_claim),
            Some(&required_lifetimes),
            Some(plan_members),
        )
    }

    /// Relinquish one lifetime-authenticated bind attempt after proving that it
    /// created no effect.
    ///
    /// The exact live guard prevents another process from clearing a claimed
    /// attempt while its owner may still bind. A replay after an exact
    /// no-effect failure is idempotent because that transition already clears
    /// the lifetime.
    pub fn abandon_bind_with_lifetime_without_effect(
        &self,
        request: &PortLeaseRequest,
        reservation_claim: Option<&NetworkReservationClaim>,
        claim: &PortBindClaim,
        lifetime: &PortLeaseLifetimeGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        if lifetime.request != *request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        self.transaction(|state| {
            authenticate_scalar_plan_if_present(state, request)?;
            let record = exact_record_mut(state, request)?;
            require_reservation_claim(record, reservation_claim)?;
            match record.phase {
                PortLeasePhase::Reserved
                    if record.bind_claim.as_ref() == Some(claim)
                        && record.active_lifetime == Some(lifetime.lifetime) =>
                {
                    record.bind_claim = None;
                    record.active_lifetime = None;
                }
                PortLeasePhase::Reserved
                    if record.bind_claim.is_none()
                        && record.active_lifetime.is_none()
                        && record.last_lifetime_generation
                            == lifetime.lifetime.generation.as_u64() => {}
                PortLeasePhase::Failed
                    if record.active_lifetime.is_none()
                        && record.failure.as_ref().is_some_and(|failure| {
                            failure.provider_attempt() == claim.provider_attempt()
                        }) => {}
                PortLeasePhase::Reserved => {
                    return Err(PortLeaseOperationError::LifetimeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase,
                        operation: PortLeaseOperation::AbandonBindClaimWithoutEffect,
                    });
                }
            }
            Ok(record.clone())
        })
    }

    /// Relinquish one no-effect planned member while authenticating the exact
    /// complete plan witness and launch coordinator.
    pub fn abandon_bind_plan_member_with_lifetime_without_effect(
        &self,
        plan_members: &[PortLeaseRequest],
        request: &PortLeaseRequest,
        reservation_claim: &NetworkReservationClaim,
        claim: &PortBindClaim,
        lifetime: &PortLeaseLifetimeGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        if lifetime.request != *request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        let witness = plan_members.iter().collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_member(state, &witness, request)?;
            let record = exact_record_mut(state, request)?;
            require_reservation_claim(record, Some(reservation_claim))?;
            match record.phase {
                PortLeasePhase::Reserved
                    if record.bind_claim.as_ref() == Some(claim)
                        && record.active_lifetime == Some(lifetime.lifetime) =>
                {
                    record.bind_claim = None;
                    record.active_lifetime = None;
                }
                PortLeasePhase::Reserved
                    if record.bind_claim.is_none()
                        && record.active_lifetime.is_none()
                        && record.last_lifetime_generation
                            == lifetime.lifetime.generation.as_u64() => {}
                PortLeasePhase::Failed
                    if record.active_lifetime.is_none()
                        && record.failure.as_ref().is_some_and(|failure| {
                            failure.provider_attempt() == claim.provider_attempt()
                        }) => {}
                PortLeasePhase::Reserved => {
                    return Err(PortLeaseOperationError::LifetimeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase,
                        operation: PortLeaseOperation::AbandonBindClaimWithoutEffect,
                    });
                }
            }
            Ok(record.clone())
        })
    }

    /// Atomically relinquish one exact lifetime-fenced no-effect batch.
    pub fn abandon_bind_batch_with_lifetimes_without_effect(
        &self,
        claims: &[(PortLeaseRequest, PortBindClaim)],
        reservation_claim: Option<&NetworkReservationClaim>,
        lifetimes: &[PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let required_lifetimes = exact_lifetime_batch(claims, lifetimes)?;
        self.transaction(|state| {
            let planned = claims
                .iter()
                .map(|(request, _)| request)
                .collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &planned)?;
            for (request, claim) in claims {
                let lifetime = required_lifetimes[request.lease_id()];
                let record = exact_record(state, request)?;
                require_reservation_claim(record, reservation_claim)?;
                match record.phase {
                    PortLeasePhase::Reserved
                        if record.bind_claim.as_ref() == Some(claim)
                            && record.active_lifetime == Some(lifetime) => {}
                    PortLeasePhase::Reserved
                        if record.bind_claim.is_none()
                            && record.active_lifetime.is_none()
                            && record.last_lifetime_generation == lifetime.generation.as_u64() => {}
                    PortLeasePhase::Failed
                        if record.active_lifetime.is_none()
                            && record.last_lifetime_generation == lifetime.generation.as_u64()
                            && record.failure.as_ref().is_some_and(|failure| {
                                failure.provider_attempt() == claim.provider_attempt()
                            }) => {}
                    PortLeasePhase::Reserved => {
                        return Err(PortLeaseOperationError::LifetimeMismatch {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::AbandonBindClaimWithoutEffect,
                        });
                    }
                }
            }
            for (request, claim) in claims {
                let record = exact_record_mut(state, request)?;
                if record.bind_claim.as_ref() == Some(claim) {
                    record.bind_claim = None;
                    record.active_lifetime = None;
                }
            }
            claims
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Atomically relinquish a no-effect provider subset while proving the
    /// complete immutable plan and exact launch coordinator.
    pub fn abandon_bind_plan_members_with_lifetimes_without_effect(
        &self,
        plan_members: &[PortLeaseRequest],
        claims: &[(PortLeaseRequest, PortBindClaim)],
        reservation_claim: &NetworkReservationClaim,
        lifetimes: &[PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let required_lifetimes = exact_lifetime_batch(claims, lifetimes)?;
        let witness = plan_members.iter().collect::<Vec<_>>();
        let members = claims
            .iter()
            .map(|(request, _)| request)
            .collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &members)?;
            for (request, claim) in claims {
                let lifetime = required_lifetimes[request.lease_id()];
                let record = exact_record(state, request)?;
                require_reservation_claim(record, Some(reservation_claim))?;
                match record.phase {
                    PortLeasePhase::Reserved
                        if record.bind_claim.as_ref() == Some(claim)
                            && record.active_lifetime == Some(lifetime) => {}
                    PortLeasePhase::Reserved
                        if record.bind_claim.is_none()
                            && record.active_lifetime.is_none()
                            && record.last_lifetime_generation == lifetime.generation.as_u64() => {}
                    PortLeasePhase::Failed
                        if record.active_lifetime.is_none()
                            && record.last_lifetime_generation == lifetime.generation.as_u64()
                            && record.failure.as_ref().is_some_and(|failure| {
                                failure.provider_attempt() == claim.provider_attempt()
                            }) => {}
                    PortLeasePhase::Reserved => {
                        return Err(PortLeaseOperationError::LifetimeMismatch {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::AbandonBindClaimWithoutEffect,
                        });
                    }
                }
            }
            for (request, claim) in claims {
                let record = exact_record_mut(state, request)?;
                if record.bind_claim.as_ref() == Some(claim) {
                    record.bind_claim = None;
                    record.active_lifetime = None;
                }
            }
            claims
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Reconcile every dead process-bound lease in stable ID order.
    ///
    /// This explicit operation performs no socket probe and no provider
    /// inspection. Provider-managed and lifetime-less records remain fenced
    /// and are reported for their owning adapter.
    pub fn reconcile_dead_process_bound_leases(
        &self,
    ) -> Result<PortLeaseLifetimeReconciliation, PortLeaseError> {
        let mut report = PortLeaseLifetimeReconciliation::default();
        for record in self.list()? {
            if record.phase().is_terminal() {
                continue;
            }
            let Some(lifetime) = record.active_lifetime() else {
                report
                    .missing_lifetime
                    .push(record.request().lease_id().clone());
                continue;
            };
            if lifetime.effect_scope() == PortLeaseEffectScope::ProviderManaged {
                report
                    .provider_managed
                    .push(record.request().lease_id().clone());
                continue;
            }
            match self.recover_dead_lifetime(record.request())? {
                PortLeaseRecoveryAttempt::LiveOwner(current) => {
                    report.live.push(current.request().lease_id().clone());
                }
                PortLeaseRecoveryAttempt::Acquired(recovery) => {
                    self.mark_cleanup_pending_after_owner_death(record.request(), &recovery)?;
                    let released =
                        self.release_process_bound_after_owner_death(record.request(), &recovery)?;
                    report.released.push(released.request().lease_id().clone());
                }
                PortLeaseRecoveryAttempt::Settled(_) => {}
            }
        }
        Ok(report)
    }

    /// Inspect whether an exact durable process owner is still live.
    ///
    /// This never waits for the lifetime owner and never performs a provider
    /// effect. `Acquired` holds the same exclusive OS lock until the caller
    /// completes or checkpoints reconciliation.
    pub fn recover_dead_lifetime(
        &self,
        request: &PortLeaseRequest,
    ) -> Result<PortLeaseRecoveryAttempt, PortLeaseError> {
        let initial = self.exact_lifetime_record(request)?;
        if initial.phase().is_terminal() {
            return Ok(PortLeaseRecoveryAttempt::Settled(initial));
        }
        if initial.active_lifetime().is_none() {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }

        match self.try_acquire_lifetime_lock(request.lease_id())? {
            LifetimeLockAttempt::Contended => {
                let current = self.exact_lifetime_record(request)?;
                if current.phase().is_terminal() {
                    Ok(PortLeaseRecoveryAttempt::Settled(current))
                } else if current.active_lifetime().is_some() {
                    Ok(PortLeaseRecoveryAttempt::LiveOwner(current))
                } else {
                    Err(PortLeaseError::LifetimeMismatch {
                        lease_id: request.lease_id().clone(),
                    })
                }
            }
            LifetimeLockAttempt::Acquired(lock) => {
                let current = self.exact_lifetime_record(request)?;
                if current.phase().is_terminal() {
                    return Ok(PortLeaseRecoveryAttempt::Settled(current));
                }
                let lifetime =
                    current
                        .active_lifetime()
                        .ok_or_else(|| PortLeaseError::LifetimeMismatch {
                            lease_id: request.lease_id().clone(),
                        })?;
                Ok(PortLeaseRecoveryAttempt::Acquired(PortLeaseRecoveryGuard {
                    request: request.clone(),
                    lifetime,
                    _lock: lock,
                }))
            }
        }
    }

    /// Acquire dead-owner authority for an exact reconcilable subset of one plan.
    ///
    /// The complete immutable plan witness authenticates membership while the
    /// requested subset identifies only the provider effects this caller owns.
    /// Unrequested siblings may remain reserved. This operation performs no
    /// durable mutation and acquires every requested lifetime lock atomically.
    pub fn recover_dead_plan_members(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
    ) -> Result<Vec<PortLeaseRecoveryGuard>, PortLeaseError> {
        if requests.is_empty() {
            return Err(PortLeaseError::CorruptAuthority {
                reason: "planned lifetime recovery requires at least one member".to_owned(),
            });
        }
        let witness = plan_members.iter().collect::<Vec<_>>();
        let requested = requests.iter().collect::<Vec<_>>();
        let initial_witness = self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &requested)?;
            for request in requests {
                let record = exact_record(state, request)?;
                if !matches!(
                    record.phase(),
                    PortLeasePhase::Active | PortLeasePhase::CleanupPending
                ) || record.active_lifetime().is_none()
                {
                    return Err(PortLeaseOperationError::LifetimeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
            }
            plan_members
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect::<Result<Vec<_>, _>>()
        })?;

        let mut locks = BTreeMap::new();
        let mut stable_ids = requests
            .iter()
            .map(|request| request.lease_id().clone())
            .collect::<Vec<_>>();
        stable_ids.sort();
        stable_ids.dedup();
        for lease_id in stable_ids {
            let lock = match self.try_acquire_lifetime_lock(&lease_id)? {
                LifetimeLockAttempt::Acquired(lock) => lock,
                LifetimeLockAttempt::Contended => {
                    return Err(PortLeaseError::LifetimeOwnerLive { lease_id });
                }
            };
            locks.insert(lease_id, lock);
        }

        let current = self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &requested)?;
            for (request, expected) in plan_members.iter().zip(&initial_witness) {
                let record = exact_record(state, request)?;
                if record != expected {
                    return Err(PortLeaseOperationError::LifetimeConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
            }
            requests
                .iter()
                .map(|request| {
                    let record = exact_record(state, request)?;
                    if !matches!(
                        record.phase(),
                        PortLeasePhase::Active | PortLeasePhase::CleanupPending
                    ) || record.active_lifetime().is_none()
                    {
                        return Err(PortLeaseOperationError::LifetimeMismatch {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    Ok(record.clone())
                })
                .collect::<Result<Vec<_>, _>>()
        })?;

        requests
            .iter()
            .zip(current)
            .map(|(request, record)| {
                let lifetime =
                    record
                        .active_lifetime()
                        .ok_or_else(|| PortLeaseError::LifetimeMismatch {
                            lease_id: request.lease_id().clone(),
                        })?;
                Ok(PortLeaseRecoveryGuard {
                    request: request.clone(),
                    lifetime,
                    _lock: locks
                        .remove(request.lease_id())
                        .expect("every recovered plan member owns one stable lock"),
                })
            })
            .collect()
    }

    /// Quarantine an exact recovered subset while preserving plan siblings.
    ///
    /// This is the durable crash checkpoint between dead-owner authentication
    /// and provider-specific cleanup or rebind. Every supplied recovery guard
    /// must belong to the exact requested member and complete plan witness.
    pub fn mark_cleanup_pending_plan_members_after_owner_death(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let recoveries = exact_plan_recoveries(requests, recoveries)?;
        let witness = plan_members.iter().collect::<Vec<_>>();
        let requested = requests.iter().collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &requested)?;
            for request in requests {
                let recovery = recoveries[request.lease_id()];
                let record = exact_record(state, request)?;
                authenticate_recovery(record, request, recovery)?;
                if !matches!(
                    record.phase,
                    PortLeasePhase::Active | PortLeasePhase::CleanupPending
                ) {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::MarkCleanupPending,
                    });
                }
                if record.binding.is_none()
                    || record.bind_claim.is_some()
                    || record.adoption_claim.is_none()
                {
                    return Err(PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
            }
            for request in requests {
                exact_record_mut(state, request)?.phase = PortLeasePhase::CleanupPending;
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Retain exact process-bound plan members for rebind after owner death.
    ///
    /// Callers must first durably checkpoint the same subset as cleanup
    /// pending. Replays with the same recovery generation are idempotent.
    pub fn prepare_rebind_process_bound_plan_members_after_owner_death(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let recoveries = exact_plan_recoveries(requests, recoveries)?;
        let witness = plan_members.iter().collect::<Vec<_>>();
        let requested = requests.iter().collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &requested)?;
            for request in requests {
                let recovery = recoveries[request.lease_id()];
                let record = exact_record(state, request)?;
                let replay = record.phase == PortLeasePhase::Reserved
                    && record.active_lifetime.is_none()
                    && record.last_lifetime_generation == recovery.lifetime.generation.as_u64()
                    && record.confirmed_stopped_binding.is_some()
                    && recovery.request == *request;
                if replay {
                    continue;
                }
                authenticate_recovery(record, request, recovery)?;
                if recovery.lifetime.effect_scope != PortLeaseEffectScope::ProcessBound {
                    return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if record.phase != PortLeasePhase::CleanupPending {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::PrepareRebindAfterOwnerDeath,
                    });
                }
                if record.binding.is_none()
                    || record.bind_claim.is_some()
                    || record.adoption_claim.is_none()
                {
                    return Err(PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
            }
            for request in requests {
                let recovery = recoveries[request.lease_id()];
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::Reserved
                    && record.active_lifetime.is_none()
                    && record.last_lifetime_generation == recovery.lifetime.generation.as_u64()
                    && record.confirmed_stopped_binding.is_some()
                {
                    continue;
                }
                let binding = record.binding.take().ok_or_else(|| {
                    PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id().clone(),
                    }
                })?;
                record.phase = PortLeasePhase::Reserved;
                record.bind_claim = None;
                record.adoption_claim = None;
                record.confirmed_stopped_binding = Some(binding);
                record.failure = None;
                record.active_lifetime = None;
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Retain an exact provider-managed plan subset after confirmed stop.
    pub fn prepare_rebind_provider_managed_plan_members_after_confirmed_stop(
        &self,
        plan_members: &[PortLeaseRequest],
        bindings: &[(PortLeaseRequest, PortLeaseBinding)],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let requests = bindings
            .iter()
            .map(|(request, _)| request.clone())
            .collect::<Vec<_>>();
        let recoveries = exact_plan_recoveries(&requests, recoveries)?;
        let witness = plan_members.iter().collect::<Vec<_>>();
        let requested = requests.iter().collect::<Vec<_>>();
        self.transaction(|state| {
            authenticate_complete_plan_members(state, &witness, &requested)?;
            for (request, expected_binding) in bindings {
                let recovery = recoveries[request.lease_id()];
                if recovery.lifetime.effect_scope != PortLeaseEffectScope::ProviderManaged {
                    return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                match record.phase {
                    PortLeasePhase::CleanupPending => {
                        authenticate_recovery(record, request, recovery)?;
                        if record.binding.as_ref() != Some(expected_binding)
                            || record.reserved_port != Some(expected_binding.actual_port())
                        {
                            return Err(PortLeaseOperationError::BindingConflict {
                                lease_id: request.lease_id().clone(),
                            });
                        }
                    }
                    PortLeasePhase::Reserved
                        if record.active_lifetime.is_none()
                            && record.last_lifetime_generation
                                == recovery.lifetime.generation.as_u64()
                            && record.confirmed_stopped_binding.as_ref()
                                == Some(expected_binding)
                            && record.reservation_claim.is_none()
                            && record.binding.is_none()
                            && record.bind_claim.is_none()
                            && record.adoption_claim.is_none()
                            && record.failure.is_none() => {}
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::PrepareRebindAfterConfirmedStop,
                        });
                    }
                }
            }
            for (request, expected_binding) in bindings {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::CleanupPending {
                    record.phase = PortLeasePhase::Reserved;
                    record.reservation_claim = None;
                    record.binding = None;
                    record.bind_claim = None;
                    record.adoption_claim = None;
                    record.confirmed_stopped_binding = Some(expected_binding.clone());
                    record.failure = None;
                    record.active_lifetime = None;
                }
            }
            bindings
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Acquire every dead lifetime in one exact durable batch.
    ///
    /// Locks are acquired in stable lease-ID order and any contention drops all
    /// earlier locks before returning. Planned callers must present the
    /// complete immutable member set; standalone callers still receive the same
    /// exact request and lifetime fencing.
    pub fn recover_dead_lifetimes(
        &self,
        requests: &[PortLeaseRequest],
    ) -> Result<Vec<PortLeaseRecoveryGuard>, PortLeaseError> {
        let initial = self.transaction(|state| {
            let planned = requests.iter().collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &planned)?;
            let mut distinct = BTreeMap::new();
            for request in requests {
                if distinct
                    .insert(request.lease_id().clone(), request)
                    .is_some()
                {
                    return Err(PortLeaseOperationError::IdentityConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                if record.phase().is_terminal() || record.active_lifetime().is_none() {
                    return Err(PortLeaseOperationError::LifetimeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect::<Result<Vec<_>, _>>()
        })?;

        let mut locks = BTreeMap::new();
        let mut stable_ids = requests
            .iter()
            .map(|request| request.lease_id().clone())
            .collect::<Vec<_>>();
        stable_ids.sort();
        stable_ids.dedup();
        for lease_id in stable_ids {
            let lock = match self.try_acquire_lifetime_lock(&lease_id)? {
                LifetimeLockAttempt::Acquired(lock) => lock,
                LifetimeLockAttempt::Contended => {
                    return Err(PortLeaseError::LifetimeOwnerLive { lease_id });
                }
            };
            locks.insert(lease_id, lock);
        }

        let current = self.transaction(|state| {
            let planned = requests.iter().collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &planned)?;
            for (request, expected) in requests.iter().zip(&initial) {
                let record = exact_record(state, request)?;
                if record.phase().is_terminal()
                    || record.active_lifetime() != expected.active_lifetime()
                {
                    return Err(PortLeaseOperationError::LifetimeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect::<Result<Vec<_>, _>>()
        })?;

        requests
            .iter()
            .zip(current)
            .map(|(request, record)| {
                let lifetime =
                    record
                        .active_lifetime()
                        .ok_or_else(|| PortLeaseError::LifetimeMismatch {
                            lease_id: request.lease_id().clone(),
                        })?;
                Ok(PortLeaseRecoveryGuard {
                    request: request.clone(),
                    lifetime,
                    _lock: locks
                        .remove(request.lease_id())
                        .expect("every recovered request owns one stable lock"),
                })
            })
            .collect()
    }

    /// Quarantine every possibly live effect owned by a dead process generation.
    pub fn mark_cleanup_pending_after_owner_death(
        &self,
        request: &PortLeaseRequest,
        recovery: &PortLeaseRecoveryGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            authenticate_scalar_plan_if_present(state, request)?;
            let record = exact_record_mut(state, request)?;
            authenticate_recovery(record, request, recovery)?;
            match record.phase {
                PortLeasePhase::Reserved
                | PortLeasePhase::Binding
                | PortLeasePhase::Active
                | PortLeasePhase::Withdrawing => {
                    record.phase = PortLeasePhase::CleanupPending;
                }
                PortLeasePhase::CleanupPending => {}
                phase => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase,
                        operation: PortLeaseOperation::MarkCleanupPending,
                    });
                }
            }
            Ok(record.clone())
        })
    }

    /// Transfer one exact surviving provider-managed binding to this process.
    ///
    /// The recovery guard proves the former Nimbus coordinator is dead. The
    /// effect adapter must separately possess and authenticate the surviving
    /// provider resource before calling this transition. The binding and
    /// provider handle remain unchanged while a higher process-lifetime
    /// generation fences the former owner.
    pub fn reclaim_provider_managed_binding_after_owner_death(
        &self,
        request: &PortLeaseRequest,
        expected_binding: &PortLeaseBinding,
        recovery: PortLeaseRecoveryGuard,
    ) -> Result<PortLeaseLifetimeGuard, PortLeaseError> {
        if recovery.request != *request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        if recovery.lifetime.effect_scope != PortLeaseEffectScope::ProviderManaged {
            return Err(PortLeaseError::LifetimeScopeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        let lifetime = self.transaction(|state| {
            authenticate_scalar_plan_if_present(state, request)?;
            let record = exact_record_mut(state, request)?;
            authenticate_recovery(record, request, &recovery)?;
            if let Some(mismatch) = expected_binding.mismatch(request.binding()) {
                return Err(PortLeaseOperationError::BindingMismatch {
                    lease_id: request.lease_id().clone(),
                    mismatch,
                });
            }
            if record.reserved_port != Some(expected_binding.actual_port()) {
                return Err(PortLeaseOperationError::BindingConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            match record.phase {
                PortLeasePhase::Active
                | PortLeasePhase::Withdrawing
                | PortLeasePhase::CleanupPending
                    if record.binding.as_ref() == Some(expected_binding)
                        && record.bind_claim.is_none() => {}
                PortLeasePhase::Reserved
                    if record.binding.is_none()
                        && record.adoption_claim.is_none()
                        && record.bind_claim.as_ref().is_some_and(|claim| {
                            expected_binding.provider_registration_matches_claim(claim)
                        }) =>
                {
                    record.adoption_claim = record.bind_claim.take();
                    record.binding = Some(expected_binding.clone());
                    record.confirmed_stopped_binding = None;
                }
                _ => {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::ReclaimProviderManagedBinding,
                    });
                }
            }
            record.phase = PortLeasePhase::Active;
            advance_lifetime(record, request, PortLeaseEffectScope::ProviderManaged)
        })?;
        let PortLeaseRecoveryGuard { _lock, .. } = recovery;
        Ok(PortLeaseLifetimeGuard {
            request: request.clone(),
            lifetime,
            _lock,
        })
    }

    /// Atomically quarantine one exact dead-owner provider batch.
    pub fn mark_cleanup_pending_batch_after_owner_death(
        &self,
        requests: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let recoveries = exact_recovery_batch(requests, recoveries)?;
        self.transaction(|state| {
            let requested = requests.iter().collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &requested)?;
            for request in requests {
                let recovery = recoveries[request.lease_id()];
                let record = exact_record(state, request)?;
                authenticate_recovery(record, request, recovery)?;
                if !matches!(
                    record.phase,
                    PortLeasePhase::Reserved
                        | PortLeasePhase::Binding
                        | PortLeasePhase::Active
                        | PortLeasePhase::Withdrawing
                        | PortLeasePhase::CleanupPending
                ) {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::MarkCleanupPending,
                    });
                }
            }
            for request in requests {
                let record = exact_record_mut(state, request)?;
                record.phase = PortLeasePhase::CleanupPending;
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Retain a provider-managed batch after its adapter proves exact absence.
    ///
    /// Process death grants only the recovery guards. The sandbox/provider
    /// adapter must independently establish absence before calling this
    /// transition with each exact adopted binding.
    pub fn prepare_rebind_provider_managed_batch_after_confirmed_stop(
        &self,
        bindings: &[(PortLeaseRequest, PortLeaseBinding)],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let requests = bindings
            .iter()
            .map(|(request, _)| request.clone())
            .collect::<Vec<_>>();
        let recoveries = exact_recovery_batch(&requests, recoveries)?;
        self.transaction(|state| {
            let requested = bindings
                .iter()
                .map(|(request, _)| request)
                .collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &requested)?;
            for (request, expected_binding) in bindings {
                let recovery = recoveries[request.lease_id()];
                if recovery.lifetime.effect_scope != PortLeaseEffectScope::ProviderManaged {
                    return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                match record.phase {
                    PortLeasePhase::CleanupPending => {
                        authenticate_recovery(record, request, recovery)?;
                        if record.binding.as_ref() != Some(expected_binding)
                            || record.reserved_port != Some(expected_binding.actual_port())
                        {
                            return Err(PortLeaseOperationError::BindingConflict {
                                lease_id: request.lease_id().clone(),
                            });
                        }
                    }
                    PortLeasePhase::Reserved
                        if record.active_lifetime.is_none()
                            && record.last_lifetime_generation
                                == recovery.lifetime.generation.as_u64()
                            && record.confirmed_stopped_binding.as_ref()
                                == Some(expected_binding)
                            && record.binding.is_none()
                            && record.bind_claim.is_none()
                            && record.failure.is_none() => {}
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::PrepareRebindAfterConfirmedStop,
                        });
                    }
                }
            }
            for (request, expected_binding) in bindings {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::CleanupPending {
                    record.phase = PortLeasePhase::Reserved;
                    record.reservation_claim = None;
                    record.binding = None;
                    record.bind_claim = None;
                    record.adoption_claim = None;
                    record.confirmed_stopped_binding = Some(expected_binding.clone());
                    record.failure = None;
                    record.active_lifetime = None;
                }
            }
            bindings
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Retire dead provider bind claims after the adapter confirms absence.
    ///
    /// A restarted provider can crash after claiming a retained numeric slot
    /// but before adopting a binding. Process death authenticates only the
    /// dead coordinator; the adapter must separately prove the provider effect
    /// absent before invoking this transition. The selected slot, launch
    /// reservation claim, and any prior confirmed-stop receipt remain intact
    /// so the same desired generation can begin one higher bind lifetime.
    pub fn prepare_rebind_provider_managed_claim_batch_after_confirmed_stop(
        &self,
        requests: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let recoveries = exact_recovery_batch(requests, recoveries)?;
        self.transaction(|state| {
            let requested = requests.iter().collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &requested)?;
            for request in requests {
                let recovery = recoveries[request.lease_id()];
                if recovery.lifetime.effect_scope != PortLeaseEffectScope::ProviderManaged {
                    return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                match record.phase {
                    PortLeasePhase::CleanupPending => {
                        authenticate_recovery(record, request, recovery)?;
                        if record.bind_claim.is_none()
                            || record.binding.is_some()
                            || record.adoption_claim.is_some()
                            || record.failure.is_some()
                        {
                            return Err(PortLeaseOperationError::BindClaimConflict {
                                lease_id: request.lease_id().clone(),
                            });
                        }
                    }
                    PortLeasePhase::Reserved
                        if record.active_lifetime.is_none()
                            && record.last_lifetime_generation
                                == recovery.lifetime.generation.as_u64()
                            && record.bind_claim.is_none()
                            && record.binding.is_none()
                            && record.adoption_claim.is_none()
                            && record.failure.is_none() => {}
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::PrepareRebindAfterConfirmedStop,
                        });
                    }
                }
            }
            for request in requests {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::CleanupPending {
                    record.phase = PortLeasePhase::Reserved;
                    record.bind_claim = None;
                    record.active_lifetime = None;
                }
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Atomically release a live provider-managed batch after exact stop.
    ///
    /// The non-cloneable guards authenticate the process generation whose
    /// provider effects the adapter stopped. Binding and adoption evidence
    /// remain immutable terminal audit data; only active lifetime authority is
    /// cleared. Exact terminal replay is idempotent.
    pub fn release_provider_managed_batch_after_confirmed_stop_with_lifetimes(
        &self,
        bindings: &[(PortLeaseRequest, PortLeaseBinding)],
        lifetimes: &[PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        if bindings.len() != lifetimes.len() {
            let lease_id = bindings
                .first()
                .map(|(request, _)| request.lease_id().clone())
                .or_else(|| {
                    lifetimes
                        .first()
                        .map(|lifetime| lifetime.request().lease_id().clone())
                })
                .ok_or_else(|| PortLeaseError::CorruptAuthority {
                    reason: "empty confirmed-stop lifetime batch has divergent lengths".to_owned(),
                })?;
            return Err(PortLeaseError::LifetimeMismatch { lease_id });
        }

        let mut required = BTreeMap::new();
        for lifetime in lifetimes {
            if lifetime.lifetime().effect_scope != PortLeaseEffectScope::ProviderManaged {
                return Err(PortLeaseError::LifetimeScopeMismatch {
                    lease_id: lifetime.request().lease_id().clone(),
                });
            }
            if required
                .insert(
                    lifetime.request().lease_id().clone(),
                    (lifetime.request(), lifetime.lifetime()),
                )
                .is_some()
            {
                return Err(PortLeaseError::IdentityConflict {
                    lease_id: lifetime.request().lease_id().clone(),
                });
            }
        }

        let mut distinct = BTreeMap::<PortLeaseId, (&PortLeaseRequest, &PortLeaseBinding)>::new();
        for (request, expected_binding) in bindings {
            if distinct
                .insert(request.lease_id().clone(), (request, expected_binding))
                .is_some()
            {
                return Err(PortLeaseError::IdentityConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
            let Some((guard_request, _)) = required.get(request.lease_id()) else {
                return Err(PortLeaseError::LifetimeMismatch {
                    lease_id: request.lease_id().clone(),
                });
            };
            if *guard_request != request {
                return Err(PortLeaseError::LifetimeMismatch {
                    lease_id: request.lease_id().clone(),
                });
            }
        }

        self.transaction(|state| {
            let requested = bindings
                .iter()
                .map(|(request, _)| request)
                .collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &requested)?;
            for (request, expected_binding) in distinct.values().copied() {
                let lifetime = required[request.lease_id()].1;
                let record = exact_record(state, request)?;
                if let Some(mismatch) = expected_binding.mismatch(request.binding()) {
                    return Err(PortLeaseOperationError::BindingMismatch {
                        lease_id: request.lease_id().clone(),
                        mismatch,
                    });
                }
                if record.reserved_port != Some(expected_binding.actual_port()) {
                    return Err(PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                match record.phase {
                    PortLeasePhase::Active | PortLeasePhase::Withdrawing
                        if record.binding.as_ref() == Some(expected_binding)
                            && record.bind_claim.is_none()
                            && record.active_lifetime == Some(lifetime) => {}
                    PortLeasePhase::Released
                        if record.binding.as_ref() == Some(expected_binding)
                            && record.bind_claim.is_none()
                            && record.confirmed_stopped_binding.is_none()
                            && record.failure.is_none()
                            && record.active_lifetime.is_none()
                            && record.last_lifetime_generation
                                == lifetime.generation().as_u64() => {}
                    PortLeasePhase::Active
                    | PortLeasePhase::Withdrawing
                    | PortLeasePhase::Released
                        if record.binding.as_ref() == Some(expected_binding) =>
                    {
                        return Err(PortLeaseOperationError::LifetimeMismatch {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::ReleaseAfterConfirmedStop,
                        });
                    }
                }
            }

            for (request, _) in distinct.into_values() {
                let record = exact_record_mut(state, request)?;
                if matches!(
                    record.phase,
                    PortLeasePhase::Active | PortLeasePhase::Withdrawing
                ) {
                    record.phase = PortLeasePhase::Released;
                    record.reservation_claim = None;
                    record.bind_claim = None;
                    record.confirmed_stopped_binding = None;
                    record.failure = None;
                    record.active_lifetime = None;
                }
            }
            bindings
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Release a provider-managed batch after its adapter proves exact absence.
    pub fn release_provider_managed_batch_after_confirmed_stop(
        &self,
        requests: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let recoveries = exact_recovery_batch(requests, recoveries)?;
        self.transaction(|state| {
            let requested = requests.iter().collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &requested)?;
            for request in requests {
                let recovery = recoveries[request.lease_id()];
                if recovery.lifetime.effect_scope != PortLeaseEffectScope::ProviderManaged {
                    return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                match record.phase {
                    PortLeasePhase::CleanupPending => {
                        authenticate_recovery(record, request, recovery)?;
                    }
                    PortLeasePhase::Released
                        if record.active_lifetime.is_none()
                            && record.last_lifetime_generation
                                == recovery.lifetime.generation.as_u64() => {}
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::ReleaseAfterOwnerDeath,
                        });
                    }
                }
            }
            for request in requests {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::CleanupPending {
                    record.phase = PortLeasePhase::Released;
                    record.bind_claim = None;
                    record.confirmed_stopped_binding = None;
                    record.failure = None;
                    record.active_lifetime = None;
                }
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Release an exact process-bound effect after its owner lifetime died.
    ///
    /// Provider-managed effects are rejected because coordinator death cannot
    /// prove their absence.
    pub fn release_process_bound_after_owner_death(
        &self,
        request: &PortLeaseRequest,
        recovery: &PortLeaseRecoveryGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            authenticate_scalar_plan_if_present(state, request)?;
            let record = exact_record_mut(state, request)?;
            if record.phase == PortLeasePhase::Released
                && record.active_lifetime.is_none()
                && record.last_lifetime_generation == recovery.lifetime.generation.as_u64()
                && recovery.request == *request
            {
                return Ok(record.clone());
            }
            authenticate_recovery(record, request, recovery)?;
            if recovery.lifetime.effect_scope != PortLeaseEffectScope::ProcessBound {
                return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                    lease_id: request.lease_id().clone(),
                });
            }
            if record.phase != PortLeasePhase::CleanupPending {
                return Err(PortLeaseOperationError::InvalidTransition {
                    lease_id: request.lease_id().clone(),
                    phase: record.phase,
                    operation: PortLeaseOperation::ReleaseAfterOwnerDeath,
                });
            }
            record.phase = PortLeasePhase::Released;
            record.reservation_claim = None;
            record.bind_claim = None;
            record.adoption_claim = None;
            record.binding = None;
            record.confirmed_stopped_binding = None;
            record.failure = None;
            record.active_lifetime = None;
            Ok(record.clone())
        })
    }

    /// Retain the exact numeric slot for rebind after a process-bound owner
    /// died.
    ///
    /// The dead lifetime lock is the provider-absence proof. The prior binding
    /// becomes a durable confirmed-stop receipt while all mutable bind and
    /// lifetime authority is cleared, so the same immutable request can start
    /// one higher lifetime generation through the normal bind path.
    pub fn prepare_rebind_process_bound_after_owner_death(
        &self,
        request: &PortLeaseRequest,
        recovery: &PortLeaseRecoveryGuard,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            authenticate_scalar_plan_if_present(state, request)?;
            let record = exact_record_mut(state, request)?;
            if record.phase == PortLeasePhase::Reserved
                && record.active_lifetime.is_none()
                && record.last_lifetime_generation == recovery.lifetime.generation.as_u64()
                && record.confirmed_stopped_binding.is_some()
                && recovery.request == *request
            {
                return Ok(record.clone());
            }
            authenticate_recovery(record, request, recovery)?;
            if recovery.lifetime.effect_scope != PortLeaseEffectScope::ProcessBound {
                return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                    lease_id: request.lease_id().clone(),
                });
            }
            if record.phase != PortLeasePhase::CleanupPending {
                return Err(PortLeaseOperationError::InvalidTransition {
                    lease_id: request.lease_id().clone(),
                    phase: record.phase,
                    operation: PortLeaseOperation::PrepareRebindAfterOwnerDeath,
                });
            }
            let binding =
                record
                    .binding
                    .take()
                    .ok_or_else(|| PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id().clone(),
                    })?;
            record.phase = PortLeasePhase::Reserved;
            record.bind_claim = None;
            record.adoption_claim = None;
            record.confirmed_stopped_binding = Some(binding);
            record.failure = None;
            record.active_lifetime = None;
            Ok(record.clone())
        })
    }

    fn exact_lifetime_record(
        &self,
        request: &PortLeaseRequest,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.transaction(|state| {
            authenticate_scalar_plan_if_present(state, request)?;
            exact_record(state, request).cloned()
        })
    }

    fn try_acquire_lifetime_lock(
        &self,
        lease_id: &PortLeaseId,
    ) -> Result<LifetimeLockAttempt, PortLeaseError> {
        let directory = self
            .store
            .state_root()
            .join("networks")
            .join("control-plane")
            .join(LIFETIME_LOCK_DIRECTORY);
        create_dir_all_owner_only(&directory).map_err(PortLeaseError::Store)?;
        let path = directory.join(format!("{}.lock", lease_id.as_str()));
        let file = open_owner_file(&path, false).map_err(PortLeaseError::Store)?;
        match file.try_lock_exclusive() {
            Ok(()) => Ok(LifetimeLockAttempt::Acquired(LifetimeFileGuard {
                _file: file,
            })),
            Err(source) if is_lock_contended(&source) => Ok(LifetimeLockAttempt::Contended),
            Err(source) => Err(PortLeaseError::Store(crate::NetworkStateStoreError::Io {
                operation: "acquire port-lease lifetime lock",
                path,
                source,
            })),
        }
    }
}

fn exact_lifetime_batch(
    claims: &[(PortLeaseRequest, PortBindClaim)],
    lifetimes: &[PortLeaseLifetimeGuard],
) -> Result<BTreeMap<PortLeaseId, PortLeaseLifetime>, PortLeaseError> {
    if claims.len() != lifetimes.len() {
        let lease_id = claims
            .first()
            .map(|(request, _)| request.lease_id().clone())
            .or_else(|| {
                lifetimes
                    .first()
                    .map(|lifetime| lifetime.request.lease_id().clone())
            })
            .ok_or_else(|| PortLeaseError::CorruptAuthority {
                reason: "empty lifetime batch has divergent lengths".to_owned(),
            })?;
        return Err(PortLeaseError::LifetimeMismatch { lease_id });
    }

    let mut required_lifetimes = BTreeMap::new();
    for lifetime in lifetimes {
        if required_lifetimes
            .insert(
                lifetime.request.lease_id().clone(),
                (lifetime.request(), lifetime.lifetime()),
            )
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: lifetime.request.lease_id().clone(),
            });
        }
    }
    let mut distinct_claims = BTreeMap::new();
    for (request, _) in claims {
        if distinct_claims
            .insert(request.lease_id().clone(), request)
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: request.lease_id().clone(),
            });
        }
        let Some((guard_request, _)) = required_lifetimes.get(request.lease_id()) else {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        };
        if *guard_request != request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
    }

    Ok(required_lifetimes
        .into_iter()
        .map(|(lease_id, (_, lifetime))| (lease_id, lifetime))
        .collect())
}

fn exact_recovery_batch<'a>(
    requests: &[PortLeaseRequest],
    recoveries: &'a [PortLeaseRecoveryGuard],
) -> Result<BTreeMap<PortLeaseId, &'a PortLeaseRecoveryGuard>, PortLeaseError> {
    if requests.len() != recoveries.len() {
        let lease_id = requests
            .first()
            .map(|request| request.lease_id().clone())
            .or_else(|| {
                recoveries
                    .first()
                    .map(|recovery| recovery.request.lease_id().clone())
            })
            .ok_or_else(|| PortLeaseError::CorruptAuthority {
                reason: "empty recovery batch has divergent lengths".to_owned(),
            })?;
        return Err(PortLeaseError::LifetimeMismatch { lease_id });
    }

    let mut exact = BTreeMap::new();
    for recovery in recoveries {
        if exact
            .insert(recovery.request.lease_id().clone(), recovery)
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: recovery.request.lease_id().clone(),
            });
        }
    }
    let mut distinct_requests = BTreeMap::new();
    for request in requests {
        if distinct_requests
            .insert(request.lease_id().clone(), request)
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: request.lease_id().clone(),
            });
        }
        let Some(recovery) = exact.get(request.lease_id()) else {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        };
        if recovery.request != *request {
            return Err(PortLeaseError::LifetimeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
    }
    Ok(exact)
}

fn authenticate_recovery(
    record: &PortLeaseRecord,
    request: &PortLeaseRequest,
    recovery: &PortLeaseRecoveryGuard,
) -> Result<(), PortLeaseOperationError> {
    if recovery.request != *request || record.active_lifetime != Some(recovery.lifetime) {
        return Err(PortLeaseOperationError::LifetimeMismatch {
            lease_id: request.lease_id().clone(),
        });
    }
    Ok(())
}

fn advance_lifetime(
    record: &mut PortLeaseRecord,
    request: &PortLeaseRequest,
    effect_scope: PortLeaseEffectScope,
) -> Result<PortLeaseLifetime, PortLeaseOperationError> {
    let next = record
        .last_lifetime_generation
        .checked_add(1)
        .ok_or_else(|| PortLeaseOperationError::LifetimeGenerationExhausted {
            lease_id: request.lease_id().clone(),
        })?;
    let lifetime = PortLeaseLifetime {
        generation: PortLeaseLifetimeGeneration(next),
        effect_scope,
    };
    record.last_lifetime_generation = next;
    record.active_lifetime = Some(lifetime);
    Ok(lifetime)
}

#[cfg(test)]
#[path = "lifetime/tests.rs"]
mod tests;
