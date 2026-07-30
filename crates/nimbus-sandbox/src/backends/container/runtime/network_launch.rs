//! Container adapter for the shared OCI attachment lifecycle.
//!
//! Provider effects and their ordering live in the OCI attachment owner.
//! This module contributes only container launch-time inputs and reconstructs
//! the exact port authority selected by the persisted execution context.

use nimbus_core::TenantId;
use nimbus_network::NetworkReservationClaim;

use super::*;
use crate::backends::oci::network::{
    AttachmentBackendKind, OciAttachmentAdapter, OciAttachmentAuxiliaryListener,
    OciAttachmentInput, OciHostManagedAttachmentBackend, OciMachineForwardedAttachmentBackend,
    OciMachinePortForwarderConfig,
};
use crate::backends::oci::network::{OciAttachmentLifecycle, OciNetworkConfig, OciNetworkLayout};

impl OciHostManagedAttachmentBackend for ContainerSandboxBackend {
    const ATTACHMENT_BACKEND_KIND: AttachmentBackendKind = AttachmentBackendKind::Container;
}

impl OciMachineForwardedAttachmentBackend for ContainerSandboxBackend {}

impl ContainerSandboxBackend {
    pub(super) fn attachment_lifecycle<'a>(
        &'a self,
        ports: &'a OciPortLeaseCoordinator,
    ) -> OciAttachmentLifecycle<'a> {
        OciAttachmentLifecycle::new(
            self.segment_allocator.as_ref(),
            &self.ipam_authority,
            ports,
            &self.netavark_port_lifetimes,
        )
    }

    pub(super) fn attachment_adapter<'a>(
        &'a self,
        manifest: &'a ContainerSandboxManifest,
        network_config: &'a OciNetworkConfig,
        hostname: &'a str,
        machine_port_forwarder: Option<&'a OciMachinePortForwarderConfig>,
    ) -> OciAttachmentAdapter<'a> {
        let input = OciAttachmentInput {
            workload_state_root: &manifest.runner_config.workload_state_root,
            tenant_id: &manifest.spec.tenant_id,
            sandbox_id: &manifest.handle.id,
            display_name: manifest.spec.display_name(),
            hostname,
            bindings: &manifest.spec.port_bindings,
            leases: &manifest.port_leases,
            auxiliary_listener: manifest.egress_proxy.as_ref().map(|assignment| {
                OciAttachmentAuxiliaryListener::egress_pep(
                    &assignment.port_lease,
                    &assignment.host,
                    assignment.port,
                )
            }),
            layout: &manifest.network_layout,
            config: network_config,
            launch_claim: manifest.launch_reservation_claim.as_ref(),
        };
        match machine_port_forwarder {
            Some(forwarder) => {
                <Self as OciMachineForwardedAttachmentBackend>::machine_forwarded_attachment_adapter(
                    input, forwarder,
                )
            }
            None => {
                <Self as OciHostManagedAttachmentBackend>::host_managed_attachment_adapter(input)
            }
        }
    }

    #[cfg(test)]
    pub(super) fn network_config(&self, tenant: &TenantId) -> Result<OciNetworkConfig> {
        let segment = self.segment_allocator.segment_for(tenant)?;
        let reservation_claim = crate::backends::oci::port_lease::new_launch_reservation_claim()?;
        Ok(OciAttachmentLifecycle::config_from_segment(
            self.config.netavark_path.clone(),
            self.config.aardvark_dns_path.clone(),
            &segment,
            &reservation_claim,
        ))
    }

    /// Reserve the attachment before IPAM and resolve its exact block config.
    pub(super) fn place_sandbox_config(
        &self,
        tenant: &TenantId,
        layout: &OciNetworkLayout,
        sandbox_id: &SandboxId,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<OciNetworkConfig> {
        let ports = self.port_lease_coordinator();
        <Self as OciHostManagedAttachmentBackend>::reserve_attachment_config(
            &self.attachment_lifecycle(&ports),
            tenant,
            layout,
            sandbox_id,
            reservation_claim,
            self.config.netavark_path.clone(),
            self.config.aardvark_dns_path.clone(),
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
        let ports = self.port_lease_coordinator();
        let port_compensation = ports.release_never_bound_launch_claim(reservation_claim);
        self.attachment_lifecycle(&ports).compensate_reserved(
            layout,
            tenant_id,
            sandbox_id,
            reservation_claim,
            planning_error,
            port_compensation,
        )
    }

    /// Release a cancelled runner launch before provider effects.
    pub(super) fn release_reserved_launch(
        &self,
        manifest: &ContainerSandboxManifest,
        reservation_claim: &NetworkReservationClaim,
    ) -> Result<()> {
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let port_compensation = ports.release_never_bound_launch_claim(reservation_claim);
        self.attachment_lifecycle(&ports).release_reserved(
            &manifest.network_layout,
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            reservation_claim,
            port_compensation,
        )
    }

    /// Keep the existing focused fail-before harness routed through the shared
    /// lifecycle owner instead of preserving a container-local compensator.
    #[cfg(test)]
    pub(super) fn complete_network_setup(
        &self,
        manifest: &ContainerSandboxManifest,
        network_config: &OciNetworkConfig,
        machine_port_forwarder: Option<&OciMachinePortForwarderConfig>,
        setup: Result<Vec<std::net::Ipv4Addr>>,
    ) -> Result<Vec<std::net::Ipv4Addr>> {
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        self.attachment_adapter(manifest, network_config, &hostname, machine_port_forwarder)
            .complete_injected_setup(&lifecycle, setup)
    }
}
