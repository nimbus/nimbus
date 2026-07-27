//! OCI-family adapter between durable portable port leases and provider effects.
//!
//! `nimbus-network` owns reservation identity and lifecycle. This module owns
//! the sandbox-specific translation from real socket/Netavark observations to
//! portable bind evidence; it never allocates by probing or scanning manifests.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::num::NonZeroU16;
use std::path::Path;

use nimbus_core::TenantId;
use nimbus_network::{
    ListenerId, LocalPortLeaseAuthority, NetworkLeaseEpoch, NetworkProviderHandle,
    NetworkProviderId, NetworkReservationClaim, NetworkResourceGeneration, NetworkResourceId,
    PortBindAttempt, PortBindClaim, PortBindFailure, PortBindFailureKind, PortBindRealm,
    PortBindTarget, PortBindingProvenance, PortBindingSpec, PortBoundEndpoint, PortExposure,
    PortIpv6Overlap, PortLeaseAccounting, PortLeaseBinding, PortLeaseId, PortLeasePhase,
    PortLeaseRecord, PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
    TenantPublishedPortLimit,
};
use ulid::Ulid;

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

pub(crate) struct ReservedPortLeaseBatch {
    selected: Vec<(PortLeaseRequest, NonZeroU16)>,
    reservation_claim: NetworkReservationClaim,
}

impl ReservedPortLeaseBatch {
    pub(crate) fn into_parts(
        self,
    ) -> (Vec<(PortLeaseRequest, NonZeroU16)>, NetworkReservationClaim) {
        (self.selected, self.reservation_claim)
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
            Self::EgressPep => "nimbus-sandbox.egress-pep",
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
    state_root: &Path,
    request: PortLeaseRequest,
) -> Result<(PortLeaseRequest, NonZeroU16, NetworkReservationClaim)> {
    let authority = open_authority(state_root)?;
    let reservation_claim = new_launch_reservation_claim()?;
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
                &authority,
                std::slice::from_ref(&request),
                &reservation_claim,
                projection_error,
            ));
        }
    };
    Ok((request, port, reservation_claim))
}

fn compensate_projection_failure(
    authority: &LocalPortLeaseAuthority,
    requests: &[PortLeaseRequest],
    reservation_claim: &NetworkReservationClaim,
    projection_error: SandboxError,
) -> SandboxError {
    match authority.release_reserved_batch_without_effect(requests, reservation_claim) {
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
    state_root: &Path,
    requests: Vec<PortLeaseRequest>,
    reservation_claim: &NetworkReservationClaim,
) -> Result<ReservedPortLeaseBatch> {
    let authority = open_authority(state_root)?;
    let records = authority
        .reserve_batch_for_coordinator(requests.clone(), reservation_claim)
        .map_err(port_lease_error)?;
    finish_reserved_batch(authority, requests, records, reservation_claim)
}

/// Reserve a complete launch batch under one caller-supplied tenant limit.
pub(crate) fn reserve_batch_with_tenant_limit(
    state_root: &Path,
    requests: Vec<PortLeaseRequest>,
    tenant_id: &TenantId,
    maximum: usize,
    reservation_claim: &NetworkReservationClaim,
) -> Result<ReservedPortLeaseBatch> {
    let authority = open_authority(state_root)?;
    let records = authority
        .reserve_batch_with_tenant_limit_for_coordinator(
            requests.clone(),
            TenantPublishedPortLimit::new(tenant_id.clone(), maximum),
            reservation_claim,
        )
        .map_err(port_lease_error)?;
    finish_reserved_batch(authority, requests, records, reservation_claim)
}

fn finish_reserved_batch(
    authority: LocalPortLeaseAuthority,
    requests: Vec<PortLeaseRequest>,
    records: Vec<PortLeaseRecord>,
    reservation_claim: &NetworkReservationClaim,
) -> Result<ReservedPortLeaseBatch> {
    match selected_ports(&requests, &records) {
        Ok(selected) => Ok(ReservedPortLeaseBatch {
            selected,
            reservation_claim: reservation_claim.clone(),
        }),
        Err(projection_error) => Err(compensate_projection_failure(
            &authority,
            &requests,
            reservation_claim,
            projection_error,
        )),
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
    state_root: &Path,
    requests: &[PortLeaseRequest],
    reservation_claim: &NetworkReservationClaim,
) -> Result<Vec<PortLeaseRecord>> {
    open_authority(state_root)?
        .release_reserved_batch_without_effect(requests, reservation_claim)
        .map_err(port_lease_error)
}

/// Authenticate a complete still-never-bound launch batch before effects.
pub(crate) fn verify_reserved_batch_for_coordinator(
    state_root: &Path,
    requests: &[PortLeaseRequest],
    reservation_claim: &NetworkReservationClaim,
) -> Result<Vec<PortLeaseRecord>> {
    open_authority(state_root)?
        .verify_reserved_batch_for_coordinator(requests, reservation_claim)
        .map_err(port_lease_error)
}

/// Claim a complete Nimbus-owned listener batch before any provider bind.
pub(crate) fn claim_bind_attempts(
    state_root: &Path,
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
    open_authority(state_root)?
        .claim_bind_batch(&claimed, reservation_claim)
        .map_err(port_lease_error)?;
    Ok(claims)
}

/// Relinquish exact bind claims after all corresponding effects are absent.
pub(crate) fn abandon_bind_attempts_without_effect(
    state_root: &Path,
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
    open_authority(state_root)?
        .abandon_bind_claims_without_effect(&claimed, reservation_claim)
        .map_err(port_lease_error)
}

/// Reserve a provider-assigned identity whose numeric port is adopted later.
#[cfg(test)]
pub(crate) fn reserve_provider_assigned(
    state_root: &Path,
    request: PortLeaseRequest,
) -> Result<PortLeaseRequest> {
    let authority = open_authority(state_root)?;
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
    state_root: &Path,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(state_root, request)?;
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
    state_root: &Path,
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

    let record = inspect_exact(state_root, request)?;
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
    state_root: &Path,
    expected: ExpectedListenerAuthority<'_>,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    let record = require_listener_authority(state_root, expected, request)?;
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

/// Adopt and activate a successful bind owned by the exact durable claim.
pub(crate) fn adopt_claimed_and_activate(
    state_root: &Path,
    request: &PortLeaseRequest,
    reservation_claim: Option<&NetworkReservationClaim>,
    claim: &PortBindClaim,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let authority = open_authority(state_root)?;
    let binding = provider_binding(request, actual_addr, provider)?;
    let adopted = authority
        .adopt_claimed(request, reservation_claim, claim, binding)
        .map_err(port_lease_error)?;
    debug_assert_eq!(adopted.phase(), PortLeasePhase::Binding);
    authority
        .activate_claimed(request, claim)
        .map_err(port_lease_error)
}

/// Atomically adopt and activate a complete Nimbus-owned listener batch.
pub(crate) fn adopt_claimed_and_activate_batch(
    state_root: &Path,
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
    open_authority(state_root)?
        .adopt_claimed_and_activate_batch(&bindings, reservation_claim)
        .map_err(port_lease_error)
}

/// Verify that one sandbox-owned provider effect has exact active evidence.
pub(crate) fn require_active_provider_binding(
    state_root: &Path,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(state_root, request)?;
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
    state_root: &Path,
    request: &PortLeaseRequest,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
) -> Result<PortLeaseRecord> {
    let record = inspect_exact(state_root, request)?;
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

/// Record a confirmed no-effect provider bind failure.
pub(crate) fn record_bind_failure(
    state_root: &Path,
    request: &PortLeaseRequest,
    claim: &PortBindClaim,
    attempted_addr: SocketAddr,
    provider: OciPortProvider,
    error_kind: io::ErrorKind,
    reservation_claim: Option<&NetworkReservationClaim>,
) -> Result<PortLeaseRecord> {
    let authority = open_authority(state_root)?;
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
    authority
        .record_claimed_bind_failure_without_effect(request, reservation_claim, claim, failure)
        .map_err(port_lease_error)
}

/// Fence new use before the provider effect is stopped or detached.
pub(crate) fn withdraw(state_root: &Path, request: &PortLeaseRequest) -> Result<PortLeaseRecord> {
    let authority = open_authority(state_root)?;
    let record = inspect_exact_with(&authority, request)?;
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
    state_root: &Path,
    request: &PortLeaseRequest,
    expected_binding: &PortLeaseBinding,
) -> Result<PortLeaseRecord> {
    open_authority(state_root)?
        .prepare_rebind_after_confirmed_stop(request, expected_binding)
        .map_err(port_lease_error)
}

/// Atomically retain an exact stopped listener batch for same-generation rebind.
pub(crate) fn prepare_rebind_batch_after_confirmed_stop(
    state_root: &Path,
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
    open_authority(state_root)?
        .prepare_rebind_batch_after_confirmed_stop(&expected)
        .map_err(port_lease_error)
}

/// Release the numeric slot only after provider effect removal is confirmed.
pub(crate) fn release(state_root: &Path, request: &PortLeaseRequest) -> Result<PortLeaseRecord> {
    let authority = open_authority(state_root)?;
    let record = inspect_exact_with(&authority, request)?;
    if matches!(
        record.phase(),
        PortLeasePhase::Released | PortLeasePhase::Failed
    ) {
        return Ok(record);
    }
    authority.release(request).map_err(port_lease_error)
}

/// Release a restart-retained slot using its exact durable stopped-binding receipt.
pub(crate) fn release_after_confirmed_stop(
    state_root: &Path,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    open_authority(state_root)?
        .release_after_confirmed_stop(request)
        .map_err(port_lease_error)
}

/// Atomically release a complete restart-retained listener batch.
pub(crate) fn release_batch_after_confirmed_stop(
    state_root: &Path,
    requests: &[PortLeaseRequest],
) -> Result<Vec<PortLeaseRecord>> {
    open_authority(state_root)?
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
    state_root: &Path,
    request: &PortLeaseRequest,
) -> Result<PortLeaseRecord> {
    inspect_exact_with(&open_authority(state_root)?, request)
}

fn inspect_exact_with(
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

fn open_authority(state_root: &Path) -> Result<LocalPortLeaseAuthority> {
    LocalPortLeaseAuthority::open(state_root).map_err(port_lease_error)
}

fn port_lease_error(error: impl std::fmt::Display) -> SandboxError {
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
