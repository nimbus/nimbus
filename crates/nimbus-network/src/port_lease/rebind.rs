//! Confirmed-stop receipt transitions for restart and terminal release.

use std::collections::BTreeMap;

use super::{
    LocalPortLeaseAuthority, PortLeaseBinding, PortLeaseError, PortLeaseOperation,
    PortLeaseOperationError, PortLeasePhase, PortLeaseRecord, PortLeaseRequest, exact_record,
    exact_record_mut,
};
use crate::PortLeaseId;

impl LocalPortLeaseAuthority {
    /// Return an exact active or withdrawing binding to `Reserved` after
    /// confirmed stop.
    ///
    /// This transition grants no provider-absence authority: the adapter must
    /// already hold exact process-local evidence and an acknowledged stop for
    /// `expected_binding`. `Withdrawing` is accepted so effect owners can fence
    /// new use before stopping the provider. The selected numeric port remains
    /// fenced while old binding evidence is cleared so the same generation can
    /// execute the normal claim → bind → adopt → activate sequence again.
    pub fn prepare_rebind_after_confirmed_stop(
        &self,
        request: &PortLeaseRequest,
        expected_binding: &PortLeaseBinding,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.prepare_rebind_batch_after_confirmed_stop(&[(
            request.clone(),
            expected_binding.clone(),
        )])
        .map(|mut records| {
            records
                .pop()
                .expect("one confirmed-stop rebind must return one durable record")
        })
    }

    /// Atomically retain an exact listener batch for rebind after confirmed stop.
    ///
    /// Every request, selected port, and adopted binding is authenticated
    /// before any record changes. A multi-listener restart therefore cannot
    /// leave a partial `Active`/`Reserved` authority split.
    pub fn prepare_rebind_batch_after_confirmed_stop(
        &self,
        bindings: &[(PortLeaseRequest, PortLeaseBinding)],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.transaction(|state| {
            let mut distinct =
                BTreeMap::<PortLeaseId, (&PortLeaseRequest, &PortLeaseBinding)>::new();
            for (request, expected_binding) in bindings {
                if let Some((existing_request, existing_binding)) =
                    distinct.insert(request.lease_id().clone(), (request, expected_binding))
                    && (existing_request != request || existing_binding != expected_binding)
                {
                    return Err(PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
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
                            && record.bind_claim.is_none() => {}
                    PortLeasePhase::Active | PortLeasePhase::Withdrawing => {
                        return Err(PortLeaseOperationError::BindingConflict {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    PortLeasePhase::Reserved
                        if record.bind_claim.is_none()
                            && record.binding.is_none()
                            && record.failure.is_none()
                            && record.confirmed_stopped_binding.as_ref()
                                == Some(expected_binding) => {}
                    PortLeasePhase::Reserved => {
                        return Err(PortLeaseOperationError::BindingConflict {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    phase => {
                        return Err(PortLeaseOperationError::InvalidTransition {
                            lease_id: request.lease_id().clone(),
                            phase,
                            operation: PortLeaseOperation::PrepareRebindAfterConfirmedStop,
                        });
                    }
                }
            }

            for (request, expected_binding) in distinct.into_values() {
                let record = exact_record_mut(state, request)?;
                if matches!(
                    record.phase,
                    PortLeasePhase::Active | PortLeasePhase::Withdrawing
                ) {
                    debug_assert_eq!(record.binding.as_ref(), Some(expected_binding));
                    record.phase = PortLeasePhase::Reserved;
                    record.reservation_claim = None;
                    record.binding = None;
                    record.bind_claim = None;
                    record.adoption_claim = None;
                    record.confirmed_stopped_binding = Some(expected_binding.clone());
                    record.failure = None;
                }
            }

            bindings
                .iter()
                .map(|(request, _)| exact_record(state, request).cloned())
                .collect()
        })
    }

    /// Release a restart-retained port using exact durable provider-absence evidence.
    ///
    /// This is the only path that may release directly from `Reserved`. A
    /// fresh reservation has no stopped-binding receipt and therefore cannot
    /// impersonate an acknowledged provider stop. Replays of the exact
    /// terminal request are idempotent.
    pub fn release_after_confirmed_stop(
        &self,
        request: &PortLeaseRequest,
    ) -> Result<PortLeaseRecord, PortLeaseError> {
        self.release_batch_after_confirmed_stop(std::slice::from_ref(request))
            .map(|mut records| {
                records
                    .pop()
                    .expect("one confirmed-stop release must return one durable record")
            })
    }

    /// Atomically release an exact restart-retained listener batch.
    ///
    /// Every member must be restart-retained, or every member must already be
    /// released for an idempotent replay. A mixed terminal/retained batch is
    /// rejected: the atomic transition cannot produce that state, and one
    /// invalid member leaves the complete batch unchanged.
    pub fn release_batch_after_confirmed_stop(
        &self,
        requests: &[PortLeaseRequest],
    ) -> Result<Vec<PortLeaseRecord>, PortLeaseError> {
        self.transaction(|state| {
            let mut distinct = BTreeMap::<PortLeaseId, &PortLeaseRequest>::new();
            let mut retained = 0usize;
            let mut released_lease_id = None;
            for request in requests {
                if let Some(existing) = distinct.insert(request.lease_id().clone(), request)
                    && existing != request
                {
                    return Err(PortLeaseOperationError::BindingConflict {
                        lease_id: request.lease_id().clone(),
                    });
                }
                let record = exact_record(state, request)?;
                match record.phase {
                    PortLeasePhase::Reserved
                        if record.reservation_claim.is_none()
                            && record.bind_claim.is_none()
                            && record.binding.is_none()
                            && record.confirmed_stopped_binding.is_some()
                            && record.failure.is_none() =>
                    {
                        retained += 1;
                    }
                    PortLeasePhase::Released => {
                        released_lease_id.get_or_insert_with(|| request.lease_id().clone());
                    }
                    PortLeasePhase::Reserved if record.bind_claim.is_some() => {
                        return Err(PortLeaseOperationError::BindClaimConflict {
                            lease_id: request.lease_id().clone(),
                        });
                    }
                    PortLeasePhase::Reserved if record.reservation_claim.is_some() => {
                        return Err(PortLeaseOperationError::ReservationClaimConflict {
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
            if retained > 0
                && let Some(lease_id) = released_lease_id
            {
                return Err(PortLeaseOperationError::InvalidTransition {
                    lease_id,
                    phase: PortLeasePhase::Released,
                    operation: PortLeaseOperation::ReleaseAfterConfirmedStop,
                });
            }
            for request in distinct.into_values() {
                let record = exact_record_mut(state, request)?;
                if record.phase == PortLeasePhase::Reserved {
                    record.phase = PortLeasePhase::Released;
                    record.confirmed_stopped_binding = None;
                }
            }
            requests
                .iter()
                .map(|request| exact_record(state, request).cloned())
                .collect()
        })
    }
}
