//! First-bind claims under a live process lifetime.
//!
//! A claim fences the provider effect before it starts. The same lifetime then
//! adopts the effect into `Active`, or abandons the claim when no effect ran.

use std::collections::BTreeMap;

use super::super::plan_batch::{
    authenticate_complete_plan_batch_if_present, authenticate_complete_plan_member,
    authenticate_complete_plan_members, authenticate_scalar_plan_if_present,
};
use super::{
    LifetimeLockAttempt, LocalPortLeaseAuthority, PortBindClaim, PortLeaseEffectScope,
    PortLeaseError, PortLeaseLifetimeGuard, PortLeaseOperation, PortLeaseOperationError,
    PortLeasePhase, PortLeaseRecord, PortLeaseRequest, PortLeaseReservationWithLifetime,
    advance_lifetime, exact_lifetime_batch, exact_record, exact_record_mut,
    require_reservation_claim,
};
use crate::{NetworkReservationClaim, PortLeaseBinding, PortLeaseId};

impl LocalPortLeaseAuthority {
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
        binding: PortLeaseBinding,
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
        binding: PortLeaseBinding,
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
}
