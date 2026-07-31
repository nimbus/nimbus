//! Container composition of common OCI attachment and publication readiness.

use crate::backends::oci::egress::EgressReadinessState;
use crate::backends::oci::network::{
    OciAttachmentBaseReadinessState, OciAttachmentReadinessState, OciMachinePortForwarderConfig,
};
use crate::error::{Result, SandboxError};

use super::{ContainerSandboxBackend, ContainerSandboxManifest, hostname_for};

impl ContainerSandboxBackend {
    pub(super) fn host_managed_attachment_readiness(
        &self,
        manifest: &ContainerSandboxManifest,
        pep: EgressReadinessState,
    ) -> Result<OciAttachmentReadinessState> {
        if manifest
            .runner_config
            .validated_machine_port_forwarder(&manifest.handle.id)?
            .is_some()
        {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "container sandbox {} cannot use host-managed readiness for a \
                     machine-forwarded publication",
                    manifest.handle.id
                ),
            });
        }
        let network_config = manifest.require_network_config()?;
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let hostname = hostname_for(&manifest.spec);
        Ok(self
            .attachment_adapter(manifest, network_config, &hostname, None)
            .inspect_host_managed_readiness(
                &self.attachment_lifecycle(&ports),
                self.egress_pin_provider.as_ref(),
                manifest.egress_proxy.as_ref(),
                pep,
            ))
    }

    fn machine_forwarded_attachment_readiness(
        &self,
        manifest: &ContainerSandboxManifest,
        forwarder: &OciMachinePortForwarderConfig,
        pep: EgressReadinessState,
    ) -> Result<OciAttachmentReadinessState> {
        let network_config = manifest.require_network_config()?;
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let hostname = hostname_for(&manifest.spec);
        let adapter = self.attachment_adapter(manifest, network_config, &hostname, Some(forwarder));
        let base = match adapter.inspect_machine_forwarded_base_readiness(
            &self.attachment_lifecycle(&ports),
            self.egress_pin_provider.as_ref(),
            manifest.egress_proxy.as_ref(),
            pep,
        ) {
            OciAttachmentBaseReadinessState::Ready(base) => base,
            OciAttachmentBaseReadinessState::NotReady(reason) => {
                return Ok(OciAttachmentReadinessState::NotReady(reason));
            }
        };
        let publication = self
            .inspect_machine_forwarded_publication(manifest, base.assigned_ips())
            .map_err(|error| error.to_string());
        Ok(adapter.complete_machine_forwarded_readiness(base, publication))
    }

    pub(super) fn complete_attachment_readiness(
        &self,
        manifest: &ContainerSandboxManifest,
        pep: EgressReadinessState,
    ) -> Result<OciAttachmentReadinessState> {
        match manifest
            .runner_config
            .validated_machine_port_forwarder(&manifest.handle.id)?
        {
            Some(forwarder) => {
                self.machine_forwarded_attachment_readiness(manifest, forwarder, pep)
            }
            None => self.host_managed_attachment_readiness(manifest, pep),
        }
    }

    pub(super) fn require_complete_attachment_readiness(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        let state = self.complete_attachment_readiness(
            manifest,
            self.authenticated_egress_readiness(manifest)?,
        )?;
        match state {
            OciAttachmentReadinessState::Ready(_) => Ok(()),
            OciAttachmentReadinessState::NotReady(reason) => Err(SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {} denied launch: complete network attachment is not \
                         ready: {reason:?}",
                    manifest.handle.id
                ),
            }),
        }
    }
}
