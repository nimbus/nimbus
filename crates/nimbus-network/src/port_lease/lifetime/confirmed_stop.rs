//! Provider-managed batch transitions after the adapter confirms a stop.
//!
//! The effect adapter owns the stop or absence proof. These transitions retain,
//! release, or prepare a rebind for the exact authenticated batch.

use std::collections::BTreeMap;

use super::super::plan_batch::{
    authenticate_complete_plan_batch_if_present, authenticate_complete_plan_members,
};
use super::{
    LocalPortLeaseAuthority, PortLeaseEffectScope, PortLeaseError, PortLeaseLifetimeGuard,
    PortLeaseOperation, PortLeaseOperationError, PortLeasePhase, PortLeaseRecord,
    PortLeaseRecoveryGuard, PortLeaseRequest, authenticate_recovery, exact_plan_recoveries,
    exact_record, exact_record_mut, exact_recovery_batch,
};
use crate::{PortLeaseBinding, PortLeaseId};

impl LocalPortLeaseAuthority {
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

    /// Retain a complete provider-managed batch after exact provider absence.
    ///
    /// The effect adapter owns the absence proof. Recovery guards authenticate
    /// the dead process generation. This transition clears only live process
    /// authority: the complete batch remains `CleanupPending`, non-bindable,
    /// and auditable until the workload teardown reaches terminal release.
    pub fn retain_provider_managed_batch_after_confirmed_absence(
        &self,
        requests: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let recoveries = exact_recovery_batch(requests, recoveries)?;
        self.transaction(|state| {
            let requested = requests.iter().collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &requested)?;
            let mut adopted_batch = None;
            for request in requests {
                let recovery = recoveries[request.lease_id()];
                if recovery.lifetime.effect_scope != PortLeaseEffectScope::ProviderManaged {
                    return Err(PortLeaseOperationError::LifetimeScopeMismatch {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                let exact_replay = record.phase == PortLeasePhase::CleanupPending
                    && record.active_lifetime.is_none()
                    && record.last_lifetime_generation == recovery.lifetime.generation.as_u64()
                    && recovery.request == *request;
                if !exact_replay {
                    authenticate_recovery(record, request, recovery)?;
                }
                let adopted = matches!(
                    record.phase,
                    PortLeasePhase::Active
                        | PortLeasePhase::Withdrawing
                        | PortLeasePhase::CleanupPending
                ) && record.binding.is_some()
                    && record.adoption_claim.is_some()
                    && record.bind_claim.is_none();
                let unadopted = matches!(
                    record.phase,
                    PortLeasePhase::Reserved | PortLeasePhase::CleanupPending
                ) && record.binding.is_none()
                    && record.adoption_claim.is_none()
                    && record.bind_claim.is_some();
                if (!adopted && !unadopted) || record.failure.is_some() {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::RetainAfterConfirmedAbsence,
                    });
                }
                if adopted_batch.is_some_and(|current| current != adopted) {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::RetainAfterConfirmedAbsence,
                    });
                }
                adopted_batch = Some(adopted);
            }
            for request in requests {
                let record = exact_record_mut(state, request)?;
                record.phase = PortLeasePhase::CleanupPending;
                record.active_lifetime = None;
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Release a complete provider-managed batch from retained absence truth.
    ///
    /// `retain_provider_managed_batch_after_confirmed_absence` clears the
    /// process lifetime while preserving the exact binding audit trail. This
    /// transition consumes that durable non-bindable state after the workload
    /// owner confirms its network teardown. Exact terminal replay is
    /// idempotent; mixed retained and terminal batches are rejected.
    pub fn release_retained_provider_managed_batch_after_confirmed_absence(
        &self,
        requests: &[PortLeaseRequest],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.transaction(|state| {
            let requested = requests.iter().collect::<Vec<_>>();
            authenticate_complete_plan_batch_if_present(state, &requested)?;
            let mut batch_shape = None;
            for request in requests {
                let record = exact_record(state, request)?;
                let candidate = match record.phase {
                    PortLeasePhase::CleanupPending
                        if record.binding.is_some()
                            && record.adoption_claim.is_some()
                            && record.bind_claim.is_none()
                            && record.failure.is_none()
                            && record.active_lifetime.is_none() =>
                    {
                        (true, true)
                    }
                    PortLeasePhase::CleanupPending
                        if record.binding.is_none()
                            && record.adoption_claim.is_none()
                            && record.bind_claim.is_some()
                            && record.failure.is_none()
                            && record.active_lifetime.is_none() =>
                    {
                        (false, true)
                    }
                    PortLeasePhase::Released
                        if record.binding.is_some()
                            && record.adoption_claim.is_some()
                            && record.bind_claim.is_none()
                            && record.failure.is_none()
                            && record.active_lifetime.is_none() =>
                    {
                        (true, false)
                    }
                    PortLeasePhase::Released
                        if record.binding.is_none()
                            && record.adoption_claim.is_none()
                            && record.bind_claim.is_none()
                            && record.failure.is_none()
                            && record.active_lifetime.is_none() =>
                    {
                        (false, false)
                    }
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::ReleaseRetainedAfterConfirmedAbsence,
                        });
                    }
                };
                if batch_shape.is_some_and(|current| current != candidate) {
                    return Err(PortLeaseOperationError::InvalidTransition {
                        lease_id: request.lease_id().clone(),
                        phase: record.phase,
                        operation: PortLeaseOperation::ReleaseRetainedAfterConfirmedAbsence,
                    });
                }
                batch_shape = Some(candidate);
            }
            if batch_shape.is_some_and(|(_, retained)| retained) {
                for request in requests {
                    let record = exact_record_mut(state, request)?;
                    record.phase = PortLeasePhase::Released;
                    record.reservation_claim = None;
                    record.bind_claim = None;
                    record.confirmed_stopped_binding = None;
                }
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
        self.prepare_rebind_provider_managed_claims_after_confirmed_stop_inner(
            requests, recoveries, None,
        )
    }

    /// Retain dead provider claims for a selected subset while authenticating
    /// the complete immutable plan witness.
    pub fn prepare_rebind_provider_managed_plan_claims_after_confirmed_stop(
        &self,
        plan_members: &[PortLeaseRequest],
        requests: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.prepare_rebind_provider_managed_claims_after_confirmed_stop_inner(
            requests,
            recoveries,
            Some(plan_members),
        )
    }

    fn prepare_rebind_provider_managed_claims_after_confirmed_stop_inner(
        &self,
        requests: &[PortLeaseRequest],
        recoveries: &[PortLeaseRecoveryGuard],
        plan_members: Option<&[PortLeaseRequest]>,
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        let recoveries = exact_recovery_batch(requests, recoveries)?;
        self.transaction(|state| {
            let requested = requests.iter().collect::<Vec<_>>();
            match plan_members {
                Some(plan_members) => {
                    let witness = plan_members.iter().collect::<Vec<_>>();
                    authenticate_complete_plan_members(state, &witness, &requested)?;
                }
                None => authenticate_complete_plan_batch_if_present(state, &requested)?,
            }
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
}
