//! Durable PEP assignment shape and bridge-gateway registration composition.

use std::net::{IpAddr, SocketAddr};
#[cfg(test)]
use std::num::NonZeroU16;

use nimbus_core::TenantId;
use nimbus_egress::EgressPolicy;
use nimbus_network::{
    NetworkLeaseEpoch, NetworkProviderId, PortBindRealm, PortExposure, PortLeaseAccounting,
    PortLeaseId, PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};
use serde::{Deserialize, Serialize};

use crate::backends::oci::network::{OciNetworkConfig, bridge_gateway_addr};
use crate::backends::oci::port_lease::target_for_ip;
#[cfg(test)]
use crate::backends::oci::port_lease::{
    OciPortLeaseIntent, port_lease_request, reserve_provider_assigned,
};
#[cfg(test)]
use crate::backends::oci::port_lifecycle::OciPortLeaseCoordinator;
use crate::backends::oci::port_lifecycle::{InternalListenerReservation, ReservedInternalListener};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::provision::SandboxProvisionNetworkPlan;

use super::{EgressProxyRegistry, PepPreAdoptionReleaseAuthority};
use crate::backends::capabilities::SANDBOX_EGRESS_PEP_PROVIDER_KEY;

/// Tier-neutral host-side egress PEP assignment for an execute-mode sandbox.
///
/// The proxy binds on the bridge gateway address so it is the only reachable
/// outbound path from inside the sandbox's deny-by-default network namespace.
/// Every sandbox backend persists this same shape and renders the guest-facing
/// proxy URL through [`EgressProxyAssignment::proxy_url`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct EgressProxyAssignment {
    pub(crate) host: String,
    pub(crate) port: u16,
    pub(crate) port_lease: PortLeaseRequest,
}

impl EgressProxyAssignment {
    pub(crate) fn compiled_plan_members(
        &self,
        plan: &SandboxProvisionNetworkPlan,
    ) -> Vec<PortLeaseRequest> {
        let mut members = plan.port_leases();
        members.push(self.port_lease.clone());
        members
    }

    /// Authenticate this concrete assignment against the compiler-issued PEP
    /// listener identity and network-plan fence.
    pub(crate) fn require_compiled_plan_authority(
        &self,
        tenant_id: &TenantId,
        plan: &SandboxProvisionNetworkPlan,
    ) -> Result<()> {
        let dependency = plan
            .dependency_listeners()
            .iter()
            .find(|dependency| dependency.name() == "egress-pep")
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "compiled network plan omitted its egress PEP listener".to_owned(),
            })?;
        let expected_provider =
            NetworkProviderId::for_registration_key(SANDBOX_EGRESS_PEP_PROVIDER_KEY);
        let bind_addr = self.bind_addr()?;
        let requested_port_matches = match self.port_lease.binding().port() {
            PortRequestMode::Exact(port) => port.get() == bind_addr.port(),
            PortRequestMode::Range(range) => {
                range.start().get() <= bind_addr.port() && bind_addr.port() <= range.end().get()
            }
            PortRequestMode::ProviderAssigned => true,
        };
        let expected_owner = dependency.listener_id().clone().into();
        if plan.tenant_id() != tenant_id
            || dependency.provider_id() != &expected_provider
            || self.port_lease.lease_id() != &PortLeaseId::for_listener(dependency.listener_id())
            || self.port_lease.owner_id() != &expected_owner
            || self.port_lease.plan_id() != Some(plan.plan_id())
            || self.port_lease.tenant_id() != Some(tenant_id)
            || self.port_lease.generation() != plan.generation()
            || self.port_lease.lease_epoch() != NetworkLeaseEpoch::new(1)
            || self.port_lease.accounting() != PortLeaseAccounting::HostInternal
            || self.port_lease.publication() != &PortPublicationIntent::Unpublished
            || self.port_lease.binding().protocol() != PortProtocol::Tcp
            || self.port_lease.binding().realm() != &PortBindRealm::Host
            || self.port_lease.binding().target() != &target_for_ip(bind_addr.ip())?
            || self.port_lease.binding().exposure() != PortExposure::Private
            || !requested_port_matches
        {
            return Err(SandboxError::OperationFailed {
                message: "egress PEP assignment crossed its compiled listener, plan, generation, or binding authority".to_owned(),
            });
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn for_test(host: &str, port: u16) -> Self {
        let tenant_id = TenantId::new("egress-assignment-test").expect("static tenant id");
        let sandbox_id = SandboxId::new(format!("egress-assignment-{port}"));
        let ip = host
            .parse::<IpAddr>()
            .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));
        let mode = NonZeroU16::new(port)
            .map(nimbus_network::PortRequestMode::Exact)
            .unwrap_or(nimbus_network::PortRequestMode::ProviderAssigned);
        Self {
            host: host.to_owned(),
            port,
            port_lease: port_lease_request(
                &tenant_id,
                &sandbox_id,
                "egress-pep",
                OciPortLeaseIntent::host_internal(
                    target_for_ip(ip)
                        .expect("parsed test IP should produce a portable bind target"),
                    PortExposure::Private,
                ),
                mode,
            ),
        }
    }

    /// Bind address the PEP listens on. The host must be an IP literal (the
    /// bridge gateway), so a non-IP value fails closed as an invalid spec.
    pub(crate) fn bind_addr(&self) -> Result<SocketAddr> {
        let host = self
            .host
            .parse::<IpAddr>()
            .map_err(|_| SandboxError::InvalidSpec {
                message: format!("egress proxy host {:?} must be an IP address", self.host),
            })?;
        Ok(SocketAddr::new(host, self.port))
    }

    /// Container-shape proxy URL the guest env is pointed at. Rendering through
    /// [`SocketAddr`] brackets IPv6 gateways correctly.
    pub(crate) fn proxy_url(&self) -> Result<String> {
        Ok(format!("http://{}", self.bind_addr()?))
    }
}

/// Assign a test PEP on the bridge gateway through the legacy test manager.
#[cfg(test)]
pub(crate) fn allocate_egress_proxy(
    network_config: &OciNetworkConfig,
    port_lease_coordinator: &OciPortLeaseCoordinator,
    tenant_id: &TenantId,
    id: &SandboxId,
) -> Result<EgressProxyAssignment> {
    let gateway = bridge_gateway_addr(network_config)?;
    let (port, port_lease) = port_lease_coordinator.reserve_internal_listener(
        tenant_id,
        id,
        "egress-pep",
        target_for_ip(IpAddr::V4(gateway))?,
        PortExposure::Private,
    )?;
    Ok(EgressProxyAssignment {
        host: gateway.to_string(),
        port,
        port_lease,
    })
}

/// Portable egress-listener intent to include in one sandbox launch batch.
pub(crate) fn egress_listener_reservation(
    network_config: &OciNetworkConfig,
) -> Result<InternalListenerReservation> {
    let gateway = bridge_gateway_addr(network_config)?;
    Ok(InternalListenerReservation::new(
        "egress-pep",
        target_for_ip(IpAddr::V4(gateway))?,
        PortExposure::Private,
    ))
}

/// Convert one atomically reserved internal listener into persisted PEP state.
pub(crate) fn egress_proxy_assignment(
    network_config: &OciNetworkConfig,
    reservation: ReservedInternalListener,
) -> Result<EgressProxyAssignment> {
    Ok(EgressProxyAssignment {
        host: bridge_gateway_addr(network_config)?.to_string(),
        port: reservation.port,
        port_lease: reservation.lease,
    })
}

/// Build an exact persisted assignment from a real reserved listener while
/// allowing a loopback bind on non-Linux test hosts.
#[cfg(test)]
pub(crate) fn egress_proxy_assignment_for_test(
    host: IpAddr,
    reservation: ReservedInternalListener,
) -> EgressProxyAssignment {
    EgressProxyAssignment {
        host: host.to_string(),
        port: reservation.port,
        port_lease: reservation.lease,
    }
}

/// Start the PEP or authenticate the already-running exact assignment.
pub(crate) fn ensure_egress_proxy_running(
    registry: &EgressProxyRegistry,
    tenant_id: &TenantId,
    id: &SandboxId,
    assignment: Option<&EgressProxyAssignment>,
    policy: &EgressPolicy,
) -> Result<()> {
    ensure_egress_proxy_running_with_release_authority(
        registry,
        tenant_id,
        id,
        assignment,
        policy,
        PepPreAdoptionReleaseAuthority::Retain,
    )
}

pub(crate) fn ensure_egress_proxy_running_with_release_authority(
    registry: &EgressProxyRegistry,
    tenant_id: &TenantId,
    id: &SandboxId,
    assignment: Option<&EgressProxyAssignment>,
    policy: &EgressPolicy,
    release_authority: PepPreAdoptionReleaseAuthority<'_>,
) -> Result<()> {
    let Some(assignment) = assignment else {
        return Err(SandboxError::OperationFailed {
            message: format!("sandbox {id} has no egress proxy assignment"),
        });
    };
    let bind_addr = assignment.bind_addr()?;
    #[cfg(test)]
    let test_port_lease = if assignment.port == 0 {
        let request = port_lease_request(
            tenant_id,
            id,
            "egress-pep",
            OciPortLeaseIntent::host_internal(
                target_for_ip(bind_addr.ip())?,
                PortExposure::Private,
            ),
            nimbus_network::PortRequestMode::ProviderAssigned,
        );
        Some(reserve_provider_assigned(
            registry.port_authority()?,
            request,
        )?)
    } else {
        None
    };
    #[cfg(test)]
    let port_lease = test_port_lease.as_ref().unwrap_or(&assignment.port_lease);
    #[cfg(not(test))]
    let port_lease = &assignment.port_lease;
    registry.ensure_running_with_lease_and_release_authority(
        tenant_id,
        id,
        policy,
        bind_addr,
        port_lease,
        release_authority,
    )
}
