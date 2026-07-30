//! Read-only composition of terminal OCI-family network authority.
//!
//! Workload manifests are desired/observed lifecycle records, not allocation
//! authority. Before either OCI-family backend publishes `Stopped` or `Failed`,
//! it asks this seam to authenticate the exact durable port, IPAM, and segment
//! generations without performing cleanup or inferring provider absence.

use nimbus_core::TenantId;
use nimbus_network::{
    LocalPortLeaseAuthority, NetworkAttachmentReservationState, NetworkReservationClaim,
    PortLeaseRequest,
};

use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;

use super::ipam::{
    ContainerIpamAuthorityState, OciIpamAuthority, inspect_container_ipam_authority,
};
use super::{
    OciNetworkConfig, OciNetworkLayout, OciSegmentAllocator, default_network_attachment_id,
};

/// Immutable workload evidence authenticated before terminal publication.
pub(crate) struct TerminalNetworkFinalityEvidence<'a> {
    tenant_id: &'a TenantId,
    sandbox_id: &'a SandboxId,
    layout: &'a OciNetworkLayout,
    network_config: Option<&'a OciNetworkConfig>,
    published_port_leases: &'a [PortLeaseRequest],
    egress_port_lease: Option<&'a PortLeaseRequest>,
}

impl<'a> TerminalNetworkFinalityEvidence<'a> {
    pub(crate) fn new(
        tenant_id: &'a TenantId,
        sandbox_id: &'a SandboxId,
        layout: &'a OciNetworkLayout,
        network_config: Option<&'a OciNetworkConfig>,
        published_port_leases: &'a [PortLeaseRequest],
        egress_port_lease: Option<&'a PortLeaseRequest>,
    ) -> Self {
        Self {
            tenant_id,
            sandbox_id,
            layout,
            network_config,
            published_port_leases,
            egress_port_lease,
        }
    }
}

/// Exact authority set that must be terminal before workload terminal status.
pub(crate) struct TerminalNetworkAuthoritySet<'a> {
    allocator: &'a OciSegmentAllocator,
    ipam_authority: &'a OciIpamAuthority,
    port_authority: &'a LocalPortLeaseAuthority,
    evidence: TerminalNetworkFinalityEvidence<'a>,
}

impl<'a> TerminalNetworkAuthoritySet<'a> {
    pub(crate) fn new(
        allocator: &'a OciSegmentAllocator,
        ipam_authority: &'a OciIpamAuthority,
        port_authority: &'a LocalPortLeaseAuthority,
        evidence: TerminalNetworkFinalityEvidence<'a>,
    ) -> Self {
        Self {
            allocator,
            ipam_authority,
            port_authority,
            evidence,
        }
    }

    /// Require every exact canonical authority to be terminal.
    ///
    /// This method is deliberately observation-only. Cleanup owners must first
    /// drive their own state machines to terminal evidence, then retry manifest
    /// publication against the same immutable identities and fences.
    pub(crate) fn require_released(&self) -> Result<()> {
        for request in self
            .evidence
            .published_port_leases
            .iter()
            .chain(self.evidence.egress_port_lease)
        {
            let record =
                crate::backends::oci::port_lease::inspect_exact(self.port_authority, request)?;
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
                        self.evidence.sandbox_id,
                        request.lease_id(),
                        record.phase()
                    ),
                });
            }
        }

        let Some(network_config) = self.evidence.network_config else {
            return Ok(());
        };

        match inspect_container_ipam_authority(
            self.ipam_authority,
            self.evidence.layout,
            network_config,
            self.evidence.sandbox_id,
        )? {
            ContainerIpamAuthorityState::Live => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "terminal network finality rejected for {}: exact IPAM generation remains \
                         live under coordinator {}",
                        self.evidence.sandbox_id,
                        network_config
                            .reservation_claim
                            .coordinator_attempt()
                            .provider_id()
                    ),
                });
            }
            ContainerIpamAuthorityState::Released | ContainerIpamAuthorityState::Absent => {}
        }

        let attachment_id = default_network_attachment_id(self.evidence.sandbox_id);
        let attachment_state = self.allocator.inspect_attachment_reservation(
            self.evidence.tenant_id,
            &attachment_id,
            &network_config.reservation_claim,
        )?;
        if attachment_state.state() != NetworkAttachmentReservationState::Absent {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "terminal network finality rejected for {}: attachment {} remains \
                     {attachment_state:?} under coordinator {}",
                    self.evidence.sandbox_id,
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
