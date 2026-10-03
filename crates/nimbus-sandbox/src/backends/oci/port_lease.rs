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
    NetworkReservationLifetimeGuard, NetworkResourceGeneration, PortBindClaim, PortBindFailureKind,
    PortBindRealm, PortBindTarget, PortBindingProvenance, PortBindingSpec, PortBoundEndpoint,
    PortExposure, PortIpv6Overlap, PortLeaseAccounting, PortLeaseBinding, PortLeaseLifetimeGuard,
    PortLeaseRecord, PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
    TenantPublishedPortLimit,
};
use ulid::Ulid;

use crate::backends::capabilities::SANDBOX_EGRESS_PEP_PROVIDER_KEY;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

mod bind_claim;
mod bind_outcome;
mod binding_authority;
mod teardown;

#[cfg(any(test, feature = "test-hooks"))]
pub use bind_claim::claim_bind_attempts;
pub use bind_claim::{
    abandon_bind_attempt_with_lifetime_without_effect,
    abandon_bind_attempts_with_lifetimes_without_effect, abandon_bind_attempts_without_effect,
    abandon_bind_plan_member_attempt_with_lifetime_without_effect,
    abandon_bind_plan_members_attempts_with_lifetimes_without_effect,
    abandon_rebind_plan_member_attempt_with_lifetime_without_effect,
    abandon_rebind_plan_members_attempts_with_lifetimes_without_effect,
    claim_bind_attempt_with_lifetime, claim_bind_attempts_with_lifetimes,
    claim_bind_plan_member_attempt_with_lifetime, claim_bind_plan_members_attempts_with_lifetimes,
    claim_rebind_plan_member_attempt_with_lifetime,
    claim_rebind_plan_members_attempts_with_lifetimes,
};
#[cfg(any(test, feature = "test-hooks"))]
pub(crate) use bind_outcome::adopt_claimed_and_activate_batch;
#[cfg(test)]
pub(crate) use bind_outcome::{adopt_claimed_and_activate, record_bind_failure};
pub(crate) use bind_outcome::{
    adopt_claimed_and_activate_batch_with_lifetimes,
    adopt_claimed_and_activate_plan_member_with_lifetime,
    adopt_claimed_and_activate_plan_members_with_lifetimes,
    adopt_claimed_and_activate_rebind_plan_member_with_lifetime,
    adopt_claimed_and_activate_rebind_plan_members_with_lifetimes,
    adopt_claimed_and_activate_with_lifetime, record_bind_failure_with_lifetime,
    record_plan_member_bind_failure_with_lifetime,
};
pub(crate) use binding_authority::{
    ExpectedListenerAuthority, require_active_listener_binding, require_active_provider_binding,
    require_current_bind_authority, require_current_listener_authority, require_listener_authority,
    require_provider_recovery_binding, require_releasable_provider_binding,
};
#[cfg(test)]
pub(crate) use teardown::prepare_rebind_batch_after_confirmed_stop;
pub use teardown::{
    prepare_process_bound_plan_member_rebind_after_owner_death,
    prepare_process_bound_rebind_after_owner_death,
    prepare_provider_managed_batch_after_confirmed_stop,
    prepare_provider_managed_claim_batch_after_confirmed_stop,
    prepare_provider_managed_plan_claims_after_confirmed_stop,
    prepare_provider_managed_plan_members_after_confirmed_stop,
    prepare_provider_managed_plan_members_after_confirmed_stop_with_lifetimes,
    prepare_rebind_after_confirmed_stop, prepare_rebind_after_confirmed_stop_with_lifetime,
    prepare_rebind_batch_after_confirmed_stop_with_lifetimes,
    recover_provider_managed_batch_after_owner_death,
    recover_provider_managed_plan_members_after_owner_death, release, release_after_confirmed_stop,
    release_batch_after_confirmed_stop, release_provider_managed_batch_after_confirmed_stop,
    release_provider_managed_batch_after_confirmed_stop_with_lifetimes, release_with_lifetime,
    withdraw,
};

const INITIAL_RESOURCE_GENERATION: NetworkResourceGeneration = NetworkResourceGeneration::new(1);
const INITIAL_LEASE_EPOCH: NetworkLeaseEpoch = NetworkLeaseEpoch::new(1);
const RESERVATION_COORDINATOR_KEY: &str = "nimbus-sandbox.network-launch-coordinator";

/// Sandbox effect owner that interprets one durable provider handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OciPortProvider {
    Netavark,
    MachinePortProxy,
    EgressPep,
}

pub(crate) struct OciPortActivation<'a> {
    claim: &'a PortBindClaim,
    actual_addr: SocketAddr,
    provider: OciPortProvider,
    lifetime: &'a PortLeaseLifetimeGuard,
}

impl<'a> OciPortActivation<'a> {
    pub(crate) fn new(
        claim: &'a PortBindClaim,
        actual_addr: SocketAddr,
        provider: OciPortProvider,
        lifetime: &'a PortLeaseLifetimeGuard,
    ) -> Self {
        Self {
            claim,
            actual_addr,
            provider,
            lifetime,
        }
    }
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

pub(crate) struct ReservedPortLeaseRecordBatch {
    records: Vec<PortLeaseRecord>,
    reservation_claim: NetworkReservationClaim,
    publication_lifetime: NetworkReservationLifetimeGuard,
}

/// Exact provider claims and non-cloneable process lifetimes for one batch.
pub struct OciPortBindLifetimeBatch {
    claims: Vec<PortBindClaim>,
    lifetimes: Vec<PortLeaseLifetimeGuard>,
}

impl OciPortBindLifetimeBatch {
    pub fn claims(&self) -> &[PortBindClaim] {
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

impl ReservedPortLeaseRecordBatch {
    pub(crate) fn into_parts(
        self,
    ) -> (
        Vec<PortLeaseRecord>,
        NetworkReservationClaim,
        NetworkReservationLifetimeGuard,
    ) {
        (
            self.records,
            self.reservation_claim,
            self.publication_lifetime,
        )
    }
}

pub fn new_launch_reservation_claim() -> Result<NetworkReservationClaim> {
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

/// Reserve an exact request batch without requiring every request to have a
/// numeric port yet. Provider-assigned listeners remain durable unbound
/// authority until their effect owner binds port zero and adopts the result.
pub(crate) fn reserve_request_batch(
    authority: &LocalPortLeaseAuthority,
    requests: Vec<PortLeaseRequest>,
    tenant_limit: Option<(&TenantId, usize)>,
    reservation_claim: &NetworkReservationClaim,
) -> Result<ReservedPortLeaseRecordBatch> {
    let publication_lifetime = acquire_reservation_lifetime(authority, reservation_claim)?;
    let records = match tenant_limit {
        Some((tenant_id, maximum)) => authority.reserve_batch_with_tenant_limit_for_coordinator(
            requests.clone(),
            TenantPublishedPortLimit::new(tenant_id.clone(), maximum),
            reservation_claim,
        ),
        None => authority.reserve_batch_for_coordinator(requests.clone(), reservation_claim),
    }
    .map_err(port_lease_error)?;
    if records.len() != requests.len()
        || records
            .iter()
            .zip(&requests)
            .any(|(record, request)| record.request() != request)
    {
        let projection_error = SandboxError::OperationFailed {
            message: "port authority returned a crossed exact request batch".to_owned(),
        };
        return Err(compensate_projection_failure(
            authority,
            &requests,
            &publication_lifetime,
            projection_error,
        ));
    }
    Ok(ReservedPortLeaseRecordBatch {
        records,
        reservation_claim: reservation_claim.clone(),
        publication_lifetime,
    })
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

/// Release one never-bound provider subset against its complete plan witness.
pub(crate) fn release_reserved_plan_members_without_effect(
    authority: &LocalPortLeaseAuthority,
    plan_members: &[PortLeaseRequest],
    requests: &[PortLeaseRequest],
    reservation_claim: &NetworkReservationClaim,
) -> Result<Vec<PortLeaseRecord>> {
    authority
        .release_reserved_plan_members_without_effect(plan_members, requests, reservation_claim)
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

/// Reserve a provider-assigned identity whose numeric port is adopted later.
#[cfg(any(test, feature = "test-hooks"))]
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
