//! Read-only composition of terminal OCI-family network authority.
//!
//! Workload manifests are desired/observed lifecycle records, not allocation
//! authority. Before either OCI-family backend publishes `Stopped` or `Failed`,
//! it asks this seam to authenticate the exact durable port, IPAM, and segment
//! generations without performing cleanup or inferring provider absence.

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentReservationState, NetworkReservationClaim, PortLeaseRequest,
};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::ipam::{ContainerIpamAuthorityState, inspect_container_ipam_authority};
use super::{
    OciNetworkConfig, OciNetworkLayout, OciSegmentAllocator, default_network_attachment_id,
};

/// Exact authority set that must be terminal before workload terminal status.
pub(crate) struct TerminalNetworkAuthoritySet<'a> {
    allocator: &'a OciSegmentAllocator,
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    layout: &'a OciNetworkLayout,
    network_config: Option<&'a OciNetworkConfig>,
    published_port_leases: &'a [PortLeaseRequest],
    egress_port_lease: Option<&'a PortLeaseRequest>,
}

impl<'a> TerminalNetworkAuthoritySet<'a> {
    pub(crate) fn new(
        allocator: &'a OciSegmentAllocator,
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        layout: &'a OciNetworkLayout,
        network_config: Option<&'a OciNetworkConfig>,
        published_port_leases: &'a [PortLeaseRequest],
        egress_port_lease: Option<&'a PortLeaseRequest>,
    ) -> Self {
        Self {
            allocator,
            tenant_id,
            sandbox_id,
            layout,
            network_config,
            published_port_leases,
            egress_port_lease,
        }
    }

    /// Require every exact canonical authority to be terminal.
    ///
    /// This method is deliberately observation-only. Cleanup owners must first
    /// drive their own state machines to terminal evidence, then retry manifest
    /// publication against the same immutable identities and fences.
    pub(crate) fn require_released(&self) -> Result<()> {
        for request in self
            .published_port_leases
            .iter()
            .chain(self.egress_port_lease)
        {
            let record =
                crate::backends::oci::port_lease::inspect_exact(&self.layout.state_root, request)?;
            if !record.phase().is_terminal() {
                let coordinator = record
                    .reservation_claim()
                    .map(NetworkReservationClaim::coordinator_attempt)
                    .map(|attempt| attempt.provider_id().as_str())
                    .unwrap_or("none");
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "terminal network finality rejected for {}: port lease {} remains {:?} \
                         under coordinator {coordinator}",
                        self.sandbox_id,
                        request.lease_id(),
                        record.phase()
                    ),
                });
            }
        }

        let Some(network_config) = self.network_config else {
            return Ok(());
        };

        match inspect_container_ipam_authority(self.layout, network_config, self.sandbox_id)? {
            ContainerIpamAuthorityState::Live => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "terminal network finality rejected for {}: exact IPAM generation remains \
                         live under coordinator {}",
                        self.sandbox_id,
                        network_config
                            .reservation_claim
                            .coordinator_attempt()
                            .provider_id()
                    ),
                });
            }
            ContainerIpamAuthorityState::Released | ContainerIpamAuthorityState::Absent => {}
        }

        let attachment_id = default_network_attachment_id(self.sandbox_id);
        let attachment_state = self.allocator.inspect_attachment_reservation(
            self.tenant_id,
            &attachment_id,
            &network_config.reservation_claim,
        )?;
        if attachment_state != NetworkAttachmentReservationState::Absent {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "terminal network finality rejected for {}: attachment {} remains \
                     {attachment_state:?} under coordinator {}",
                    self.sandbox_id,
                    attachment_id,
                    network_config
                        .reservation_claim
                        .coordinator_attempt()
                        .provider_id()
                ),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "finality/tests.rs"]
mod tests;
