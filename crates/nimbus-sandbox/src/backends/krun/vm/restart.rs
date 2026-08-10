//! Exact, provider-owned restart phases for krun workloads.
//!
//! The portable workload saga owns restart policy and ordering. This module
//! owns only the krun effects needed to quiesce one exact source execution,
//! durably switch to one exact target execution, and reattach retained
//! host-managed connectivity without publishing ingress.

use std::io::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::backends::conmon::creator::{CreatorQuiescenceProof, confirm_dead_conmon_receipt};
use crate::backends::conmon::lifecycle::{
    RuntimeStateObservation, remove_if_exists, runtime_state,
};
use crate::backends::oci::egress::PepPreAdoptionReleaseAuthority;
use crate::backends::oci::network::{AttachmentAttachAuthority, OciAttachmentBaseReadinessState};
use crate::error::{Result, SandboxError};
use crate::execution_attempt::SandboxRestartAttemptFence;
use crate::instance::{SandboxId, SandboxStatus};
use crate::provision::SandboxProvisionPhaseObservation;

use super::readiness::synchronize_handle_status;
use super::start::hostname_for;
use super::{
    KrunCreatorHandoffState, KrunLaunchAuthority, KrunSandboxBackend, KrunSandboxManifest,
};

const RESTART_RECORD_FILE: &str = ".nimbus-krun-restart.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum KrunRestartProviderPhase {
    SourceQuiesced,
    TargetPrepared,
    NetworkAttached,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct KrunRestartProviderRecord {
    fence: SandboxRestartAttemptFence,
    phase: KrunRestartProviderPhase,
}

impl KrunRestartProviderRecord {
    fn new(fence: &SandboxRestartAttemptFence, phase: KrunRestartProviderPhase) -> Self {
        Self {
            fence: fence.clone(),
            phase,
        }
    }

    fn require_fence(&self, fence: &SandboxRestartAttemptFence, operation: &str) -> Result<()> {
        if &self.fence == fence {
            return Ok(());
        }
        Err(SandboxError::InvalidSpec {
            message: format!(
                "{operation} crossed krun restart fence: requested source {}, target {}, ordinal {}; durable source {}, target {}, ordinal {}",
                fence.source_attempt_id(),
                fence.attempt_id(),
                fence.restart_ordinal(),
                self.fence.source_attempt_id(),
                self.fence.attempt_id(),
                self.fence.restart_ordinal(),
            ),
        })
    }
}

impl KrunSandboxBackend {
    pub(super) fn execution_drain_pending_restart_evidence(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<Option<Vec<u8>>> {
        let Some(record) = self.read_restart_provider_record(manifest)? else {
            return Ok(None);
        };
        let settled = record.phase == KrunRestartProviderPhase::NetworkAttached
            && record.fence.attempt_id() == &manifest.execution_attempt_id
            && matches!(
                manifest.launch_authority,
                KrunLaunchAuthority::ProviderOwned
            )
            && matches!(
                manifest.creator_handoff,
                KrunCreatorHandoffState::RuntimeObserved { .. }
            );
        if settled {
            Ok(None)
        } else {
            serde_json::to_vec(&(
                "krun_restart_owner_pending",
                &record,
                &manifest.execution_attempt_id,
                &manifest.launch_authority,
                &manifest.creator_handoff,
            ))
            .map(Some)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to encode pending Krun restart evidence for {}: {error}",
                    manifest.handle.id
                ),
            })
        }
    }

    /// Stop the exact source runtime and durably prove its creator quiescence.
    /// Network, listener, attachment, IPAM, and PEP authority remain retained.
    pub fn quiesce_restart_source(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(observed) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        let _lifecycle = self.lock_launch_lifecycle(&observed)?;
        let mut manifest = self.read_required_restart_manifest(sandbox_id)?;
        manifest.require_execution_admission_open("Krun restart source quiescence")?;
        let durable_record = self.read_restart_provider_record(&manifest)?;

        if &manifest.execution_attempt_id == fence.attempt_id() {
            let record = durable_record.ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "krun restart quiescence for {sandbox_id} observed target attempt {} without its durable source fence",
                    fence.attempt_id()
                ),
            })?;
            record.require_fence(fence, "krun restart quiescence replay")?;
            return Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: restart_phase_evidence("source_quiesced", &manifest, &record, None)?,
            });
        }
        manifest.require_execution_attempt(
            fence.source_attempt_id(),
            "krun restart source quiescence",
        )?;
        self.require_provider_owned_restart(&manifest, "krun restart source quiescence")?;
        self.require_restart_record_transition(durable_record.as_ref(), fence)?;
        if let Some(record) = durable_record
            .as_ref()
            .filter(|record| &record.fence == fence)
        {
            if !matches!(
                manifest.creator_handoff,
                KrunCreatorHandoffState::Quiesced { .. }
            ) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun restart quiescence replay for {sandbox_id} found durable phase {:?} but creator handoff {:?} is not quiesced",
                        record.phase, manifest.creator_handoff
                    ),
                });
            }
            self.require_source_runtime_absent(&manifest)?;
            return Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: restart_phase_evidence("source_quiesced", &manifest, record, None)?,
            });
        }

        self.reconcile_pending_creator_before_cleanup(&mut manifest)?;
        match runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        ) {
            Ok(RuntimeStateObservation::ExplicitlyAbsent) => {}
            Ok(RuntimeStateObservation::Present(_)) => {
                self.delete_runtime_and_confirm_absent(&manifest)?;
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "krun restart for {sandbox_id} cannot quiesce source runtime because its state is unknown: {error}"
                    ),
                });
            }
        }
        self.persist_restart_creator_quiescence_after_runtime_absence(&mut manifest)?;
        let record = self.persist_and_read_restart_provider_record(
            &manifest,
            KrunRestartProviderRecord::new(fence, KrunRestartProviderPhase::SourceQuiesced),
        )?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: restart_phase_evidence("source_quiesced", &manifest, &record, None)?,
        })
    }

    /// Inspect exact source quiescence without repairing or restarting it.
    pub fn inspect_restart_source_quiescence(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(observed) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: serde_json::to_vec(&("source_quiescence_absent", sandbox_id)).map_err(
                    |error| SandboxError::OperationFailed {
                        message: format!("failed to encode krun restart absence: {error}"),
                    },
                )?,
            });
        };
        let (_inspection, manifest) = self.lock_current_inspection(&observed)?;
        let Some(record) = self.read_restart_provider_record(&manifest)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: restart_phase_evidence("source_quiescence_absent", &manifest, &(), None)?,
            });
        };
        record.require_fence(fence, "krun restart quiescence inspection")?;
        if &manifest.execution_attempt_id != fence.source_attempt_id()
            && &manifest.execution_attempt_id != fence.attempt_id()
        {
            return Err(crossed_manifest_attempt(
                &manifest,
                fence,
                "krun restart quiescence inspection",
            ));
        }
        if &manifest.execution_attempt_id == fence.attempt_id() {
            return Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: restart_phase_evidence("source_quiesced", &manifest, &record, None)?,
            });
        }
        if !matches!(
            manifest.creator_handoff,
            KrunCreatorHandoffState::Quiesced { .. }
        ) {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: restart_phase_evidence(
                    "source_creator_not_quiesced",
                    &manifest,
                    &record,
                    None,
                )?,
            });
        }
        match runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        ) {
            Ok(RuntimeStateObservation::ExplicitlyAbsent) => {
                Ok(SandboxProvisionPhaseObservation::Succeeded {
                    evidence: restart_phase_evidence(
                        "source_quiesced",
                        &manifest,
                        &record,
                        Some("runtime_absent"),
                    )?,
                })
            }
            Ok(RuntimeStateObservation::Present(state)) => {
                Ok(SandboxProvisionPhaseObservation::InProgress {
                    evidence: restart_phase_evidence(
                        "source_runtime_present",
                        &manifest,
                        &record,
                        Some(&state),
                    )?,
                })
            }
            Err(error) => Ok(SandboxProvisionPhaseObservation::Ambiguous {
                evidence: restart_phase_evidence(
                    "source_runtime_unknown",
                    &manifest,
                    &record,
                    Some(&error.to_string()),
                )?,
            }),
        }
    }

    /// Consume only authenticated source-runtime receipts, then durably switch
    /// the provider manifest to the exact target execution attempt.
    pub fn prepare_restart_target(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(observed) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        let _lifecycle = self.lock_launch_lifecycle(&observed)?;
        let mut manifest = self.read_required_restart_manifest(sandbox_id)?;
        manifest.require_execution_admission_open("Krun restart target preparation")?;
        let record = self
            .read_restart_provider_record(&manifest)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "krun restart target preparation for {sandbox_id} requires durable source quiescence"
                ),
            })?;
        record.require_fence(fence, "krun restart target preparation")?;

        if &manifest.execution_attempt_id == fence.attempt_id() {
            let record = if record.phase < KrunRestartProviderPhase::TargetPrepared {
                let repaired =
                    KrunRestartProviderRecord::new(fence, KrunRestartProviderPhase::TargetPrepared);
                self.persist_and_read_restart_provider_record(&manifest, repaired)?
            } else {
                record
            };
            return Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: restart_phase_evidence("target_prepared", &manifest, &record, None)?,
            });
        }
        manifest.require_execution_attempt(
            fence.source_attempt_id(),
            "krun restart target preparation",
        )?;
        self.require_provider_owned_restart(&manifest, "krun restart target preparation")?;
        if record.phase != KrunRestartProviderPhase::SourceQuiesced {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart target preparation for {sandbox_id} requires source-quiesced provider state, got {:?}",
                    record.phase
                ),
            });
        }
        self.require_source_quiescence_before_target_switch(&manifest)?;
        self.clear_quiesced_source_runtime_receipts(&manifest)?;

        let reservation_claim = manifest.require_network_config()?.reservation_claim.clone();
        manifest.execution_attempt_id = fence.attempt_id().clone();
        manifest.execution_teardown = Default::default();
        manifest.creator_handoff = KrunCreatorHandoffState::NotSpawned;
        // The existing activation phase accepts an adopted attachment and
        // returns it to ProviderOwned after the exact target VMM is running.
        // The restart record distinguishes this retained authority from a
        // fresh never-bound launch.
        manifest.launch_authority = KrunLaunchAuthority::Adopted { reservation_claim };
        synchronize_handle_status(&mut manifest, SandboxStatus::Starting);
        self.persist_effect_barrier(&manifest, "krun restart target-attempt switch")?;
        let record = self.persist_and_read_restart_provider_record(
            &manifest,
            KrunRestartProviderRecord::new(fence, KrunRestartProviderPhase::TargetPrepared),
        )?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: restart_phase_evidence("target_prepared", &manifest, &record, None)?,
        })
    }

    /// Inspect the durable target-attempt switch without provider effects.
    pub fn inspect_restart_target_preparation(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(observed) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: serde_json::to_vec(&("target_preparation_absent", sandbox_id)).map_err(
                    |error| SandboxError::OperationFailed {
                        message: format!("failed to encode krun restart absence: {error}"),
                    },
                )?,
            });
        };
        let (_inspection, manifest) = self.lock_current_inspection(&observed)?;
        let Some(record) = self.read_restart_provider_record(&manifest)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: restart_phase_evidence(
                    "target_preparation_absent",
                    &manifest,
                    &(),
                    None,
                )?,
            });
        };
        record.require_fence(fence, "krun restart target-preparation inspection")?;
        if &manifest.execution_attempt_id == fence.source_attempt_id()
            && record.phase == KrunRestartProviderPhase::SourceQuiesced
        {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: restart_phase_evidence(
                    "target_preparation_pending",
                    &manifest,
                    &record,
                    None,
                )?,
            });
        }
        manifest.require_execution_attempt(
            fence.attempt_id(),
            "krun restart target-preparation inspection",
        )?;
        if record.phase >= KrunRestartProviderPhase::TargetPrepared {
            Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: restart_phase_evidence("target_prepared", &manifest, &record, None)?,
            })
        } else {
            Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: restart_phase_evidence(
                    "target_preparation_pending",
                    &manifest,
                    &record,
                    None,
                )?,
            })
        }
    }

    /// Reattach the retained private attachment and PEP for the exact target.
    /// Ingress remains unpublished and no lease or attachment is reallocated.
    pub fn attach_restart_retained_network(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        self.attach_restart_retained_network_with(sandbox_id, fence, |backend, manifest| {
            backend.configure_network(manifest, AttachmentAttachAuthority::RestartRetained, false)
        })
    }

    #[cfg(test)]
    pub(super) fn attach_restart_retained_network_with_test_host(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        self.attach_restart_retained_network_with(sandbox_id, fence, |backend, manifest| {
            let network_config = manifest.require_network_config()?;
            let ports = backend.port_lease_coordinator();
            let hostname = hostname_for(&manifest.spec);
            backend
                .non_routable_attachment_adapter(manifest, network_config, &hostname)
                .attach_with_test_host(
                    &backend.attachment_lifecycle(&ports),
                    AttachmentAttachAuthority::RestartRetained,
                    |_| {
                        if let Some(proxy) = manifest.egress_proxy.as_ref() {
                            backend
                                .egress_pin_provider
                                .apply(&manifest.network_layout, proxy)?;
                        }
                        Ok(())
                    },
                )
                .map(|_| ())
        })
    }

    fn attach_restart_retained_network_with(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        attach: impl FnOnce(&Self, &KrunSandboxManifest) -> Result<()>,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(observed) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        let _lifecycle = self.lock_launch_lifecycle(&observed)?;
        let manifest = self.read_required_restart_manifest(sandbox_id)?;
        manifest.require_execution_admission_open("Krun retained restart attachment")?;
        manifest
            .require_execution_attempt(fence.attempt_id(), "krun retained restart attachment")?;
        self.require_retained_target_authority(&manifest, "krun retained restart attachment")?;
        let durable = self
            .read_restart_provider_record(&manifest)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "krun retained restart attachment for {sandbox_id} lacks target-preparation state"
                ),
            })?;
        durable.require_fence(fence, "krun retained restart attachment")?;
        if durable.phase < KrunRestartProviderPhase::TargetPrepared {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun retained restart attachment for {sandbox_id} requires target preparation, got {:?}",
                    durable.phase
                ),
            });
        }
        if durable.phase == KrunRestartProviderPhase::NetworkAttached {
            let observed = self.inspect_restart_retained_network_locked(&manifest, &durable)?;
            if matches!(observed, SandboxProvisionPhaseObservation::Succeeded { .. }) {
                return Ok(observed);
            }
        }

        let retained_plan_members = Self::provision_port_plan_witness(&manifest);
        self.ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::PlannedRebind {
                plan_members: &retained_plan_members,
            },
        )?;
        let observation = self.inspect_restart_retained_network_provider(&manifest, &durable)?;
        if matches!(
            observation,
            SandboxProvisionPhaseObservation::Succeeded { .. }
        ) {
            if durable.phase == KrunRestartProviderPhase::NetworkAttached {
                return Ok(observation);
            }
            let record = self.persist_and_read_restart_provider_record(
                &manifest,
                KrunRestartProviderRecord::new(fence, KrunRestartProviderPhase::NetworkAttached),
            )?;
            return self.inspect_restart_retained_network_provider(&manifest, &record);
        }

        attach(self, &manifest)?;
        let observation = self.inspect_restart_retained_network_provider(&manifest, &durable)?;
        if !matches!(
            observation,
            SandboxProvisionPhaseObservation::Succeeded { .. }
        ) {
            return Ok(observation);
        }
        if durable.phase == KrunRestartProviderPhase::NetworkAttached {
            return Ok(observation);
        }
        let record = self.persist_and_read_restart_provider_record(
            &manifest,
            KrunRestartProviderRecord::new(fence, KrunRestartProviderPhase::NetworkAttached),
        )?;
        self.inspect_restart_retained_network_provider(&manifest, &record)
    }

    /// Inspect exact retained attachment readiness without binding or repair.
    pub fn inspect_restart_retained_network(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(observed) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: serde_json::to_vec(&("retained_network_absent", sandbox_id)).map_err(
                    |error| SandboxError::OperationFailed {
                        message: format!("failed to encode krun restart absence: {error}"),
                    },
                )?,
            });
        };
        let (_inspection, manifest) = self.lock_current_inspection(&observed)?;
        manifest.require_execution_attempt(
            fence.attempt_id(),
            "krun retained restart attachment inspection",
        )?;
        let Some(record) = self.read_restart_provider_record(&manifest)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: restart_phase_evidence("retained_network_absent", &manifest, &(), None)?,
            });
        };
        record.require_fence(fence, "krun retained restart attachment inspection")?;
        self.inspect_restart_retained_network_locked(&manifest, &record)
    }

    fn inspect_restart_retained_network_locked(
        &self,
        manifest: &KrunSandboxManifest,
        record: &KrunRestartProviderRecord,
    ) -> Result<SandboxProvisionPhaseObservation> {
        if record.phase < KrunRestartProviderPhase::NetworkAttached {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: restart_phase_evidence(
                    "retained_network_pending",
                    manifest,
                    record,
                    None,
                )?,
            });
        }
        self.inspect_restart_retained_network_provider(manifest, record)
    }

    fn inspect_restart_retained_network_provider(
        &self,
        manifest: &KrunSandboxManifest,
        record: &KrunRestartProviderRecord,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let network_config = manifest.require_network_config()?;
        let ports = self.port_lease_coordinator();
        let hostname = hostname_for(&manifest.spec);
        let readiness = self
            .non_routable_attachment_adapter(manifest, network_config, &hostname)
            .inspect_non_routable_readiness(
                &self.attachment_lifecycle(&ports),
                self.egress_pin_provider.as_ref(),
                manifest.egress_proxy.as_ref(),
                self.authenticated_egress_readiness(manifest)?,
            );
        let evidence = restart_phase_evidence(
            "retained_network_inspection",
            manifest,
            record,
            Some(&format!("{readiness:?}")),
        )?;
        match readiness {
            OciAttachmentBaseReadinessState::Ready(_) => {
                Ok(SandboxProvisionPhaseObservation::Succeeded { evidence })
            }
            OciAttachmentBaseReadinessState::NotReady(_) => {
                Ok(SandboxProvisionPhaseObservation::InProgress { evidence })
            }
        }
    }

    fn read_required_restart_manifest(
        &self,
        sandbox_id: &SandboxId,
    ) -> Result<KrunSandboxManifest> {
        self.read_manifest(sandbox_id)?
            .ok_or_else(|| SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            })
    }

    fn require_provider_owned_restart(
        &self,
        manifest: &KrunSandboxManifest,
        operation: &str,
    ) -> Result<()> {
        if manifest.launch_authority == KrunLaunchAuthority::ProviderOwned {
            return Ok(());
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "{operation} for {} requires provider-owned retained authority, got {:?}",
                manifest.handle.id, manifest.launch_authority
            ),
        })
    }

    fn require_restart_record_transition(
        &self,
        durable: Option<&KrunRestartProviderRecord>,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<()> {
        let Some(durable) = durable else {
            return Ok(());
        };
        if &durable.fence == fence {
            return Ok(());
        }
        let next_ordinal = durable.fence.restart_ordinal().checked_add(1);
        if durable.phase == KrunRestartProviderPhase::NetworkAttached
            && durable.fence.attempt_id() == fence.source_attempt_id()
            && next_ordinal == Some(fence.restart_ordinal())
        {
            return Ok(());
        }
        durable.require_fence(fence, "krun restart source quiescence")
    }

    fn require_source_quiescence_before_target_switch(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<()> {
        if !matches!(
            manifest.creator_handoff,
            KrunCreatorHandoffState::Quiesced { .. }
        ) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart for {} cannot switch target attempt while creator handoff {:?} may still materialize provider effects",
                    manifest.handle.id, manifest.creator_handoff
                ),
            });
        }
        match runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        ) {
            Ok(RuntimeStateObservation::ExplicitlyAbsent) => Ok(()),
            Ok(RuntimeStateObservation::Present(state)) => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart for {} cannot switch target attempt while source runtime remains {state:?}",
                    manifest.handle.id
                ),
            }),
            Err(error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart for {} cannot switch target attempt because source runtime absence is unknown: {error}",
                    manifest.handle.id
                ),
            }),
        }
    }

    fn require_source_runtime_absent(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        match runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        ) {
            Ok(RuntimeStateObservation::ExplicitlyAbsent) => Ok(()),
            Ok(RuntimeStateObservation::Present(state)) => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart for {} found source runtime {state:?} after durable quiescence; refusing replay",
                    manifest.handle.id
                ),
            }),
            Err(error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart for {} cannot authenticate source runtime absence after durable quiescence: {error}",
                    manifest.handle.id
                ),
            }),
        }
    }

    fn require_retained_target_authority(
        &self,
        manifest: &KrunSandboxManifest,
        operation: &str,
    ) -> Result<()> {
        if matches!(
            manifest.launch_authority,
            KrunLaunchAuthority::Adopted { .. } | KrunLaunchAuthority::ProviderOwned
        ) {
            return Ok(());
        }
        Err(SandboxError::OperationFailed {
            message: format!(
                "{operation} for {} requires adopted or provider-owned retained authority, got {:?}",
                manifest.handle.id, manifest.launch_authority
            ),
        })
    }

    fn clear_quiesced_source_runtime_receipts(&self, manifest: &KrunSandboxManifest) -> Result<()> {
        let KrunCreatorHandoffState::Quiesced { proof } = &manifest.creator_handoff else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart for {} cannot clear source receipts without durable creator quiescence",
                    manifest.handle.id
                ),
            });
        };
        match proof {
            CreatorQuiescenceProof::DeadContained { .. }
            | CreatorQuiescenceProof::NeverSpawned { .. }
            | CreatorQuiescenceProof::LaunchGateNeverReleased { .. } => {
                // Quiescence is already durable in the canonical manifest.
                // Authenticate a retained receipt before consuming it. An
                // absent receipt is an idempotent retry after that consumption,
                // not new evidence manufactured from absence.
                if manifest.conmon_layout.conmon_pidfile.exists() {
                    confirm_dead_conmon_receipt(&manifest.conmon_layout.conmon_pidfile)?;
                }
            }
        }
        remove_if_exists(&manifest.conmon_layout.pidfile)?;
        remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
        remove_if_exists(&manifest.conmon_layout.exit_status_file)
    }

    fn restart_provider_record_path(&self, manifest: &KrunSandboxManifest) -> PathBuf {
        manifest
            .conmon_layout
            .container_state_dir
            .join(RESTART_RECORD_FILE)
    }

    fn read_restart_provider_record(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> Result<Option<KrunRestartProviderRecord>> {
        let path = self.restart_provider_record_path(manifest);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to inspect krun restart record {}: {error}",
                        path.display()
                    ),
                });
            }
        };
        if !metadata.file_type().is_file() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart record {} is not a regular file",
                    path.display()
                ),
            });
        }
        let bytes = std::fs::read(&path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read krun restart record {}: {error}",
                path.display()
            ),
        })?;
        serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse krun restart record {}: {error}",
                    path.display()
                ),
            })
    }

    fn write_restart_provider_record(
        &self,
        manifest: &KrunSandboxManifest,
        record: &KrunRestartProviderRecord,
    ) -> Result<()> {
        let path = self.restart_provider_record_path(manifest);
        let parent = path.parent().ok_or_else(|| SandboxError::OperationFailed {
            message: format!("krun restart record {} has no parent", path.display()),
        })?;
        std::fs::create_dir_all(parent).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to create krun restart record directory {}: {error}",
                parent.display()
            ),
        })?;
        let mut rendered =
            serde_json::to_vec_pretty(record).map_err(|error| SandboxError::OperationFailed {
                message: format!("failed to serialize krun restart record: {error}"),
            })?;
        rendered.push(b'\n');
        let staged = parent.join(format!(
            ".nimbus-krun-restart.{}.stage",
            ulid::Ulid::new().to_string().to_ascii_lowercase()
        ));
        let publish = (|| -> Result<()> {
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&staged)
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to create staged krun restart record {}: {error}",
                        staged.display()
                    ),
                })?;
            file.write_all(&rendered)
                .and_then(|()| file.sync_all())
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to durably stage krun restart record {}: {error}",
                        staged.display()
                    ),
                })?;
            std::fs::rename(&staged, &path).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to publish krun restart record {}: {error}",
                    path.display()
                ),
            })?;
            std::fs::File::open(parent)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to sync krun restart record directory {}: {error}",
                        parent.display()
                    ),
                })
        })();
        if publish.is_err() {
            let _ = std::fs::remove_file(&staged);
        }
        publish
    }

    fn persist_and_read_restart_provider_record(
        &self,
        manifest: &KrunSandboxManifest,
        candidate: KrunRestartProviderRecord,
    ) -> Result<KrunRestartProviderRecord> {
        self.write_restart_provider_record(manifest, &candidate)?;
        let observed = self
            .read_restart_provider_record(manifest)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "krun restart record for {} disappeared after durable publication",
                    manifest.handle.id
                ),
            })?;
        if observed != candidate {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "krun restart record for {} differs from its durably published candidate",
                    manifest.handle.id
                ),
            });
        }
        Ok(observed)
    }
}

fn restart_phase_evidence(
    phase: &'static str,
    manifest: &KrunSandboxManifest,
    provider: &impl Serialize,
    observation: Option<&str>,
) -> Result<Vec<u8>> {
    serde_json::to_vec(&(
        phase,
        &manifest.handle.id,
        &manifest.execution_attempt_id,
        provider,
        observation,
    ))
    .map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to encode krun restart {phase} evidence: {error}"),
    })
}

fn crossed_manifest_attempt(
    manifest: &KrunSandboxManifest,
    fence: &SandboxRestartAttemptFence,
    operation: &str,
) -> SandboxError {
    SandboxError::InvalidSpec {
        message: format!(
            "{operation} for {} crossed execution attempt {}; expected source {} or target {}",
            manifest.handle.id,
            manifest.execution_attempt_id,
            fence.source_attempt_id(),
            fence.attempt_id()
        ),
    }
}
