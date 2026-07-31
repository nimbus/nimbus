//! Durable lifecycle authority for provider-managed machine port publication.
//!
//! The container manifest remains desired state. This sibling state machine
//! journals every ambiguous per-binding effect before it reaches the
//! sandbox-owned provider capability, so response loss or process death can
//! always inspect before retrying.

use std::path::Path;

use nimbus_core::TenantId;
use nimbus_network::{
    NetworkAttachmentId, NetworkProviderHandle, NetworkResourceGeneration, NetworkResourceId,
    NetworkResourceVersion, NetworkSegmentId, PortLeaseRequest,
};
use serde::{Deserialize, Serialize};

use crate::backends::capabilities::{
    SandboxAttachmentRegistrationKind, host_managed_attachment_provider_id,
};
#[cfg(test)]
use crate::backends::oci::network::DeterministicMachinePortForwardingProvider;
use crate::backends::oci::network::{
    AttachmentBackendKind, MachinePortForwardOutcome, MachinePortForwardReceipt,
    MachinePortForwardingProvider, MachinePortForwardingSlotObservation,
    OciMachinePortForwarderConfig, default_network_attachment_id, oci_attachment_plan,
};
use crate::error::{Result, SandboxError};
use crate::instance::SandboxId;
use crate::spec::SandboxPortBinding;

use super::{ContainerSandboxBackend, ContainerSandboxManifest};

const CURRENT_MACHINE_PORT_PUBLICATION_VERSION: u32 = 2;

mod store;

#[cfg(test)]
use store::{
    MACHINE_PORT_EVIDENCE_FILE, MACHINE_PORT_EVIDENCE_LOCK_FILE, MACHINE_PORT_EVIDENCE_STAGE_FILE,
    MachinePortEvidenceLockError, MachinePortEvidenceStoreCheckpoint,
    MachinePortEvidenceStoreObserver, lock_publication_for_test, publish_record,
    publish_record_with_observer,
};
use store::{
    lock_publication, publish_record_locked, read_record, read_record_if_present,
    remove_stale_stage, sync_directory,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum MachinePortPublicationPhase {
    Absent,
    Exposing,
    Exposed,
    Withdrawing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "receipt", rename_all = "snake_case")]
enum MachinePortPublicationSlot {
    Pending,
    EffectMayExist,
    ObservedExposed(MachinePortForwardReceipt),
    ObservedAbsent(MachinePortForwardReceipt),
}

/// Testable durable/effect boundaries for the machine publication coordinator.
///
/// The no-op production observer keeps orchestration independent of test
/// machinery. Fault and real-process tests substitute an observer at this
/// private seam so every named boundary is exercised without weakening the
/// provider capability or duplicating lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MachinePortPublicationCheckpoint {
    BatchPrepared {
        action: MachinePortPublicationAction,
        generation: u64,
    },
    SlotEffectPrepared {
        action: MachinePortPublicationAction,
        generation: u64,
        index: usize,
    },
    SlotEffectReturned {
        action: MachinePortPublicationAction,
        generation: u64,
        index: usize,
    },
    SlotObserved {
        action: MachinePortPublicationAction,
        generation: u64,
        index: usize,
    },
    BatchTerminal {
        action: MachinePortPublicationAction,
        generation: u64,
    },
}

pub(super) trait MachinePortPublicationObserver {
    fn checkpoint(&mut self, checkpoint: MachinePortPublicationCheckpoint) -> Result<()>;
}

struct NoopMachinePortPublicationObserver;

impl MachinePortPublicationObserver for NoopMachinePortPublicationObserver {
    fn checkpoint(&mut self, _checkpoint: MachinePortPublicationCheckpoint) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MachinePortPublicationRecord {
    version: u32,
    phase: MachinePortPublicationPhase,
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    attachment_id: NetworkAttachmentId,
    attachment_version: NetworkResourceVersion,
    provider_instance: NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
    batch_generation: u64,
    bindings: Vec<SandboxPortBinding>,
    port_leases: Vec<PortLeaseRequest>,
    slots: Vec<MachinePortPublicationSlot>,
}

/// Complete durable provider observation for an absent machine publication.
///
/// Header identity remains available when the exact binding set is empty and
/// therefore has no per-binding receipts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachinePortAbsenceEvidence {
    pub tenant_id: TenantId,
    pub sandbox_id: SandboxId,
    pub receipts: Vec<MachinePortForwardReceipt>,
}

#[derive(Clone)]
struct MachinePortPublicationExpectation {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    attachment_id: NetworkAttachmentId,
    attachment_version: NetworkResourceVersion,
    provider_instance: NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
    bindings: Vec<SandboxPortBinding>,
    port_leases: Vec<PortLeaseRequest>,
}

impl MachinePortPublicationExpectation {
    fn from_manifest(
        backend: &ContainerSandboxBackend,
        manifest: &ContainerSandboxManifest,
        provider: &impl MachinePortForwardingProvider,
        authority_phase: MachinePortPublicationPhase,
    ) -> Result<Self> {
        Self::from_manifest_with_authority_phase(backend, manifest, provider, Some(authority_phase))
    }

    #[cfg(test)]
    fn from_manifest_for_record_test(
        backend: &ContainerSandboxBackend,
        manifest: &ContainerSandboxManifest,
        provider: &impl MachinePortForwardingProvider,
    ) -> Result<Self> {
        Self::from_manifest_with_authority_phase(backend, manifest, provider, None)
    }

    fn from_manifest_with_authority_phase(
        backend: &ContainerSandboxBackend,
        manifest: &ContainerSandboxManifest,
        provider: &impl MachinePortForwardingProvider,
        authority_phase: Option<MachinePortPublicationPhase>,
    ) -> Result<Self> {
        backend.validate_manifest_execution_context(manifest)?;
        let forwarder = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {} has no machine forwarder authority; observed machine \
                     port evidence is unavailable",
                    manifest.handle.id
                ),
            })?;
        if forwarder.provider_instance() != provider.provider_instance()
            || forwarder.provider_generation() != provider.provider_generation()
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port provider authority is crossed or stale for tenant {} sandbox \
                     {}; effects remain fenced",
                    manifest.spec.tenant_id, manifest.handle.id
                ),
            });
        }
        if manifest.spec.port_bindings.len() != manifest.port_leases.len() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port publication for tenant {} sandbox {} has {} bindings but {} \
                     durable listener leases",
                    manifest.spec.tenant_id,
                    manifest.handle.id,
                    manifest.spec.port_bindings.len(),
                    manifest.port_leases.len()
                ),
            });
        }
        let port_leases = backend.port_lease_coordinator_for_manifest(manifest)?;
        match authority_phase {
            Some(MachinePortPublicationPhase::Exposing | MachinePortPublicationPhase::Exposed) => {
                port_leases.require_active_machine_bindings(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.spec.port_bindings,
                    &manifest.port_leases,
                )?;
            }
            Some(MachinePortPublicationPhase::Withdrawing) => {
                port_leases.require_machine_publication_withdrawal_fence(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.spec.port_bindings,
                    &manifest.port_leases,
                )?;
            }
            Some(MachinePortPublicationPhase::Absent) => {
                port_leases.require_machine_publication_identity(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.spec.port_bindings,
                    &manifest.port_leases,
                )?;
            }
            #[cfg(test)]
            None => {
                port_leases.require_binding_leases(
                    &manifest.spec.tenant_id,
                    &manifest.handle.id,
                    &manifest.spec.port_bindings,
                    &manifest.port_leases,
                )?;
            }
            #[cfg(not(test))]
            None => unreachable!("production publication always selects an authority phase"),
        }
        let network_config = manifest.require_network_config()?;
        let attachment_id = default_network_attachment_id(&manifest.handle.id);
        let attachment_authority =
            backend
                .attachment_authority
                .as_ref()
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!(
                        "machine port publication for tenant {} sandbox {} has no manager-derived \
                     attachment authority",
                        manifest.spec.tenant_id, manifest.handle.id
                    ),
                })?;
        let attachment = attachment_authority
            .get(&manifest.spec.tenant_id, &attachment_id)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "machine port publication could not inspect attachment {attachment_id}: \
                     {error}"
                ),
            })?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "machine port publication has no durable attachment {attachment_id} for \
                     tenant {} sandbox {}",
                    manifest.spec.tenant_id, manifest.handle.id
                ),
            })?;
        let configured_segment = network_config
            .segment_id
            .parse::<NetworkSegmentId>()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "machine port publication carries invalid segment identity {:?}: {error}",
                    network_config.segment_id
                ),
            })?;
        if attachment.association().reservation_claim() != &network_config.reservation_claim
            || attachment.association().segment_id() != &configured_segment
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port publication attachment {attachment_id} does not match the exact \
                     reservation claim and segment in the durable manifest"
                ),
            });
        }
        let selected_provider =
            host_managed_attachment_provider_id(SandboxAttachmentRegistrationKind::Container);
        let attachment_plan = oci_attachment_plan(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            AttachmentBackendKind::Container,
        );
        let expected_attachment_version = NetworkResourceVersion::for_plan(
            &attachment_plan,
            NetworkResourceId::Attachment(attachment_id.clone()),
            attachment.association().lease_epoch(),
        );
        if attachment.selected_provider_id() != &selected_provider {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port publication attachment {attachment_id} selected provider {}, \
                     not the canonical container attachment provider {selected_provider}",
                    attachment.selected_provider_id()
                ),
            });
        }
        attachment
            .resource()
            .authenticate_version(&expected_attachment_version)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "machine port publication attachment {attachment_id} has a substituted plan \
                     generation, digest, or lease epoch: {error}"
                ),
            })?;
        Ok(Self {
            tenant_id: manifest.spec.tenant_id.clone(),
            sandbox_id: manifest.handle.id.clone(),
            attachment_id,
            attachment_version: attachment.resource().version().clone(),
            provider_instance: provider.provider_instance().clone(),
            provider_generation: provider.provider_generation(),
            bindings: manifest.spec.port_bindings.clone(),
            port_leases: manifest.port_leases.clone(),
        })
    }
}

impl ContainerSandboxBackend {
    pub(super) fn converge_exposed_machine_port_publication(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        let provider = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {} has no machine forwarder authority",
                    manifest.handle.id
                ),
            })?;
        self.converge_machine_port_publication(
            manifest,
            provider,
            MachinePortPublicationAction::Expose,
        )
        .map(|_| ())
    }

    pub(super) fn converge_absent_machine_port_publication(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        forwarder: &OciMachinePortForwarderConfig,
    ) -> Result<()> {
        let manifest = self.read_manifest(sandbox_id)?.ok_or_else(|| {
            SandboxError::OperationFailed {
                message: format!(
                    "cannot converge machine port absence because sandbox manifest {sandbox_id} \
                     is missing"
                ),
            }
        })?;
        if manifest.spec.tenant_id != *tenant_id || manifest.spec.port_bindings != bindings {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port absence request does not match the current durable manifest \
                     for tenant {tenant_id} sandbox {sandbox_id}"
                ),
            });
        }
        self.converge_machine_port_publication(
            &manifest,
            forwarder,
            MachinePortPublicationAction::Withdraw,
        )
        .map(|_| ())
    }

    /// Durably declare the exact withdrawal batch before any listener
    /// authority transition, local worker stop, or external provider effect.
    ///
    /// This operation performs no provider I/O. The later convergence step
    /// reopens the same generation, inspects current provider state, and
    /// advances only exact observations.
    pub(super) fn prepare_machine_port_publication_withdrawal(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        self.prepare_machine_port_publication_withdrawal_with_observer(
            manifest,
            &mut NoopMachinePortPublicationObserver,
        )
    }

    #[cfg(test)]
    pub(super) fn prepare_machine_port_publication_withdrawal_for_test_with_observer(
        &self,
        manifest: &ContainerSandboxManifest,
        observer: &mut impl MachinePortPublicationObserver,
    ) -> Result<()> {
        self.prepare_machine_port_publication_withdrawal_with_observer(manifest, observer)
    }

    fn prepare_machine_port_publication_withdrawal_with_observer(
        &self,
        manifest: &ContainerSandboxManifest,
        observer: &mut impl MachinePortPublicationObserver,
    ) -> Result<()> {
        let provider = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "container sandbox {} has no machine forwarder authority",
                    manifest.handle.id
                ),
            })?;
        let action = MachinePortPublicationAction::Withdraw;
        let state_root = &manifest.runner_config.workload_state_root;
        let state_dir = &manifest.conmon_layout.container_state_dir;
        crate::backends::oci::durable_directory::establish_durable_directory_chain_with(
            state_root,
            state_dir,
            "machine port publication",
            sync_directory,
        )?;
        let _guard = lock_publication(state_dir)?;
        remove_stale_stage(state_dir)?;
        let existing = read_record_if_present(state_dir)?;
        let terminal_replay = existing
            .as_ref()
            .is_some_and(|record| record.phase == action.terminal_phase());
        let expectation = MachinePortPublicationExpectation::from_manifest(
            self,
            manifest,
            provider,
            if terminal_replay {
                action.terminal_phase()
            } else {
                action.in_progress_phase()
            },
        )?;
        let prepared =
            MachinePortPublicationRecord::prepare(existing.clone(), &expectation, action)?;
        if existing.as_ref() != Some(&prepared) {
            publish_record_locked(state_dir, &prepared)?;
        }
        observer.checkpoint(MachinePortPublicationCheckpoint::BatchPrepared {
            action,
            generation: prepared.batch_generation,
        })?;
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn ensure_machine_port_publication_attachment_for_test(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        let network_config = manifest.require_network_config()?;
        let attachment_id = default_network_attachment_id(&manifest.handle.id);
        let reservation = self.segment_allocator.inspect_attachment_reservation(
            &manifest.spec.tenant_id,
            &attachment_id,
            &network_config.reservation_claim,
        )?;
        let association = reservation
            .association()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "test machine publication attachment {} has no exact allocator association",
                    manifest.handle.id
                ),
            })?
            .clone();
        let plan = oci_attachment_plan(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            AttachmentBackendKind::Container,
        );
        self.attachment_authority
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "test machine publication attachment authority is unavailable".to_owned(),
            })?
            .reserve(
                &manifest.spec.tenant_id,
                host_managed_attachment_provider_id(SandboxAttachmentRegistrationKind::Container),
                &plan,
                attachment_id,
                association,
            )
            .map(|_| ())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "test machine publication attachment authority rejected reservation: {error}"
                ),
            })
    }

    #[cfg(test)]
    pub(super) fn converge_exposed_machine_port_publication_for_test(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        self.ensure_machine_port_publication_attachment_for_test(manifest)?;
        let forwarder = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "test manifest has no machine forwarder".to_owned(),
            })?;
        self.converge_machine_port_publication(
            manifest,
            &DeterministicMachinePortForwardingProvider::exposed(forwarder),
            MachinePortPublicationAction::Expose,
        )
        .map(|_| ())
    }

    #[cfg(test)]
    pub(super) fn converge_absent_machine_port_publication_for_test(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        self.ensure_machine_port_publication_attachment_for_test(manifest)?;
        let forwarder = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "test manifest has no machine forwarder".to_owned(),
            })?;
        self.converge_machine_port_publication(
            manifest,
            &DeterministicMachinePortForwardingProvider::absent(forwarder),
            MachinePortPublicationAction::Withdraw,
        )
        .map(|_| ())
    }

    /// Read the exact complete exposed receipt batch without mutating provider
    /// or lease authority.
    pub fn exposed_machine_port_receipts(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        self.read_machine_port_receipts(sandbox_id, MachinePortPublicationPhase::Exposed)
    }

    /// Read the exact complete withdrawn/already-absent receipt batch without
    /// mutating provider or lease authority.
    pub fn absent_machine_port_receipts(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        self.read_machine_port_receipts(sandbox_id, MachinePortPublicationPhase::Absent)
    }

    /// Read exact durable absence, including header identity for an empty
    /// binding set. Missing evidence is distinct from malformed or stale
    /// evidence so retry callers preserve not-found semantics.
    pub fn absent_machine_port_evidence(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<Option<MachinePortAbsenceEvidence>> {
        let record = match self.read_manifest(sandbox_id)? {
            Some(manifest) => {
                let forwarder = manifest
                    .runner_config
                    .machine_port_forwarder
                    .as_ref()
                    .ok_or_else(|| SandboxError::OperationFailed {
                        message: format!(
                            "container sandbox {sandbox_id} has no persisted machine forwarder \
                             authority; observed machine port evidence is unavailable"
                        ),
                    })?;
                let expectation = MachinePortPublicationExpectation::from_manifest(
                    self,
                    &manifest,
                    forwarder,
                    MachinePortPublicationPhase::Absent,
                )?;
                let Some(record) =
                    read_record_if_present(&manifest.conmon_layout.container_state_dir)?
                else {
                    return Ok(None);
                };
                authenticate_record(&record, MachinePortPublicationPhase::Absent, &expectation)?;
                record
            }
            None => {
                let forwarder = self.config.machine_port_forwarder.as_ref().ok_or_else(|| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "detached container sandbox {sandbox_id} has no current machine \
                             forwarder authority; observed machine port evidence is unavailable"
                        ),
                    }
                })?;
                let Some(record) =
                    find_record_without_manifest(&self.config.workload_state_root, sandbox_id)?
                else {
                    return Ok(None);
                };
                authenticate_detached_record(
                    &record,
                    MachinePortPublicationPhase::Absent,
                    sandbox_id,
                    forwarder,
                )?;
                record
            }
        };
        let receipts = terminal_receipts(&record, MachinePortPublicationPhase::Absent)?;
        Ok(Some(MachinePortAbsenceEvidence {
            tenant_id: record.tenant_id,
            sandbox_id: record.sandbox_id,
            receipts,
        }))
    }

    #[cfg(test)]
    pub(super) fn persist_exposed_machine_port_receipts(
        &self,
        manifest: &ContainerSandboxManifest,
        receipts: Vec<MachinePortForwardReceipt>,
    ) -> Result<()> {
        let provider = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "test manifest has no machine forwarder".to_owned(),
            })?;
        self.persist_terminal_machine_port_receipts(
            manifest,
            provider,
            MachinePortPublicationPhase::Exposed,
            receipts,
        )
    }

    #[cfg(test)]
    pub(super) fn persist_absent_machine_port_receipts(
        &self,
        tenant_id: &TenantId,
        sandbox_id: &SandboxId,
        bindings: &[SandboxPortBinding],
        forwarder: &OciMachinePortForwarderConfig,
        receipts: Vec<MachinePortForwardReceipt>,
    ) -> Result<()> {
        let manifest =
            self.read_manifest(sandbox_id)?
                .ok_or_else(|| SandboxError::OperationFailed {
                    message: format!("test manifest {sandbox_id} is missing"),
                })?;
        if manifest.spec.tenant_id != *tenant_id || manifest.spec.port_bindings != bindings {
            return Err(SandboxError::OperationFailed {
                message: "test terminal receipt identity is crossed".to_owned(),
            });
        }
        self.persist_terminal_machine_port_receipts(
            &manifest,
            forwarder,
            MachinePortPublicationPhase::Absent,
            receipts,
        )
    }

    fn read_machine_port_receipts(
        &self,
        sandbox_id: &SandboxId,
        phase: MachinePortPublicationPhase,
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        let manifest = self.read_manifest(sandbox_id)?.ok_or_else(|| {
            SandboxError::OperationFailed {
                message: format!(
                    "cannot read machine port evidence because sandbox manifest {sandbox_id} is \
                     missing"
                ),
            }
        })?;
        let provider = manifest
            .runner_config
            .machine_port_forwarder
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!("sandbox {sandbox_id} has no machine forwarder authority"),
            })?;
        let expectation =
            MachinePortPublicationExpectation::from_manifest(self, &manifest, provider, phase)?;
        let record = read_record(&manifest.conmon_layout.container_state_dir)?;
        authenticate_record(&record, phase, &expectation)?;
        terminal_receipts(&record, phase)
    }

    #[cfg(test)]
    fn persist_terminal_machine_port_receipts(
        &self,
        manifest: &ContainerSandboxManifest,
        provider: &impl MachinePortForwardingProvider,
        phase: MachinePortPublicationPhase,
        receipts: Vec<MachinePortForwardReceipt>,
    ) -> Result<()> {
        let expectation = MachinePortPublicationExpectation::from_manifest_for_record_test(
            self, manifest, provider,
        )?;
        let slots = receipts
            .into_iter()
            .map(|receipt| match phase {
                MachinePortPublicationPhase::Exposed => {
                    MachinePortPublicationSlot::ObservedExposed(receipt)
                }
                MachinePortPublicationPhase::Absent => {
                    MachinePortPublicationSlot::ObservedAbsent(receipt)
                }
                MachinePortPublicationPhase::Exposing
                | MachinePortPublicationPhase::Withdrawing => unreachable!(),
            })
            .collect();
        let record = MachinePortPublicationRecord::new(expectation, phase, 1, slots);
        record.validate_self()?;
        publish_record(
            &self.config.workload_state_root,
            &manifest.conmon_layout.container_state_dir,
            record,
        )
    }
}

fn authenticate_record(
    record: &MachinePortPublicationRecord,
    phase: MachinePortPublicationPhase,
    expectation: &MachinePortPublicationExpectation,
) -> Result<()> {
    record.validate_self()?;
    if record.phase != phase {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "machine port publication for tenant {} sandbox {} is not a complete {phase:?} \
                 batch at the current version",
                expectation.tenant_id, expectation.sandbox_id
            ),
        });
    }
    if !record.matches(expectation) {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "machine port publication identity does not match the current durable manifest \
                 for tenant {} sandbox {}; the batch remains fenced",
                expectation.tenant_id, expectation.sandbox_id
            ),
        });
    }
    terminal_receipts(record, phase).map(|_| ())
}

fn authenticate_detached_record(
    record: &MachinePortPublicationRecord,
    phase: MachinePortPublicationPhase,
    sandbox_id: &SandboxId,
    forwarder: &OciMachinePortForwarderConfig,
) -> Result<()> {
    record.validate_self()?;
    if record.phase != phase
        || record.sandbox_id != *sandbox_id
        || record.provider_instance != *forwarder.provider_instance()
        || record.provider_generation != forwarder.provider_generation()
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "detached machine port publication does not authenticate the exact sandbox and \
                 current provider generation for {sandbox_id}; retry remains fenced"
            ),
        });
    }
    terminal_receipts(record, phase).map(|_| ())
}

fn find_record_without_manifest(
    state_root: &Path,
    sandbox_id: &SandboxId,
) -> Result<Option<MachinePortPublicationRecord>> {
    let state_dirs =
        crate::artifact_paths::all_container_state_dirs(state_root).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to find detached machine port evidence for {sandbox_id} under {}: \
                     {error}",
                    state_root.display()
                ),
            }
        })?;
    let mut selected = None;
    for state_dir in state_dirs {
        if state_dir.file_name() != Some(std::ffi::OsStr::new(sandbox_id.as_str())) {
            continue;
        }
        let Some(record) = read_record_if_present(&state_dir)? else {
            continue;
        };
        if selected.is_some() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "detached machine port evidence for sandbox {sandbox_id} exists in multiple \
                     tenant roots; retry remains fenced"
                ),
            });
        }
        selected = Some(record);
    }
    Ok(selected)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MachinePortPublicationAction {
    Expose,
    Withdraw,
}

impl MachinePortPublicationAction {
    fn in_progress_phase(self) -> MachinePortPublicationPhase {
        match self {
            Self::Expose => MachinePortPublicationPhase::Exposing,
            Self::Withdraw => MachinePortPublicationPhase::Withdrawing,
        }
    }

    fn terminal_phase(self) -> MachinePortPublicationPhase {
        match self {
            Self::Expose => MachinePortPublicationPhase::Exposed,
            Self::Withdraw => MachinePortPublicationPhase::Absent,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Expose => "expose",
            Self::Withdraw => "withdraw",
        }
    }

    fn observed_receipt(
        self,
        slot: &MachinePortForwardingSlotObservation,
    ) -> Option<&MachinePortForwardReceipt> {
        match self {
            Self::Expose => slot.exposed_receipt(),
            Self::Withdraw => slot.absent_receipt(),
        }
    }

    fn durable_slot(self, receipt: MachinePortForwardReceipt) -> MachinePortPublicationSlot {
        match self {
            Self::Expose => MachinePortPublicationSlot::ObservedExposed(receipt),
            Self::Withdraw => MachinePortPublicationSlot::ObservedAbsent(receipt),
        }
    }
}

impl MachinePortPublicationRecord {
    fn new(
        expectation: MachinePortPublicationExpectation,
        phase: MachinePortPublicationPhase,
        batch_generation: u64,
        slots: Vec<MachinePortPublicationSlot>,
    ) -> Self {
        Self {
            version: CURRENT_MACHINE_PORT_PUBLICATION_VERSION,
            phase,
            tenant_id: expectation.tenant_id,
            sandbox_id: expectation.sandbox_id,
            attachment_id: expectation.attachment_id,
            attachment_version: expectation.attachment_version,
            provider_instance: expectation.provider_instance,
            provider_generation: expectation.provider_generation,
            batch_generation,
            bindings: expectation.bindings,
            port_leases: expectation.port_leases,
            slots,
        }
    }

    fn matches(&self, expectation: &MachinePortPublicationExpectation) -> bool {
        self.tenant_id == expectation.tenant_id
            && self.sandbox_id == expectation.sandbox_id
            && self.attachment_id == expectation.attachment_id
            && self.attachment_version == expectation.attachment_version
            && self.provider_instance == expectation.provider_instance
            && self.provider_generation == expectation.provider_generation
            && self.bindings == expectation.bindings
            && self.port_leases == expectation.port_leases
    }

    fn prepare(
        existing: Option<Self>,
        expectation: &MachinePortPublicationExpectation,
        action: MachinePortPublicationAction,
    ) -> Result<Self> {
        let Some(mut record) = existing else {
            return Ok(Self::new(
                expectation.clone(),
                action.in_progress_phase(),
                1,
                vec![MachinePortPublicationSlot::Pending; expectation.bindings.len()],
            ));
        };
        record.validate_self()?;
        if !record.matches(expectation) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "machine port publication batch identity is crossed or stale for tenant {} \
                     sandbox {}; provider I/O remains fenced",
                    expectation.tenant_id, expectation.sandbox_id
                ),
            });
        }
        match (record.phase, action) {
            (MachinePortPublicationPhase::Exposing, MachinePortPublicationAction::Expose)
            | (MachinePortPublicationPhase::Exposed, MachinePortPublicationAction::Expose)
            | (MachinePortPublicationPhase::Withdrawing, MachinePortPublicationAction::Withdraw)
            | (MachinePortPublicationPhase::Absent, MachinePortPublicationAction::Withdraw) => {
                Ok(record)
            }
            (MachinePortPublicationPhase::Absent, MachinePortPublicationAction::Expose)
            | (
                MachinePortPublicationPhase::Exposing | MachinePortPublicationPhase::Exposed,
                MachinePortPublicationAction::Withdraw,
            ) => {
                record.batch_generation =
                    record.batch_generation.checked_add(1).ok_or_else(|| {
                        SandboxError::OperationFailed {
                            message: format!(
                                "machine port publication generation overflow for tenant {} \
                                 sandbox {}",
                                expectation.tenant_id, expectation.sandbox_id
                            ),
                        }
                    })?;
                record.phase = action.in_progress_phase();
                record.slots =
                    vec![MachinePortPublicationSlot::Pending; expectation.bindings.len()];
                Ok(record)
            }
            (MachinePortPublicationPhase::Withdrawing, MachinePortPublicationAction::Expose) => {
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "machine port publication for tenant {} sandbox {} is still withdrawing; \
                     exposure remains fenced",
                        expectation.tenant_id, expectation.sandbox_id
                    ),
                })
            }
        }
    }

    fn validate_self(&self) -> Result<()> {
        if self.version != CURRENT_MACHINE_PORT_PUBLICATION_VERSION
            || self.batch_generation == 0
            || self.bindings.len() != self.port_leases.len()
            || self.bindings.len() != self.slots.len()
            || self.attachment_version.resource_id()
                != &NetworkResourceId::Attachment(self.attachment_id.clone())
        {
            return Err(invalid_record(
                self,
                "header, attachment, binding, lease, or slot cardinality is invalid",
            ));
        }
        for index in 0..self.bindings.len() {
            if self.bindings[..index].contains(&self.bindings[index])
                || self.port_leases[..index]
                    .iter()
                    .any(|prior| prior.lease_id() == self.port_leases[index].lease_id())
            {
                return Err(invalid_record(
                    self,
                    format!("canonical member {index} duplicates an earlier identity"),
                ));
            }
            match (&self.phase, &self.slots[index]) {
                (
                    MachinePortPublicationPhase::Exposing,
                    MachinePortPublicationSlot::Pending
                    | MachinePortPublicationSlot::EffectMayExist,
                )
                | (
                    MachinePortPublicationPhase::Withdrawing,
                    MachinePortPublicationSlot::Pending
                    | MachinePortPublicationSlot::EffectMayExist,
                ) => {}
                (
                    MachinePortPublicationPhase::Exposing | MachinePortPublicationPhase::Exposed,
                    MachinePortPublicationSlot::ObservedExposed(receipt),
                ) => self.validate_receipt(index, receipt, true)?,
                (
                    MachinePortPublicationPhase::Withdrawing | MachinePortPublicationPhase::Absent,
                    MachinePortPublicationSlot::ObservedAbsent(receipt),
                ) => self.validate_receipt(index, receipt, false)?,
                _ => {
                    return Err(invalid_record(
                        self,
                        format!("slot {index} is illegal for durable phase {:?}", self.phase),
                    ));
                }
            }
        }
        if self.phase == MachinePortPublicationPhase::Exposed
            && self
                .slots
                .iter()
                .any(|slot| !matches!(slot, MachinePortPublicationSlot::ObservedExposed(_)))
        {
            return Err(invalid_record(
                self,
                "terminal Exposed batch contains an incomplete slot",
            ));
        }
        if self.phase == MachinePortPublicationPhase::Absent
            && self
                .slots
                .iter()
                .any(|slot| !matches!(slot, MachinePortPublicationSlot::ObservedAbsent(_)))
        {
            return Err(invalid_record(
                self,
                "terminal Absent batch contains an incomplete slot",
            ));
        }
        Ok(())
    }

    fn validate_receipt(
        &self,
        index: usize,
        receipt: &MachinePortForwardReceipt,
        exposed: bool,
    ) -> Result<()> {
        let outcome_matches = if exposed {
            receipt.outcome == MachinePortForwardOutcome::Exposed
        } else {
            matches!(
                receipt.outcome,
                MachinePortForwardOutcome::Withdrawn
                    | MachinePortForwardOutcome::ExactAlreadyAbsent
            )
        };
        if receipt.tenant_id != self.tenant_id
            || receipt.sandbox_id != self.sandbox_id
            || receipt.binding != self.bindings[index]
            || receipt.provider_instance != self.provider_instance
            || receipt.provider_generation != self.provider_generation
            || !outcome_matches
        {
            return Err(invalid_record(
                self,
                format!("slot {index} receipt is crossed, stale, or has the wrong outcome"),
            ));
        }
        Ok(())
    }
}

impl ContainerSandboxBackend {
    fn converge_machine_port_publication(
        &self,
        manifest: &ContainerSandboxManifest,
        provider: &impl MachinePortForwardingProvider,
        action: MachinePortPublicationAction,
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        self.converge_machine_port_publication_with_observer(
            manifest,
            provider,
            action,
            &mut NoopMachinePortPublicationObserver,
        )
    }

    #[cfg(test)]
    pub(super) fn converge_machine_port_publication_for_test_with_observer(
        &self,
        manifest: &ContainerSandboxManifest,
        provider: &impl MachinePortForwardingProvider,
        action: MachinePortPublicationAction,
        observer: &mut impl MachinePortPublicationObserver,
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        self.converge_machine_port_publication_with_observer(manifest, provider, action, observer)
    }

    fn converge_machine_port_publication_with_observer(
        &self,
        manifest: &ContainerSandboxManifest,
        provider: &impl MachinePortForwardingProvider,
        action: MachinePortPublicationAction,
        observer: &mut impl MachinePortPublicationObserver,
    ) -> Result<Vec<MachinePortForwardReceipt>> {
        let state_root = &manifest.runner_config.workload_state_root;
        let state_dir = &manifest.conmon_layout.container_state_dir;
        crate::backends::oci::durable_directory::establish_durable_directory_chain_with(
            state_root,
            state_dir,
            "machine port publication",
            sync_directory,
        )?;
        let _guard = lock_publication(state_dir)?;
        remove_stale_stage(state_dir)?;
        let existing = read_record_if_present(state_dir)?;
        let terminal_replay = existing
            .as_ref()
            .is_some_and(|record| record.phase == action.terminal_phase());
        let expectation = MachinePortPublicationExpectation::from_manifest(
            self,
            manifest,
            provider,
            if terminal_replay {
                action.terminal_phase()
            } else {
                action.in_progress_phase()
            },
        )?;
        let mut record =
            MachinePortPublicationRecord::prepare(existing.clone(), &expectation, action)?;
        if existing.as_ref() != Some(&record) {
            publish_record_locked(state_dir, &record)?;
        }
        observer.checkpoint(MachinePortPublicationCheckpoint::BatchPrepared {
            action,
            generation: record.batch_generation,
        })?;

        let mut observation = inspect_provider(provider, &expectation)?;
        reject_provider_conflicts(&expectation, &observation)?;
        if terminal_replay {
            if observation
                .slots()
                .iter()
                .all(|slot| action.observed_receipt(slot).is_some())
            {
                return terminal_receipts(&record, action.terminal_phase());
            }
            return Err(publication_error(
                &expectation,
                format!(
                    "terminal {:?} publication no longer matches exact current provider evidence; \
                     ordinary drift repair is fenced",
                    action.terminal_phase()
                ),
            ));
        }

        let mut failures = Vec::new();
        for index in 0..expectation.bindings.len() {
            if index != 0 {
                observation = inspect_provider(provider, &expectation)?;
                reject_provider_conflicts(&expectation, &observation)?;
            }
            persist_observed_progress(state_dir, &mut record, action, &observation)?;
            let slot = &observation.slots()[index];
            if let Some(receipt) = action.observed_receipt(slot) {
                let durable_slot = action.durable_slot(receipt.clone());
                if record.slots[index] != durable_slot {
                    record.slots[index] = durable_slot;
                    publish_record_locked(state_dir, &record)?;
                }
                observer.checkpoint(MachinePortPublicationCheckpoint::SlotObserved {
                    action,
                    generation: record.batch_generation,
                    index,
                })?;
                continue;
            }
            if matches!(
                (&record.slots[index], action),
                (
                    MachinePortPublicationSlot::ObservedExposed(_),
                    MachinePortPublicationAction::Expose
                ) | (
                    MachinePortPublicationSlot::ObservedAbsent(_),
                    MachinePortPublicationAction::Withdraw
                )
            ) {
                return Err(publication_error(
                    &expectation,
                    format!(
                        "durably observed slot {index} no longer matches current provider evidence; \
                         blind drift repair is fenced"
                    ),
                ));
            }

            if record.slots[index] != MachinePortPublicationSlot::EffectMayExist {
                record.slots[index] = MachinePortPublicationSlot::EffectMayExist;
                publish_record_locked(state_dir, &record)?;
            }
            observer.checkpoint(MachinePortPublicationCheckpoint::SlotEffectPrepared {
                action,
                generation: record.batch_generation,
                index,
            })?;
            let mutation = match action {
                MachinePortPublicationAction::Expose => {
                    provider.expose_one(&expectation.bindings[index])
                }
                MachinePortPublicationAction::Withdraw => {
                    provider.withdraw_one(&expectation.bindings[index])
                }
            };
            observer.checkpoint(MachinePortPublicationCheckpoint::SlotEffectReturned {
                action,
                generation: record.batch_generation,
                index,
            })?;
            let after = inspect_provider(provider, &expectation).map_err(|inspection_error| {
                ambiguous_mutation_error(
                    &expectation,
                    action,
                    index,
                    mutation.as_ref().ok().copied(),
                    mutation.as_ref().err(),
                    inspection_error,
                )
            })?;
            reject_provider_conflicts(&expectation, &after)?;
            persist_observed_progress(state_dir, &mut record, action, &after)?;
            let slot = &after.slots()[index];
            let Some(receipt) = action.observed_receipt(slot) else {
                failures.push(ambiguous_mutation_error(
                    &expectation,
                    action,
                    index,
                    mutation.as_ref().ok().copied(),
                    mutation.as_ref().err(),
                    publication_error(
                        &expectation,
                        "provider still reports the exact pre-mutation slot state",
                    ),
                ));
                continue;
            };
            debug_assert_eq!(record.slots[index], action.durable_slot(receipt.clone()));
            observer.checkpoint(MachinePortPublicationCheckpoint::SlotObserved {
                action,
                generation: record.batch_generation,
                index,
            })?;
        }

        if !failures.is_empty() {
            return Err(publication_error(
                &expectation,
                format!(
                    "{} batch retained for retry after {} exact slot failure(s): {}",
                    action.label(),
                    failures.len(),
                    failures
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                ),
            ));
        }

        if record.phase != action.terminal_phase() {
            record.phase = action.terminal_phase();
            record.validate_self()?;
            publish_record_locked(state_dir, &record)?;
        }
        observer.checkpoint(MachinePortPublicationCheckpoint::BatchTerminal {
            action,
            generation: record.batch_generation,
        })?;
        terminal_receipts(&record, action.terminal_phase())
    }
}

fn reject_provider_conflicts(
    expectation: &MachinePortPublicationExpectation,
    observation: &crate::backends::oci::network::CurrentMachinePortForwardingObservation,
) -> Result<()> {
    if let Some((index, detail)) = observation
        .slots()
        .iter()
        .enumerate()
        .find_map(|(index, slot)| slot.conflict_detail().map(|detail| (index, detail)))
    {
        return Err(publication_error(
            expectation,
            format!("provider conflict at slot {index}: {detail}"),
        ));
    }
    Ok(())
}

fn persist_observed_progress(
    state_dir: &Path,
    record: &mut MachinePortPublicationRecord,
    action: MachinePortPublicationAction,
    observation: &crate::backends::oci::network::CurrentMachinePortForwardingObservation,
) -> Result<()> {
    let mut changed = false;
    for (index, slot) in observation.slots().iter().enumerate() {
        let Some(receipt) = action.observed_receipt(slot) else {
            continue;
        };
        let durable = action.durable_slot(receipt.clone());
        if record.slots[index] != durable {
            record.slots[index] = durable;
            changed = true;
        }
    }
    if changed {
        publish_record_locked(state_dir, record)?;
    }
    Ok(())
}

fn inspect_provider(
    provider: &impl MachinePortForwardingProvider,
    expectation: &MachinePortPublicationExpectation,
) -> Result<crate::backends::oci::network::CurrentMachinePortForwardingObservation> {
    let observation = provider.inspect(
        &expectation.tenant_id,
        &expectation.sandbox_id,
        &expectation.bindings,
    )?;
    if observation.provider_instance() != &expectation.provider_instance
        || observation.provider_generation() != expectation.provider_generation
        || observation.slots().len() != expectation.bindings.len()
    {
        return Err(publication_error(
            expectation,
            "provider observation crossed the selected generation or omitted canonical slots",
        ));
    }
    for (index, slot) in observation.slots().iter().enumerate() {
        match slot {
            MachinePortForwardingSlotObservation::Exposed(receipt)
            | MachinePortForwardingSlotObservation::Absent(receipt) => {
                if receipt.tenant_id != expectation.tenant_id
                    || receipt.sandbox_id != expectation.sandbox_id
                    || receipt.binding != expectation.bindings[index]
                    || receipt.provider_instance != expectation.provider_instance
                    || receipt.provider_generation != expectation.provider_generation
                {
                    return Err(publication_error(
                        expectation,
                        format!("provider observation slot {index} is crossed or stale"),
                    ));
                }
            }
            MachinePortForwardingSlotObservation::Conflicting { binding, .. }
                if binding != &expectation.bindings[index] =>
            {
                return Err(publication_error(
                    expectation,
                    format!("provider conflict slot {index} names a substituted binding"),
                ));
            }
            MachinePortForwardingSlotObservation::Conflicting { .. } => {}
        }
    }
    Ok(observation)
}

fn terminal_receipts(
    record: &MachinePortPublicationRecord,
    phase: MachinePortPublicationPhase,
) -> Result<Vec<MachinePortForwardReceipt>> {
    record.validate_self()?;
    if record.phase != phase
        || !matches!(
            phase,
            MachinePortPublicationPhase::Exposed | MachinePortPublicationPhase::Absent
        )
    {
        return Err(invalid_record(
            record,
            "terminal receipt batch is incomplete",
        ));
    }
    record
        .slots
        .iter()
        .map(|slot| match (phase, slot) {
            (
                MachinePortPublicationPhase::Exposed,
                MachinePortPublicationSlot::ObservedExposed(receipt),
            )
            | (
                MachinePortPublicationPhase::Absent,
                MachinePortPublicationSlot::ObservedAbsent(receipt),
            ) => Ok(receipt.clone()),
            _ => Err(invalid_record(
                record,
                "terminal receipt batch contains a non-terminal slot",
            )),
        })
        .collect()
}

fn invalid_record(
    record: &MachinePortPublicationRecord,
    detail: impl std::fmt::Display,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "machine port publication record for tenant {} sandbox {} is invalid: {detail}; \
             provider effects remain fenced",
            record.tenant_id, record.sandbox_id
        ),
    }
}

fn publication_error(
    expectation: &MachinePortPublicationExpectation,
    detail: impl std::fmt::Display,
) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "machine port publication for tenant {} sandbox {} is ambiguous at provider \
             generation {}: {detail}",
            expectation.tenant_id,
            expectation.sandbox_id,
            expectation.provider_generation.as_u64()
        ),
    }
}

fn ambiguous_mutation_error(
    expectation: &MachinePortPublicationExpectation,
    action: MachinePortPublicationAction,
    index: usize,
    diagnostic: Option<crate::backends::oci::network::MachinePortMutationDiagnostic>,
    mutation_error: Option<&SandboxError>,
    inspection_error: SandboxError,
) -> SandboxError {
    let mutation = match (diagnostic, mutation_error) {
        (Some(diagnostic), _) => format!("native status accepted={}", diagnostic.status_accepted()),
        (None, Some(error)) => format!("native request failed: {error}"),
        (None, None) => "native outcome unavailable".to_owned(),
    };
    publication_error(
        expectation,
        format!(
            "{} slot {index} {}:{} ({mutation}); exact post-effect inspection failed: \
             {inspection_error}",
            action.label(),
            expectation.bindings[index].host_address,
            expectation.bindings[index].host_port
        ),
    )
}

#[cfg(test)]
mod tests;
