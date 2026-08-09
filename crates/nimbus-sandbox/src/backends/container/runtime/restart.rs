//! Coordinator-issued, execution-attempt-fenced Container restart phases.
//!
//! This adapter owns only provider effects. Restart admission, policy,
//! scheduling, and phase order remain above `nimbus-sandbox`.

use nimbus_network::{NetworkProviderHandle, NetworkResourceGeneration};
use serde::Serialize;

use crate::backends::conmon::creator::{CreatorQuiescenceProof, confirm_dead_conmon_receipt};
use crate::backends::conmon::lifecycle::{
    RuntimeStateObservation, configured_stop_signal, configured_stop_timeout,
    delete_runtime_and_confirm_absent, read_exit_code, read_pid, remove_if_exists, runtime_state,
    runtime_state_for_creator_attempt, signal_process, wait_for_path,
};
use crate::backends::oci::egress::PepPreAdoptionReleaseAuthority;
use crate::backends::oci::network::{
    AttachmentAttachAuthority, MachinePortForwardReceipt, MachinePortForwardingProvider,
    MachinePortPreparationReleaseAuthority, OciAttachmentBaseReadinessState,
    OciMachinePortForwarderConfig,
};
use crate::error::{Result, SandboxError};
use crate::instance::{SandboxId, SandboxStatus};
use crate::provision::{SandboxProvisionNetworkPlan, SandboxProvisionPhaseObservation};

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

fn authenticate_restart_machine_plan<'a>(
    backend: &ContainerSandboxBackend,
    manifest: &'a ContainerSandboxManifest,
    network_plan: &SandboxProvisionNetworkPlan,
    provider_instance: &NetworkProviderHandle,
    provider_generation: NetworkResourceGeneration,
    operation: &str,
) -> Result<&'a OciMachinePortForwarderConfig> {
    backend.validate_manifest_execution_context(manifest)?;
    require_execute_restart(manifest, operation)?;
    let config = manifest.require_network_config()?;
    let durable_plan =
        config
            .network_plan
            .as_ref()
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "{operation} for {} lacks a compiled network plan",
                    manifest.handle.id
                ),
            })?;
    if network_plan.tenant_id() != &manifest.spec.tenant_id
        || network_plan.network_plan() != durable_plan
        || network_plan.generation() != durable_plan.generation()
        || network_plan.attachment_id() != &config.attachment_id
        || network_plan.bindings() != manifest.spec.port_bindings
        || network_plan.port_leases() != manifest.port_leases
    {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "{operation} for {} crossed its exact plan, attachment, listener, lease, or network generation authority",
                manifest.handle.id
            ),
        });
    }
    let forwarder = manifest
        .runner_config
        .validated_machine_port_forwarder(&manifest.handle.id)?
        .ok_or_else(|| SandboxError::InvalidSpec {
            message: format!(
                "{operation} for {} lacks machine-forwarder authority",
                manifest.handle.id
            ),
        })?;
    if forwarder.provider_instance() != provider_instance
        || forwarder.provider_generation() != provider_generation
    {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "{operation} for {} crossed its machine-forwarder provider instance or generation",
                manifest.handle.id
            ),
        });
    }
    Ok(forwarder)
}

fn current_absence_receipts(
    provider: &impl MachinePortForwardingProvider,
    manifest: &ContainerSandboxManifest,
) -> Result<Vec<MachinePortForwardReceipt>> {
    let current = provider.inspect(
        &manifest.spec.tenant_id,
        &manifest.handle.id,
        &manifest.spec.port_bindings,
    )?;
    if current.provider_instance() != provider.provider_instance()
        || current.provider_generation() != provider.provider_generation()
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container restart machine publication for {} returned crossed provider evidence",
                manifest.handle.id
            ),
        });
    }
    current
        .slots()
        .iter()
        .map(|slot| {
            slot.absent_receipt().cloned().ok_or_else(|| {
                SandboxError::OperationFailed {
                    message: format!(
                        "container restart machine publication for {} is not exactly absent at the current provider",
                        manifest.handle.id
                    ),
                }
            })
        })
        .collect()
}

fn require_restart_machine_ingress_withdrawn(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<()> {
    let Some(forwarder) = manifest
        .runner_config
        .validated_machine_port_forwarder(&manifest.handle.id)?
    else {
        return Ok(());
    };
    backend.require_restart_machine_port_proxies_absent(manifest)?;
    let durable = backend
        .absent_machine_port_evidence(&manifest.handle.id)?
        .ok_or_else(|| SandboxError::OperationFailed {
            message: format!(
                "container restart source {} cannot quiesce before machine-ingress withdrawal is durably absent",
                manifest.handle.id
            ),
        })?;
    let current = current_absence_receipts(forwarder, manifest)?;
    if current != durable.receipts {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container restart source {} cannot quiesce because current machine-ingress absence differs from durable evidence",
                manifest.handle.id
            ),
        });
    }
    Ok(())
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
        require_restart_machine_ingress_withdrawn(self, &manifest)?;

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

    fn withdraw_restart_machine_ingress_with(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
        withdraw: impl FnOnce(&ContainerSandboxManifest) -> Result<()>,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let manifest = self
            .read_manifest(sandbox_id)?
            .ok_or_else(|| SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            })?;
        let (_lifecycle, manifest) =
            super::runner::lock_current_execute_lifecycle_for_backend(self, &manifest)?;
        manifest.require_execution_attempt(
            fence.source_attempt_id(),
            "container restart machine-ingress withdrawal",
        )?;
        if let Some(transition) = manifest.restart_transition.as_ref()
            && !is_completed_predecessor(transition, fence)
        {
            return Err(crossed_fence_error(
                &manifest,
                "container restart machine-ingress withdrawal",
                fence,
            ));
        }
        authenticate_restart_machine_plan(
            self,
            &manifest,
            network_plan,
            provider_instance,
            provider_generation,
            "container restart machine-ingress withdrawal",
        )?;

        let cleanup = self.begin_machine_port_proxy_restart_for_manifest(&manifest)?;
        withdraw(&manifest)?;
        if let Some(cleanup) = cleanup.as_ref() {
            self.complete_machine_port_proxy_cleanup(cleanup)?;
        }
        self.require_restart_machine_port_proxies_absent(&manifest)?;
        let absence = self
            .absent_machine_port_evidence(sandbox_id)?
            .ok_or_else(|| SandboxError::OperationFailed {
                message: format!(
                    "container restart machine-ingress withdrawal for {sandbox_id} lacks durable exact absence"
                ),
            })?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence(
                "machine_ingress_withdrawn",
                &manifest,
                fence,
                &(
                    network_plan.plan_id(),
                    network_plan.generation(),
                    provider_instance,
                    provider_generation,
                    absence.receipts,
                ),
            )?,
        })
    }

    /// Withdraw one exact source-attempt machine ingress publication while
    /// retaining its listener leases and network authority for the target.
    pub fn withdraw_restart_machine_ingress(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<SandboxProvisionPhaseObservation> {
        self.withdraw_restart_machine_ingress_with(
            sandbox_id,
            fence,
            network_plan,
            provider_instance,
            provider_generation,
            |manifest| self.converge_absent_machine_port_publication_for_restart(manifest),
        )
    }

    #[cfg(test)]
    fn withdraw_restart_machine_ingress_with_test_provider(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<SandboxProvisionPhaseObservation> {
        self.withdraw_restart_machine_ingress_with(
            sandbox_id,
            fence,
            network_plan,
            provider_instance,
            provider_generation,
            |manifest| self.converge_absent_machine_port_publication_for_test(manifest),
        )
    }

    fn inspect_restart_machine_ingress_with_provider(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
        provider: &impl MachinePortForwardingProvider,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: serde_json::to_vec(&(
                    "restart_machine_ingress_manifest_absent",
                    sandbox_id,
                    fence,
                ))
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to encode absent restart machine-ingress evidence: {error}"
                    ),
                })?,
            });
        };
        manifest.require_execution_attempt(
            fence.source_attempt_id(),
            "container restart machine-ingress withdrawal inspection",
        )?;
        authenticate_restart_machine_plan(
            self,
            &manifest,
            network_plan,
            provider_instance,
            provider_generation,
            "container restart machine-ingress withdrawal inspection",
        )?;
        let Some(absence) = self.absent_machine_port_evidence(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "machine_ingress_withdrawal_not_durable",
                    &manifest,
                    fence,
                    &(network_plan.plan_id(), network_plan.generation()),
                )?,
            });
        };
        if self
            .require_restart_machine_port_proxies_absent(&manifest)
            .is_err()
        {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "machine_ingress_local_withdrawal_in_progress",
                    &manifest,
                    fence,
                    &(network_plan.plan_id(), network_plan.generation()),
                )?,
            });
        }
        let current = match current_absence_receipts(provider, &manifest) {
            Ok(receipts) => receipts,
            Err(error) => {
                return Ok(SandboxProvisionPhaseObservation::Ambiguous {
                    evidence: phase_evidence(
                        "machine_ingress_absence_observation_ambiguous",
                        &manifest,
                        fence,
                        &error.to_string(),
                    )?,
                });
            }
        };
        if current != absence.receipts {
            return Ok(SandboxProvisionPhaseObservation::Ambiguous {
                evidence: phase_evidence(
                    "machine_ingress_absence_receipt_mismatch",
                    &manifest,
                    fence,
                    &(current, absence.receipts),
                )?,
            });
        }
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence(
                "machine_ingress_withdrawn_current",
                &manifest,
                fence,
                &(
                    network_plan.plan_id(),
                    network_plan.generation(),
                    provider_instance,
                    provider_generation,
                    current,
                ),
            )?,
        })
    }

    /// Inspect withdrawal without stopping a worker, changing a listener
    /// lease, or mutating the external machine forwarder.
    pub fn inspect_restart_machine_ingress_withdrawal(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let manifest = self
            .read_manifest(sandbox_id)?
            .ok_or_else(|| SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            })?;
        let forwarder = authenticate_restart_machine_plan(
            self,
            &manifest,
            network_plan,
            provider_instance,
            provider_generation,
            "container restart machine-ingress withdrawal inspection",
        )?;
        self.inspect_restart_machine_ingress_with_provider(
            sandbox_id,
            fence,
            network_plan,
            provider_instance,
            provider_generation,
            forwarder,
        )
    }

    #[cfg(test)]
    fn inspect_restart_machine_ingress_withdrawal_with_test_provider(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let provider =
            crate::backends::oci::network::DeterministicMachinePortForwardingProvider::absent(
                self.read_manifest(sandbox_id)?
                    .ok_or_else(|| SandboxError::NotFound {
                        sandbox_id: sandbox_id.as_str().to_owned(),
                    })?
                    .runner_config
                    .machine_port_forwarder
                    .as_ref()
                    .ok_or_else(|| SandboxError::InvalidSpec {
                        message: format!("container {sandbox_id} has no machine forwarder"),
                    })?,
            );
        self.inspect_restart_machine_ingress_with_provider(
            sandbox_id,
            fence,
            network_plan,
            provider_instance,
            provider_generation,
            &provider,
        )
    }

    fn publish_restart_machine_ingress_with(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
        publish: impl FnOnce(&ContainerSandboxManifest) -> Result<()>,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let manifest = self
            .read_manifest(sandbox_id)?
            .ok_or_else(|| SandboxError::NotFound {
                sandbox_id: sandbox_id.as_str().to_owned(),
            })?;
        let (_lifecycle, manifest) =
            super::runner::lock_current_execute_lifecycle_for_backend(self, &manifest)?;
        manifest.require_execution_attempt(
            fence.attempt_id(),
            "container restart machine-ingress publication",
        )?;
        authenticate_restart_machine_plan(
            self,
            &manifest,
            network_plan,
            provider_instance,
            provider_generation,
            "container restart machine-ingress publication",
        )?;
        let transition = require_exact_transition(
            &manifest,
            fence,
            "container restart machine-ingress publication",
        )?;
        if !matches!(
            transition,
            ContainerRestartTransition::RetainedNetworkAttached { .. }
        ) {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container restart machine-ingress publication for {sandbox_id} requires exact retained-network readiness; found {transition:?}"
                ),
            });
        }
        let assigned_ip = self.ready_machine_publication_address(&manifest)?;
        let plan_members = Self::provision_port_plan_witness(&manifest);
        self.ensure_machine_port_proxies_running_with_publication(
            sandbox_id,
            &[assigned_ip],
            &manifest,
            MachinePortPreparationReleaseAuthority::RetainPlanned {
                plan_members: &plan_members,
            },
            || publish(&manifest),
        )?;
        let receipts = self.exposed_machine_port_receipts(sandbox_id)?;
        Ok(SandboxProvisionPhaseObservation::Succeeded {
            evidence: phase_evidence(
                "machine_ingress_published_for_target",
                &manifest,
                fence,
                &(
                    network_plan.plan_id(),
                    network_plan.generation(),
                    provider_instance,
                    provider_generation,
                    receipts,
                ),
            )?,
        })
    }

    /// Publish the retained listeners for one exact prepared target attempt.
    pub fn publish_restart_machine_ingress(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<SandboxProvisionPhaseObservation> {
        self.publish_restart_machine_ingress_with(
            sandbox_id,
            fence,
            network_plan,
            provider_instance,
            provider_generation,
            |manifest| self.converge_exposed_machine_port_publication(manifest),
        )
    }

    /// Inspect the target-attempt publication without starting listeners or
    /// repairing either the local proxy owner or the external forwarder.
    pub fn inspect_restart_machine_ingress_publication(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<SandboxProvisionPhaseObservation> {
        let Some(manifest) = self.read_manifest(sandbox_id)? else {
            return Ok(SandboxProvisionPhaseObservation::Absent {
                evidence: serde_json::to_vec(&(
                    "restart_machine_ingress_manifest_absent",
                    sandbox_id,
                    fence,
                ))
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to encode absent restart machine-ingress evidence: {error}"
                    ),
                })?,
            });
        };
        manifest.require_execution_attempt(
            fence.attempt_id(),
            "container restart machine-ingress publication inspection",
        )?;
        authenticate_restart_machine_plan(
            self,
            &manifest,
            network_plan,
            provider_instance,
            provider_generation,
            "container restart machine-ingress publication inspection",
        )?;
        let transition = require_exact_transition(
            &manifest,
            fence,
            "container restart machine-ingress publication inspection",
        )?;
        if !matches!(
            transition,
            ContainerRestartTransition::RetainedNetworkAttached { .. }
        ) {
            return Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "machine_ingress_waiting_for_retained_network",
                    &manifest,
                    fence,
                    &format!("{transition:?}"),
                )?,
            });
        }
        let assigned_ip = match self.ready_machine_publication_address(&manifest) {
            Ok(address) => address,
            Err(error) => {
                return Ok(SandboxProvisionPhaseObservation::InProgress {
                    evidence: phase_evidence(
                        "machine_ingress_waiting_for_private_attachment",
                        &manifest,
                        fence,
                        &error.to_string(),
                    )?,
                });
            }
        };
        match self.inspect_durable_machine_port_publication(&manifest) {
            Ok(super::machine_port_publication::DurableMachinePortPublicationObservation::Exposed {
                receipts,
            }) => match self.inspect_machine_forwarded_publication(&manifest, &[assigned_ip]) {
                Ok(current) => Ok(SandboxProvisionPhaseObservation::Succeeded {
                    evidence: phase_evidence(
                        "machine_ingress_current_for_target",
                        &manifest,
                        fence,
                        &(
                            network_plan.plan_id(),
                            network_plan.generation(),
                            current.provider_instance(),
                            current.provider_generation(),
                            receipts,
                        ),
                    )?,
                }),
                Err(error) => Ok(SandboxProvisionPhaseObservation::Ambiguous {
                    evidence: phase_evidence(
                        "machine_ingress_target_observation_ambiguous",
                        &manifest,
                        fence,
                        &error.to_string(),
                    )?,
                }),
            },
            Ok(super::machine_port_publication::DurableMachinePortPublicationObservation::Absent) => {
                Ok(SandboxProvisionPhaseObservation::Absent {
                    evidence: phase_evidence(
                        "machine_ingress_target_absent",
                        &manifest,
                        fence,
                        &(network_plan.plan_id(), network_plan.generation()),
                    )?,
                })
            }
            Ok(super::machine_port_publication::DurableMachinePortPublicationObservation::InProgress {
                generation,
            }) => Ok(SandboxProvisionPhaseObservation::InProgress {
                evidence: phase_evidence(
                    "machine_ingress_target_publication_in_progress",
                    &manifest,
                    fence,
                    &(network_plan.plan_id(), generation),
                )?,
            }),
            Err(error) => Ok(SandboxProvisionPhaseObservation::Ambiguous {
                evidence: phase_evidence(
                    "machine_ingress_target_durable_observation_ambiguous",
                    &manifest,
                    fence,
                    &error.to_string(),
                )?,
            }),
        }
    }

    #[cfg(test)]
    fn publish_restart_machine_ingress_with_test_provider(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
        network_plan: &SandboxProvisionNetworkPlan,
        provider_instance: &NetworkProviderHandle,
        provider_generation: NetworkResourceGeneration,
    ) -> Result<SandboxProvisionPhaseObservation> {
        self.publish_restart_machine_ingress_with(
            sandbox_id,
            fence,
            network_plan,
            provider_instance,
            provider_generation,
            |manifest| self.converge_exposed_machine_port_publication_for_test(manifest),
        )
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

#[cfg(test)]
use crate::backends::conmon::lifecycle::{
    delete_runtime_and_confirm_absent as legacy_delete_runtime_and_confirm_absent,
    remove_if_exists as legacy_remove_if_exists,
};
#[cfg(test)]
use crate::backends::oci::network::{AttachmentAuxiliaryDisposition, AttachmentTeardownMode};

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
