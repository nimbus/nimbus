//! Exact compiled-plan fixtures for sandbox provider contract tests.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::num::NonZeroU16;

use nimbus_network::{
    ListenerId, NetworkAttachmentId, NetworkLeaseEpoch, NetworkPlan, NetworkPlanContentDigest,
    NetworkPlanId, NetworkResourceGeneration, PortBindRealm, PortBindTarget, PortBindingSpec,
    PortExposure, PortIpv6Overlap, PortLeaseAccounting, PortLeaseFence, PortLeaseId,
    PortLeaseRequest, PortProtocol, PortPublicationIntent, PortRequestMode,
};

use super::{
    SandboxProvisionDependencyListener, SandboxProvisionEndpointIdentity, SandboxProvisionListener,
    SandboxProvisionNetworkPlan,
};
use crate::backends::sandbox_network_plan_requirements;
use crate::instance::SandboxId;
use crate::spec::SandboxSpec;

/// Build a fully authenticated upper-compiled input with identities that differ
/// from the legacy sandbox-derived defaults.
pub(crate) fn sandbox_provision_network_plan_fixture(
    spec: &SandboxSpec,
    sandbox_id: &SandboxId,
    label: &str,
) -> SandboxProvisionNetworkPlan {
    let incarnation = format!("compiled-{label}:{}", sandbox_id.as_str());
    let generation = NetworkResourceGeneration::new(7);
    build_sandbox_provision_network_plan_fixture(
        spec,
        label,
        &incarnation,
        generation,
        NetworkAttachmentId::for_workload_attachment(&incarnation, "primary"),
    )
}

/// Supply test-only coarse-start fixtures with explicit attachment desired
/// state without changing their legacy port-reservation identities.
pub(crate) fn legacy_start_attachment_network_plan_fixture(
    spec: &SandboxSpec,
    sandbox_id: &SandboxId,
    _label: &str,
) -> NetworkPlan {
    let backend = match spec.backend {
        crate::SandboxBackendKind::Container => {
            crate::backends::oci::network::AttachmentBackendKind::Container
        }
        crate::SandboxBackendKind::Krun => {
            crate::backends::oci::network::AttachmentBackendKind::Krun
        }
    };
    crate::backends::oci::network::oci_attachment_plan(&spec.tenant_id, sandbox_id, backend)
}

fn build_sandbox_provision_network_plan_fixture(
    spec: &SandboxSpec,
    label: &str,
    incarnation: &str,
    generation: NetworkResourceGeneration,
    attachment_id: NetworkAttachmentId,
) -> SandboxProvisionNetworkPlan {
    let requirements = sandbox_network_plan_requirements(spec.backend);
    let plan = NetworkPlan::new(
        NetworkPlanId::for_tenant_workload_plan(&spec.tenant_id, incarnation),
        generation,
        NetworkPlanContentDigest::sha256(format!("sandbox-provision-fixture:{label}")),
        requirements.capability_requirements().clone(),
    );
    let plan_id = plan.plan_id().clone();
    let endpoint_identities = spec.port_bindings.iter().map(|binding| {
        SandboxProvisionEndpointIdentity::new(
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, incarnation, &binding.name),
            nimbus_network::PublishedEndpointId::for_workload_endpoint(incarnation, &binding.name),
        )
    });
    let listeners = spec.port_bindings.iter().map(|binding| {
        let listener_id =
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, incarnation, &binding.name);
        let request = PortLeaseRequest::new(
            PortLeaseId::for_listener(&listener_id),
            listener_id.clone().into(),
            Some(spec.tenant_id.clone()),
            PortLeaseFence::new(generation, NetworkLeaseEpoch::new(1)),
            PortLeaseAccounting::TenantPublished,
            PortPublicationIntent::host(binding.host_address),
            PortBindingSpec::new(
                PortProtocol::Tcp,
                PortBindRealm::Host,
                bind_target(binding.host_address),
                exposure(binding.host_address),
                NonZeroU16::new(binding.host_port)
                    .map_or(PortRequestMode::ProviderAssigned, PortRequestMode::Exact),
            ),
        )
        .with_plan_id(plan_id.clone());
        SandboxProvisionListener::new(
            nimbus_network::PublishedEndpointId::for_workload_endpoint(incarnation, &binding.name),
            listener_id,
            binding.clone(),
            request,
        )
    });
    SandboxProvisionNetworkPlan::new(
        plan,
        spec.tenant_id.clone(),
        generation,
        attachment_id,
        endpoint_identities,
        listeners,
        [SandboxProvisionDependencyListener::new(
            ListenerId::for_tenant_workload_listener(&spec.tenant_id, incarnation, "egress-pep"),
            "egress-pep",
            requirements.pep_provider_id().clone(),
        )],
    )
    .expect("sandbox provision fixture should validate")
}

fn bind_target(address: IpAddr) -> PortBindTarget {
    match address {
        IpAddr::V4(address) if address == Ipv4Addr::UNSPECIFIED => PortBindTarget::ipv4_wildcard(),
        IpAddr::V4(address) => PortBindTarget::ipv4_specific(address),
        IpAddr::V6(address) if address == Ipv6Addr::UNSPECIFIED => {
            PortBindTarget::ipv6_wildcard(PortIpv6Overlap::Unknown)
        }
        IpAddr::V6(address) => PortBindTarget::ipv6_specific(address, PortIpv6Overlap::Unknown)
            .expect("test fixture never uses IPv4-mapped IPv6"),
    }
}

fn exposure(address: IpAddr) -> PortExposure {
    match address {
        address if address.is_loopback() => PortExposure::Loopback,
        IpAddr::V4(address) if address.is_private() || address.is_link_local() => {
            PortExposure::Private
        }
        IpAddr::V6(address) if address.is_unique_local() || address.is_unicast_link_local() => {
            PortExposure::Private
        }
        _ => PortExposure::Public,
    }
}
