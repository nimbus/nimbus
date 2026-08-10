//! Machine-forwarded composition for exact Container attachment teardown.
//!
//! The guest lifecycle owns the outer provider-command stream. These methods
//! borrow that authority and never publish a second generic result.

use sha2::{Digest, Sha256};

use crate::{
    ProviderCommandCurrentExecution, ProviderCommandObservation, ProviderCommandObservationKind,
    SandboxError, SandboxNetworkTeardownCommand, SandboxNetworkTeardownObservation,
};

use super::super::machine_port_publication::DurableMachinePortTeardownPublicationObservation;
use super::{
    ContainerSandboxBackend, ContainerSandboxManifest, NetworkTeardownAdapterError,
    NetworkTeardownComposition, OciMachinePortForwarderConfig,
};
use crate::backends::oci::network::RetainedAttachmentPublicationEvidence;

const FORWARDED_PUBLICATION_ABSENCE_DOMAIN: &[u8] =
    b"nimbus.sandbox.container.forwarded-publication-absence.v1\0";

pub(super) enum ForwardedPublicationTeardownInspection {
    Present,
    Partial,
    Absent,
}

impl ContainerSandboxBackend {
    /// Authenticate a guest-owned forwarded teardown substep without effects.
    #[doc(hidden)]
    pub fn preflight_forwarded_network_teardown_substep(
        &self,
        command: &SandboxNetworkTeardownCommand,
        prior_observation: &ProviderCommandObservation,
        expected_forwarder: &OciMachinePortForwarderConfig,
    ) -> Result<(), SandboxNetworkTeardownObservation> {
        self.preflight_network_teardown_for_composition(
            command,
            NetworkTeardownComposition::Forwarded {
                expected_forwarder,
                prior_observation,
            },
        )
    }

    /// Execute one forwarded child transition under a caller-owned stream lock.
    ///
    /// This method does not open, claim, or publish the current generic stream.
    #[doc(hidden)]
    pub fn execute_forwarded_network_teardown_substep(
        &self,
        command: &SandboxNetworkTeardownCommand,
        current_execution: &ProviderCommandCurrentExecution,
        prior_observation: &ProviderCommandObservation,
        expected_forwarder: &OciMachinePortForwarderConfig,
    ) -> SandboxNetworkTeardownObservation {
        if current_execution.claim() != command.provider_claim()
            || current_execution.observation().kind() != ProviderCommandObservationKind::Claimed
        {
            return NetworkTeardownAdapterError::crossed(
                "Container forwarded network authorization",
            )
            .into_observation();
        }
        let journal = match self.attempt_idempotency_journal() {
            Ok(journal) => journal,
            Err(error) => {
                return NetworkTeardownAdapterError::ambiguous(error.to_string())
                    .into_observation();
            }
        };
        self.execute_network_teardown_inner(
            command,
            current_execution.observation(),
            &journal,
            NetworkTeardownComposition::Forwarded {
                expected_forwarder,
                prior_observation,
            },
        )
    }

    /// Inspect one forwarded child without effects, writes, or result publication.
    #[doc(hidden)]
    pub fn inspect_forwarded_network_teardown_substep(
        &self,
        command: &SandboxNetworkTeardownCommand,
        provider_observation: &ProviderCommandObservation,
        prior_observation: &ProviderCommandObservation,
        expected_forwarder: &OciMachinePortForwarderConfig,
    ) -> SandboxNetworkTeardownObservation {
        if provider_observation.claim() != command.provider_claim()
            || !matches!(
                provider_observation.kind(),
                ProviderCommandObservationKind::Claimed
                    | ProviderCommandObservationKind::InProgress
                    | ProviderCommandObservationKind::Ambiguous
            )
        {
            return NetworkTeardownAdapterError::crossed(
                "Container forwarded network inspection authorization",
            )
            .into_observation();
        }
        let journal = match self.attempt_idempotency_journal() {
            Ok(journal) => journal,
            Err(error) => {
                return NetworkTeardownAdapterError::ambiguous(error.to_string())
                    .into_observation();
            }
        };
        match self.inspect_network_teardown_inner(
            command,
            provider_observation,
            &journal,
            NetworkTeardownComposition::Forwarded {
                expected_forwarder,
                prior_observation,
            },
        ) {
            Ok(observation) => observation,
            Err(error) => error.into_observation(),
        }
    }
}

pub(super) fn retain_forwarded_publication(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    composition: NetworkTeardownComposition<'_>,
) -> crate::Result<RetainedAttachmentPublicationEvidence> {
    let expected_forwarder = expected_forwarder(composition)?;
    let cleanup = backend.begin_machine_port_proxy_retained_detach_for_manifest(manifest)?;
    backend.converge_absent_machine_port_publication(
        &manifest.spec.tenant_id,
        &manifest.handle.id,
        &manifest.spec.port_bindings,
        expected_forwarder,
    )?;
    if let Some(cleanup) = cleanup.as_ref() {
        backend.complete_machine_port_proxy_cleanup(cleanup)?;
    }
    backend.require_retained_machine_port_proxies_absent(manifest)?;
    forwarded_publication_absence_evidence(backend, manifest)
}

pub(super) fn inspect_forwarded_publication_absence(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    composition: NetworkTeardownComposition<'_>,
) -> crate::Result<RetainedAttachmentPublicationEvidence> {
    expected_forwarder(composition)?;
    forwarded_publication_absence_evidence(backend, manifest)
}

pub(super) fn inspect_forwarded_publication_for_detach(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    composition: NetworkTeardownComposition<'_>,
) -> crate::Result<ForwardedPublicationTeardownInspection> {
    expected_forwarder(composition)?;
    match backend.inspect_durable_machine_port_publication_for_teardown(manifest)? {
        DurableMachinePortTeardownPublicationObservation::Present => {
            Ok(ForwardedPublicationTeardownInspection::Present)
        }
        DurableMachinePortTeardownPublicationObservation::Partial => {
            Ok(ForwardedPublicationTeardownInspection::Partial)
        }
        DurableMachinePortTeardownPublicationObservation::Absent => {
            Ok(ForwardedPublicationTeardownInspection::Absent)
        }
    }
}

pub(super) fn release_forwarded_listener_authority(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> crate::Result<()> {
    let ports = backend.port_lease_coordinator_for_manifest(manifest)?;
    if manifest
        .port_leases
        .first()
        .is_some_and(|request| request.plan_id().is_some())
    {
        ports.release_planned_restart_retained_machine_bindings(
            &ContainerSandboxBackend::provision_port_plan_witness(manifest),
            &manifest.port_leases,
        )
    } else {
        ports.release_restart_retained_machine_bindings(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
        )
    }
}

fn expected_forwarder(
    composition: NetworkTeardownComposition<'_>,
) -> crate::Result<&OciMachinePortForwarderConfig> {
    match composition {
        NetworkTeardownComposition::Forwarded {
            expected_forwarder, ..
        } => Ok(expected_forwarder),
        NetworkTeardownComposition::HostManaged => Err(SandboxError::InvalidSpec {
            message: "machine publication evidence requires forwarded composition".to_owned(),
        }),
    }
}

fn forwarded_publication_absence_evidence(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> crate::Result<RetainedAttachmentPublicationEvidence> {
    let witness = backend
        .exact_absent_machine_port_witness(manifest)?
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "container forwarded publication for tenant {} sandbox {} has no exact durable absence witness",
                manifest.spec.tenant_id, manifest.handle.id
            ),
        })?;
    let encoded = serde_json::to_vec(&witness).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to encode exact forwarded publication absence for tenant {} sandbox {}: {error}",
            manifest.spec.tenant_id, manifest.handle.id
        ),
    })?;
    let mut hasher = Sha256::new();
    hasher.update(FORWARDED_PUBLICATION_ABSENCE_DOMAIN);
    hasher.update(encoded);
    RetainedAttachmentPublicationEvidence::machine_forwarded(format!("{:x}", hasher.finalize()))
}
