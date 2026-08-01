//! Read-only Container lifecycle observation.
//!
//! This module is deliberately incapable of launching, cleaning, repairing,
//! releasing, or persisting provider state. It converts one authenticated
//! snapshot into typed comparison evidence for the compute coordinator.

use super::*;
use crate::backends::conmon::lifecycle::RuntimeStatusProbe;
use crate::backends::inspection::{RestartAssessmentInput, assess_restart};
use crate::inspection::{
    SandboxCleanupObservation, SandboxExecutionObservation, SandboxInspection,
    SandboxObservationUnknownReason, SandboxRestartAssessment, SandboxRestartBlocker,
    SandboxRestartIneligibility,
};

impl ContainerSandboxBackend {
    pub(super) fn inspect_sync(&self, id: &SandboxId) -> Result<Option<SandboxInspection>> {
        let Some(observed) = self.read_manifest(id)? else {
            return Ok(None);
        };
        let (_inspection_guard, manifest) =
            runner::lock_current_inspection_for_backend(self, &observed)?;

        if manifest.start_mode == ContainerStartMode::PlanOnly {
            if manifest.lifecycle_coordinator
                == ContainerLifecycleCoordinator::PreparedServiceRunner
            {
                let _ = runner::plan_only_inspection_is_durably_cancelled(&manifest)?;
            }
            let cleanup = if manifest.has_terminal_network_finality() {
                SandboxCleanupObservation::Finalized
            } else if manifest.shutdown_requested
                || matches!(
                    manifest.status,
                    SandboxStatus::Stopping | SandboxStatus::Stopped | SandboxStatus::Failed
                )
            {
                SandboxCleanupObservation::Retained
            } else {
                SandboxCleanupObservation::NotRequired
            };
            let mut handle = manifest.handle.clone();
            handle.status = if cleanup == SandboxCleanupObservation::Retained {
                SandboxStatus::Stopping
            } else {
                manifest.status
            };
            if cleanup != SandboxCleanupObservation::NotRequired {
                handle.published_endpoints.clear();
            }
            let handoff_evidence = runner::inspection_handoff_evidence(&manifest)?;
            return exact_inspection_with_provider_evidence(
                &manifest,
                handle,
                SandboxExecutionObservation::PlanOnly,
                SandboxRestartAssessment::Ineligible {
                    reason: SandboxRestartIneligibility::PlanOnly,
                },
                cleanup,
                &handoff_evidence,
            )
            .map(Some);
        }

        let (handoff_phase, handoff_evidence) =
            runner::execute_handoff_phase_with_evidence(&manifest)?;
        if let Some(phase) = handoff_phase {
            match phase {
                runner::RunnerHandoffPhase::ClaimedBeforeEffects => {
                    return pending_handoff_inspection(&manifest, &handoff_evidence).map(Some);
                }
                runner::RunnerHandoffPhase::EffectsStarted
                | runner::RunnerHandoffPhase::LifecyclePublished => {}
                runner::RunnerHandoffPhase::Cancelled => {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "Execute manifest {id} contradicts a cancelled runner handoff"
                        ),
                    });
                }
            }
        } else if manifest.launch_reservation_claim.is_some() {
            return pending_handoff_inspection(&manifest, &handoff_evidence).map(Some);
        }

        let exit_present = crate::backends::conmon::lifecycle::inspect_runtime_artifact_presence(
            &manifest.conmon_layout.exit_status_file,
            "exit-status receipt",
        )?;
        if exit_present {
            let (exit_code, exit_evidence) =
                crate::backends::conmon::lifecycle::read_exit_code_evidence(
                    &manifest.conmon_layout.exit_status_file,
                )?;
            return exited_inspection(
                self,
                &manifest,
                exit_code,
                &handoff_evidence,
                &exit_evidence,
            )
            .map(Some);
        }

        if manifest.has_terminal_network_finality() {
            let execution = manifest
                .last_exit_code
                .map(|exit_code| SandboxExecutionObservation::Exited { exit_code })
                .unwrap_or(SandboxExecutionObservation::AbsentWithoutExit);
            return exact_inspection_with_provider_evidence(
                &manifest,
                project_handle(&manifest, manifest.status),
                execution,
                SandboxRestartAssessment::Ineligible {
                    reason: SandboxRestartIneligibility::ShutdownRequested,
                },
                SandboxCleanupObservation::Finalized,
                &handoff_evidence,
            )
            .map(Some);
        }

        let observation = crate::backends::conmon::lifecycle::observe_runtime_status_with_evidence(
            RuntimeStatusProbe {
                exit_status_file: &manifest.conmon_layout.exit_status_file,
                state_command: &manifest.conmon_launch.state_command,
                runtime_id: manifest.handle.id.as_str(),
                pidfile: &manifest.conmon_layout.pidfile,
                shutdown_requested: manifest.shutdown_requested,
                current_status: manifest.status,
            },
            || self.read_only_running_status(&manifest),
        )?;
        let provider_evidence =
            serde_json::to_vec(&(&handoff_evidence, &observation)).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to serialize container provider observation for {}: {error}",
                        manifest.handle.id
                    ),
                }
            })?;
        if observation.explicitly_absent {
            return exact_inspection_with_provider_evidence(
                &manifest,
                project_handle(&manifest, SandboxStatus::Stopping),
                SandboxExecutionObservation::AbsentWithoutExit,
                SandboxRestartAssessment::Ineligible {
                    reason: SandboxRestartIneligibility::RuntimeAbsenceUnproven,
                },
                SandboxCleanupObservation::Retained,
                &provider_evidence,
            )
            .map(Some);
        }

        let (status, restart, cleanup) = match observation.status {
            SandboxStatus::Stopped | SandboxStatus::Failed | SandboxStatus::Stopping => (
                SandboxStatus::Stopping,
                SandboxRestartAssessment::Ineligible {
                    reason: SandboxRestartIneligibility::CleanupPending,
                },
                SandboxCleanupObservation::Retained,
            ),
            status => (
                status,
                SandboxRestartAssessment::Ineligible {
                    reason: SandboxRestartIneligibility::RuntimePresent,
                },
                SandboxCleanupObservation::NotRequired,
            ),
        };
        exact_inspection_with_provider_evidence(
            &manifest,
            project_handle(&manifest, status),
            SandboxExecutionObservation::Present,
            restart,
            cleanup,
            &provider_evidence,
        )
        .map(Some)
    }

    fn read_only_running_status(
        &self,
        manifest: &ContainerSandboxManifest,
    ) -> Result<(SandboxStatus, Vec<u8>)> {
        let application = crate::backends::readiness_probe::inspect_application_readiness(
            manifest.status,
            &status::published_endpoints(&manifest.spec),
            status::readiness_probe_timeout(manifest),
            self.readiness_probe_provider.as_ref(),
        );
        let readiness = self.authenticated_egress_readiness(manifest)?;
        let attachment = self.complete_attachment_readiness(manifest, readiness)?;
        let status = if attachment.is_ready() {
            application.status()
        } else {
            SandboxStatus::NotReady
        };
        let evidence =
            serde_json::to_vec(&(&application, format!("{attachment:?}"))).map_err(|error| {
                SandboxError::OperationFailed {
                    message: format!(
                        "failed to serialize container readiness observation for {}: {error}",
                        manifest.handle.id
                    ),
                }
            })?;
        Ok((status, evidence))
    }
}

fn exited_inspection(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    exit_code: i32,
    handoff_evidence: &[u8],
    exit_evidence: &[u8],
) -> Result<SandboxInspection> {
    let provider_evidence =
        serde_json::to_vec(&(handoff_evidence, exit_evidence)).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to serialize container handoff and exit evidence for {}: {error}",
                    manifest.handle.id
                ),
            }
        })?;
    if manifest.has_terminal_network_finality() {
        return exact_inspection_with_provider_evidence(
            manifest,
            project_handle(manifest, manifest.status),
            SandboxExecutionObservation::Exited { exit_code },
            SandboxRestartAssessment::Ineligible {
                reason: SandboxRestartIneligibility::ShutdownRequested,
            },
            SandboxCleanupObservation::Finalized,
            &provider_evidence,
        );
    }
    let restart = assess_restart(RestartAssessmentInput {
        policy: manifest.spec.lifecycle.restart_policy,
        exit_code,
        completed_restarts: manifest.restart_count,
        persisted_not_before_millis: manifest.next_restart_at_millis,
        shutdown_requested: manifest.shutdown_requested,
        blocker: backend
            .startup_reconciliation_error
            .as_ref()
            .map(|_| SandboxRestartBlocker::StartupReconciliationUnavailable),
    });
    exact_inspection_with_provider_evidence(
        manifest,
        project_handle(manifest, SandboxStatus::Stopping),
        SandboxExecutionObservation::Exited { exit_code },
        restart,
        SandboxCleanupObservation::Retained,
        &provider_evidence,
    )
}

fn pending_handoff_inspection(
    manifest: &ContainerSandboxManifest,
    handoff_evidence: &[u8],
) -> Result<SandboxInspection> {
    let status = if matches!(
        manifest.status,
        SandboxStatus::Stopped | SandboxStatus::Failed
    ) {
        SandboxStatus::Stopping
    } else {
        manifest.status
    };
    exact_inspection_with_provider_evidence(
        manifest,
        project_handle(manifest, status),
        SandboxExecutionObservation::Unknown {
            reason: SandboxObservationUnknownReason::LaunchHandoffPending,
        },
        SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::RuntimeAbsenceUnproven,
        },
        SandboxCleanupObservation::Retained,
        handoff_evidence,
    )
}

fn project_handle(manifest: &ContainerSandboxManifest, status: SandboxStatus) -> SandboxHandle {
    let mut handle = manifest.handle.clone();
    handle.status = status;
    handle.published_endpoints =
        visible_published_endpoints(ContainerStartMode::Execute, &manifest.spec, status);
    handle
}

fn exact_inspection_with_provider_evidence(
    manifest: &ContainerSandboxManifest,
    handle: SandboxHandle,
    execution: SandboxExecutionObservation,
    restart: SandboxRestartAssessment,
    cleanup: SandboxCleanupObservation,
    provider_evidence: &[u8],
) -> Result<SandboxInspection> {
    let evidence = serde_json::to_vec(&(manifest, &handle, execution, restart, cleanup)).map_err(
        |error| SandboxError::OperationFailed {
            message: format!(
                "failed to serialize authenticated container inspection evidence for {}: {error}",
                manifest.handle.id
            ),
        },
    )?;
    Ok(SandboxInspection::exact(
        handle,
        execution,
        restart,
        cleanup,
        &[&evidence, provider_evidence],
    ))
}
