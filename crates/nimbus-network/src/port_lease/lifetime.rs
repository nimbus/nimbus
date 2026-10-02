//! Process-lifetime fencing for crash-safe port-effect reconciliation.
//!
//! The lock proves only whether the Nimbus process generation that owned an
//! effect is still live. Provider-specific inspection remains in the adapter
//! that owns the socket, subprocess, or external service.

use std::collections::BTreeMap;
use std::fmt;
use std::fs::File;

use serde::{Deserialize, Serialize};

use super::plan_batch::{authenticate_complete_plan_members, authenticate_scalar_plan_if_present};
use super::{
    LocalPortLeaseAuthority, PortBindClaim, PortLeaseError, PortLeaseOperation,
    PortLeaseOperationError, PortLeasePhase, PortLeaseRecord, PortLeaseRequest, exact_record,
    exact_record_mut, require_reservation_claim,
};
use crate::state_store::{create_dir_all_owner_only, is_lock_contended, open_owner_file};
use crate::{NetworkReservationClaim, PortLeaseId};

mod batch_reservation;
mod bind_claim;
mod confirmed_stop;
mod owner_death_recovery;
mod rebind_claim;
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
        let file = open_owner_file(&path).map_err(PortLeaseError::Store)?;
        match file.try_lock().map_err(std::io::Error::from) {
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
