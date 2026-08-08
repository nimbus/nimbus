//! Read-only Krun lifecycle observation.
//!
//! Inspection reports authenticated evidence and cannot relaunch, finalize,
//! clean, release, or persist any runtime or network state.

use super::readiness::visible_published_endpoints;
use super::*;
use crate::backends::inspection::{RestartAssessmentInput, assess_restart};
use crate::inspection::{
    SandboxCleanupObservation, SandboxExecutionAttemptObservation, SandboxExecutionObservation,
    SandboxInspection, SandboxObservationUnknownReason, SandboxRestartAssessment,
    SandboxRestartBlocker, SandboxRestartIneligibility,
};

impl KrunSandboxBackend {
    pub(super) fn inspect_sync(&self, id: &SandboxId) -> Result<Option<SandboxInspection>> {
        let Some(observed) = self.read_manifest(id)? else {
            return Ok(None);
        };
        let (_inspection_guard, manifest) = self.lock_current_inspection(&observed)?;

        if manifest.start_mode == KrunStartMode::PlanOnly {
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
            return exact_inspection(
                &manifest,
                handle,
                SandboxExecutionObservation::PlanOnly,
                SandboxRestartAssessment::Ineligible {
                    reason: SandboxRestartIneligibility::PlanOnly,
                },
                cleanup,
            )
            .map(Some);
        }

        if matches!(
            manifest.launch_authority,
            KrunLaunchAuthority::Reserved { .. } | KrunLaunchAuthority::Adopting { .. }
        ) {
            return pending_launch_inspection(&manifest).map(Some);
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
            return exited_inspection(self, &manifest, exit_code, &exit_evidence).map(Some);
        }

        if manifest.has_terminal_network_finality() {
            let execution = manifest
                .last_exit_code
                .map(|exit_code| SandboxExecutionObservation::Exited { exit_code })
                .unwrap_or(SandboxExecutionObservation::AbsentWithoutExit);
            return exact_inspection(
                &manifest,
                project_handle(&manifest, manifest.status),
                execution,
                SandboxRestartAssessment::Ineligible {
                    reason: SandboxRestartIneligibility::ShutdownRequested,
                },
                SandboxCleanupObservation::Finalized,
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
            || self.running_status_with_egress_evidence(&manifest),
        )?;
        let provider_evidence =
            serde_json::to_vec(&observation).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to serialize krun provider observation for {}: {error}",
                    manifest.handle.id
                ),
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
}

fn exited_inspection(
    backend: &KrunSandboxBackend,
    manifest: &KrunSandboxManifest,
    exit_code: i32,
    exit_evidence: &[u8],
) -> Result<SandboxInspection> {
    if manifest.has_terminal_network_finality() {
        return exact_inspection_with_provider_evidence(
            manifest,
            project_handle(manifest, manifest.status),
            SandboxExecutionObservation::Exited { exit_code },
            SandboxRestartAssessment::Ineligible {
                reason: SandboxRestartIneligibility::ShutdownRequested,
            },
            SandboxCleanupObservation::Finalized,
            exit_evidence,
        );
    }
    let restart = assess_restart(RestartAssessmentInput {
        policy: manifest.spec.lifecycle.restart_policy,
        exit_code,
        completed_restarts: manifest.restart_count,
        persisted_not_before_millis: manifest.next_restart_at_millis,
        shutdown_requested: manifest.shutdown_requested,
        blocker: backend
            .startup_network_reconciliation_error
            .as_ref()
            .map(|_| SandboxRestartBlocker::StartupReconciliationUnavailable),
    });
    exact_inspection_with_provider_evidence(
        manifest,
        project_handle(manifest, SandboxStatus::Stopping),
        SandboxExecutionObservation::Exited { exit_code },
        restart,
        SandboxCleanupObservation::Retained,
        exit_evidence,
    )
}

fn pending_launch_inspection(manifest: &KrunSandboxManifest) -> Result<SandboxInspection> {
    let status = if matches!(
        manifest.status,
        SandboxStatus::Stopped | SandboxStatus::Failed
    ) {
        SandboxStatus::Stopping
    } else {
        manifest.status
    };
    exact_inspection(
        manifest,
        project_handle(manifest, status),
        SandboxExecutionObservation::Unknown {
            reason: SandboxObservationUnknownReason::LaunchHandoffPending,
        },
        SandboxRestartAssessment::Ineligible {
            reason: SandboxRestartIneligibility::RuntimeAbsenceUnproven,
        },
        SandboxCleanupObservation::Retained,
    )
}

fn project_handle(manifest: &KrunSandboxManifest, status: SandboxStatus) -> SandboxHandle {
    let mut handle = manifest.handle.clone();
    handle.status = status;
    handle.published_endpoints =
        visible_published_endpoints(KrunStartMode::Execute, &manifest.spec, status);
    handle
}

fn exact_inspection(
    manifest: &KrunSandboxManifest,
    handle: SandboxHandle,
    execution: SandboxExecutionObservation,
    restart: SandboxRestartAssessment,
    cleanup: SandboxCleanupObservation,
) -> Result<SandboxInspection> {
    exact_inspection_with_provider_evidence(manifest, handle, execution, restart, cleanup, &[])
}

fn exact_inspection_with_provider_evidence(
    manifest: &KrunSandboxManifest,
    handle: SandboxHandle,
    execution: SandboxExecutionObservation,
    restart: SandboxRestartAssessment,
    cleanup: SandboxCleanupObservation,
    provider_evidence: &[u8],
) -> Result<SandboxInspection> {
    let evidence =
        serde_json::to_vec(&(manifest, &handle, execution, restart, cleanup)).map_err(|error| {
            SandboxError::OperationFailed {
                message: format!(
                    "failed to serialize authenticated krun inspection evidence for {}: {error}",
                    manifest.handle.id
                ),
            }
        })?;
    Ok(SandboxInspection::exact(
        handle,
        if manifest.start_mode == KrunStartMode::PlanOnly {
            SandboxExecutionAttemptObservation::PlanOnly
        } else {
            SandboxExecutionAttemptObservation::Exact(manifest.execution_attempt_id.clone())
        },
        execution,
        restart,
        cleanup,
        &[&evidence, provider_evidence],
    ))
}
