//! Rebind claims for retained plan members under a higher lifetime.
//!
//! The confirmed-stop binding is the durable receipt for the prior effect. A
//! rebind never reuses the former process generation.

use std::collections::BTreeMap;

use super::super::plan_batch::{
    authenticate_complete_plan_member, authenticate_complete_plan_members,
};
use super::{
    LifetimeLockAttempt, LocalPortLeaseAuthority, PortBindClaim, PortLeaseEffectScope,
    PortLeaseError, PortLeaseLifetimeGuard, PortLeaseOperation, PortLeaseOperationError,
    PortLeasePhase, PortLeaseRecord, PortLeaseRequest, advance_lifetime, exact_lifetime_batch,
    exact_record, exact_record_mut,
};
use crate::PortLeaseBinding;

impl LocalPortLeaseAuthority {
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
}
