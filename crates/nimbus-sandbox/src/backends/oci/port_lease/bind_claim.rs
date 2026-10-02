//! Bind and rebind claims taken before any provider effect.
//!
//! A claim fences one exact listener for a provider effect. An abandon releases
//! the claim only when the adapter proves that no effect started.

use std::net::SocketAddr;

use nimbus_network::{
    LocalPortLeaseAuthority, NetworkReservationClaim, PortBindClaim, PortLeaseEffectScope,
    PortLeaseLifetimeGuard, PortLeaseRecord, PortLeaseRequest,
};

use super::{
    OciPortBindLifetimeBatch, OciPortProvider, port_lease_error, provider_bind_claim,
    provider_binding,
};
use crate::error::{Result, SandboxError};

/// Claim a complete Nimbus-owned listener batch before any provider bind.
#[cfg(test)]
pub(crate) fn claim_bind_attempts(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    provider: OciPortProvider,
    reservation_claim: Option<&NetworkReservationClaim>,
) -> Result<Vec<PortBindClaim>> {
    let claims = requests
        .iter()
        .map(|request| provider_bind_claim(request, provider))
        .collect::<Result<Vec<_>>>()?;
    let claimed = requests
        .iter()
        .cloned()
        .zip(claims.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .claim_bind_batch(&claimed, reservation_claim)
        .map_err(port_lease_error)?;
    Ok(claims)
}

/// Claim one sandbox provider attempt together with its exact process lifetime.
pub(crate) fn claim_bind_attempt_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    provider: OciPortProvider,
    reservation_claim: Option<&NetworkReservationClaim>,
    effect_scope: PortLeaseEffectScope,
) -> Result<(PortBindClaim, PortLeaseLifetimeGuard)> {
    let claim = provider_bind_claim(request, provider)?;
    let lifetime = authority
        .claim_bind_with_lifetime(request, reservation_claim, claim.clone(), effect_scope)
        .map_err(port_lease_error)?;
    Ok((claim, lifetime))
}

/// Claim one compiler-planned effect while proving the complete immutable
/// plan membership without claiming unrelated listeners for this provider.
pub(crate) fn claim_bind_plan_member_attempt_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    request: &PortLeaseRequest,
    provider: OciPortProvider,
    reservation_claim: &NetworkReservationClaim,
    effect_scope: PortLeaseEffectScope,
) -> Result<(PortBindClaim, PortLeaseLifetimeGuard)> {
    let claim = provider_bind_claim(request, provider)?;
    let lifetime = authority
        .claim_bind_plan_member_with_lifetime(
            plan_members,
            request,
            reservation_claim,
            claim.clone(),
            effect_scope,
        )
        .map_err(port_lease_error)?;
    Ok((claim, lifetime))
}

/// Claim one retained planned binding for its next process lifetime.
pub(crate) fn claim_rebind_plan_member_attempt_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
    effect_scope: PortLeaseEffectScope,
) -> Result<(PortBindClaim, PortLeaseLifetimeGuard)> {
    let claim = provider_bind_claim(request, provider)?;
    let confirmed_stopped_binding = provider_binding(request, actual_addr, provider)?;
    let lifetime = authority
        .claim_rebind_plan_member_with_lifetime(
            plan_members,
            request,
            &confirmed_stopped_binding,
            claim.clone(),
            effect_scope,
        )
        .map_err(port_lease_error)?;
    Ok((claim, lifetime))
}

pub(crate) fn claim_rebind_plan_members_attempts_with_lifetimes(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    actual_addrs: &[SocketAddr],
    provider: OciPortProvider,
    effect_scope: PortLeaseEffectScope,
) -> Result<OciPortBindLifetimeBatch> {
    if requests.len() != actual_addrs.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot claim {} planned rebind requests from {} confirmed bindings",
                requests.len(),
                actual_addrs.len()
            ),
        });
    }
    let claims = requests
        .iter()
        .map(|request| provider_bind_claim(request, provider))
        .collect::<Result<Vec<_>>>()?;
    let entries = requests
        .iter()
        .zip(&claims)
        .zip(actual_addrs)
        .map(|((request, claim), actual_addr)| {
            Ok((
                request.clone(),
                claim.clone(),
                provider_binding(request, *actual_addr, provider)?,
            ))
        })
        .collect::<Result<Vec<_>>>()?;
    let lifetimes = authority
        .claim_rebind_plan_members_with_lifetimes(plan_members, &entries, effect_scope)
        .map_err(port_lease_error)?;
    OciPortBindLifetimeBatch::from_reclaimed(claims, lifetimes)
}

/// Claim a complete provider batch together with exact process lifetimes.
pub(crate) fn claim_bind_attempts_with_lifetimes(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    provider: OciPortProvider,
    reservation_claim: Option<&NetworkReservationClaim>,
    effect_scope: PortLeaseEffectScope,
) -> Result<OciPortBindLifetimeBatch> {
    let claims = requests
        .iter()
        .map(|request| provider_bind_claim(request, provider))
        .collect::<Result<Vec<_>>>()?;
    let claimed = requests
        .iter()
        .cloned()
        .zip(claims.iter().cloned())
        .collect::<Vec<_>>();
    let lifetimes = authority
        .claim_bind_batch_with_lifetimes(&claimed, reservation_claim, effect_scope)
        .map_err(port_lease_error)?;
    Ok(OciPortBindLifetimeBatch { claims, lifetimes })
}

/// Claim one provider-owned subset while authenticating the complete compiler
/// plan in the same durable transaction.
pub(crate) fn claim_bind_plan_members_attempts_with_lifetimes(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    provider: OciPortProvider,
    reservation_claim: &NetworkReservationClaim,
    effect_scope: PortLeaseEffectScope,
) -> Result<OciPortBindLifetimeBatch> {
    let claims = requests
        .iter()
        .map(|request| provider_bind_claim(request, provider))
        .collect::<Result<Vec<_>>>()?;
    let claimed = requests
        .iter()
        .cloned()
        .zip(claims.iter().cloned())
        .collect::<Vec<_>>();
    let lifetimes = authority
        .claim_bind_plan_members_with_lifetimes(
            plan_members,
            &claimed,
            reservation_claim,
            effect_scope,
        )
        .map_err(port_lease_error)?;
    Ok(OciPortBindLifetimeBatch { claims, lifetimes })
}

/// Relinquish exact bind claims after all corresponding effects are absent.
pub(crate) fn abandon_bind_attempts_without_effect(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    claims: &[PortBindClaim],
    reservation_claim: Option<&NetworkReservationClaim>,
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != claims.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot abandon {} sandbox bind claims for {} durable requests",
                claims.len(),
                requests.len()
            ),
        });
    }
    let claimed = requests
        .iter()
        .cloned()
        .zip(claims.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .abandon_bind_claims_without_effect(&claimed, reservation_claim)
        .map_err(port_lease_error)
}

/// Relinquish one exact lifetime-fenced attempt after proving no effect.
pub(crate) fn abandon_bind_attempt_with_lifetime_without_effect(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    lifetime: &PortLeaseLifetimeGuard,
    reservation_claim: Option<&NetworkReservationClaim>,
) -> Result<PortLeaseRecord> {
    authority
        .abandon_bind_with_lifetime_without_effect(request, reservation_claim, claim, lifetime)
        .map_err(port_lease_error)
}

pub(crate) fn abandon_bind_plan_member_attempt_with_lifetime_without_effect(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    lifetime: &PortLeaseLifetimeGuard,
    reservation_claim: &NetworkReservationClaim,
) -> Result<PortLeaseRecord> {
    authority
        .abandon_bind_plan_member_with_lifetime_without_effect(
            plan_members,
            request,
            reservation_claim,
            claim,
            lifetime,
        )
        .map_err(port_lease_error)
}

pub(crate) fn abandon_rebind_plan_member_attempt_with_lifetime_without_effect(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    lifetime: &PortLeaseLifetimeGuard,
) -> Result<PortLeaseRecord> {
    let confirmed_stopped_binding = authority
        .inspect_plan_member(plan_members, request)
        .map_err(port_lease_error)?
        .confirmed_stopped_binding()
        .cloned()
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "planned rebind {} lost its exact confirmed-stop receipt",
                request.lease_id()
            ),
        })?;
    authority
        .abandon_rebind_plan_member_with_lifetime_without_effect(
            plan_members,
            request,
            &confirmed_stopped_binding,
            claim,
            lifetime,
        )
        .map_err(port_lease_error)
}

pub(crate) fn abandon_rebind_plan_members_attempts_with_lifetimes_without_effect(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    actual_addrs: &[SocketAddr],
    provider: OciPortProvider,
    batch: &OciPortBindLifetimeBatch,
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != actual_addrs.len() || requests.len() != batch.claims.len() {
        return Err(SandboxError::OperationFailed {
            message: "planned rebind abandonment received crossed batch lengths".to_owned(),
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
        .abandon_rebind_plan_members_with_lifetimes_without_effect(
            plan_members,
            &entries,
            &batch.lifetimes,
        )
        .map_err(port_lease_error)
}

/// Relinquish one complete lifetime-fenced batch after proving no effect.
pub(crate) fn abandon_bind_attempts_with_lifetimes_without_effect(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    batch: &OciPortBindLifetimeBatch,
    reservation_claim: Option<&NetworkReservationClaim>,
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != batch.claims.len() || requests.len() != batch.lifetimes.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot abandon {} sandbox requests from {} claims and {} process lifetimes",
                requests.len(),
                batch.claims.len(),
                batch.lifetimes.len()
            ),
        });
    }
    let claims = requests
        .iter()
        .cloned()
        .zip(batch.claims.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .abandon_bind_batch_with_lifetimes_without_effect(
            &claims,
            reservation_claim,
            &batch.lifetimes,
        )
        .map_err(port_lease_error)
}

/// Relinquish one no-effect provider subset under the complete compiler plan.
pub(crate) fn abandon_bind_plan_members_attempts_with_lifetimes_without_effect(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    batch: &OciPortBindLifetimeBatch,
    reservation_claim: &NetworkReservationClaim,
) -> Result<Vec<PortLeaseRecord>> {
    if requests.len() != batch.claims.len() || requests.len() != batch.lifetimes.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot abandon {} planned sandbox requests from {} claims and {} process lifetimes",
                requests.len(),
                batch.claims.len(),
                batch.lifetimes.len()
            ),
        });
    }
    let claims = requests
        .iter()
        .cloned()
        .zip(batch.claims.iter().cloned())
        .collect::<Vec<_>>();
    authority
        .abandon_bind_plan_members_with_lifetimes_without_effect(
            plan_members,
            &claims,
            reservation_claim,
            &batch.lifetimes,
        )
        .map_err(port_lease_error)
}
