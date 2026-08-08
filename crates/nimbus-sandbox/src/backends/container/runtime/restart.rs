//! Coordinator-issued, execution-attempt-fenced Container restart phases.
//!
//! This adapter owns only provider effects. Restart admission, policy,
//! scheduling, and phase order remain above `nimbus-sandbox`.

use serde::Serialize;

use crate::backends::conmon::creator::{CreatorQuiescenceProof, confirm_dead_conmon_receipt};
use crate::backends::conmon::lifecycle::{
    RuntimeStateObservation, configured_stop_signal, configured_stop_timeout,
    delete_runtime_and_confirm_absent, read_exit_code, read_pid, remove_if_exists, runtime_state,
    runtime_state_for_creator_attempt, signal_process, wait_for_path,
};
use crate::backends::oci::egress::PepPreAdoptionReleaseAuthority;
use crate::backends::oci::network::{
    AttachmentAttachAuthority, MachinePortPreparationReleaseAuthority,
    OciAttachmentBaseReadinessState,
};
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxId, SandboxStatus};
use crate::provision::SandboxProvisionPhaseObservation;

#[cfg(test)]
use super::hostname_for;
use super::manifest::{
    ContainerCreatorHandoffState, ContainerRestartTransition, ContainerSandboxManifest,
};
use super::status::synchronize_handle_status;
use super::{ContainerSandboxBackend, ContainerStartMode, SandboxRestartAttemptFence};

#[cfg(test)]
#[path = "restart/tests.rs"]
mod tests;

fn phase_evidence(
    phase: &'static str,
    manifest: &ContainerSandboxManifest,
    fence: &SandboxRestartAttemptFence,
    provider_observation: &impl Serialize,
) -> Result<Vec<u8>> {
    serde_json::to_vec(&(
        phase,
        manifest.handle.id.as_str(),
        &manifest.execution_attempt_id,
        fence,
        provider_observation,
    ))
    .map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to encode container restart {phase} evidence: {error}"),
    })
}

fn crossed_fence_error(
    manifest: &ContainerSandboxManifest,
    operation: &str,
    fence: &SandboxRestartAttemptFence,
) -> SandboxError {
    SandboxError::InvalidSpec {
        message: format!(
            "{operation} for {} crossed restart fence source={}, target={}, ordinal={}; durable execution attempt is {}",
            manifest.handle.id,
            fence.source_attempt_id(),
            fence.attempt_id(),
            fence.restart_ordinal(),
            manifest.execution_attempt_id,
        ),
    }
}

fn require_exact_transition<'a>(
    manifest: &'a ContainerSandboxManifest,
    fence: &SandboxRestartAttemptFence,
    operation: &str,
) -> Result<&'a ContainerRestartTransition> {
    let transition = manifest.restart_transition.as_ref().ok_or_else(|| {
        SandboxError::OperationFailed {
            message: format!(
                "{operation} for {} requires durable source quiescence for the exact restart fence",
                manifest.handle.id
            ),
        }
    })?;
    if transition.fence() != fence {
        return Err(crossed_fence_error(manifest, operation, fence));
    }
    Ok(transition)
}

fn is_completed_predecessor(
    transition: &ContainerRestartTransition,
    fence: &SandboxRestartAttemptFence,
) -> bool {
    transition.target_is_prepared() && transition.fence().attempt_id() == fence.source_attempt_id()
}

fn require_execute_restart(manifest: &ContainerSandboxManifest, operation: &str) -> Result<()> {
    if manifest.start_mode == ContainerStartMode::Execute && !manifest.shutdown_requested {
        return Ok(());
    }
    Err(SandboxError::InvalidSpec {
        message: format!(
            "{operation} for {} requires an active Execute-mode Container provider; mode={:?}, shutdown_requested={}",
            manifest.handle.id, manifest.start_mode, manifest.shutdown_requested
        ),
    })
}

fn source_runtime_state(
    manifest: &ContainerSandboxManifest,
    proof: &CreatorQuiescenceProof,
) -> Result<RuntimeStateObservation> {
    match proof {
        CreatorQuiescenceProof::DeadContained { receipt } => runtime_state_for_creator_attempt(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
            receipt.attempt_id(),
        ),
        CreatorQuiescenceProof::NeverSpawned { .. }
        | CreatorQuiescenceProof::LaunchGateNeverReleased { .. } => runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        ),
    }
}

fn confirm_source_conmon_absence(
    manifest: &ContainerSandboxManifest,
    proof: &CreatorQuiescenceProof,
    retired_receipt_is_durable: bool,
) -> Result<()> {
    match proof {
        CreatorQuiescenceProof::DeadContained { .. } => {
            match manifest.conmon_layout.conmon_pidfile.try_exists() {
                Ok(true) => confirm_dead_conmon_receipt(&manifest.conmon_layout.conmon_pidfile),
                Ok(false) if retired_receipt_is_durable => Ok(()),
                Ok(false) => Err(SandboxError::OperationFailed {
                    message: format!(
                        "container restart source {} lost its conmon receipt before target preparation became durable",
                        manifest.handle.id
                    ),
                }),
                Err(error) => Err(SandboxError::OperationFailed {
                    message: format!(
                        "cannot inspect container restart conmon receipt {}: {error}; target preparation remains fenced",
                        manifest.conmon_layout.conmon_pidfile.display()
                    ),
                }),
            }
        }
        CreatorQuiescenceProof::NeverSpawned { .. }
        | CreatorQuiescenceProof::LaunchGateNeverReleased { .. } => {
            match manifest.conmon_layout.conmon_pidfile.try_exists() {
                Ok(false) => Ok(()),
                Ok(true) => Err(SandboxError::OperationFailed {
                    message: format!(
                        "container restart source {} carries an unexpected conmon receipt despite no-effect creator quiescence",
                        manifest.handle.id
                    ),
                }),
                Err(error) => Err(SandboxError::OperationFailed {
                    message: format!(
                        "cannot inspect container restart conmon receipt {}: {error}; target preparation remains fenced",
                        manifest.conmon_layout.conmon_pidfile.display()
                    ),
                }),
            }
        }
    }
}

impl ContainerSandboxBackend {
    /// Stop and delete one exact source runtime without releasing any retained
    /// attachment, port lease, PEP, or machine-forwarding authority.
    pub fn quiesce_restart_source(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: serde_json::to_vec(&(
                    "restart_source_manifest_absent",
                    sandbox_id,
                    fence,
                ))
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to encode absent container restart evidence: {error}"),
                })?,
            });
        };
        require_execute_restart(&manifest, "container restart source quiescence")?;
        let (_lifecycle, mut manifest) =
            super::runner::lock_current_execute_lifecycle_for_backend(self, &manifest)?;
        require_execute_restart(&manifest, "container restart source quiescence")?;

        if &manifest.execution_attempt_id == fence.attempt_id() {
            let transition = require_exact_transition(
                &manifest,
                fence,
                "container restart source quiescence replay",
            )?;
            return Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence(
                    "source_quiescence_replayed_after_target_switch",
                    &manifest,
                    fence,
                    &format!("{transition:?}"),
                )?,
            });
        }
        manifest.require_execution_attempt(
            fence.source_attempt_id(),
            "container restart source quiescence",
        )?;

        if let Some(transition) = manifest.restart_transition.as_ref() {
            if transition.fence() != fence {
                if !is_completed_predecessor(transition, fence) {
                    return Err(crossed_fence_error(
                        &manifest,
                        "container restart source quiescence",
                        fence,
                    ));
                }
            } else {
                if let ContainerRestartTransition::SourceQuiesced {
                    creator_quiescence, ..
                } = transition
                {
                    let runtime = source_runtime_state(&manifest, creator_quiescence)?;
                    if runtime != RuntimeStateObservation::ExplicitlyAbsent {
                        return Err(SandboxError::OperationFailed {
                            message: format!(
                                "container restart source {} regained provider runtime state {runtime:?} after durable quiescence",
                                manifest.handle.id
                            ),
                        });
                    }
                    confirm_source_conmon_absence(&manifest, creator_quiescence, false)?;
                    return Ok(SandboxProvisionPhaseObservation::Succeeded {
                        evidence: phase_evidence(
                            "source_quiesced",
                            &manifest,
                            fence,
                            &format!("{runtime:?}"),
                        )?,
                    });
                }
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container restart source {} has phase {transition:?} while the source attempt still owns the manifest",
                        manifest.handle.id
                    ),
                });
            }
        }

        let creator_quiescence = self.authenticate_restart_creator_quiescence(&mut manifest)?;
        let runtime = source_runtime_state(&manifest, &creator_quiescence)?;
        if let RuntimeStateObservation::Present(_) = runtime {
            self.stop_running_restart_source(&manifest)?;
        }
        delete_runtime_and_confirm_absent(
            &manifest.conmon_launch.delete_command,
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        )?;
        confirm_source_conmon_absence(&manifest, &creator_quiescence, false)?;
        if manifest.conmon_layout.exit_status_file.exists() {
            manifest.last_exit_code =
                Some(read_exit_code(&manifest.conmon_layout.exit_status_file)?);
        }
        self.persist_creator_quiescence(&mut manifest, creator_quiescence.clone())?;
        manifest.restart_transition = Some(ContainerRestartTransition::SourceQuiesced {
            fence: fence.clone(),
            creator_quiescence,
        });
        self.persist_restart_manifest(&mut manifest, "container restart source quiescence")?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence(
                "source_quiesced",
                &manifest,
                fence,
                &"runtime_and_creator_absent",
            )?,
        })
    }

    /// Inspect source quiescence without stopping, deleting, or repairing any
    /// provider resource.
    pub fn inspect_restart_source_quiescence(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: serde_json::to_vec(&(
                    "restart_source_manifest_absent",
                    sandbox_id,
                    fence,
                ))
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to encode absent container restart evidence: {error}"),
                })?,
            });
        };
        require_execute_restart(&manifest, "container restart source quiescence inspection")?;
        let (_inspection, manifest) =
            super::runner::lock_current_inspection_for_backend(self, &manifest)?;
        require_execute_restart(&manifest, "container restart source quiescence inspection")?;
        if &manifest.execution_attempt_id == fence.attempt_id() {
            let transition = require_exact_transition(
                &manifest,
                fence,
                "container restart source quiescence inspection",
            )?;
            return Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence(
                    "source_quiescence_durable_before_target",
                    &manifest,
                    fence,
                    &format!("{transition:?}"),
                )?,
            });
        }
        manifest.require_execution_attempt(
            fence.source_attempt_id(),
            "container restart source quiescence inspection",
        )?;
        if let Some(transition) = manifest.restart_transition.as_ref()
            && transition.fence() != fence
            && !is_completed_predecessor(transition, fence)
        {
            return Err(crossed_fence_error(
                &manifest,
                "container restart source quiescence inspection",
                fence,
            ));
        }
        let Some(ContainerRestartTransition::SourceQuiesced {
            creator_quiescence, ..
        }) = manifest.restart_transition.as_ref()
        else {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "source_quiescence_not_durable",
                    &manifest,
                    fence,
                    &"source_attempt_retained",
                )?,
            });
        };
        let runtime = source_runtime_state(&manifest, creator_quiescence)?;
        if runtime != RuntimeStateObservation::ExplicitlyAbsent {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "source_runtime_present",
                    &manifest,
                    fence,
                    &format!("{runtime:?}"),
                )?,
            });
        }
        confirm_source_conmon_absence(&manifest, creator_quiescence, false)?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence("source_quiesced", &manifest, fence, &format!("{runtime:?}"))?,
        })
    }

    /// Advance from a durably quiesced source to one exact target attempt.
    /// This phase removes only stale runtime receipts; it retains every network
    /// and listener authority.
    pub fn prepare_restart_target_attempt(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        require_execute_restart(&manifest, "container restart target preparation")?;
        let (_lifecycle, mut manifest) =
            super::runner::lock_current_execute_lifecycle_for_backend(self, &manifest)?;
        require_execute_restart(&manifest, "container restart target preparation")?;

        if &manifest.execution_attempt_id == fence.source_attempt_id() {
            let (durable_fence, creator_quiescence) = match require_exact_transition(
                &manifest,
                fence,
                "container restart target preparation",
            )? {
                ContainerRestartTransition::SourceQuiesced {
                    fence,
                    creator_quiescence,
                } => (fence.clone(), creator_quiescence.clone()),
                transition => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "container restart target preparation for {} requires SourceQuiesced; found {transition:?}",
                            manifest.handle.id
                        ),
                    });
                }
            };
            if !matches!(
                manifest.creator_handoff,
                ContainerCreatorHandoffState::Quiesced { .. }
            ) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container restart target preparation for {} lacks durable creator quiescence",
                        manifest.handle.id
                    ),
                });
            }
            let runtime = source_runtime_state(&manifest, &creator_quiescence)?;
            if runtime != RuntimeStateObservation::ExplicitlyAbsent {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container restart target preparation for {} observed source runtime state {runtime:?}",
                        manifest.handle.id
                    ),
                });
            }
            confirm_source_conmon_absence(&manifest, &creator_quiescence, false)?;
            manifest.execution_attempt_id = fence.attempt_id().clone();
            manifest.restart_transition = Some(ContainerRestartTransition::TargetPreparing {
                fence: durable_fence,
                creator_quiescence,
            });
            manifest.shutdown_requested = false;
            synchronize_handle_status(&mut manifest, SandboxStatus::Starting);
            self.persist_restart_manifest(
                &mut manifest,
                "container restart target preparation intent",
            )?;
        } else if &manifest.execution_attempt_id != fence.attempt_id() {
            return Err(crossed_fence_error(
                &manifest,
                "container restart target preparation",
                fence,
            ));
        }

        let transition = require_exact_transition(
            &manifest,
            fence,
            "container restart target preparation replay",
        )?;
        if transition.target_is_prepared() {
            return Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence(
                    "target_preparation_replayed",
                    &manifest,
                    fence,
                    &format!("{transition:?}"),
                )?,
            });
        }
        let ContainerRestartTransition::TargetPreparing {
            creator_quiescence, ..
        } = transition
        else {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container restart target preparation for {} has an invalid durable phase {transition:?}",
                    manifest.handle.id
                ),
            });
        };
        let creator_quiescence = creator_quiescence.clone();
        let runtime = source_runtime_state(&manifest, &creator_quiescence)?;
        if runtime != RuntimeStateObservation::ExplicitlyAbsent {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container restart target preparation for {} found source runtime state {runtime:?} after target intent",
                    manifest.handle.id
                ),
            });
        }
        confirm_source_conmon_absence(&manifest, &creator_quiescence, true)?;
        remove_if_exists(&manifest.conmon_layout.pidfile)?;
        remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
        remove_if_exists(&manifest.conmon_layout.exit_status_file)?;
        manifest.creator_handoff = ContainerCreatorHandoffState::NotSpawned;
        manifest.restart_transition = Some(ContainerRestartTransition::TargetPrepared {
            fence: fence.clone(),
            creator_quiescence,
        });
        manifest.last_exit_code = None;
        self.persist_restart_manifest(&mut manifest, "container restart target prepared")?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence(
                "target_prepared",
                &manifest,
                fence,
                &"source_receipts_retired",
            )?,
        })
    }

    /// Inspect the durable target switch without launching a creator or
    /// repairing provider state.
    pub fn inspect_restart_target_preparation(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: serde_json::to_vec(&(
                    "restart_target_manifest_absent",
                    sandbox_id,
                    fence,
                ))
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to encode absent container restart evidence: {error}"),
                })?,
            });
        };
        require_execute_restart(&manifest, "container restart target preparation inspection")?;
        let (_inspection, manifest) =
            super::runner::lock_current_inspection_for_backend(self, &manifest)?;
        require_execute_restart(&manifest, "container restart target preparation inspection")?;
        if &manifest.execution_attempt_id == fence.source_attempt_id() {
            if let Some(transition) = manifest.restart_transition.as_ref()
                && transition.fence() != fence
            {
                return Err(crossed_fence_error(
                    &manifest,
                    "container restart target preparation inspection",
                    fence,
                ));
            }
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "target_attempt_not_durable",
                    &manifest,
                    fence,
                    &"source_attempt_retained",
                )?,
            });
        }
        manifest.require_execution_attempt(
            fence.attempt_id(),
            "container restart target preparation inspection",
        )?;
        let transition = require_exact_transition(
            &manifest,
            fence,
            "container restart target preparation inspection",
        )?;
        if transition.target_is_prepared() {
            Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence(
                    "target_prepared",
                    &manifest,
                    fence,
                    &format!("{transition:?}"),
                )?,
            })
        } else {
            Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "target_preparation_in_progress",
                    &manifest,
                    fence,
                    &format!("{transition:?}"),
                )?,
            })
        }
    }

    /// Reattach only the retained private network and PEP for the exact target
    /// attempt. Ingress publication remains a separate owner phase.
    pub fn attach_restart_retained_network(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        self.attach_restart_retained_network_with(sandbox_id, fence, |backend, manifest| {
            backend.configure_network(
                manifest,
                AttachmentAttachAuthority::RestartRetained,
                MachinePortPreparationReleaseAuthority::Retain,
                false,
            )
        })
    }

    #[cfg(test)]
    fn attach_restart_retained_network_with_test_host(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        self.attach_restart_retained_network_with(sandbox_id, fence, |backend, manifest| {
            let ports = backend.port_lease_coordinator_for_manifest(manifest)?;
            let hostname = hostname_for(&manifest.spec);
            backend
                .non_routable_attachment_adapter(
                    manifest,
                    manifest.require_network_config()?,
                    &hostname,
                )
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
        attach: impl FnOnce(&Self, &ContainerSandboxManifest) -> Result<()>,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Err(SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            });
        };
        require_execute_restart(&manifest, "container restart retained-network attachment")?;
        let (_lifecycle, mut manifest) =
            super::runner::lock_current_execute_lifecycle_for_backend(self, &manifest)?;
        require_execute_restart(&manifest, "container restart retained-network attachment")?;
        manifest.require_execution_attempt(
            fence.attempt_id(),
            "container restart retained-network attachment",
        )?;
        let transition = require_exact_transition(
            &manifest,
            fence,
            "container restart retained-network attachment",
        )?;
        if !transition.target_is_prepared() {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container restart retained-network attachment for {} requires target preparation; found {transition:?}",
                    manifest.handle.id
                ),
            });
        }
        if self.restart_retained_network_readiness(&manifest)?.0 {
            if !matches!(
                transition,
                ContainerRestartTransition::RetainedNetworkAttached { .. }
            ) {
                let creator_quiescence = transition.creator_quiescence().clone();
                manifest.restart_transition =
                    Some(ContainerRestartTransition::RetainedNetworkAttached {
                        fence: fence.clone(),
                        creator_quiescence,
                    });
                self.persist_restart_manifest(
                    &mut manifest,
                    "container restart retained-network adoption",
                )?;
            }
            return Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence(
                    "retained_network_attached",
                    &manifest,
                    fence,
                    &"provider_readiness_replayed",
                )?,
            });
        }

        attach(self, &manifest)?;
        self.ensure_egress_proxy_running_with_release_authority(
            &manifest,
            PepPreAdoptionReleaseAuthority::Retain,
        )?;
        let (ready, observation) = self.restart_retained_network_readiness(&manifest)?;
        if !ready {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container restart retained-network attachment for {} remained not ready: {observation}",
                    manifest.handle.id
                ),
            });
        }
        let creator_quiescence = transition.creator_quiescence().clone();
        manifest.restart_transition = Some(ContainerRestartTransition::RetainedNetworkAttached {
            fence: fence.clone(),
            creator_quiescence,
        });
        self.persist_restart_manifest(
            &mut manifest,
            "container restart retained network attached",
        )?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence("retained_network_attached", &manifest, fence, &observation)?,
        })
    }

    /// Inspect retained private attachment and PEP readiness without creating,
    /// repairing, or publishing ingress provider state.
    pub fn inspect_restart_retained_network(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: serde_json::to_vec(&(
                    "restart_retained_network_manifest_absent",
                    sandbox_id,
                    fence,
                ))
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!("failed to encode absent container restart evidence: {error}"),
                })?,
            });
        };
        require_execute_restart(&manifest, "container restart retained-network inspection")?;
        let (_inspection, manifest) =
            super::runner::lock_current_inspection_for_backend(self, &manifest)?;
        require_execute_restart(&manifest, "container restart retained-network inspection")?;
        manifest.require_execution_attempt(
            fence.attempt_id(),
            "container restart retained-network inspection",
        )?;
        let transition = require_exact_transition(
            &manifest,
            fence,
            "container restart retained-network inspection",
        )?;
        if !transition.target_is_prepared() {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "retained_network_waiting_for_target",
                    &manifest,
                    fence,
                    &format!("{transition:?}"),
                )?,
            });
        }
        let (ready, observation) = self.restart_retained_network_readiness(&manifest)?;
        if ready {
            Ok(SandboxProvisionPhaseObservation::Succeeded {
                evidence: phase_evidence(
                    "retained_network_attached",
                    &manifest,
                    fence,
                    &observation,
                )?,
            })
        } else {
            Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "retained_network_not_ready",
                    &manifest,
                    fence,
                    &observation,
                )?,
            })
        }
    }

    fn stop_running_restart_source(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        if manifest.conmon_layout.exit_status_file.exists() {
            read_exit_code(&manifest.conmon_layout.exit_status_file)?;
            return Ok(());
        }
        let pid = read_pid(&manifest.conmon_layout.pidfile)?;
        let stop_signal = configured_stop_signal(manifest.image_metadata.stop_signal.as_deref());
        signal_process(&stop_signal, pid)?;
        let stop_timeout = configured_stop_timeout(&manifest.spec, self.config.stop_timeout);
        if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
            signal_process("KILL", pid)?;
            if !wait_for_path(&manifest.conmon_layout.exit_status_file, stop_timeout) {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container restart source {} did not write an exit receipt after TERM/KILL",
                        manifest.handle.id
                    ),
                });
            }
        }
        read_exit_code(&manifest.conmon_layout.exit_status_file)?;
        Ok(())
    }

    fn restart_retained_network_readiness(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<(bool, String)> {
        let readiness = self.non_routable_attachment_readiness(
            manifest,
            self.authenticated_egress_readiness(manifest)?,
        )?;
        match readiness {
            OciAttachmentBaseReadinessState::Ready(observation) => {
                Ok((true, format!("{observation:?}")))
            }
            OciAttachmentBaseReadinessState::NotReady(reason) => Ok((false, format!("{reason:?}"))),
        }
    }

    fn persist_restart_manifest(
        &self,
        manifest: &mut ContainerSandboxManifest,
        context: &str,
    ) -> Result<()> {
        let candidate = manifest.clone();
        let Err(persist_error) = self.write_existing_workload_manifest(manifest) else {
            return Ok(());
        };
        match self.read_manifest(&manifest.handle.id) {
            Ok(Some(observed)) if observed == candidate => {
                *manifest = observed;
                Ok(())
            }
            Ok(Some(observed)) => {
                *manifest = observed;
                Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to publish {context}: {persist_error}; exact readback differs from the candidate and retains its own authority"
                    ),
                })
            }
            Ok(None) => Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to publish {context}: {persist_error}; canonical manifest disappeared during readback"
                ),
            }),
            Err(inspect_error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to publish {context}: {persist_error}; canonical readback also failed: {inspect_error}"
                ),
            }),
        }
    }
}

// The historical provider-local policy machinery remains test-only until the
// NNC6.4a deletion band removes its characterization fixtures. Production
// restart authority is exclusively the explicit phase API above.
#[cfg(test)]
use crate::backends::conmon::lifecycle::{
    delete_runtime_and_confirm_absent as legacy_delete_runtime_and_confirm_absent,
    remove_if_exists as legacy_remove_if_exists, restart_backoff_delay,
    restart_policy_allows_restart,
};
#[cfg(test)]
use crate::backends::oci::network::{AttachmentAuxiliaryDisposition, AttachmentTeardownMode};

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContainerRestartDecision {
    NotRestarting,
    WaitingForBackoff,
    RestartNow,
}

#[cfg(test)]
pub(super) fn mark_restart_decision_after_exit(
    manifest: &mut ContainerSandboxManifest,
    now_millis: u64,
) -> Result<ContainerRestartDecision> {
    if manifest.shutdown_requested || !manifest.conmon_layout.exit_status_file.exists() {
        return Ok(ContainerRestartDecision::NotRestarting);
    }
    let exit_code = read_exit_code(&manifest.conmon_layout.exit_status_file)?;
    if !restart_policy_allows_restart(
        manifest.spec.lifecycle.restart_policy,
        exit_code,
        manifest.restart_count,
    ) {
        return Ok(ContainerRestartDecision::NotRestarting);
    }
    manifest.last_exit_code = Some(exit_code);
    let next_restart_at_millis = manifest.next_restart_at_millis.get_or_insert_with(|| {
        now_millis.saturating_add(restart_backoff_delay(manifest.restart_count).as_millis() as u64)
    });
    if now_millis < *next_restart_at_millis {
        synchronize_handle_status(manifest, SandboxStatus::Starting);
        return Ok(ContainerRestartDecision::WaitingForBackoff);
    }
    manifest.restart_count += 1;
    manifest.next_restart_at_millis = None;
    synchronize_handle_status(manifest, SandboxStatus::Starting);
    Ok(ContainerRestartDecision::RestartNow)
}

#[cfg(test)]
impl ContainerSandboxBackend {
    pub(super) fn reset_runtime_for_restart(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<()> {
        self.validate_manifest_execution_context(manifest)?;
        let network_config = manifest.require_network_config()?;
        match manifest
            .runner_config
            .validated_machine_port_forwarder(&manifest.handle.id)?
        {
            None => self.reset_host_managed_runtime_for_restart(manifest, network_config),
            Some(forwarder) => self.reset_machine_forwarded_runtime_for_restart(
                manifest,
                network_config,
                forwarder,
            ),
        }
    }

    fn reset_host_managed_runtime_for_restart(
        &self,
        manifest: &ContainerSandboxManifest,
        network_config: &crate::backends::oci::network::OciNetworkConfig,
    ) -> Result<()> {
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let hostname = hostname_for(&manifest.spec);
        let lifecycle = self.attachment_lifecycle(&ports);
        self.attachment_adapter(manifest, network_config, &hostname, None)
            .detach_host_managed(&lifecycle, AttachmentTeardownMode::Restart, |auxiliary| {
                legacy_delete_runtime_and_confirm_absent(
                    &manifest.conmon_launch.delete_command,
                    &manifest.conmon_launch.state_command,
                    manifest.handle.id.as_str(),
                )?;
                if auxiliary == AttachmentAuxiliaryDisposition::ProviderOwned {
                    self.egress_proxies.stop_for_restart(
                        &manifest.spec.tenant_id,
                        &manifest.handle.id,
                        manifest.egress_proxy.as_ref(),
                    )?;
                }
                Ok(())
            })?;
        clear_legacy_restart_receipts(manifest)
    }

    fn reset_machine_forwarded_runtime_for_restart(
        &self,
        manifest: &ContainerSandboxManifest,
        network_config: &crate::backends::oci::network::OciNetworkConfig,
        forwarder: &crate::backends::oci::network::OciMachinePortForwarderConfig,
    ) -> Result<()> {
        let ports = self.port_lease_coordinator_for_manifest(manifest)?;
        let lifecycle = self.attachment_lifecycle(&ports);
        let hostname = hostname_for(&manifest.spec);
        self.attachment_adapter(manifest, network_config, &hostname, Some(forwarder))
            .detach_machine_forwarded(
                &lifecycle,
                AttachmentTeardownMode::Restart,
                || {
                    self.prepare_machine_port_publication_withdrawal(manifest)?;
                    legacy_delete_runtime_and_confirm_absent(
                        &manifest.conmon_launch.delete_command,
                        &manifest.conmon_launch.state_command,
                        manifest.handle.id.as_str(),
                    )?;
                    self.egress_proxies.stop_for_restart(
                        &manifest.spec.tenant_id,
                        &manifest.handle.id,
                        manifest.egress_proxy.as_ref(),
                    )?;
                    let cleanup = self.begin_machine_port_proxy_restart_for_manifest(manifest)?;
                    if let Some(cleanup) = cleanup.as_ref() {
                        self.unexpose_machine_port_proxy_publications(cleanup, forwarder)?;
                    }
                    Ok(cleanup)
                },
                |cleanup| {
                    if let Some(cleanup) = cleanup.as_ref() {
                        self.complete_machine_port_proxy_cleanup(cleanup)?;
                    }
                    Ok(())
                },
            )?;
        clear_legacy_restart_receipts(manifest)
    }
}

#[cfg(test)]
fn clear_legacy_restart_receipts(manifest: &ContainerSandboxManifest) -> Result<()> {
    legacy_remove_if_exists(&manifest.conmon_layout.pidfile)?;
    legacy_remove_if_exists(&manifest.conmon_layout.conmon_pidfile)?;
    legacy_remove_if_exists(&manifest.conmon_layout.exit_status_file)
}
