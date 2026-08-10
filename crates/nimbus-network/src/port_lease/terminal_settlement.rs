//! Atomic terminal settlement for effect-free and process-bound plan members.
//!
//! The effect owner supplies the complete immutable plan as an authentication
//! witness and only the listener members whose process-bound effects it owns.
//! These transitions change durable lease authority only. They never stop a
//! provider, close a socket, or infer effect absence from an address.

use std::collections::BTreeMap;

use super::plan_batch::{
    authenticate_complete_plan_batch_if_present, authenticate_complete_plan_members,
};
use super::{
    LocalPortLeaseAuthority, NetworkReservationLifetimeAttempt, PortLeaseBinding,
    PortLeaseEffectScope, PortLeaseError, PortLeaseLifetime, PortLeaseLifetimeGuard,
    PortLeaseOperation, PortLeaseOperationError, PortLeasePhase, PortLeaseRecord,
    PortLeaseRecoveryGuard, PortLeaseRequest, exact_record, exact_record_mut,
};
use crate::{NetworkReservationClaim, NetworkReservationLifetimeGuard, PortLeaseId};

#[derive(Clone, Copy, PartialEq, Eq)]
enum TerminalSelectionMode {
    Active,
    Withdrawing,
    CleanupPending,
    Released,
}

impl LocalPortLeaseAuthority {
    /// Atomically release a batch that is still proven never bound.
    ///
    /// Every distinct request is authenticated before any record changes. A
    /// concurrent provider adoption makes the complete compensation fail.
    pub fn release_reserved_batch_without_effect(
        &self,
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.release_reserved_without_effect(requests, reservation_claim, None)
    }

    /// Release a never-bound subset under its complete immutable plan witness.
    pub fn release_reserved_plan_members_without_effect(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.release_reserved_without_effect(requests, reservation_claim, Some(plan_members))
    }

    fn release_reserved_without_effect(
        &self,
        requests: &[PortLeaseRequest],
        reservation_claim: &NetworkReservationClaim,
        plan_members: Option<&[PortLeaseRequest]>,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        if requests.is_empty() {
            return Ok(Vec::new());
        }
        let lifetime = match self.try_acquire_reservation_lifetime(reservation_claim)? {
            NetworkReservationLifetimeAttempt::Acquired(lifetime) => lifetime,
            NetworkReservationLifetimeAttempt::LiveOwner => {
                return Err(PortLeaseError::ReservationLifetimeOwnerLive {
                    provider_id: reservation_claim
                        .coordinator_attempt()
                        .provider_id()
                        .clone(),
                });
            }
        };
        self.release_reserved_without_effect_with_lifetime(requests, &lifetime, plan_members)
    }

    /// Release a never-bound batch under its exact live reservation lifetime.
    pub fn release_reserved_batch_without_effect_with_lifetime(
        &self,
        requests: &[PortLeaseRequest],
        lifetime: &NetworkReservationLifetimeGuard,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.release_reserved_without_effect_with_lifetime(requests, lifetime, None)
    }

    /// Release a live-coordinator subset under its complete plan witness.
    pub fn release_reserved_plan_members_without_effect_with_lifetime(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
        lifetime: &NetworkReservationLifetimeGuard,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.release_reserved_without_effect_with_lifetime(requests, lifetime, Some(plan_members))
    }

    fn release_reserved_without_effect_with_lifetime(
        &self,
        requests: &[PortLeaseRequest],
        lifetime: &NetworkReservationLifetimeGuard,
        plan_members: Option<&[PortLeaseRequest]>,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let reservation_claim = lifetime.claim();
        self.transaction(|state| {
            let selected = requests.iter().collect::<Vec<_>>();
            match plan_members {
                Some(plan_members) => {
                    let witness = plan_members.iter().collect::<Vec<_>>();
                    authenticate_complete_plan_members(state, &witness, &selected)?;
                }
                None => authenticate_complete_plan_batch_if_present(state, &selected)?,
            }
            for request in requests {
                let record = exact_record(state, request)?;
                if record.reservation_claim.as_ref() != Some(reservation_claim) {
                    return Err(PortLeaseOperationError::ReservationClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if record.bind_claim.is_some() {
                    return Err(PortLeaseOperationError::BindClaimConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                if !matches!(
                    record.phase,
                    PortLeasePhase::Reserved | PortLeasePhase::Released | PortLeasePhase::Failed
                ) {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::ReleaseReservedWithoutEffect,
                    });
                }
            }
            let distinct = requests
                .iter()
                .map(|request| (request.lease_id().clone(), request))
                .collect::<BTreeMap<_, _>>();
            for request in distinct.into_values() {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::Reserved {
                    record.phase = PortLeasePhase::Released;
                    record.active_lifetime = None;
                }
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Atomically fence a live process-bound plan subset before local stop.
    ///
    /// `plan_members` is the complete immutable plan witness. `bindings` and
    /// `lifetimes` identify one non-empty subset whose effects are owned by the
    /// caller. Exact replay keeps every selected member `Withdrawing`.
    pub fn withdraw_process_bound_plan_members_with_lifetimes(
        &self,
        plan_members: &[PortLeaseRequest],
        bindings: &[(PortLeaseRequest, PortLeaseBinding)],
        lifetimes: &[&PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let required = exact_process_bound_lifetimes(bindings, lifetimes)?;
        self.transaction(|state| {
            let selected = bindings
                .iter()
                .map(|(request, _)| request)
                .collect::<Vec<_>>();
            let witness = plan_members.iter().collect::<Vec<_>>();
            authenticate_complete_plan_members(state, &witness, &selected)?;

            let mut mode = None;
            for (request, expected_binding) in bindings {
                let lifetime = required[request.lease_id()];
                let record = exact_record(state, request)?;
                authenticate_live_binding(record, request, expected_binding, lifetime)?;
                let candidate = match record.phase {
                    PortLeasePhase::Active => TerminalSelectionMode::Active,
                    PortLeasePhase::Withdrawing => TerminalSelectionMode::Withdrawing,
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::Withdraw,
                        });
                    }
                };
                require_uniform_mode(
                    &mut mode,
                    candidate,
                    request,
                    record.phase,
                    PortLeaseOperation::Withdraw,
                )?;
            }

            for (request, _) in bindings {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::Active {
                    record.phase = PortLeasePhase::Withdrawing;
                }
            }
            bindings
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Atomically release a stopped live process-bound plan subset.
    ///
    /// The caller must retain the same binding and non-cloneable lifetime
    /// evidence used for withdrawal. Every selected member must be uniformly
    /// `Withdrawing`, or uniformly `Released` for exact terminal replay.
    pub fn release_process_bound_plan_members_after_confirmed_stop_with_lifetimes(
        &self,
        plan_members: &[PortLeaseRequest],
        bindings: &[(PortLeaseRequest, PortLeaseBinding)],
        lifetimes: &[&PortLeaseLifetimeGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let required = exact_process_bound_lifetimes(bindings, lifetimes)?;
        self.transaction(|state| {
            let selected = bindings
                .iter()
                .map(|(request, _)| request)
                .collect::<Vec<_>>();
            let witness = plan_members.iter().collect::<Vec<_>>();
            authenticate_complete_plan_members(state, &witness, &selected)?;

            let mut mode = None;
            for (request, expected_binding) in bindings {
                let lifetime = required[request.lease_id()];
                let record = exact_record(state, request)?;
                let candidate = match record.phase {
                    PortLeasePhase::Withdrawing => {
                        authenticate_live_binding(record, request, expected_binding, lifetime)?;
                        TerminalSelectionMode::Withdrawing
                    }
                    PortLeasePhase::Released => {
                        authenticate_terminal_binding(record, request, expected_binding, lifetime)?;
                        TerminalSelectionMode::Released
                    }
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::ReleaseAfterConfirmedStop,
                        });
                    }
                };
                require_uniform_mode(
                    &mut mode,
                    candidate,
                    request,
                    record.phase,
                    PortLeaseOperation::ReleaseAfterConfirmedStop,
                )?;
            }

            for (request, _) in bindings {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::Withdrawing {
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

    /// Atomically release an exact process-bound plan subset after owner death.
    ///
    /// The recovery guards prove that every selected process generation is
    /// dead. Selected members must be uniformly `Active`, `Withdrawing`, or
    /// `CleanupPending`; an exact uniformly terminal batch is an idempotent
    /// replay. Unrelated plan members remain unchanged.
    pub fn release_process_bound_plan_members_after_owner_death(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let required = exact_process_bound_recoveries(requests, recoveries)?;
        self.transaction(|state| {
            let selected = requests.iter().collect::<Vec<_>>();
            let witness = plan_members.iter().collect::<Vec<_>>();
            authenticate_complete_plan_members(state, &witness, &selected)?;

            let mut mode = None;
            for request in requests {
                let recovery = required[request.lease_id()];
                let record = exact_record(state, request)?;
                let candidate = match record.phase {
                    PortLeasePhase::Active => TerminalSelectionMode::Active,
                    PortLeasePhase::Withdrawing => TerminalSelectionMode::Withdrawing,
                    PortLeasePhase::CleanupPending => TerminalSelectionMode::CleanupPending,
                    PortLeasePhase::Released => TerminalSelectionMode::Released,
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::ReleaseAfterOwnerDeath,
                        });
                    }
                };
                match candidate {
                    TerminalSelectionMode::Released => {
                        authenticate_terminal_recovery(record, request, recovery)?;
                    }
                    _ => authenticate_live_recovery(record, request, recovery)?,
                }
                require_uniform_mode(
                    &mut mode,
                    candidate,
                    request,
                    record.phase,
                    PortLeaseOperation::ReleaseAfterOwnerDeath,
                )?;
            }

            for request in requests {
                let record = exact_record_mut(state, request)?;
                if record.phase != PortLeasePhase::Released {
                    record.phase = PortLeasePhase::Released;
                    record.reservation_claim = None;
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
}

fn exact_process_bound_lifetimes(
    bindings: &[(PortLeaseRequest, PortLeaseBinding)],
    lifetimes: &[&PortLeaseLifetimeGuard],
) -> Result<BTreeMap<PortLeaseId, PortLeaseLifetime>, PortLeaseError> {
    let Some((first_request, _)) = bindings.first() else {
        return Err(PortLeaseError::CorruptAuthority {
            reason: "process-bound terminal subset cannot be empty".to_owned(),
        });
    };
    if bindings.len() != lifetimes.len() {
        return Err(PortLeaseError::LifetimeMismatch {
            lease_id: first_request.lease_id().clone(),
        });
    }
    let mut selected = BTreeMap::new();
    for (request, _) in bindings {
        if selected
            .insert(request.lease_id().clone(), request)
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: request.lease_id().clone(),
            });
        }
    }
    let mut required = BTreeMap::new();
    for lifetime in lifetimes {
        let request = lifetime.request();
        if lifetime.lifetime().effect_scope() != PortLeaseEffectScope::ProcessBound {
            return Err(PortLeaseError::LifetimeScopeMismatch {
                lease_id: request.lease_id().clone(),
            });
        }
        if required
            .insert(request.lease_id().clone(), lifetime.lifetime())
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: request.lease_id().clone(),
            });
        }
    }
    for (lease_id, request) in selected {
        if lifetimes
            .iter()
            .find(|lifetime| lifetime.request().lease_id() == &lease_id)
            .is_none_or(|lifetime| lifetime.request() != request)
        {
            return Err(PortLeaseError::LifetimeMismatch { lease_id });
        }
    }
    Ok(required)
}

fn exact_process_bound_recoveries<'a>(
    requests: &[PortLeaseRequest],
    recoveries: &'a [PortLeaseRecoveryGuard],
) -> Result<BTreeMap<PortLeaseId, &'a PortLeaseRecoveryGuard>, PortLeaseError> {
    let Some(first_request) = requests.first() else {
        return Err(PortLeaseError::CorruptAuthority {
            reason: "process-bound dead-owner terminal subset cannot be empty".to_owned(),
        });
    };
    if requests.len() != recoveries.len() {
        return Err(PortLeaseError::LifetimeMismatch {
            lease_id: first_request.lease_id().clone(),
        });
    }
    let mut selected = BTreeMap::new();
    for request in requests {
        if selected
            .insert(request.lease_id().clone(), request)
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: request.lease_id().clone(),
            });
        }
    }
    let mut required = BTreeMap::new();
    for recovery in recoveries {
        if recovery.lifetime().effect_scope() != PortLeaseEffectScope::ProcessBound {
            return Err(PortLeaseError::LifetimeScopeMismatch {
                lease_id: recovery.request().lease_id().clone(),
            });
        }
        if required
            .insert(recovery.request().lease_id().clone(), recovery)
            .is_some()
        {
            return Err(PortLeaseError::IdentityConflict {
                lease_id: recovery.request().lease_id().clone(),
            });
        }
    }
    for (lease_id, request) in selected {
        if required
            .get(&lease_id)
            .is_none_or(|recovery| recovery.request() != request)
        {
            return Err(PortLeaseError::LifetimeMismatch { lease_id });
        }
    }
    Ok(required)
}

fn authenticate_live_binding(
    record: &PortLeaseRecord,
    request: &PortLeaseRequest,
    expected_binding: &PortLeaseBinding,
    lifetime: PortLeaseLifetime,
) -> Result<(), PortLeaseOperationError> {
    if let Some(mismatch) = expected_binding.mismatch(request.binding()) {
        return Err(PortLeaseOperationError::BindingMismatch {
            lease_id: request.lease_id().clone(),
            mismatch,
        });
    }
    if record.binding.as_ref() != Some(expected_binding)
        || record.reserved_port != Some(expected_binding.actual_port())
        || record.adoption_claim.is_none()
        || record.bind_claim.is_some()
        || record.confirmed_stopped_binding.is_some()
        || record.failure.is_some()
    {
        return Err(PortLeaseOperationError::BindingConflict {
            lease_id: request.lease_id().clone(),
        });
    }
    if record.active_lifetime != Some(lifetime) {
        return Err(PortLeaseOperationError::LifetimeMismatch {
            lease_id: request.lease_id().clone(),
        });
    }
    Ok(())
}

fn authenticate_terminal_binding(
    record: &PortLeaseRecord,
    request: &PortLeaseRequest,
    expected_binding: &PortLeaseBinding,
    lifetime: PortLeaseLifetime,
) -> Result<(), PortLeaseOperationError> {
    if let Some(mismatch) = expected_binding.mismatch(request.binding()) {
        return Err(PortLeaseOperationError::BindingMismatch {
            lease_id: request.lease_id().clone(),
            mismatch,
        });
    }
    if record.binding.as_ref() != Some(expected_binding)
        || record.reserved_port != Some(expected_binding.actual_port())
        || record.adoption_claim.is_none()
        || record.bind_claim.is_some()
        || record.reservation_claim.is_some()
        || record.confirmed_stopped_binding.is_some()
        || record.failure.is_some()
    {
        return Err(PortLeaseOperationError::BindingConflict {
            lease_id: request.lease_id().clone(),
        });
    }
    if record.active_lifetime.is_some()
        || record.last_lifetime_generation != lifetime.generation().as_u64()
    {
        return Err(PortLeaseOperationError::LifetimeMismatch {
            lease_id: request.lease_id().clone(),
        });
    }
    Ok(())
}

fn authenticate_live_recovery(
    record: &PortLeaseRecord,
    request: &PortLeaseRequest,
    recovery: &PortLeaseRecoveryGuard,
) -> Result<(), PortLeaseOperationError> {
    if recovery.request() != request || record.active_lifetime != Some(recovery.lifetime()) {
        return Err(PortLeaseOperationError::LifetimeMismatch {
            lease_id: request.lease_id().clone(),
        });
    }
    if record.binding.is_none()
        || record.adoption_claim.is_none()
        || record.bind_claim.is_some()
        || record.confirmed_stopped_binding.is_some()
        || record.failure.is_some()
    {
        return Err(PortLeaseOperationError::BindingConflict {
            lease_id: request.lease_id().clone(),
        });
    }
    Ok(())
}

fn authenticate_terminal_recovery(
    record: &PortLeaseRecord,
    request: &PortLeaseRequest,
    recovery: &PortLeaseRecoveryGuard,
) -> Result<(), PortLeaseOperationError> {
    if recovery.request() != request
        || record.active_lifetime.is_some()
        || record.last_lifetime_generation != recovery.lifetime().generation().as_u64()
    {
        return Err(PortLeaseOperationError::LifetimeMismatch {
            lease_id: request.lease_id().clone(),
        });
    }
    if record.binding.is_none()
        || record.adoption_claim.is_none()
        || record.bind_claim.is_some()
        || record.reservation_claim.is_some()
        || record.confirmed_stopped_binding.is_some()
        || record.failure.is_some()
    {
        return Err(PortLeaseOperationError::BindingConflict {
            lease_id: request.lease_id().clone(),
        });
    }
    Ok(())
}

fn require_uniform_mode(
    mode: &mut Option<TerminalSelectionMode>,
    candidate: TerminalSelectionMode,
    request: &PortLeaseRequest,
    phase: PortLeasePhase,
    operation: PortLeaseOperation,
) -> Result<(), PortLeaseOperationError> {
    if mode.is_some_and(|current| current != candidate) {
        return Err(PortLeaseOperationError::InvalidTransition {
            lease_id: request.lease_id().clone(),
            phase,
            operation,
        });
    }
    *mode = Some(candidate);
    Ok(())
}

#[cfg(test)]
#[path = "terminal_settlement/tests.rs"]
mod tests;
