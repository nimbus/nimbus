//! Container launch coordination across segment, IPAM, and port authorities.
//!
//! Provider effects remain in the runtime and OCI adapter modules. This module
//! owns only the pre-effect reservation ordering and reverse-order
//! compensation seam shared by execute and runner-owned launches.

use nimbus_core::TenantId;
use nimbus_network::NetworkReservationClaim;

use super::*;
use crate::backends::oci::network::{
    OciMachinePortForwarderConfig, OciNetworkConfig, OciNetworkDirectEgress, OciNetworkLayout,
    OciSegmentRealization, compensate_reserved_network_launch_after_ports, place_sandbox_on_block,
    release_reserved_network_launch_after_ports,
};

impl ContainerSandboxBackend {
    /// Build the OCI network config for a specific resolved block segment.
    fn config_from_segment(
        &self,
        segment: &OciSegmentRealization,
        reservation_claim: &NetworkReservationClaim,
    ) -> OciNetworkConfig {
        OciNetworkConfig {
            netavark_path: self.config.netavark_path.clone(),
            aardvark_dns_path: self.config.aardvark_dns_path.clone(),
            network_name: segment.network_name().to_owned(),
            network_interface: segment.network_interface().to_owned(),
            network_subnet: segment.cidr().to_string(),
            segment_id: segment.segment_id().as_str().to_owned(),
            reservation_claim: reservation_claim.clone(),
            direct_egress: OciNetworkDirectEgress::Deny,
            // DNS-off on both backends: host-side PEP resolution owns names and
            // an in-subnet resolver would be unreachable plus cross-tenant risk.
            enable_dns: false,
            network_id: segment.network_id().as_str().to_owned(),
        }
    }

    #[cfg(test)]
    pub(super) fn network_config(&self, tenant: &TenantId) -> Result<OciNetworkConfig> {
        let segment = self.segment_allocator.segment_for(tenant)?;
        let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()?;
        Ok(self.config_from_segment(&segment, &reservation_claim))
    }

    /// Reserve the attachment before IPAM and resolve its exact block config.
    pub(super) fn place_sandbox_config(
        &self,
        tenant: &TenantId,
        layout: &OciNetworkLayout,
        sandbox_id: &SandboxId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciNetworkConfig> {
        place_sandbox_on_block(
            self.segment_allocator.as_ref(),
            tenant,
            layout,
            sandbox_id,
            reservation_claim,
            |segment, claim| self.config_from_segment(segment, claim),
        )
    }

    /// Preserve a primary planning failure and reverse-order compensation.
    pub(super) fn compensate_reserved_launch(
        &self,
        layout: &OciNetworkLayout,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        reservation_claim: &NetworkReservationClaim,
        planning_error: SandboxError,
    ) -> SandboxError {
        let manager = self.port_manager();
        compensate_reserved_network_launch_after_ports(
            self.segment_allocator.as_ref(),
            layout,
            tenant_id,
            sandbox_id,
            reservation_claim,
            planning_error,
            manager.release_never_bound_launch_claim(reservation_claim),
        )
    }

    /// Release a cancelled runner launch before provider effects.
    pub(super) fn release_reserved_launch(
        &self,
        manifest: &ContainerSandboxManifest,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        let manager = self.port_manager_for_manifest(manifest)?;
        release_reserved_network_launch_after_ports(
            self.segment_allocator.as_ref(),
            &manifest.network_layout,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            reservation_claim,
            manager.release_never_bound_launch_claim(reservation_claim),
        )
    }

    /// Preserve the primary setup failure and retain the namespace until exact
    /// generation-authenticated Netavark detach is confirmed.
    pub(super) fn failed_network_configuration(
        &self,
        manifest: &ContainerSandboxManifest,
        network_config: &OciNetworkConfig,
        machine_port_forwarder: Option<&OciMachinePortForwarderConfig>,
        primary: SandboxError,
    ) -> SandboxError {
        let cleanup = teardown_container_network(
            &manifest.network_layout,
            network_config,
            &manifest.handle.id,
            manifest.spec.display_name(),
            &hostname_for(&manifest.spec),
            &manifest.spec.port_bindings,
            machine_port_forwarder,
        )
        .and_then(|()| remove_persistent_network_namespace(&manifest.network_layout.netns_path));
        match cleanup {
            Ok(()) => primary,
            Err(cleanup) => SandboxError::OperationFailed {
                message: format!(
                    "container network configuration failed: {primary}; exact-generation \
                     detach compensation also failed while the namespace remains fenced: {cleanup}"
                ),
            },
        }
    }

    /// Route the provider setup boundary through the same exact-detach
    /// compensation used by every later network activation failure.
    pub(super) fn complete_network_setup(
        &self,
        manifest: &ContainerSandboxManifest,
        network_config: &OciNetworkConfig,
        machine_port_forwarder: Option<&OciMachinePortForwarderConfig>,
        setup: Result<Vec<std::net::Ipv4Addr>>,
    ) -> Result<Vec<std::net::Ipv4Addr>> {
        setup.map_err(|error| {
            self.failed_network_configuration(
                manifest,
                network_config,
                machine_port_forwarder,
                error,
            )
        })
    }
}
