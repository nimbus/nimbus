//! Outcomes of claimed provider binds.
//!
//! A claimed bind ends in one durable outcome. The adapter adopts the bound
//! endpoint and activates the lease, or it records a confirmed no-effect
//! failure.

use std::io;
use std::net::SocketAddr;

use nimbus_network::{
    LocalPortLeaseAuthority, NetworkProviderId, NetworkReservationClaim, PortBindAttempt,
    PortBindClaim, PortBindFailure, PortBindRealm, PortLeaseLifetimeGuard, PortLeaseRecord,
    PortLeaseRequest, PortProtocol,
};

use super::{
    OciConfirmedBindFailure, OciPortActivation, OciPortBindLifetimeBatch, OciPortProvider,
    failure_kind, port_lease_error, provider_binding, target_for_ip,
};
use crate::error::{Result, SandboxError};

/// Adopt and activate a successful bind owned by the exact durable claim.
#[cfg(test)]
pub(crate) fn adopt_claimed_and_activate(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    reservation_claim: Option<&NetworkReservationClaim>,
    claim: &PortBindClaim,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let binding = provider_binding(request, actual_addr, provider)?;
    let adopted = authority
        .adopt_claimed(request, reservation_claim, claim, binding)
        .map_err(port_lease_error)?;
    debug_assert_eq!(adopted.phase(), nimbus_network::PortLeasePhase::Binding);
    authority
        .activate_claimed(request, claim)
        .map_err(port_lease_error)
}

/// Adopt and activate one binding under the exact process-lifetime guard.
pub(crate) fn adopt_claimed_and_activate_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    reservation_claim: Option<&NetworkReservationClaim>,
    claim: &PortBindClaim,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
    lifetime: &PortLeaseLifetimeGuard,
) -> Result<PortLeaseRecord> {
    let binding = provider_binding(request, actual_addr, provider)?;
    authority
        .adopt_claimed_and_activate_with_lifetime(
            request,
            reservation_claim,
            claim,
            binding,
            lifetime,
        )
        .map_err(port_lease_error)
}

pub(crate) fn adopt_claimed_and_activate_plan_member_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    request: &PortLeaseRequest,
    reservation_claim: &NetworkReservationClaim,
    activation: OciPortActivation<'_>,
) -> Result<PortLeaseRecord> {
    let binding = provider_binding(request, activation.actual_addr, activation.provider)?;
    authority
        .adopt_claimed_and_activate_plan_member_with_lifetime(
            plan_members,
            request,
            reservation_claim,
            activation.claim,
            binding,
            activation.lifetime,
        )
        .map_err(port_lease_error)
}

pub(crate) fn adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
    lifetime: &PortLeaseLifetimeGuard,
) -> Result<PortLeaseRecord> {
    let binding = provider_binding(request, actual_addr, provider)?;
    authority
        .adopt_claimed_and_activate_rebind_plan_member_with_lifetime(
            plan_members,
            request,
            &binding,
            claim,
            binding.clone(),
            lifetime,
        )
        .map_err(port_lease_error)
}

pub(crate) fn adopt_claimed_and_activate_rebind_plan_members_with_lifetimes(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    actual_addrs: &[SocketAddr],
    provider: OciPortProvider,
    batch: &OciPortBindLifetimeBatch,
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != actual_addrs.len() || requests.len() != batch.claims.len() {
        return Err(SandboxError::OperationFailed {
            message: "planned rebind activation received crossed batch lengths".to_owned(),
        });
    }
    let entries = requests
        .iter()
        .zip(&batch.claims)
        .zip(actual_addrs)
        .map(|((request, claim), actual_addr)| {
            Ok((
                request.clone(),
                claim.clone(),
                provider_binding(request, *actual_addr, provider)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    authority
        .adopt_claimed_and_activate_rebind_plan_members_with_lifetimes(
            plan_members,
            &entries,
            &batch.lifetimes,
        )
        .map_err(port_lease_error)
}

/// Atomically adopt and activate a complete Nimbus-owned listener batch.
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn adopt_claimed_and_activate_batch(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    claims: &[PortBindClaim],
    actual_addrs: &[SocketAddr],
    provider: OciPortProvider,
    reservation_claim: Option<&NetworkReservationClaim>,
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != claims.len() || requests.len() != actual_addrs.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot activate {} sandbox listeners from {} claims and {} provider addresses",
                requests.len(),
                claims.len(),
                actual_addrs.len()
            ),
        });
    }
    let bindings = requests
        .iter()
        .zip(claims)
        .zip(actual_addrs)
        .map(|((request, claim), actual_addr)| {
            Ok((
                request.clone(),
                claim.clone(),
                provider_binding(request, *actual_addr, provider)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    authority
        .adopt_claimed_and_activate_batch(&bindings, reservation_claim)
        .map_err(port_lease_error)
}

/// Atomically activate a complete batch under its exact live lifetimes.
pub(crate) fn adopt_claimed_and_activate_batch_with_lifetimes(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    batch: &OciPortBindLifetimeBatch,
    actual_addrs: &[SocketAddr],
    provider: OciPortProvider,
    reservation_claim: Option<&NetworkReservationClaim>,
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != batch.claims.len()
        || requests.len() != batch.lifetimes.len()
        || requests.len() != actual_addrs.len()
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot activate {} sandbox listeners from {} claims, {} process lifetimes, and \
                 {} provider addresses",
                requests.len(),
                batch.claims.len(),
                batch.lifetimes.len(),
                actual_addrs.len()
            ),
        });
    }
    let bindings = requests
        .iter()
        .zip(&batch.claims)
        .zip(actual_addrs)
        .map(|((request, claim), actual_addr)| {
            Ok((
                request.clone(),
                claim.clone(),
                provider_binding(request, *actual_addr, provider)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    authority
        .adopt_claimed_and_activate_batch_with_lifetimes(
            &bindings,
            reservation_claim,
            &batch.lifetimes,
        )
        .map_err(port_lease_error)
}

/// Atomically activate one provider-owned subset under the complete compiler
/// plan and exact process lifetimes.
pub(crate) fn adopt_claimed_and_activate_plan_members_with_lifetimes(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    batch: &OciPortBindLifetimeBatch,
    actual_addrs: &[SocketAddr],
    provider: OciPortProvider,
    reservation_claim: &NetworkReservationClaim,
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != batch.claims.len()
        || requests.len() != batch.lifetimes.len()
        || requests.len() != actual_addrs.len()
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot activate {} planned sandbox listeners from {} claims, {} process lifetimes, and {} provider addresses",
                requests.len(),
                batch.claims.len(),
                batch.lifetimes.len(),
                actual_addrs.len()
            ),
        });
    }
    let bindings = requests
        .iter()
        .zip(&batch.claims)
        .zip(actual_addrs)
        .map(|((request, claim), actual_addr)| {
            Ok((
                request.clone(),
                claim.clone(),
                provider_binding(request, *actual_addr, provider)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    authority
        .adopt_claimed_and_activate_plan_members_with_lifetimes(
            plan_members,
            &bindings,
            reservation_claim,
            &batch.lifetimes,
        )
        .map_err(port_lease_error)
}

/// Record a confirmed no-effect provider bind failure.
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) fn record_bind_failure(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    observed: OciConfirmedBindFailure,
    reservation_claim: Option<&NetworkReservationClaim>,
) -> Result<PortLeaseRecord> {
    let failure = provider_bind_failure(
        request,
        claim,
        observed.attempted_addr,
        observed.provider,
        observed.error_kind,
    )?;
    authority
        .record_claimed_bind_failure_without_effect(request, reservation_claim, claim, failure)
        .map_err(port_lease_error)
}

/// Record a confirmed no-effect failure under its exact live lifetime.
pub(crate) fn record_bind_failure_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    observed: OciConfirmedBindFailure,
    reservation_claim: Option<&NetworkReservationClaim>,
    lifetime: &PortLeaseLifetimeGuard,
) -> Result<PortLeaseRecord> {
    let failure = provider_bind_failure(
        request,
        claim,
        observed.attempted_addr,
        observed.provider,
        observed.error_kind,
    )?;
    authority
        .record_claimed_bind_failure_with_lifetime_without_effect(
            request,
            reservation_claim,
            claim,
            failure,
            lifetime,
        )
        .map_err(port_lease_error)
}

pub(crate) fn record_plan_member_bind_failure_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    observed: OciConfirmedBindFailure,
    reservation_claim: &NetworkReservationClaim,
    lifetime: &PortLeaseLifetimeGuard,
) -> Result<PortLeaseRecord> {
    let failure = provider_bind_failure(
        request,
        claim,
        observed.attempted_addr,
        observed.provider,
        observed.error_kind,
    )?;
    authority
        .record_claimed_plan_member_bind_failure_with_lifetime_without_effect(
            plan_members,
            request,
            reservation_claim,
            claim,
            failure,
            lifetime,
        )
        .map_err(port_lease_error)
}

fn provider_bind_failure(
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    attempted_addr: SocketAddr,
    provider: OciPortProvider,
    error_kind: io::ErrorKind,
) -> Result<PortBindFailure> {
    let expected_provider = NetworkProviderId::for_registration_key(provider.registration_key());
    if claim.provider_attempt().provider_id() != &expected_provider {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} bind claim belongs to a different provider",
                request.lease_id()
            ),
        });
    }
    let attempt = PortBindAttempt::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        target_for_ip(attempted_addr.ip())?,
        attempted_addr.port(),
    )
    .map_err(|error| SandboxError::OperationFailed {
        message: format!("invalid sandbox port bind failure evidence: {error}"),
    })?;
    let failure = PortBindFailure::new(
        failure_kind(error_kind),
        attempt,
        claim.provider_attempt().clone(),
    );
    Ok(failure)
}
