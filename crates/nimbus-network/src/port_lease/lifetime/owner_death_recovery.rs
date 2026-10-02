//! Reconciliation of leases whose process owner died.
//!
//! Recovery acquires the dead owner's exclusive lifetime lock. It never probes
//! sockets or providers. Provider-managed records stay fenced for their adapter.

use std::collections::BTreeMap;

use super::super::plan_batch::{
    authenticate_complete_plan_batch_if_present, authenticate_complete_plan_members,
    authenticate_scalar_plan_if_present,
};
use super::{
    LifetimeLockAttempt, LocalPortLeaseAuthority, PortLeaseEffectScope, PortLeaseError,
    PortLeaseLifetimeGuard, PortLeaseLifetimeReconciliation, PortLeaseOperation,
    PortLeaseOperationError, PortLeasePhase, PortLeaseRecord, PortLeaseRecoveryAttempt,
    PortLeaseRecoveryGuard, PortLeaseRequest, advance_lifetime, authenticate_recovery,
    exact_plan_recoveries, exact_record, exact_record_mut, exact_recovery_batch,
};
use crate::PortLeaseBinding;

impl LocalPortLeaseAuthority {
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
                    PortLeasePhase::Active
                        | PortLeasePhase::Withdrawing
                        | PortLeasePhase::CleanupPending
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
                        PortLeasePhase::Active
                            | PortLeasePhase::Withdrawing
                            | PortLeasePhase::CleanupPending
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
}
