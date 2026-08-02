//! OCI-family adapter between durable portable port leases and provider effects.
//!
//! `nimbus-network` owns reservation identity and lifecycle. This module owns
//! the sandbox-specific translation from real socket/Netavark observations to
//! portable bind evidence; it never allocates by probing or scanning manifests.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;

use nimbus_core::TenantId;
use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkProviderHandle,
    NetworkProviderId, NetworkReservationClaim, NetworkReservationLifetimeAttempt,
    NetworkReservationLifetimeGuard, NetworkResourceGeneration, NetworkResourceId, PortBindAttempt,
    PortBindClaim, PortBindFailure, PortBindFailureKind, PortBindRealm, PortBindTarget,
    PortBindingProvenance, PortBindingSpec, PortBoundEndpoint, PortExposure, PortIpv6Overlap,
    PortLeaseAccounting, PortLeaseBinding, PortLeaseEffectScope, PortLeaseId,
    PortLeaseLifetimeGuard, PortLeasePhase, PortLeaseRecord, PortLeaseRecoveryAttempt,
    PortLeaseRecoveryGuard, PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
    TenantPublishedPortLimit,
};
use ulid::Ulid;

use crate::backends::capabilities::SANDBOX_EGRESS_PEP_PROVIDER_KEY;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

const INITIAL_RESOURCE_GENERATION: NetworkResourceGeneration = NetworkResourceGeneration::new(1);
const INITIAL_LEASE_EPOCH: NetworkLeaseEpoch = NetworkLeaseEpoch::new(1);
const RESERVATION_COORDINATOR_KEY: &str = "nimbus-sandbox.network-launch-coordinator";

/// Sandbox effect owner that interprets one durable provider handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OciPortProvider {
    Netavark,
    MachinePortProxy,
    EgressPep,
}

/// Provider-owned evidence that one bind attempt produced no effect.
#[derive(Debug, Clone, Copy)]
pub(crate) struct OciConfirmedBindFailure {
    attempted_addr: SocketAddr,
    provider: OciPortProvider,
    error_kind: io::ErrorKind,
}

impl OciConfirmedBindFailure {
    pub(crate) fn new(
        attempted_addr: SocketAddr,
        provider: OciPortProvider,
        error_kind: io::ErrorKind,
    ) -> Self {
        Self {
            attempted_addr,
            provider,
            error_kind,
        }
    }
}

pub(crate) struct ReservedPortLeaseBatch {
    selected: Vec<(PortLeaseRequest, NonZeroU16)>,
    reservation_claim: NetworkReservationClaim,
    publication_lifetime: NetworkReservationLifetimeGuard,
}

/// Exact provider claims and non-cloneable process lifetimes for one batch.
pub(crate) struct OciPortBindLifetimeBatch {
    claims: Vec<PortBindClaim>,
    lifetimes: Vec<PortLeaseLifetimeGuard>,
}

impl OciPortBindLifetimeBatch {
    pub(crate) fn claims(&self) -> &[PortBindClaim] {
        &self.claims
    }

    pub(crate) fn lifetimes(&self) -> &[PortLeaseLifetimeGuard] {
        &self.lifetimes
    }

    pub(crate) fn from_reclaimed(
        claims: Vec<PortBindClaim>,
        lifetimes: Vec<PortLeaseLifetimeGuard>,
    ) -> Result<Self> {
        if claims.len() != lifetimes.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot retain {} reclaimed provider lifetimes with {} adoption claims",
                    lifetimes.len(),
                    claims.len()
                ),
            });
        }
        Ok(Self { claims, lifetimes })
    }
}

impl ReservedPortLeaseBatch {
    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<(PortLeaseRequest, NonZeroU16)>,
        NetworkReservationClaim,
        NetworkReservationLifetimeGuard,
    ) {
        (
            self.selected,
            self.reservation_claim,
            self.publication_lifetime,
        )
    }
}

pub(crate) fn new_launch_reservation_claim() -> Result<NetworkReservationClaim> {
    let provider_id = NetworkProviderId::for_registration_key(RESERVATION_COORDINATOR_KEY);
    let handle = NetworkProviderHandle::new(provider_id, format!("attempt:{}", Ulid::new()))
        .map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to create network launch coordinator claim: {error}"),
        })?;
    Ok(NetworkReservationClaim::new(handle))
}

impl OciPortProvider {
    fn registration_key(self) -> &'static str {
        match self {
            Self::Netavark => "nimbus-sandbox.netavark",
            Self::MachinePortProxy => "nimbus-sandbox.machine-port-proxy",
            Self::EgressPep => SANDBOX_EGRESS_PEP_PROVIDER_KEY,
        }
    }

    pub(crate) fn provider_id(self) -> NetworkProviderId {
        NetworkProviderId::for_registration_key(self.registration_key())
    }
}

/// One OCI-family interpretation of portable bind, publication, and accounting intent.
pub(crate) struct OciPortLeaseIntent {
    target: PortBindTarget,
    publication: PortPublicationIntent,
    exposure: PortExposure,
    accounting: PortLeaseAccounting,
}

impl OciPortLeaseIntent {
    pub(crate) fn tenant_published(
        target: PortBindTarget,
        address: IpAddr,
        exposure: PortExposure,
    ) -> Self {
        Self {
            target,
            publication: PortPublicationIntent::host(address),
            exposure,
            accounting: PortLeaseAccounting::TenantPublished,
        }
    }

    pub(crate) fn host_internal(target: PortBindTarget, exposure: PortExposure) -> Self {
        Self {
            target,
            publication: PortPublicationIntent::Unpublished,
            exposure,
            accounting: PortLeaseAccounting::HostInternal,
        }
    }
}

/// Create the immutable request for one named listener on a sandbox incarnation.
pub(crate) fn port_lease_request(
    tenant_id: &TenantId,
    sandbox_id: &SandboxId,
    listener_name: &str,
    intent: OciPortLeaseIntent,
    port: PortRequestMode,
) -> PortLeaseRequest {
    let OciPortLeaseIntent {
        target,
        publication,
        exposure,
        accounting,
    } = intent;
    let listener_id =
        ListenerId::for_tenant_workload_listener(tenant_id, sandbox_id.as_str(), listener_name);
    PortLeaseRequest::new(
        nimbus_network::PortLeaseId::for_listener(&listener_id),
        listener_id.into(),
        Some(tenant_id.clone()),
        nimbus_network::PortLeaseFence::new(INITIAL_RESOURCE_GENERATION, INITIAL_LEASE_EPOCH),
        accounting,
        publication,
        PortBindingSpec::new(
            PortProtocol::Tcp,
            PortBindRealm::Host,
            target,
            exposure,
            port,
        ),
    )
}

/// Atomically reserve and return the selected non-zero host port.
#[cfg(test)]
pub(crate) fn reserve(
    authority: &LocalPortLeaseAuthority,
    request: PortLeaseRequest,
) -> Result<(PortLeaseRequest, NonZeroU16, NetworkReservationClaim)> {
    let reservation_claim = new_launch_reservation_claim()?;
    let publication_lifetime = acquire_reservation_lifetime(authority, &reservation_claim)?;
    let record = authority
        .reserve_for_coordinator(request.clone(), &reservation_claim)
        .map_err(port_lease_error)?;
    let port = match record.reserved_port() {
        Some(port) => port,
        None => {
            let projection_error = SandboxError::OperationFailed {
                message: format!(
                    "sandbox port lease {} did not select a numeric host port",
                    request.lease_id()
                ),
            };
            return Err(compensate_projection_failure(
                authority,
                std::slice::from_ref(&request),
                &publication_lifetime,
                projection_error,
            ));
        }
    };
    Ok((request, port, reservation_claim))
}

fn compensate_projection_failure(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    publication_lifetime: &NetworkReservationLifetimeGuard,
    projection_error: SandboxError,
) -> SandboxError {
    match authority
        .release_reserved_batch_without_effect_with_lifetime(requests, publication_lifetime)
    {
        Ok(_) => projection_error,
        Err(compensation_error) => SandboxError::OperationFailed {
            message: format!(
                "{projection_error}; malformed reservation projection compensation also failed: \
                 {compensation_error}"
            ),
        },
    }
}

/// Atomically reserve an ordered group and return each selected host port.
pub(crate) fn reserve_batch(
    authority: &LocalPortLeaseAuthority,
    requests: Vec<PortLeaseRequest>,
    reservation_claim: &NetworkReservationClaim,
) -> Result<ReservedPortLeaseBatch> {
    let publication_lifetime = acquire_reservation_lifetime(authority, reservation_claim)?;
    let records = authority
        .reserve_batch_for_coordinator(requests.clone(), reservation_claim)
        .map_err(port_lease_error)?;
    finish_reserved_batch(
        authority,
        requests,
        records,
        reservation_claim,
        publication_lifetime,
    )
}

/// Reserve a complete launch batch under one caller-supplied tenant limit.
pub(crate) fn reserve_batch_with_tenant_limit(
    authority: &LocalPortLeaseAuthority,
    requests: Vec<PortLeaseRequest>,
    tenant_id: &TenantId,
    maximum: usize,
    reservation_claim: &NetworkReservationClaim,
) -> Result<ReservedPortLeaseBatch> {
    let publication_lifetime = acquire_reservation_lifetime(authority, reservation_claim)?;
    let records = authority
        .reserve_batch_with_tenant_limit_for_coordinator(
            requests.clone(),
            TenantPublishedPortLimit::new(tenant_id.clone(), maximum),
            reservation_claim,
        )
        .map_err(port_lease_error)?;
    finish_reserved_batch(
        authority,
        requests,
        records,
        reservation_claim,
        publication_lifetime,
    )
}

fn finish_reserved_batch(
    authority: &LocalPortLeaseAuthority,
    requests: Vec<PortLeaseRequest>,
    records: Vec<PortLeaseRecord>,
    reservation_claim: &NetworkReservationClaim,
    publication_lifetime: NetworkReservationLifetimeGuard,
) -> Result<ReservedPortLeaseBatch> {
    match selected_ports(&requests, &records) {
        Ok(selected) => Ok(ReservedPortLeaseBatch {
            selected,
            reservation_claim: reservation_claim.clone(),
            publication_lifetime,
        }),
        Err(projection_error) => Err(compensate_projection_failure(
            authority,
            &requests,
            &publication_lifetime,
            projection_error,
        )),
    }
}

fn acquire_reservation_lifetime(
    authority: &LocalPortLeaseAuthority,
    reservation_claim: &NetworkReservationClaim,
) -> Result<NetworkReservationLifetimeGuard> {
    match authority
        .try_acquire_reservation_lifetime(reservation_claim)
        .map_err(port_lease_error)?
    {
        NetworkReservationLifetimeAttempt::Acquired(lifetime) => Ok(lifetime),
        NetworkReservationLifetimeAttempt::LiveOwner => Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox launch reservation for provider {} still has a live process owner",
                reservation_claim.coordinator_attempt().provider_id()
            ),
        }),
    }
}

fn selected_ports(
    requests: &[PortLeaseRequest],
    records: &[PortLeaseRecord],
) -> Result<Vec<(PortLeaseRequest, NonZeroU16)>> {
    if requests.len() != records.len() {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "port authority returned {} records for {} sandbox requests",
                records.len(),
                requests.len()
            ),
        });
    }
    requests
        .iter()
        .zip(records)
        .map(|(request, record)| {
            let port = record
                .reserved_port()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "sandbox port lease {} did not select a numeric host port",
                        request.lease_id()
                    ),
                })?;
            Ok((request.clone(), port))
        })
        .collect()
}

/// Release one never-bound planning batch after a coordinator failure.
pub(crate) fn release_reserved_batch_without_effect(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    reservation_claim: &NetworkReservationClaim,
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .release_reserved_batch_without_effect(requests, reservation_claim)
        .map_err(port_lease_error)
}

/// Release one exact never-bound batch while its original coordinator remains
/// live and has not yet published the canonical request set.
pub(crate) fn release_reserved_batch_with_lifetime_without_effect(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    publication_lifetime: &NetworkReservationLifetimeGuard,
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .release_reserved_batch_without_effect_with_lifetime(requests, publication_lifetime)
        .map_err(port_lease_error)
}

/// Authenticate a complete still-never-bound launch batch before effects.
pub(crate) fn verify_reserved_batch_for_coordinator(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    reservation_claim: &NetworkReservationClaim,
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .verify_reserved_batch_for_coordinator(requests, reservation_claim)
        .map_err(port_lease_error)
}

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

/// Reserve a provider-assigned identity whose numeric port is adopted later.
#[cfg(test)]
pub(crate) fn reserve_provider_assigned(
    authority: &LocalPortLeaseAuthority,
    request: PortLeaseRequest,
) -> Result<PortLeaseRequest> {
    let record = authority
        .reserve(request.clone())
        .map_err(port_lease_error)?;
    if !matches!(
        record.request().binding().port(),
        PortRequestMode::ProviderAssigned
    ) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} is not provider-assigned",
                request.lease_id()
            ),
        });
    }
    Ok(request)
}

/// Verify that a request exactly matches current durable bind authority.
///
/// `Active` is accepted for idempotent provider reconstruction after a
/// confirmed local teardown; NNC3.8 owns explicit crash/ambiguity
/// reconciliation for that reconstruction window.
pub(crate) fn require_current_bind_authority(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    if !matches!(
        record.phase(),
        PortLeasePhase::Reserved | PortLeasePhase::Binding | PortLeasePhase::Active
    ) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} is not current bind authority in phase {:?}",
                request.lease_id(),
                record.phase()
            ),
        });
    }
    Ok(record)
}

/// Logical listener identity and binding intent expected by one effect caller.
pub(crate) struct ExpectedListenerAuthority<'a> {
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    listener_name: String,
    target: PortBindTarget,
    publication: PortPublicationIntent,
    exposure: PortExposure,
    accounting: PortLeaseAccounting,
    port: Option<NonZeroU16>,
}

impl<'a> ExpectedListenerAuthority<'a> {
    /// Expected authority for one externally published sandbox endpoint.
    pub(crate) fn published(
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        listener_name: impl Into<String>,
        target: PortBindTarget,
        publication: PortPublicationIntent,
        exposure: PortExposure,
        port: NonZeroU16,
    ) -> Self {
        Self {
            tenant_id,
            sandbox_id,
            listener_name: listener_name.into(),
            target,
            publication,
            exposure,
            accounting: PortLeaseAccounting::TenantPublished,
            port: Some(port),
        }
    }

    /// Expected authority for the private per-sandbox egress PEP.
    pub(crate) fn egress_pep(
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        bind_addr: SocketAddr,
    ) -> Result<Self> {
        Ok(Self {
            tenant_id,
            sandbox_id,
            listener_name: "egress-pep".to_owned(),
            target: target_for_ip(bind_addr.ip())?,
            publication: PortPublicationIntent::Unpublished,
            exposure: PortExposure::Private,
            accounting: PortLeaseAccounting::HostInternal,
            port: NonZeroU16::new(bind_addr.port()),
        })
    }
}

/// Verify that a persisted request belongs to the named sandbox listener.
///
/// Durable record equality alone is not enough: a corrupted or cross-tenant
/// manifest must not borrow another listener's otherwise-current authority.
/// The expected port is absent only for a provider-assigned port-zero bind.
pub(crate) fn require_listener_authority(
    authority: &LocalPortLeaseAuthority,
    expected: ExpectedListenerAuthority<'_>,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let listener_id = ListenerId::for_tenant_workload_listener(
        expected.tenant_id,
        expected.sandbox_id.as_str(),
        &expected.listener_name,
    );
    let expected_lease_id = PortLeaseId::for_listener(&listener_id);
    let expected_owner = NetworkResourceId::Listener(listener_id);
    let binding = request.binding();
    let identity_matches = request.lease_id() == &expected_lease_id
        && request.owner_id() == &expected_owner
        && request.tenant_id() == Some(expected.tenant_id)
        && request.generation() == INITIAL_RESOURCE_GENERATION
        && request.lease_epoch() == INITIAL_LEASE_EPOCH
        && request.accounting() == expected.accounting;
    let binding_matches = binding.protocol() == PortProtocol::Tcp
        && binding.realm() == &PortBindRealm::Host
        && binding.target() == &expected.target
        && request.publication() == &expected.publication
        && binding.exposure() == expected.exposure
        && match (expected.port, binding.port()) {
            (Some(port), PortRequestMode::Exact(expected)) => *expected == port,
            (Some(port), PortRequestMode::Range(range)) => {
                range.start() <= port && port <= range.end()
            }
            // Provider-assigned intent deliberately carries no port in the
            // request. Authenticate the concrete port against the durable
            // record below instead of rejecting the original request shape.
            (Some(_), PortRequestMode::ProviderAssigned) => true,
            (None, PortRequestMode::ProviderAssigned) => true,
            _ => false,
        };
    if !identity_matches || !binding_matches {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox listener {:?} rejected port lease {} because its owner, tenant, \
                generation, epoch, accounting, or binding intent does not match the caller",
                expected.listener_name,
                request.lease_id()
            ),
        });
    }

    let record = inspect_exact(authority, request)?;
    if let Some(expected_port) = expected.port
        && record.reserved_port() != Some(expected_port)
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox listener {:?} port lease {} does not own expected port {}",
                expected.listener_name,
                request.lease_id(),
                expected_port
            ),
        });
    }
    Ok(record)
}

/// Verify logical listener ownership plus a phase that may create or
/// reconstruct the named provider effect.
pub(crate) fn require_current_listener_authority(
    authority: &LocalPortLeaseAuthority,
    expected: ExpectedListenerAuthority<'_>,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let record = require_listener_authority(authority, expected, request)?;
    if !matches!(
        record.phase(),
        PortLeasePhase::Reserved | PortLeasePhase::Binding | PortLeasePhase::Active
    ) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} is not current bind authority in phase {:?}",
                request.lease_id(),
                record.phase()
            ),
        });
    }
    Ok(record)
}

/// Verify logical listener ownership and one exact Active provider binding.
pub(crate) fn require_active_listener_binding(
    authority: &LocalPortLeaseAuthority,
    expected: ExpectedListenerAuthority<'_>,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = require_listener_authority(authority, expected, request)?;
    let expected_binding = provider_binding(request, actual_addr, provider)?;
    if record.phase() != PortLeasePhase::Active || record.binding() != Some(&expected_binding) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} does not carry the exact Active {:?} listener binding",
                request.lease_id(),
                provider
            ),
        });
    }
    Ok(record)
}

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
    debug_assert_eq!(adopted.phase(), PortLeasePhase::Binding);
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

/// Convert an exact dead process-bound effect into a restart-retained slot.
pub(crate) fn prepare_process_bound_rebind_after_owner_death(
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

/// Atomically adopt and activate a complete Nimbus-owned listener batch.
#[cfg(test)]
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

/// Verify that one sandbox-owned provider effect has exact active evidence.
pub(crate) fn require_active_provider_binding(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    let expected = provider_binding(request, actual_addr, provider)?;
    if record.phase() != PortLeasePhase::Active || record.binding() != Some(&expected) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} cannot start provider {:?}: expected exact Active binding \
                 evidence, found phase {:?}",
                request.lease_id(),
                provider,
                record.phase()
            ),
        });
    }
    Ok(record)
}

/// Verify exact provider evidence for final cleanup after durable withdrawal.
///
/// Final release may begin from `Active` or resume from `Withdrawing`; every
/// other phase either lacks a live effect or belongs to a different lifecycle
/// disposition. The exact binding remains mandatory in both accepted phases.
pub(crate) fn require_releasable_provider_binding(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    let expected = provider_binding(request, actual_addr, provider)?;
    if !matches!(
        record.phase(),
        PortLeasePhase::Active | PortLeasePhase::Withdrawing
    ) || record.binding() != Some(&expected)
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} cannot release provider {:?}: expected exact Active or \
                 Withdrawing binding evidence, found phase {:?}",
                request.lease_id(),
                provider,
                record.phase()
            ),
        });
    }
    Ok(record)
}

/// Verify exact provider evidence that may require owner-death recovery.
pub(crate) fn require_provider_recovery_binding(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(authority, request)?;
    let expected = provider_binding(request, actual_addr, provider)?;
    if !matches!(
        record.phase(),
        PortLeasePhase::Active | PortLeasePhase::Withdrawing | PortLeasePhase::CleanupPending
    ) || record.binding() != Some(&expected)
        || record.active_lifetime().is_none()
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} cannot recover provider {:?}: expected exact Active, \
                 Withdrawing, or CleanupPending lifetime-fenced binding evidence, found phase {:?}",
                request.lease_id(),
                provider,
                record.phase()
            ),
        });
    }
    Ok(record)
}

/// Record a confirmed no-effect provider bind failure.
#[cfg(test)]
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

/// Fence new use before the provider effect is stopped or detached.
pub(crate) fn withdraw(
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
pub(crate) fn prepare_rebind_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    expected_binding: &PortLeaseBinding,
) -> Result<PortLeaseRecord> {
    authority
        .prepare_rebind_after_confirmed_stop(request, expected_binding)
        .map_err(port_lease_error)
}

/// Retain one exact live-owner listener after acknowledged provider stop.
pub(crate) fn prepare_rebind_after_confirmed_stop_with_lifetime(
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
pub(crate) fn prepare_rebind_batch_after_confirmed_stop_with_lifetimes(
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

/// Atomically release a live provider batch after exact provider stop.
pub(crate) fn release_provider_managed_batch_after_confirmed_stop_with_lifetimes(
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
pub(crate) fn recover_provider_managed_batch_after_owner_death(
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

/// Retain a recovered provider batch after the adapter confirms exact absence.
pub(crate) fn prepare_provider_managed_batch_after_confirmed_stop(
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

/// Retire a recovered provider-claim batch while retaining its exact slots.
pub(crate) fn prepare_provider_managed_claim_batch_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    recoveries: &[PortLeaseRecoveryGuard],
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .prepare_rebind_provider_managed_claim_batch_after_confirmed_stop(requests, recoveries)
        .map_err(port_lease_error)
}

/// Release a recovered provider batch after the adapter confirms exact absence.
pub(crate) fn release_provider_managed_batch_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    recoveries: &[PortLeaseRecoveryGuard],
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .release_provider_managed_batch_after_confirmed_stop(requests, recoveries)
        .map_err(port_lease_error)
}

/// Release the numeric slot only after provider effect removal is confirmed.
pub(crate) fn release(
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
pub(crate) fn release_with_lifetime(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
    lifetime: &PortLeaseLifetimeGuard,
) -> Result<PortLeaseRecord> {
    authority
        .release_with_lifetime(request, lifetime)
        .map_err(port_lease_error)
}

/// Release a restart-retained slot using its exact durable stopped-binding receipt.
pub(crate) fn release_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    authority
        .release_after_confirmed_stop(request)
        .map_err(port_lease_error)
}

/// Atomically release a complete restart-retained listener batch.
pub(crate) fn release_batch_after_confirmed_stop(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .release_batch_after_confirmed_stop(requests)
        .map_err(port_lease_error)
}

pub(crate) fn target_for_ip(ip: IpAddr) -> Result<PortBindTarget> {
    match canonical_socket_ip(ip) {
        IpAddr::V4(address) if address.is_unspecified() => Ok(PortBindTarget::ipv4_wildcard()),
        IpAddr::V4(address) => Ok(PortBindTarget::ipv4_specific(address)),
        IpAddr::V6(address) if address.is_unspecified() => {
            Ok(PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown))
        }
        IpAddr::V6(address) => PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!("invalid sandbox socket bind target: {error}"),
            }),
    }
}

/// Normalize one published host address into portable target and reachability.
pub(crate) fn published_scope(ip: IpAddr) -> Result<(PortBindTarget, PortExposure)> {
    let ip = canonical_socket_ip(ip);
    let exposure = match ip {
        IpAddr::V4(address) if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V4(address) if address.is_private() || address.is_link_local() => {
            PortExposure::Private
        }
        IpAddr::V6(address) if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V6(address) if address.is_unique_local() || address.is_unicast_link_local() => {
            PortExposure::Private
        }
        IpAddr::V4(_) | IpAddr::V6(_) => PortExposure::Public,
    };
    Ok((target_for_ip(ip)?, exposure))
}

pub(crate) fn canonical_socket_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(address) => address
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(address)),
        IpAddr::V4(_) => ip,
    }
}

pub(crate) fn provider_binding(
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseBinding> {
    let actual_port =
        NonZeroU16::new(actual_addr.port()).ok_or_else(|| SandboxError::OperationFailed {
            message: "sandbox provider reported an active TCP binding on port zero".to_owned(),
        })?;
    let endpoint = PortBoundEndpoint::new(
        PortProtocol::Tcp,
        PortBindRealm::Host,
        target_for_ip(actual_addr.ip())?,
        actual_port,
    )
    .map_err(|error| SandboxError::OperationFailed {
        message: format!("invalid sandbox provider bind evidence: {error}"),
    })?;
    Ok(PortLeaseBinding::new(
        endpoint,
        if matches!(request.binding().port(), PortRequestMode::ProviderAssigned) {
            PortBindingProvenance::ProviderAssigned
        } else {
            PortBindingProvenance::NimbusOwned
        },
        provider_handle(request, provider)?,
    ))
}

fn provider_handle(
    request: &PortLeaseRequest,
    provider: OciPortProvider,
) -> Result<NetworkProviderHandle> {
    NetworkProviderHandle::new(
        NetworkProviderId::for_registration_key(provider.registration_key()),
        format!("{}:{}", provider.registration_key(), request.lease_id()),
    )
    .map_err(|error| SandboxError::OperationFailed {
        message: format!("invalid sandbox port provider handle: {error}"),
    })
}

fn provider_bind_claim(
    request: &PortLeaseRequest,
    provider: OciPortProvider,
) -> Result<PortBindClaim> {
    NetworkProviderHandle::new(
        NetworkProviderId::for_registration_key(provider.registration_key()),
        format!(
            "{}:{}:{}",
            provider.registration_key(),
            request.lease_id(),
            Ulid::new()
        ),
    )
    .map(PortBindClaim::new)
    .map_err(|error| SandboxError::OperationFailed {
        message: format!("invalid sandbox port bind claim: {error}"),
    })
}

fn failure_kind(kind: io::ErrorKind) -> PortBindFailureKind {
    match kind {
        io::ErrorKind::AddrInUse => PortBindFailureKind::AddrInUse,
        io::ErrorKind::PermissionDenied => PortBindFailureKind::PermissionDenied,
        io::ErrorKind::AddrNotAvailable => PortBindFailureKind::AddressNotAvailable,
        io::ErrorKind::Unsupported => PortBindFailureKind::Unsupported,
        io::ErrorKind::OutOfMemory | io::ErrorKind::StorageFull => {
            PortBindFailureKind::ResourceExhausted
        }
        _ => PortBindFailureKind::Other,
    }
}

pub(crate) fn inspect_exact(
    authority: &LocalPortLeaseAuthority,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let record = authority
        .inspect(request.lease_id())
        .map_err(port_lease_error)?
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} has no durable reservation",
                request.lease_id()
            ),
        })?;
    if record.request() != request {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "sandbox port lease {} does not match its durable identity and fence",
                request.lease_id()
            ),
        });
    }
    Ok(record)
}

pub(crate) fn port_lease_error(error: impl std::fmt::Display) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("sandbox port lease authority rejected the operation: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    use super::{published_scope, target_for_ip};
    use nimbus_network::PortExposure;

    #[test]
    fn ipv4_mapped_ipv6_socket_target_normalizes_without_panicking() {
        let mapped = Ipv6Addr::from([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 127, 0, 0, 1]);

        let target = target_for_ip(IpAddr::V6(mapped))
            .expect("IPv4-mapped socket addresses should normalize to portable IPv4");

        assert_eq!(
            target.specific_address(),
            Some(IpAddr::V4(Ipv4Addr::LOCALHOST))
        );
    }

    #[test]
    fn published_scope_preserves_loopback_private_and_public_reachability() {
        let (_, loopback) =
            published_scope(IpAddr::V4(Ipv4Addr::LOCALHOST)).expect("loopback scope");
        let (_, private) =
            published_scope(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1))).expect("private scope");
        let (_, public) =
            published_scope(IpAddr::V4(Ipv4Addr::UNSPECIFIED)).expect("wildcard scope");

        assert_eq!(loopback, PortExposure::Loopback);
        assert_eq!(private, PortExposure::Private);
        assert_eq!(public, PortExposure::Public);
    }
}
