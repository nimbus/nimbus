//! Lease transitions after a provider effect stops or its owner dies.
//!
//! The adapter withdraws a lease before it stops the effect. After a confirmed
//! stop or owner death, the lease is prepared for rebind or released.

use nimbus_network::{
    LocalPortLeaseAuthority, PortLeaseBinding, PortLeaseLifetimeGuard, PortLeasePhase,
    PortLeaseRecord, PortLeaseRecoveryAttempt, PortLeaseRecoveryGuard, PortLeaseRequest,
};

use super::{inspect_exact, port_lease_error};
use crate::error::{Result, SandboxError};

/// Convert an exact dead process-bound effect into a restart-retained slot.
pub fn prepare_process_bound_rebind_after_owner_death(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    match authority
        .recover_dead_lifetime(request)
        .map_err(port_lease_error)?
    {
        PortLeaseRecoveryAttempt::LiveOwner(record) => Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} remains owned by live process lifetime {:?}",
                request.lease_id(),
                record.active_lifetime()
            ),
        }),
        PortLeaseRecoveryAttempt::Acquired(recovery) => {
            authority
                .mark_cleanup_pending_after_owner_death(request, &recovery)
                .map_err(port_lease_error)?;
            authority
                .prepare_rebind_process_bound_after_owner_death(request, &recovery)
                .map_err(port_lease_error)
        }
        PortLeaseRecoveryAttempt::Settled(record) => Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} reached terminal phase {:?} and cannot be rebound",
                request.lease_id(),
                record.phase()
            ),
        }),
    }
}

/// Convert one exact dead process-bound plan member into a retained rebind slot.
pub fn prepare_process_bound_plan_member_rebind_after_owner_death(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let requests = std::slice::from_ref(request);
    let recoveries = authority
        .recover_dead_plan_members(plan_members, requests)
        .map_err(port_lease_error)?;
    authority
        .mark_cleanup_pending_plan_members_after_owner_death(plan_members, requests, &recoveries)
        .map_err(port_lease_error)?;
    authority
        .prepare_rebind_process_bound_plan_members_after_owner_death(
            plan_members,
            requests,
            &recoveries,
        )
        .map_err(port_lease_error)?
        .pop()
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "planned process-bound rebind returned no record for {}",
                request.lease_id()
            ),
        })
}

/// Fence new use before the provider effect is stopped or detached.
pub fn withdraw(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    if matches!(
        record.phase(),
        PortLeasePhase::Withdrawing | PortLeasePhase::Released | PortLeasePhase::Failed
    ) {
        return Ok(record);
    }
    authority.withdraw(request).map_err(port_lease_error)
}

/// Retain an exact port for rebind after this process confirmed provider stop.
pub fn prepare_rebind_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    expected_binding: &PortLeaseBinding,
) -> Result<PortLeaseRecord> {
    authority
        .prepare_rebind_after_confirmed_stop(request, expected_binding)
        .map_err(port_lease_error)
}

/// Retain one exact live-owner listener after acknowledged provider stop.
pub fn prepare_rebind_after_confirmed_stop_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    expected_binding: &PortLeaseBinding,
    lifetime: &PortLeaseLifetimeGuard,
) -> Result<PortLeaseRecord> {
    authority
        .prepare_rebind_batch_after_confirmed_stop_with_lifetimes(
            &[(request.clone(), expected_binding.clone())],
            std::slice::from_ref(lifetime),
        )
        .map(|mut records| {
            records
                .pop()
                .expect("one confirmed-stop lifetime rebind returns one record")
        })
        .map_err(port_lease_error)
}

/// Atomically retain an exact stopped listener batch for same-generation rebind.
#[cfg(test)]
pub(crate) fn prepare_rebind_batch_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    expected_bindings: &[PortLeaseBinding],
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != expected_bindings.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot prepare {} durable listener requests for {} confirmed stopped bindings",
                requests.len(),
                expected_bindings.len()
            ),
        });
    }
    let expected = requests
        .iter()
        .cloned()
        .zip(expected_bindings.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .prepare_rebind_batch_after_confirmed_stop(&expected)
        .map_err(port_lease_error)
}

/// Retain a live-owner batch after exact provider stop.
pub fn prepare_rebind_batch_after_confirmed_stop_with_lifetimes(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    expected_bindings: &[PortLeaseBinding],
    lifetimes: &[PortLeaseLifetimeGuard],
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != expected_bindings.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot prepare {} live-owner requests for {} confirmed stopped bindings",
                requests.len(),
                expected_bindings.len()
            ),
        });
    }
    let expected = requests
        .iter()
        .cloned()
        .zip(expected_bindings.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .prepare_rebind_batch_after_confirmed_stop_with_lifetimes(&expected, lifetimes)
        .map_err(port_lease_error)
}

pub fn prepare_provider_managed_plan_members_after_confirmed_stop_with_lifetimes(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    expected_bindings: &[PortLeaseBinding],
    lifetimes: &[PortLeaseLifetimeGuard],
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != expected_bindings.len() {
        return Err(SandboxError::OperationFailed {
            message: "planned live provider rebind received crossed binding lengths".to_owned(),
        });
    }
    let expected = requests
        .iter()
        .cloned()
        .zip(expected_bindings.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .prepare_rebind_provider_managed_plan_members_after_confirmed_stop_with_lifetimes(
            plan_members,
            &expected,
            lifetimes,
        )
        .map_err(port_lease_error)
}

/// Atomically release a live provider batch after exact provider stop.
pub fn release_provider_managed_batch_after_confirmed_stop_with_lifetimes(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    expected_bindings: &[PortLeaseBinding],
    lifetimes: &[PortLeaseLifetimeGuard],
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != expected_bindings.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot release {} live-owner requests from {} confirmed stopped bindings",
                requests.len(),
                expected_bindings.len()
            ),
        });
    }
    let expected = requests
        .iter()
        .cloned()
        .zip(expected_bindings.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .release_provider_managed_batch_after_confirmed_stop_with_lifetimes(&expected, lifetimes)
        .map_err(port_lease_error)
}

/// Acquire exact dead-owner authority and quarantine one provider batch.
///
/// This operation proves only owner death. The returned guards must remain
/// held while the OCI adapter inspects or removes the provider effect.
pub fn recover_provider_managed_batch_after_owner_death(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
) -> Result<Vec<PortLeaseRecoveryGuard>> {
    let mut recoveries = Vec::with_capacity(requests.len());
    for request in requests {
        match authority
            .recover_dead_lifetime(request)
            .map_err(port_lease_error)?
        {
            PortLeaseRecoveryAttempt::Acquired(recovery) => recoveries.push(recovery),
            PortLeaseRecoveryAttempt::LiveOwner(record) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "sandbox port lease {} remains owned by live process lifetime {:?}",
                        request.lease_id(),
                        record.active_lifetime()
                    ),
                });
            }
            PortLeaseRecoveryAttempt::Settled(record) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "sandbox port lease {} reached terminal phase {:?} while recovering its \
                         provider-managed batch",
                        request.lease_id(),
                        record.phase()
                    ),
                });
            }
        }
    }
    authority
        .mark_cleanup_pending_batch_after_owner_death(requests, &recoveries)
        .map_err(port_lease_error)?;
    Ok(recoveries)
}

pub fn recover_provider_managed_plan_members_after_owner_death(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
) -> Result<Vec<PortLeaseRecoveryGuard>> {
    authority
        .recover_dead_plan_members(plan_members, requests)
        .map_err(port_lease_error)
}

/// Retain a recovered provider batch after the adapter confirms exact absence.
pub fn prepare_provider_managed_batch_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    expected_bindings: &[PortLeaseBinding],
    recoveries: &[PortLeaseRecoveryGuard],
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != expected_bindings.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot retain {} recovered listener requests from {} confirmed stopped bindings",
                requests.len(),
                expected_bindings.len()
            ),
        });
    }
    let expected = requests
        .iter()
        .cloned()
        .zip(expected_bindings.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .prepare_rebind_provider_managed_batch_after_confirmed_stop(&expected, recoveries)
        .map_err(port_lease_error)
}

pub fn prepare_provider_managed_plan_members_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    expected_bindings: &[PortLeaseBinding],
    recoveries: &[PortLeaseRecoveryGuard],
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != expected_bindings.len() {
        return Err(SandboxError::OperationFailed {
            message: "planned provider rebind received crossed binding lengths".to_owned(),
        });
    }
    authority
        .mark_cleanup_pending_plan_members_after_owner_death(plan_members, requests, recoveries)
        .map_err(port_lease_error)?;
    let expected = requests
        .iter()
        .cloned()
        .zip(expected_bindings.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .prepare_rebind_provider_managed_plan_members_after_confirmed_stop(
            plan_members,
            &expected,
            recoveries,
        )
        .map_err(port_lease_error)
}

/// Retire a recovered provider-claim batch while retaining its exact slots.
pub fn prepare_provider_managed_claim_batch_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    recoveries: &[PortLeaseRecoveryGuard],
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .prepare_rebind_provider_managed_claim_batch_after_confirmed_stop(requests, recoveries)
        .map_err(port_lease_error)
}

pub fn prepare_provider_managed_plan_claims_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    recoveries: &[PortLeaseRecoveryGuard],
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .prepare_rebind_provider_managed_plan_claims_after_confirmed_stop(
            plan_members,
            requests,
            recoveries,
        )
        .map_err(port_lease_error)
}

/// Release a recovered provider batch after the adapter confirms exact absence.
pub fn release_provider_managed_batch_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    recoveries: &[PortLeaseRecoveryGuard],
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .release_provider_managed_batch_after_confirmed_stop(requests, recoveries)
        .map_err(port_lease_error)
}

/// Release the numeric slot only after provider effect removal is confirmed.
pub fn release(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    if matches!(
        record.phase(),
        PortLeasePhase::Released | PortLeasePhase::Failed
    ) {
        return Ok(record);
    }
    authority.release(request).map_err(port_lease_error)
}

/// Release a live-owner slot after the adapter confirms exact effect absence.
pub fn release_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    lifetime: &PortLeaseLifetimeGuard,
) -> Result<PortLeaseRecord> {
    authority
        .release_with_lifetime(request, lifetime)
        .map_err(port_lease_error)
}

/// Release a restart-retained slot using its exact durable stopped-binding receipt.
pub fn release_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    authority
        .release_after_confirmed_stop(request)
        .map_err(port_lease_error)
}

/// Atomically release a complete restart-retained listener batch.
pub fn release_batch_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .release_batch_after_confirmed_stop(requests)
        .map_err(port_lease_error)
}
