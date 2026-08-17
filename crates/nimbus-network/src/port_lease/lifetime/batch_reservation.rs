//! Atomic direct-provider batch reservation and lifetime ownership.

use std::collections::BTreeMap;
use std::fmt;

use super::super::plan_batch::{
    authenticate_complete_plan_batch, authenticate_new_or_exact_plan_batch,
};
use super::{
    LifetimeLockAttempt, LocalPortLeaseAuthority, PortBindClaim, PortLeaseEffectScope,
    PortLeaseError, PortLeaseLifetime, PortLeaseLifetimeGuard, PortLeaseOperation,
    PortLeaseOperationError, PortLeasePhase, PortLeaseRecord, PortLeaseRequest, advance_lifetime,
    exact_lifetime_batch, exact_record, exact_record_mut, require_reservation_claim,
};
use crate::PortLeaseId;

/// One atomic direct-provider batch reservation and its live-owner guards.
///
/// Records and guards retain the caller's input order. The guards are
/// deliberately non-cloneable: callers must keep the complete result alive
/// until the provider effects have been adopted, compensated, or handed to
/// explicit recovery.
pub struct PortLeaseBatchReservationWithLifetimes {
    records: Vec<PortLeaseRecord>,
    lifetimes: Vec<PortLeaseLifetimeGuard>,
}

impl PortLeaseBatchReservationWithLifetimes {
    /// Durable reservations in caller input order.
    pub fn records(&self) -> &[PortLeaseRecord] {
        &self.records
    }

    /// Live-owner guards in caller input order.
    pub fn lifetimes(&self) -> &[PortLeaseLifetimeGuard] {
        &self.lifetimes
    }

    /// Split ordered durable records from their ordered non-cloneable guards.
    pub fn into_parts(self) -> (Vec<PortLeaseRecord>, Vec<PortLeaseLifetimeGuard>) {
        (self.records, self.lifetimes)
    }
}

impl fmt::Debug for PortLeaseBatchReservationWithLifetimes {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PortLeaseBatchReservationWithLifetimes")
            .field("records", &self.records)
            .field("lifetimes", &self.lifetimes)
            .finish()
    }
}

impl LocalPortLeaseAuthority {
    /// Atomically reserve and claim one complete provider-managed plan batch.
    ///
    /// The input must contain distinct lease identities. Lifetime locks are
    /// acquired in stable lease-ID order before entering the one durable
    /// transaction. Reservation, port-allocation, exact-identity, phase,
    /// bind-claim, active-lifetime, and generation-exhaustion validation all
    /// complete against a staged authority copy before the durable payload is
    /// replaced. Any rejection therefore drops every acquired lock and leaves
    /// every durable record byte-for-byte unchanged.
    ///
    /// This transition deliberately carries no launch reservation coordinator
    /// claim and hard-codes provider-managed recovery semantics. Callers cannot
    /// accidentally downgrade a surviving provider effect to process-bound
    /// cleanup.
    pub fn reserve_and_claim_provider_managed_batch_with_lifetimes(
        &self,
        requests: &[(PortLeaseRequest, PortBindClaim)],
    ) -> Result<PortLeaseBatchReservationWithLifetimes, PortLeaseError> {
        let mut distinct = BTreeMap::<PortLeaseId, (&PortLeaseRequest, &PortBindClaim)>::new();
        for (request, claim) in requests {
            if distinct
                .insert(request.lease_id().clone(), (request, claim))
                .is_some()
            {
                return Err(PortLeaseError::IdentityConflict {
                    lease_id: request.lease_id().clone(),
                });
            }
        }
        let requested = requests
            .iter()
            .map(|(request, _)| request)
            .collect::<Vec<_>>();
        self.transaction(|state| authenticate_new_or_exact_plan_batch(state, &requested))?;

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

        let (records, lifetimes) = self.transaction(|state| {
            authenticate_new_or_exact_plan_batch(state, &requested)?;
            let mut staged = state.clone();
            for (request, _) in requests {
                staged.reserve_request(request.clone(), None)?;
            }
            authenticate_complete_plan_batch(&staged, &requested)?;

            for (request, claim) in distinct.values().copied() {
                let record = exact_record(&staged, request)?;
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

            let mut lifetimes = BTreeMap::<PortLeaseId, PortLeaseLifetime>::new();
            for (request, claim) in distinct.values().copied() {
                let record = exact_record_mut(&mut staged, request)?;
                let lifetime =
                    advance_lifetime(record, request, PortLeaseEffectScope::ProviderManaged)?;
                record.bind_claim = Some(claim.clone());
                lifetimes.insert(request.lease_id().clone(), lifetime);
            }
            let records = requests
                .iter()
                .map(|(request, _)| exact_record(&staged, request).cloned())
                .collect::<Result<Vec<_>, _>>()?;
            *state = staged;
            Ok((records, lifetimes))
        })?;

        let lifetimes = requests
            .iter()
            .map(|(request, _)| {
                let lease_id = request.lease_id();
                PortLeaseLifetimeGuard {
                    request: request.clone(),
                    lifetime: lifetimes[lease_id],
                    _lock: locks
                        .remove(lease_id)
                        .expect("every reserved lifetime owns one acquired lock"),
                }
            })
            .collect();

        Ok(PortLeaseBatchReservationWithLifetimes { records, lifetimes })
    }

    /// Atomically fence a complete live provider-managed plan before stop I/O.
    ///
    /// Active members enter `Withdrawing` while retaining exact binding,
    /// adoption, and lifetime evidence. An unadopted ambiguous-start batch
    /// enters `CleanupPending` while retaining its bind claims and lifetimes.
    /// Mixed modes, caller subsets, stale requests, or mismatched guards reject
    /// the entire transition byte-unchanged.
    pub fn withdraw_provider_managed_batch_with_lifetimes(
        &self,
        requests: &[PortLeaseRequest],
        lifetimes: &[PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let required = exact_request_lifetime_batch(requests, lifetimes)?;
        self.transaction(|state| {
            let requested = requests.iter().collect::<Vec<_>>();
            authenticate_complete_plan_batch(state, &requested)?;
            let mut mode = None;
            for request in requests {
                let lifetime = required[request.lease_id()];
                if lifetime.effect_scope() != PortLeaseEffectScope::ProviderManaged {
                    return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                if record.active_lifetime() != Some(lifetime) {
                    return Err(PortLeaseOperationError::LifetimeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let candidate = match record.phase() {
                    PortLeasePhase::Active | PortLeasePhase::Withdrawing
                        if record.binding().is_some()
                            && record.adoption_claim().is_some()
                            && record.bind_claim().is_none() =>
                    {
                        WithdrawalMode::Adopted
                    }
                    PortLeasePhase::Reserved | PortLeasePhase::CleanupPending
                        if record.binding().is_none()
                            && record.adoption_claim().is_none()
                            && record.bind_claim().is_some() =>
                    {
                        WithdrawalMode::Unadopted
                    }
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::Withdraw,
                        });
                    }
                };
                if mode.is_some_and(|current| current != candidate) {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase(),
                        operation: PortLeaseOperation::Withdraw,
                    });
                }
                mode = Some(candidate);
            }
            for request in requests {
                let record = exact_record_mut(state, request)?;
                match record.phase {
                    PortLeasePhase::Active => record.phase = PortLeasePhase::Withdrawing,
                    PortLeasePhase::Reserved => record.phase = PortLeasePhase::CleanupPending,
                    PortLeasePhase::Withdrawing | PortLeasePhase::CleanupPending => {}
                    _ => unreachable!("every phase was prevalidated"),
                }
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Atomically release a complete unadopted provider-managed claim batch.
    ///
    /// The effect owner calls this after proving no provider API byte was sent
    /// or after the provider confirms exact absence. Exact claims, live guards,
    /// and durable plan membership jointly authenticate the terminal release.
    pub fn release_provider_managed_claim_batch_after_confirmed_absence_with_lifetimes(
        &self,
        claims: &[(PortLeaseRequest, PortBindClaim)],
        lifetimes: &[PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let required = exact_lifetime_batch(claims, lifetimes)?;
        self.transaction(|state| {
            let requested = claims
                .iter()
                .map(|(request, _)| request)
                .collect::<Vec<_>>();
            authenticate_complete_plan_batch(state, &requested)?;
            let mut terminal_replay = None;
            for (request, claim) in claims {
                let lifetime = required[request.lease_id()];
                if lifetime.effect_scope() != PortLeaseEffectScope::ProviderManaged {
                    return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                let is_terminal = match record.phase() {
                    PortLeasePhase::Reserved | PortLeasePhase::CleanupPending
                        if record.bind_claim() == Some(claim)
                            && record.binding().is_none()
                            && record.adoption_claim().is_none()
                            && record.failure().is_none()
                            && record.active_lifetime() == Some(lifetime) =>
                    {
                        false
                    }
                    PortLeasePhase::Released
                        if record.bind_claim().is_none()
                            && record.binding().is_none()
                            && record.adoption_claim().is_none()
                            && record.failure().is_none()
                            && record.active_lifetime().is_none()
                            && record.last_lifetime_generation() == Some(lifetime.generation()) =>
                    {
                        true
                    }
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::ReleaseProviderClaimWithoutEffect,
                        });
                    }
                };
                if terminal_replay.is_some_and(|current| current != is_terminal) {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase(),
                        operation: PortLeaseOperation::ReleaseProviderClaimWithoutEffect,
                    });
                }
                terminal_replay = Some(is_terminal);
            }
            for (request, _) in claims {
                let record = exact_record_mut(state, request)?;
                if !record.phase.is_terminal() {
                    record.phase = PortLeasePhase::Released;
                    record.reservation_claim = None;
                    record.bind_claim = None;
                    record.adoption_claim = None;
                    record.binding = None;
                    record.confirmed_stopped_binding = None;
                    record.failure = None;
                    record.active_lifetime = None;
                }
            }
            claims
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WithdrawalMode {
    Adopted,
    Unadopted,
}

fn exact_request_lifetime_batch(
    requests: &[PortLeaseRequest],
    lifetimes: &[PortLeaseLifetimeGuard],
) -> Result<BTreeMap<PortLeaseId, PortLeaseLifetime>, PortLeaseError> {
    if requests.len() != lifetimes.len() {
        let lease_id = requests
            .first()
            .map(|request| request.lease_id().clone())
            .or_else(|| {
                lifetimes
                    .first()
                    .map(|lifetime| lifetime.request().lease_id().clone())
            })
            .ok_or_else(|| PortLeaseError::CorruptAuthority {
                reason: "empty withdrawal lifetime batch has divergent lengths".to_owned(),
            })?;
        return Err(PortLeaseError::LifetimeMismatch { lease_id });
    }
    let mut exact = BTreeMap::new();
    for lifetime in lifetimes {
        if exact
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
    let mut distinct = BTreeMap::new();
    for request in requests {
        if distinct
            .insert(request.lease_id().clone(), request)
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: request.lease_id().clone(),
            });
        }
        let Some((guard_request, _)) = exact.get(request.lease_id()) else {
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
    Ok(exact
        .into_iter()
        .map(|(lease_id, (_, lifetime))| (lease_id, lifetime))
        .collect())
}

#[cfg(test)]
#[path = "batch_reservation/tests.rs"]
mod tests;
