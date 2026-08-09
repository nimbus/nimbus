//! Prepared plan-only workload runner entrypoint.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

use crate::backends::conmon::lifecycle::read_exit_code;
use crate::backends::poll::poll_until_deadline;
use crate::error::{Result, SandboxError};

use super::config::ContainerStartMode;
use super::manifest::{ContainerLifecycleCoordinator, ContainerSandboxManifest, RunnerHandoffId};
use super::{ContainerSandboxBackend, synchronize_handle_status, visible_published_endpoints};

mod identity;
use identity::{execution_identity_sha256, pre_effect_authority_sha256, prepared_manifest_sha256};
mod recovery;
#[cfg(test)]
pub(super) use recovery::RUNNER_RESULT_ANCHOR_FILE;
pub(in crate::backends::container::runtime) use recovery::{
    RunnerEffectOutcome, reconcile_runner_effects_started, record_runner_effect_outcome,
};
use recovery::{RunnerEffectReceipt, validate_runner_effect_receipt};

#[cfg(test)]
mod test_probe;
#[cfg(test)]
pub(super) use test_probe::{
    RunnerDecisionStageFault, RunnerLifecycleLockTestProbe, claim_runner_execution_for_test,
    claim_runner_execution_with_stage_fault_for_test, persist_claimed_runner_execution_for_test,
};

pub(super) const RUNNER_MANIFEST_POINTER_FILE: &str = ".nimbus-container-manifest";
pub(super) const RUNNER_HANDOFF_DECISION_FILE: &str = ".nimbus-runner-handoff-decision.json";
pub(super) const RUNNER_HANDOFF_LOCK_FILE: &str = ".nimbus-runner-handoff.lock";
const RUNNER_HANDOFF_DECISION_STAGE_FILE: &str = ".nimbus-runner-handoff-decision.stage";
const RUNNER_HANDOFF_PHASE_STAGE_FILE: &str = ".nimbus-runner-handoff-phase.stage";
const RUNNER_HANDOFF_DECISION_VERSION: u32 = 6;
const RUNNER_HANDOFF_LOCK_TIMEOUT: Duration = Duration::from_secs(5);
const RUNNER_HANDOFF_LOCK_RETRY: Duration = Duration::from_millis(10);
const RUNNER_CLEANUP_RETRY_DELAY: Duration = Duration::from_millis(250);
const RUNNER_CONVERGENCE_ATTEMPTS: usize = 4;

/// Process-scoped ownership of a prepared runner's decision and manifest
/// transition. Closing the file releases the OS lock, so a successor can
/// replay an already-durable Execute decision after owner death.
#[derive(Debug)]
pub(super) struct RunnerHandoffGuard {
    _lock: File,
}

pub(super) struct RunnerInspectionGuard {
    _lock: File,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum RunnerHandoffDecision {
    Execute,
    Cancel,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum RunnerHandoffPhase {
    ClaimedBeforeEffects,
    EffectsStarted,
    LifecyclePublished,
    Cancelled,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RunnerHandoffDecisionRecord {
    version: u32,
    tenant_id: String,
    sandbox_id: String,
    decision: RunnerHandoffDecision,
    phase: RunnerHandoffPhase,
    decision_id: RunnerHandoffId,
    prepared_manifest_sha256: String,
    pre_effect_authority_sha256: String,
    execution_identity_sha256: String,
    #[serde(deserialize_with = "crate::backends::oci::deserialize_required_option")]
    effect_receipt: Option<RunnerEffectReceipt>,
}

#[derive(Debug)]
struct PublishedRunnerLifecycleAuthority {
    manifest_path: PathBuf,
    state_dir: PathBuf,
    tenant_id: String,
    sandbox_id: String,
    execution_identity_sha256: String,
}

impl PublishedRunnerLifecycleAuthority {
    fn capture(manifest: &ContainerSandboxManifest) -> Result<Self> {
        manifest.require_lifecycle_coordinator(
            ContainerLifecycleCoordinator::PreparedServiceRunner,
            "container runner finalization",
        )?;
        Ok(Self {
            manifest_path: manifest.conmon_layout.manifest_path.clone(),
            state_dir: manifest.conmon_layout.container_state_dir.clone(),
            tenant_id: manifest.spec.tenant_id.to_string(),
            sandbox_id: manifest.handle.id.to_string(),
            execution_identity_sha256: execution_identity_sha256(manifest)?,
        })
    }

    fn authenticate(&self, manifest: &ContainerSandboxManifest) -> Result<()> {
        manifest.require_lifecycle_coordinator(
            ContainerLifecycleCoordinator::PreparedServiceRunner,
            "container runner finalization",
        )?;
        let prepared = prepared_projection(manifest);
        let decision_path = runner_handoff_decision_path(&prepared);
        let decision = read_runner_handoff_decision(&decision_path)?;
        let identity_matches = manifest.start_mode == ContainerStartMode::Execute
            && manifest.conmon_layout.manifest_path == self.manifest_path
            && manifest.conmon_layout.container_state_dir == self.state_dir
            && manifest.spec.tenant_id.as_str() == self.tenant_id
            && manifest.handle.id.as_str() == self.sandbox_id
            && execution_identity_sha256(manifest)? == self.execution_identity_sha256;
        if !identity_matches {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container runner finalization found changed lifecycle identity for {}; \
                     provider cleanup remains fenced",
                    self.sandbox_id
                ),
            });
        }
        validate_runner_handoff_decision(&prepared, &decision)?;
        if decision.decision != RunnerHandoffDecision::Execute
            || decision.phase != RunnerHandoffPhase::LifecyclePublished
        {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container runner finalization requires an authenticated Execute lifecycle \
                     publication for {}; found {:?}/{:?}",
                    self.sandbox_id, decision.decision, decision.phase
                ),
            });
        }
        Ok(())
    }
}

pub fn run_prepared_container_service_workload(bundle_dir: impl AsRef<Path>) -> Result<()> {
    let bundle_dir = bundle_dir.as_ref();
    let manifest_path = read_runner_manifest_pointer(bundle_dir)?;
    let mut manifest = read_runner_manifest(&manifest_path)?;
    if manifest.bundle_layout.bundle_dir != bundle_dir {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner bundle {} does not match prepared manifest bundle {}",
                bundle_dir.display(),
                manifest.bundle_layout.bundle_dir.display()
            ),
        });
    }
    validate_runner_authority_roots(&manifest)?;
    let backend =
        ContainerSandboxBackend::reconstruct_for_runner(manifest.runner_config.to_backend_config());
    let acquisition = acquire_runner_execution_ownership(&backend, &mut manifest, true)?;
    let (handoff, recovered_outcome) = match acquisition {
        RunnerExecutionAcquisition::Fresh(handoff) => (handoff, None),
        RunnerExecutionAcquisition::Recovered { handoff, outcome } => (handoff, Some(outcome)),
    };
    if recovered_outcome == Some(RunnerEffectOutcome::Absent) {
        drop(handoff);
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container runner recovered an explicitly absent initial provider effect for {}; \
                 cleanup converged without replaying launch",
                manifest.handle.id
            ),
        });
    }
    if recovered_outcome.is_none() {
        converge_runner_effects_started(&manifest, &handoff)?;
        let launch_result = backend.execute_start(&mut manifest).map(drop);
        let cleanup = launch_result
            .as_ref()
            .err()
            .map(|failure| (failure.cleanup_state, failure.terminal_status));
        let effect_outcome = if launch_result.is_ok() {
            RunnerEffectOutcome::Present
        } else {
            RunnerEffectOutcome::Absent
        };
        let launch_result = launch_result.map_err(|failure| failure.error);
        converge_runner_launch_result_with_fallible_cleanup(
            &mut manifest,
            launch_result,
            |candidate| {
                let (cleanup_state, terminal_status) =
                    cleanup.expect("launch failure must carry an exact cleanup state");
                converge_initial_launch_cleanup(&backend, candidate, terminal_status, cleanup_state)
            },
            |candidate| {
                record_runner_effect_outcome(candidate, effect_outcome, &handoff)?;
                publish_runner_lifecycle_ownership(candidate, &handoff)
            },
            wait_for_runner_ownership,
        )?;
    }
    let lifecycle_authority = PublishedRunnerLifecycleAuthority::capture(&manifest)?;
    // The durable LifecyclePublished phase prevents execution replay while
    // allowing ordinary stop/inspect operations to own the long-running
    // workload lifecycle. The handoff lock protects only start
    // linearization; retaining it through the workload wait would deny stop.
    drop(handoff);
    let exit_code = match wait_for_container_runner_exit(&manifest) {
        Ok(exit_code) => exit_code,
        Err(error) => {
            return Err(finalize_runner_failure(
                &backend,
                &mut manifest,
                &lifecycle_authority,
                error,
            ));
        }
    };
    finalize_runner_exit_with_authority(&backend, &mut manifest, &lifecycle_authority, exit_code)?;
    if exit_code != 0 {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container workload {} exited with status {exit_code}",
                manifest.handle.id
            ),
        });
    }
    Ok(())
}

fn finalize_runner_failure(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    authority: &PublishedRunnerLifecycleAuthority,
    primary: SandboxError,
) -> SandboxError {
    match finalize_published_runner_lifecycle(
        backend,
        manifest,
        authority,
        crate::instance::SandboxStatus::Failed,
        None,
    ) {
        Ok(()) => primary,
        Err(finalization) => SandboxError::OperationFailed {
            message: format!(
                "container workload wait failed: {primary}; lifecycle finalization remains \
                 fenced: {finalization}"
            ),
        },
    }
}

#[cfg(test)]
pub(super) fn finalize_runner_failure_for_test(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    primary: SandboxError,
) -> SandboxError {
    match PublishedRunnerLifecycleAuthority::capture(manifest) {
        Ok(authority) => finalize_runner_failure(backend, manifest, &authority, primary),
        Err(error) => error,
    }
}

fn require_prepared_runner_manifest(manifest: &ContainerSandboxManifest) -> Result<()> {
    manifest.require_lifecycle_coordinator(
        ContainerLifecycleCoordinator::PreparedServiceRunner,
        "container runner",
    )?;
    if manifest.start_mode != ContainerStartMode::PlanOnly {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner expected a prepared plan-only workload manifest, got {:?}",
                manifest.start_mode
            ),
        });
    }
    Ok(())
}

fn require_execute_handoff_source(
    manifest: &ContainerSandboxManifest,
    operation: &str,
) -> Result<()> {
    if manifest.start_mode == ContainerStartMode::Execute {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "{operation} requires an Execute source manifest for {}; found {:?}",
            manifest.handle.id, manifest.start_mode
        ),
    })
}

#[cfg(test)]
pub(super) fn persist_runner_execution_ownership(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
) -> Result<RunnerHandoffGuard> {
    match acquire_runner_execution_ownership(backend, manifest, false)? {
        RunnerExecutionAcquisition::Fresh(handoff) => Ok(handoff),
        RunnerExecutionAcquisition::Recovered { .. } => {
            unreachable!("test-facing ownership acquisition disables effect recovery")
        }
    }
}

enum RunnerExecutionAcquisition {
    Fresh(RunnerHandoffGuard),
    Recovered {
        handoff: RunnerHandoffGuard,
        outcome: RunnerEffectOutcome,
    },
}

fn acquire_runner_execution_ownership(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    recover_effects_started: bool,
) -> Result<RunnerExecutionAcquisition> {
    manifest.require_lifecycle_coordinator(
        ContainerLifecycleCoordinator::PreparedServiceRunner,
        "container runner",
    )?;
    let handoff = lock_runner_handoff(manifest)?;
    let caller_prepared = match manifest.start_mode {
        ContainerStartMode::PlanOnly => manifest.clone(),
        ContainerStartMode::Execute => prepared_projection(manifest),
    };
    let caller_decision_path = runner_handoff_decision_path(&caller_prepared);
    if caller_decision_path.exists() {
        let decision = read_runner_handoff_decision(&caller_decision_path)?;
        if decision.decision == RunnerHandoffDecision::Cancel {
            validate_plan_only_cancellation_progress(&caller_prepared, &decision)?;
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container runner handoff is already decided as Cancel in phase {:?}; \
                     refusing Execute replay at {}",
                    decision.phase,
                    caller_decision_path.display()
                ),
            });
        }
        validate_runner_handoff_decision(&caller_prepared, &decision)?;
    }
    let persisted = read_runner_manifest(&manifest.conmon_layout.manifest_path)?;
    let prepared = match persisted.start_mode {
        ContainerStartMode::PlanOnly if persisted == *manifest => persisted.clone(),
        ContainerStartMode::Execute => {
            let prepared = prepared_projection(&persisted);
            if *manifest != persisted && *manifest != prepared {
                return Err(changed_runner_manifest_error(manifest));
            }
            *manifest = persisted.clone();
            prepared
        }
        ContainerStartMode::PlanOnly => {
            return Err(changed_runner_manifest_error(manifest));
        }
    };
    let decision_path = runner_handoff_decision_path(&prepared);
    let execute_handoff_id;
    if decision_path.exists() {
        let decision = read_runner_handoff_decision(&decision_path)?;
        if decision.decision == RunnerHandoffDecision::Cancel {
            validate_plan_only_cancellation_progress(&prepared, &decision)?;
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container runner handoff is already decided as Cancel in phase {:?}; \
                     refusing Execute replay at {}",
                    decision.phase,
                    decision_path.display()
                ),
            });
        }
        validate_runner_handoff_decision(&prepared, &decision)?;
        execute_handoff_id = decision.decision_id.clone();
        match (decision.decision, decision.phase) {
            (RunnerHandoffDecision::Execute, RunnerHandoffPhase::ClaimedBeforeEffects) => {}
            (RunnerHandoffDecision::Execute, RunnerHandoffPhase::EffectsStarted) => {
                if recover_effects_started {
                    let outcome = reconcile_runner_effects_started(backend, manifest, &handoff)?;
                    return Ok(RunnerExecutionAcquisition::Recovered { handoff, outcome });
                }
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner effects may already exist for {}; inspect-before-retry \
                         reconciliation is required",
                        manifest.handle.id
                    ),
                });
            }
            (RunnerHandoffDecision::Execute, RunnerHandoffPhase::LifecyclePublished) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner lifecycle is already published for {}; refusing a \
                         duplicate Execute owner",
                        manifest.handle.id
                    ),
                });
            }
            (winner, phase) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner handoff is already decided as {winner:?} in phase \
                         {phase:?}; refusing Execute replay at {}",
                        decision_path.display()
                    ),
                });
            }
        }
    } else {
        claim_runner_handoff_decision(
            &prepared,
            RunnerHandoffDecision::Execute,
            ContainerLifecycleCoordinator::PreparedServiceRunner,
            "container runner",
        )?;
        execute_handoff_id = read_runner_handoff_decision(&decision_path)?.decision_id;
    }
    if persisted.start_mode == ContainerStartMode::PlanOnly {
        *manifest = persisted;
        persist_claimed_runner_execution_ownership(
            backend,
            manifest,
            &decision_path,
            execute_handoff_id,
        )?;
    } else {
        match manifest.runner_handoff_id.as_ref() {
            Some(persisted) if persisted == &execute_handoff_id => {}
            Some(_) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner manifest {} carries a substituted handoff generation; \
                         provider effects remain fenced",
                        manifest.handle.id
                    ),
                });
            }
            None => {
                manifest.runner_handoff_id = Some(execute_handoff_id);
                backend.write_manifest(manifest)?;
            }
        }
    }
    Ok(RunnerExecutionAcquisition::Fresh(handoff))
}

/// Persist and exclusively fence an ordinary Execute start before effects.
///
/// Unlike a PlanOnly runner handoff, the caller already owns an Execute-shaped
/// manifest. Publishing that manifest and the `ClaimedBeforeEffects` decision
/// under one OS lock gives concurrent inspect/stop paths an unambiguous crash
/// boundary: no decision means no effect has started; `EffectsStarted` means
/// provider inspection is required before retry.
#[cfg(test)]
pub(super) fn persist_direct_execution_ownership(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
) -> Result<RunnerHandoffGuard> {
    if manifest.start_mode != ContainerStartMode::Execute {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "direct container execution requires an Execute manifest, got {:?}",
                manifest.start_mode
            ),
        });
    }
    manifest.require_lifecycle_coordinator(
        ContainerLifecycleCoordinator::DirectBackend,
        "direct container execution",
    )?;
    fs::create_dir_all(&manifest.conmon_layout.container_state_dir).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "failed to create direct container execution state directory {}: {error}",
                manifest.conmon_layout.container_state_dir.display()
            ),
        }
    })?;
    let handoff = lock_runner_handoff(manifest)?;
    if manifest.conmon_layout.manifest_path.exists() {
        let persisted = read_runner_manifest(&manifest.conmon_layout.manifest_path)?;
        if persisted != *manifest {
            return Err(changed_runner_manifest_error(manifest));
        }
    } else {
        backend.write_manifest(manifest)?;
    }

    let prepared = prepared_projection(manifest);
    let decision_path = runner_handoff_decision_path(&prepared);
    let execute_handoff_id;
    if decision_path.exists() {
        let decision = read_runner_handoff_decision(&decision_path)?;
        validate_runner_handoff_decision(&prepared, &decision)?;
        execute_handoff_id = decision.decision_id.clone();
        match (decision.decision, decision.phase) {
            (RunnerHandoffDecision::Execute, RunnerHandoffPhase::ClaimedBeforeEffects) => {}
            (RunnerHandoffDecision::Execute, RunnerHandoffPhase::EffectsStarted) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "direct container effects may already exist for {}; \
                         inspect-before-retry reconciliation is required",
                        manifest.handle.id
                    ),
                });
            }
            (RunnerHandoffDecision::Execute, RunnerHandoffPhase::LifecyclePublished) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "direct container lifecycle is already published for {}; refusing a \
                         duplicate Execute owner",
                        manifest.handle.id
                    ),
                });
            }
            (winner, phase) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "direct container execution is already decided as {winner:?} in phase \
                         {phase:?}; refusing Execute replay at {}",
                        decision_path.display()
                    ),
                });
            }
        }
    } else {
        claim_runner_handoff_decision(
            &prepared,
            RunnerHandoffDecision::Execute,
            ContainerLifecycleCoordinator::DirectBackend,
            "direct container execution",
        )?;
        execute_handoff_id = read_runner_handoff_decision(&decision_path)?.decision_id;
    }
    match manifest.runner_handoff_id.as_ref() {
        Some(persisted) if persisted == &execute_handoff_id => {}
        Some(_) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "direct container manifest {} carries a substituted runner handoff \
                     generation; provider effects remain fenced",
                    manifest.handle.id
                ),
            });
        }
        None => {
            manifest.runner_handoff_id = Some(execute_handoff_id);
            backend.write_manifest(manifest)?;
        }
    }
    Ok(handoff)
}

/// Publish the post-linearization lifecycle phase only after the exact
/// resulting manifest is durable.
///
/// Retaining a durable terminal phase prevents a second runner from treating
/// the absence of a decision as permission to repeat initial provider effects,
/// while ordinary lifecycle operations treat this phase as a completed
/// handoff rather than a workload-lifetime lock.
pub(super) fn publish_runner_lifecycle_ownership(
    result_manifest: &ContainerSandboxManifest,
    _handoff: &RunnerHandoffGuard,
) -> Result<()> {
    require_execute_handoff_source(
        result_manifest,
        "container runner lifecycle ownership publication",
    )?;
    let persisted = read_runner_manifest(&result_manifest.conmon_layout.manifest_path)?;
    if persisted != *result_manifest {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot publish container lifecycle ownership for {} before its exact resulting \
                 manifest is durable",
                result_manifest.handle.id
            ),
        });
    }
    let prepared = prepared_projection(result_manifest);
    let decision_path = runner_handoff_decision_path(&prepared);
    let mut decision = read_runner_handoff_decision(&decision_path)?;
    if decision.phase == RunnerHandoffPhase::ClaimedBeforeEffects
        && result_manifest.shutdown_requested
        && result_manifest.status == crate::instance::SandboxStatus::Stopped
    {
        validate_pre_effect_cleanup_completion(&prepared, &decision)?;
    } else {
        validate_runner_handoff_decision(&prepared, &decision)?;
    }
    if decision.decision != RunnerHandoffDecision::Execute {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot complete direct container execution after {:?} won at {}",
                decision.decision,
                decision_path.display()
            ),
        });
    }
    match decision.phase {
        RunnerHandoffPhase::EffectsStarted if decision.effect_receipt.is_some() => {}
        RunnerHandoffPhase::EffectsStarted => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot publish container lifecycle ownership for {} before an exact \
                     generation-bound effect receipt is durable",
                    result_manifest.handle.id
                ),
            });
        }
        RunnerHandoffPhase::ClaimedBeforeEffects
            if result_manifest.shutdown_requested
                && result_manifest.status == crate::instance::SandboxStatus::Stopped => {}
        RunnerHandoffPhase::LifecyclePublished => {
            return sync_runner_handoff_parent(result_manifest, &decision_path);
        }
        phase => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "cannot publish container lifecycle ownership from handoff phase {phase:?} \
                     for {}",
                    result_manifest.handle.id
                ),
            });
        }
    }
    decision.phase = RunnerHandoffPhase::LifecyclePublished;
    durably_replace_runner_handoff_decision(result_manifest, &decision_path, &decision).map_err(
        |error| SandboxError::OperationFailed {
            message: format!(
                "failed to durably publish container lifecycle ownership at {}: {error}",
                decision_path.display()
            ),
        },
    )
}

/// Test seam for holding the same Execute lifecycle lock as production paths.
#[cfg(test)]
pub(super) fn lock_execute_lifecycle(
    manifest: &ContainerSandboxManifest,
) -> Result<RunnerHandoffGuard> {
    lock_current_execute_lifecycle(manifest, None).map(|(handoff, _)| handoff)
}

/// Acquire the bounded Execute lifecycle lock and return the canonical
/// manifest authenticated under that lock.
///
/// Reload needs the reread as its mutation base: using the pre-lock snapshot
/// after waiting would allow a stopped or finalized manifest to be overwritten.
pub(super) fn lock_current_execute_lifecycle_for_backend(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<(RunnerHandoffGuard, ContainerSandboxManifest)> {
    lock_current_execute_lifecycle(manifest, Some(backend))
}

/// Acquire the existing lifecycle lock in shared mode and authenticate the
/// exact manifest snapshot used by a read-only inspection.
///
/// Unlike command-side lifecycle locking, this query seam never creates a
/// directory or lock artifact. A missing synchronization artifact is an
/// explicit ambiguity and therefore fails closed.
pub(super) fn lock_current_inspection_for_backend(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<(RunnerInspectionGuard, ContainerSandboxManifest)> {
    lock_current_inspection_with_timeout(backend, manifest, RUNNER_HANDOFF_LOCK_TIMEOUT)
}

#[cfg(test)]
pub(super) fn lock_current_inspection_for_backend_with_timeout_for_test(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    timeout: Duration,
) -> Result<(RunnerInspectionGuard, ContainerSandboxManifest)> {
    lock_current_inspection_with_timeout(backend, manifest, timeout)
}

fn lock_current_inspection_with_timeout(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    timeout: Duration,
) -> Result<(RunnerInspectionGuard, ContainerSandboxManifest)> {
    #[cfg(not(test))]
    let _ = backend;
    let lock_path = manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_HANDOFF_LOCK_FILE);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&lock_path)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to open existing container inspection lock {}: {error}; \
                 inspection cannot create synchronization state",
                lock_path.display()
            ),
        })?;
    let deadline = Instant::now() + timeout;
    loop {
        match FileExt::try_lock_shared(&lock) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                #[cfg(test)]
                if let Some(probe) = backend.runner_lifecycle_lock_test_probe.as_ref() {
                    probe.record_contended()?;
                }
                if Instant::now() >= deadline {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "timed out acquiring existing container inspection lock {}; \
                             observation remains unknown",
                            lock_path.display()
                        ),
                    });
                }
                thread::sleep(RUNNER_HANDOFF_LOCK_RETRY);
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to acquire existing container inspection lock {}: {error}",
                        lock_path.display()
                    ),
                });
            }
        }
    }
    let persisted = read_runner_manifest(&manifest.conmon_layout.manifest_path)?;
    Ok((RunnerInspectionGuard { _lock: lock }, persisted))
}

fn lock_current_execute_lifecycle(
    manifest: &ContainerSandboxManifest,
    test_observer: Option<&ContainerSandboxBackend>,
) -> Result<(RunnerHandoffGuard, ContainerSandboxManifest)> {
    if manifest.start_mode != ContainerStartMode::Execute {
        return Err(SandboxError::InvalidSpec {
            message: "execute lifecycle lock requires an Execute manifest".to_owned(),
        });
    }
    let handoff = lock_runner_handoff_with_deadline(
        manifest,
        Some(Instant::now() + RUNNER_HANDOFF_LOCK_TIMEOUT),
        test_observer,
    )?;
    let persisted = read_runner_manifest(&manifest.conmon_layout.manifest_path)?;
    if persisted != *manifest {
        return Err(changed_runner_manifest_error(manifest));
    }
    Ok((handoff, persisted))
}

fn persist_claimed_runner_execution_ownership(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    decision_path: &Path,
    execute_handoff_id: RunnerHandoffId,
) -> Result<()> {
    // `PlanOnly` describes the durable preview before this runner takes
    // execution ownership. The executed manifest must enter the ordinary
    // provider-backed lifecycle before launch so later inspect/stop paths
    // cannot mistake real effects for an effect-free preview.
    manifest.start_mode = ContainerStartMode::Execute;
    manifest.runner_handoff_id = Some(execute_handoff_id);
    backend
        .write_manifest(manifest)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to persist container runner execution ownership while durable claim {} \
                 remains fenced: {error}",
                decision_path.display()
            ),
        })
}

fn prepared_projection(manifest: &ContainerSandboxManifest) -> ContainerSandboxManifest {
    let mut prepared = manifest.clone();
    prepared.start_mode = ContainerStartMode::PlanOnly;
    // A final drain can race after the runner has durably won admission but
    // before its first effect. Teardown progress is not part of the admitted
    // execution identity; the effect boundary authenticates it separately and
    // rejects a closed barrier immediately before the effect.
    prepared.execution_teardown = Default::default();
    prepared
}

fn changed_runner_manifest_error(manifest: &ContainerSandboxManifest) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!(
            "container runner handoff found a changed durable manifest for {}; execution and \
             cancellation remain fenced",
            manifest.handle.id
        ),
    }
}

/// Fence a PlanOnly manifest mutation against runner ownership.
///
/// Callers retain the returned guard through cleanup and manifest persistence.
/// Cancellation publishes the durable Cancel winner; an ordinary status
/// refresh is admitted only while no winner exists.
pub(super) fn lock_plan_only_status_update(
    manifest: &ContainerSandboxManifest,
    cancellation: bool,
) -> Result<RunnerHandoffGuard> {
    require_prepared_runner_manifest(manifest)?;
    let handoff = lock_runner_handoff(manifest)?;
    validate_durable_prepared_manifest(manifest)?;
    if cancellation {
        claim_runner_handoff_decision(
            manifest,
            RunnerHandoffDecision::Cancel,
            ContainerLifecycleCoordinator::PreparedServiceRunner,
            "container runner cancellation",
        )?;
    } else {
        reject_existing_runner_handoff_decision(manifest)?;
    }
    Ok(handoff)
}

/// Authenticate the decision evidence for a PlanOnly prepared-runner snapshot.
///
/// The caller must hold `RunnerInspectionGuard` for this manifest. The helper
/// only reads the existing decision record and never creates synchronization
/// state.
pub(super) fn plan_only_inspection_is_durably_cancelled(
    manifest: &ContainerSandboxManifest,
) -> Result<bool> {
    require_prepared_runner_manifest(manifest)?;
    let decision_path = runner_handoff_decision_path(manifest);
    let Some(decision) = read_optional_runner_handoff_decision(&decision_path)? else {
        return Ok(false);
    };
    validate_terminal_plan_only_cancellation(manifest, &decision)?;
    Ok(true)
}

fn claim_runner_handoff_decision(
    manifest: &ContainerSandboxManifest,
    decision: RunnerHandoffDecision,
    expected_coordinator: ContainerLifecycleCoordinator,
    owner: &str,
) -> Result<PathBuf> {
    claim_runner_handoff_decision_with_fault(manifest, decision, expected_coordinator, owner, None)
}

fn claim_runner_handoff_decision_with_fault(
    manifest: &ContainerSandboxManifest,
    decision: RunnerHandoffDecision,
    expected_coordinator: ContainerLifecycleCoordinator,
    owner: &str,
    #[cfg(test)] fault: Option<RunnerDecisionStageFault>,
    #[cfg(not(test))] _fault: Option<()>,
) -> Result<PathBuf> {
    manifest.require_lifecycle_coordinator(expected_coordinator, owner)?;
    if manifest.start_mode != ContainerStartMode::PlanOnly {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "{owner} requires a PlanOnly decision projection, got {:?}",
                manifest.start_mode
            ),
        });
    }
    let decision_path = runner_handoff_decision_path(manifest);
    let record = RunnerHandoffDecisionRecord {
        version: RUNNER_HANDOFF_DECISION_VERSION,
        tenant_id: manifest.spec.tenant_id.to_string(),
        sandbox_id: manifest.handle.id.to_string(),
        decision,
        phase: match decision {
            RunnerHandoffDecision::Execute => RunnerHandoffPhase::ClaimedBeforeEffects,
            RunnerHandoffDecision::Cancel => RunnerHandoffPhase::Cancelled,
        },
        decision_id: RunnerHandoffId::mint(),
        prepared_manifest_sha256: prepared_manifest_sha256(manifest)?,
        pre_effect_authority_sha256: pre_effect_authority_sha256(manifest)?,
        execution_identity_sha256: execution_identity_sha256(manifest)?,
        effect_receipt: None,
    };
    let mut rendered =
        serde_json::to_vec_pretty(&record).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize container runner handoff decision: {error}"),
        })?;
    rendered.push(b'\n');

    let staged_path = manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_HANDOFF_DECISION_STAGE_FILE);
    let publish_result = (|| -> Result<()> {
        reconcile_runner_stage_file(manifest, &staged_path)?;
        let mut staged = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create staged container runner handoff decision {}: {error}",
                    staged_path.display()
                ),
            })?;
        #[cfg(test)]
        if matches!(fault, Some(RunnerDecisionStageFault::AfterCreate)) {
            return Err(SandboxError::OperationFailed {
                message: "injected runner decision failure after stage creation".to_owned(),
            });
        }
        staged
            .write_all(&rendered)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to write staged container runner handoff decision {}: {error}",
                    staged_path.display()
                ),
            })?;
        #[cfg(test)]
        if matches!(fault, Some(RunnerDecisionStageFault::AfterWrite)) {
            return Err(SandboxError::OperationFailed {
                message: "injected runner decision failure before stage sync".to_owned(),
            });
        }
        staged
            .sync_all()
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to durably stage container runner handoff decision {}: {error}",
                    staged_path.display()
                ),
            })?;
        match fs::hard_link(&staged_path, &decision_path) {
            Ok(()) => fs::File::open(&manifest.conmon_layout.container_state_dir)
                .and_then(|directory| directory.sync_all())
                .map_err(|error| SandboxError::OperationFailed {
                    message: format!(
                        "failed to durably publish container runner handoff decision {}: {error}; \
                         the exclusive decision remains fenced",
                        decision_path.display()
                    ),
                }),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                let persisted = read_runner_handoff_decision(&decision_path)?;
                match (persisted.decision, persisted.phase, decision) {
                    (
                        RunnerHandoffDecision::Cancel,
                        RunnerHandoffPhase::Cancelled,
                        RunnerHandoffDecision::Cancel,
                    ) => validate_plan_only_cancellation_progress(manifest, &persisted),
                    // Execute/Execute is an owner-loss replay. The caller holds
                    // the process lock and has revalidated the exact durable
                    // PlanOnly manifest, so no previous executor remains live.
                    (
                        RunnerHandoffDecision::Execute,
                        RunnerHandoffPhase::ClaimedBeforeEffects,
                        RunnerHandoffDecision::Execute,
                    ) => validate_runner_handoff_decision(manifest, &persisted),
                    (
                        RunnerHandoffDecision::Execute,
                        RunnerHandoffPhase::EffectsStarted,
                        RunnerHandoffDecision::Execute,
                    ) => {
                        validate_runner_handoff_decision(manifest, &persisted)?;
                        Err(SandboxError::OperationFailed {
                            message: format!(
                                "container runner effects may already exist for {}; \
                                 inspect-before-retry reconciliation is required",
                                manifest.handle.id
                            ),
                        })
                    }
                    (winner, phase, loser) => {
                        validate_runner_handoff_decision(manifest, &persisted)?;
                        Err(SandboxError::OperationFailed {
                            message: format!(
                                "container runner handoff is already decided as {winner:?} in phase \
                                 {phase:?}; refusing conflicting {loser:?} decision at {}",
                                decision_path.display()
                            ),
                        })
                    }
                }
            }
            Err(error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to atomically publish container runner handoff decision {}: {error}",
                    decision_path.display()
                ),
            }),
        }
    })();
    let cleanup_result = reconcile_runner_stage_file(manifest, &staged_path);
    match (publish_result, cleanup_result) {
        (Ok(()), Ok(())) => {}
        (Err(error), Ok(())) => return Err(error),
        (Ok(()), Err(cleanup)) => return Err(cleanup),
        (Err(error), Err(cleanup)) => {
            return Err(SandboxError::OperationFailed {
                message: format!("{error}; staged runner decision cleanup also failed: {cleanup}"),
            });
        }
    }
    Ok(decision_path)
}

fn reconcile_runner_stage_file(
    manifest: &ContainerSandboxManifest,
    staged_path: &Path,
) -> Result<()> {
    match fs::remove_file(staged_path) {
        Ok(()) => File::open(&manifest.conmon_layout.container_state_dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to durably remove staged container runner handoff file {}: {error}",
                    staged_path.display()
                ),
            }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to remove staged container runner handoff file {}: {error}",
                staged_path.display()
            ),
        }),
    }
}

fn runner_handoff_decision_path(manifest: &ContainerSandboxManifest) -> PathBuf {
    manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_HANDOFF_DECISION_FILE)
}

fn lock_runner_handoff(manifest: &ContainerSandboxManifest) -> Result<RunnerHandoffGuard> {
    lock_runner_handoff_with_deadline(
        manifest,
        Some(Instant::now() + RUNNER_HANDOFF_LOCK_TIMEOUT),
        None,
    )
}

/// Establish the command-side synchronization artifact before the first
/// manifest publication. Query-side inspection is intentionally forbidden
/// from calling this creator.
pub(super) fn ensure_runner_handoff_lock_artifact(
    manifest: &ContainerSandboxManifest,
) -> Result<()> {
    let lock_path = manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_HANDOFF_LOCK_FILE);
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map(|_| ())
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to establish container lifecycle lock {} before manifest publication: {error}",
                lock_path.display()
            ),
        })
}

fn converge_runner_lifecycle_lock(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<RunnerHandoffGuard> {
    lock_runner_handoff_with_deadline(
        manifest,
        Some(Instant::now() + RUNNER_HANDOFF_LOCK_TIMEOUT),
        Some(backend),
    )
}

#[cfg(test)]
pub(super) fn converge_runner_lifecycle_lock_with_timeout_for_test(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
    timeout: Duration,
) -> Result<RunnerHandoffGuard> {
    lock_runner_handoff_with_deadline(manifest, Some(Instant::now() + timeout), Some(backend))
}

fn lock_runner_handoff_with_deadline(
    manifest: &ContainerSandboxManifest,
    deadline: Option<Instant>,
    test_observer: Option<&ContainerSandboxBackend>,
) -> Result<RunnerHandoffGuard> {
    let lock_path = manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_HANDOFF_LOCK_FILE);
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to open container runner handoff lock {}: {error}",
                lock_path.display()
            ),
        })?;
    loop {
        match FileExt::try_lock_exclusive(&lock) {
            Ok(()) => return Ok(RunnerHandoffGuard { _lock: lock }),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                #[cfg(test)]
                if let Some(probe) = test_observer
                    .and_then(|backend| backend.runner_lifecycle_lock_test_probe.as_ref())
                {
                    probe.record_contended()?;
                }
                #[cfg(not(test))]
                let _ = test_observer;
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "timed out acquiring container runner handoff lock {}; \
                             execution and cancellation remain fenced",
                            lock_path.display()
                        ),
                    });
                }
                thread::sleep(RUNNER_HANDOFF_LOCK_RETRY);
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to acquire container runner handoff lock {}: {error}",
                        lock_path.display()
                    ),
                });
            }
        }
    }
}

fn validate_durable_prepared_manifest(manifest: &ContainerSandboxManifest) -> Result<()> {
    let persisted = read_runner_manifest(&manifest.conmon_layout.manifest_path)?;
    if persisted == *manifest && persisted.start_mode == ContainerStartMode::PlanOnly {
        return Ok(());
    }

    let decision_path = runner_handoff_decision_path(manifest);
    if decision_path.exists() {
        let decision = read_runner_handoff_decision(&decision_path)?;
        validate_runner_handoff_decision(manifest, &decision)?;
        if decision.decision == RunnerHandoffDecision::Execute {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container runner execution is already claimed by durable decision {}",
                    decision_path.display()
                ),
            });
        }
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container runner handoff is already decided as {:?}; \
                 refusing conflicting Execute decision at {}",
                decision.decision,
                decision_path.display()
            ),
        });
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "container runner handoff found changed or non-plan-only durable manifest for {}; \
             execution and cancellation remain fenced",
            manifest.handle.id
        ),
    })
}

fn reject_existing_runner_handoff_decision(manifest: &ContainerSandboxManifest) -> Result<()> {
    let decision_path = runner_handoff_decision_path(manifest);
    if !decision_path.exists() {
        return Ok(());
    }
    let persisted = read_runner_handoff_decision(&decision_path)?;
    validate_runner_handoff_decision(manifest, &persisted)?;
    Err(SandboxError::OperationFailed {
        message: format!(
            "container runner handoff is already decided as {:?}; refusing status mutation at {}",
            persisted.decision,
            decision_path.display()
        ),
    })
}

fn read_runner_handoff_decision(path: &Path) -> Result<RunnerHandoffDecisionRecord> {
    let contents = fs::read(path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to read durable container runner handoff decision {}: {error}",
            path.display()
        ),
    })?;
    serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to parse durable container runner handoff decision {}: {error}; \
             execution and cancellation remain fenced",
            path.display()
        ),
    })
}

fn read_optional_runner_handoff_decision(
    path: &Path,
) -> Result<Option<RunnerHandoffDecisionRecord>> {
    let contents = match fs::read(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to read durable container runner handoff decision {}: {error}",
                    path.display()
                ),
            });
        }
    };
    serde_json::from_slice(&contents)
        .map(Some)
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to parse durable container runner handoff decision {}: {error}; \
                 execution and cancellation remain fenced",
                path.display()
            ),
        })
}

fn validate_terminal_plan_only_cancellation(
    manifest: &ContainerSandboxManifest,
    persisted: &RunnerHandoffDecisionRecord,
) -> Result<()> {
    let expected_execution_identity = execution_identity_sha256(manifest)?;
    let terminal_outcome_is_honest = match manifest.status {
        crate::instance::SandboxStatus::Stopped => manifest.last_exit_code == Some(0),
        crate::instance::SandboxStatus::Failed => manifest.last_exit_code != Some(0),
        _ => false,
    };
    let expected_endpoints = visible_published_endpoints(
        ContainerStartMode::PlanOnly,
        &manifest.spec,
        manifest.status,
    );
    if persisted.version != RUNNER_HANDOFF_DECISION_VERSION
        || persisted.tenant_id != manifest.spec.tenant_id.as_str()
        || persisted.sandbox_id != manifest.handle.id.as_str()
        || persisted.decision != RunnerHandoffDecision::Cancel
        || persisted.phase != RunnerHandoffPhase::Cancelled
        || persisted.pre_effect_authority_sha256 != pre_effect_authority_sha256(manifest)?
        || persisted.execution_identity_sha256 != expected_execution_identity
        || manifest.runner_handoff_id.is_some()
        || manifest.lifecycle_coordinator != ContainerLifecycleCoordinator::PreparedServiceRunner
        || manifest.start_mode != ContainerStartMode::PlanOnly
        || !manifest.shutdown_requested
        || manifest.handle.status != manifest.status
        || !terminal_outcome_is_honest
        || !manifest.has_terminal_network_finality()
        || manifest.handle.published_endpoints != expected_endpoints
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "durable container runner handoff decision does not authenticate terminal \
                 PlanOnly cancellation for {}; inspection remains fenced",
                manifest.handle.id
            ),
        });
    }
    Ok(())
}

fn validate_plan_only_cancellation_progress(
    manifest: &ContainerSandboxManifest,
    persisted: &RunnerHandoffDecisionRecord,
) -> Result<()> {
    let exact_prepared = persisted.prepared_manifest_sha256 == prepared_manifest_sha256(manifest)?;
    let cleanup_outcome_is_honest = match manifest.status {
        crate::instance::SandboxStatus::Stopping => true,
        crate::instance::SandboxStatus::Stopped => manifest.last_exit_code == Some(0),
        crate::instance::SandboxStatus::Failed => manifest.last_exit_code != Some(0),
        _ => false,
    };
    let cleanup_progress = manifest.shutdown_requested
        && manifest.handle.status == manifest.status
        && cleanup_outcome_is_honest;
    if persisted.version != RUNNER_HANDOFF_DECISION_VERSION
        || persisted.tenant_id != manifest.spec.tenant_id.as_str()
        || persisted.sandbox_id != manifest.handle.id.as_str()
        || persisted.decision != RunnerHandoffDecision::Cancel
        || persisted.phase != RunnerHandoffPhase::Cancelled
        || persisted.pre_effect_authority_sha256 != pre_effect_authority_sha256(manifest)?
        || persisted.execution_identity_sha256 != execution_identity_sha256(manifest)?
        || manifest.runner_handoff_id.is_some()
        || manifest.lifecycle_coordinator != ContainerLifecycleCoordinator::PreparedServiceRunner
        || manifest.start_mode != ContainerStartMode::PlanOnly
        || (!exact_prepared && !cleanup_progress)
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "durable container runner Cancel decision does not authenticate cleanup progress \
                 for {}; cancellation remains fenced",
                manifest.handle.id
            ),
        });
    }
    Ok(())
}

pub(super) fn execute_handoff_phase(
    manifest: &ContainerSandboxManifest,
) -> Result<Option<RunnerHandoffPhase>> {
    let decision_path = runner_handoff_decision_path(manifest);
    if !decision_path.exists() {
        return Ok(None);
    }
    require_execute_handoff_source(manifest, "container runner handoff phase authentication")?;
    let decision = read_runner_handoff_decision(&decision_path)?;
    let prepared = prepared_projection(manifest);
    if decision.decision == RunnerHandoffDecision::Execute
        && decision.phase == RunnerHandoffPhase::ClaimedBeforeEffects
        && manifest.shutdown_requested
        && manifest.status == crate::instance::SandboxStatus::Stopped
    {
        validate_pre_effect_cleanup_completion(&prepared, &decision)?;
    } else {
        validate_runner_handoff_decision(&prepared, &decision)?;
    }
    match decision.decision {
        RunnerHandoffDecision::Execute
            if decision.phase == RunnerHandoffPhase::LifecyclePublished =>
        {
            Ok(None)
        }
        RunnerHandoffDecision::Execute => Ok(Some(decision.phase)),
        RunnerHandoffDecision::Cancel => Err(SandboxError::OperationFailed {
            message: format!(
                "Execute manifest {} contradicts a durable Cancel runner handoff",
                manifest.handle.id
            ),
        }),
    }
}

pub(super) fn execute_handoff_phase_with_evidence(
    manifest: &ContainerSandboxManifest,
) -> Result<(Option<RunnerHandoffPhase>, Vec<u8>)> {
    let phase = execute_handoff_phase(manifest)?;
    Ok((phase, inspection_handoff_evidence(manifest)?))
}

pub(super) fn inspection_handoff_evidence(manifest: &ContainerSandboxManifest) -> Result<Vec<u8>> {
    let decision_path = runner_handoff_decision_path(manifest);
    match std::fs::read(&decision_path) {
        Ok(evidence) => Ok(evidence),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to read authenticated container handoff evidence {}: {error}",
                decision_path.display()
            ),
        }),
    }
}

fn validate_pre_effect_cleanup_completion(
    manifest: &ContainerSandboxManifest,
    persisted: &RunnerHandoffDecisionRecord,
) -> Result<()> {
    let expected_execution_identity = execution_identity_sha256(manifest)?;
    let expected_pre_effect_authority = pre_effect_authority_sha256(manifest)?;
    // No provider process existed at this crash cut. Initial-launch failure
    // therefore records no exit, while an explicit operator stop records the
    // successful stop outcome used by the ordinary lifecycle projection.
    let valid_no_effect_exit = matches!(manifest.last_exit_code, None | Some(0));
    if persisted.version != RUNNER_HANDOFF_DECISION_VERSION
        || persisted.tenant_id != manifest.spec.tenant_id.as_str()
        || persisted.sandbox_id != manifest.handle.id.as_str()
        || persisted.decision != RunnerHandoffDecision::Execute
        || persisted.phase != RunnerHandoffPhase::ClaimedBeforeEffects
        || persisted.pre_effect_authority_sha256 != expected_pre_effect_authority
        || persisted.execution_identity_sha256 != expected_execution_identity
        || manifest.runner_handoff_id.as_ref() != Some(&persisted.decision_id)
        || manifest.status != crate::instance::SandboxStatus::Stopped
        || !manifest.has_terminal_network_finality()
        || !valid_no_effect_exit
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "durable container runner handoff decision does not authenticate completed \
                 pre-effect cleanup for {}; lifecycle publication remains fenced",
                manifest.handle.id
            ),
        });
    }
    Ok(())
}

fn validate_runner_handoff_decision(
    manifest: &ContainerSandboxManifest,
    persisted: &RunnerHandoffDecisionRecord,
) -> Result<()> {
    let expected_prepared_fingerprint = prepared_manifest_sha256(manifest)?;
    let expected_pre_effect_authority = pre_effect_authority_sha256(manifest)?;
    let expected_execution_identity = execution_identity_sha256(manifest)?;
    let phase_matches_decision = matches!(
        (persisted.decision, persisted.phase),
        (
            RunnerHandoffDecision::Execute,
            RunnerHandoffPhase::ClaimedBeforeEffects
                | RunnerHandoffPhase::EffectsStarted
                | RunnerHandoffPhase::LifecyclePublished
        ) | (RunnerHandoffDecision::Cancel, RunnerHandoffPhase::Cancelled)
    );
    let generation_matches = match persisted.decision {
        RunnerHandoffDecision::Execute => match manifest.runner_handoff_id.as_ref() {
            Some(generation) => generation == &persisted.decision_id,
            None => persisted.phase == RunnerHandoffPhase::ClaimedBeforeEffects,
        },
        RunnerHandoffDecision::Cancel => manifest.runner_handoff_id.is_none(),
    };
    validate_runner_effect_receipt(manifest, persisted)?;
    if persisted.version != RUNNER_HANDOFF_DECISION_VERSION
        || persisted.tenant_id != manifest.spec.tenant_id.as_str()
        || persisted.sandbox_id != manifest.handle.id.as_str()
        || persisted.execution_identity_sha256 != expected_execution_identity
        || (!matches!(
            persisted.phase,
            RunnerHandoffPhase::EffectsStarted | RunnerHandoffPhase::LifecyclePublished
        ) && persisted.prepared_manifest_sha256 != expected_prepared_fingerprint)
        || (persisted.phase != RunnerHandoffPhase::LifecyclePublished
            && persisted.pre_effect_authority_sha256 != expected_pre_effect_authority)
        || !generation_matches
        || !phase_matches_decision
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "durable container runner handoff decision or generation does not match prepared \
                 workload {}; execution and cancellation remain fenced",
                manifest.handle.id
            ),
        });
    }
    Ok(())
}

pub(super) fn mark_runner_effects_started(
    manifest: &ContainerSandboxManifest,
    _handoff: &RunnerHandoffGuard,
) -> Result<()> {
    require_execute_handoff_source(manifest, "container runner effect publication")?;
    manifest.require_execution_admission_open("container runner effect publication")?;
    let prepared = prepared_projection(manifest);
    let decision_path = runner_handoff_decision_path(manifest);
    let mut record = read_runner_handoff_decision(&decision_path)?;
    validate_runner_handoff_decision(&prepared, &record)?;
    match (record.decision, record.phase) {
        (RunnerHandoffDecision::Execute, RunnerHandoffPhase::ClaimedBeforeEffects) => {
            record.phase = RunnerHandoffPhase::EffectsStarted;
            durably_replace_runner_handoff_decision(manifest, &decision_path, &record)
        }
        (RunnerHandoffDecision::Execute, RunnerHandoffPhase::EffectsStarted) => {
            sync_runner_handoff_parent(manifest, &decision_path)
        }
        (decision, phase) => Err(SandboxError::OperationFailed {
            message: format!(
                "container runner cannot start effects from {decision:?} handoff phase {phase:?} \
                 for {}",
                manifest.handle.id
            ),
        }),
    }
}

fn durably_replace_runner_handoff_decision(
    manifest: &ContainerSandboxManifest,
    decision_path: &Path,
    record: &RunnerHandoffDecisionRecord,
) -> Result<()> {
    let mut rendered =
        serde_json::to_vec_pretty(record).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to serialize container runner handoff phase: {error}"),
        })?;
    rendered.push(b'\n');
    let staged_path = manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_HANDOFF_PHASE_STAGE_FILE);
    let publish = (|| -> Result<()> {
        reconcile_runner_stage_file(manifest, &staged_path)?;
        let mut staged = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create staged container runner handoff phase {}: {error}",
                    staged_path.display()
                ),
            })?;
        staged
            .write_all(&rendered)
            .and_then(|()| staged.sync_all())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to durably stage container runner handoff phase {}: {error}",
                    staged_path.display()
                ),
            })?;
        fs::rename(&staged_path, decision_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to atomically publish container runner handoff phase {}: {error}",
                decision_path.display()
            ),
        })?;
        File::open(&manifest.conmon_layout.container_state_dir)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to durably publish container runner handoff phase {}: {error}",
                    decision_path.display()
                ),
            })
    })();
    match publish {
        Ok(()) => Ok(()),
        Err(error) => match reconcile_runner_stage_file(manifest, &staged_path) {
            Ok(()) => Err(error),
            Err(cleanup) => Err(SandboxError::OperationFailed {
                message: format!("{error}; staged runner phase cleanup also failed: {cleanup}"),
            }),
        },
    }
}

fn sync_runner_handoff_parent(
    manifest: &ContainerSandboxManifest,
    decision_path: &Path,
) -> Result<()> {
    std::fs::File::open(&manifest.conmon_layout.container_state_dir)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to durably acknowledge container runner handoff phase at {}: {error}",
                decision_path.display()
            ),
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RunnerOwnershipConvergenceStage {
    EffectsStarted,
    LifecyclePublished,
}

fn converge_runner_effects_started(
    manifest: &ContainerSandboxManifest,
    handoff: &RunnerHandoffGuard,
) -> Result<()> {
    converge_runner_effects_started_with(
        manifest,
        || mark_runner_effects_started(manifest, handoff),
        |stage, error| wait_for_runner_ownership(manifest, stage, error),
    )
}

pub(super) fn converge_runner_effects_started_with(
    manifest: &ContainerSandboxManifest,
    transition: impl FnMut() -> Result<()>,
    wait: impl FnMut(RunnerOwnershipConvergenceStage, &SandboxError),
) -> Result<()> {
    super::effect_fence::converge_persistence_with(
        super::effect_fence::EFFECT_FENCE_PERSIST_ATTEMPTS,
        transition,
        wait,
    )
    .map_err(|error| super::effect_fence::diagnose_exhaustion(manifest, error))
}

#[cfg(test)]
pub(super) fn converge_runner_lifecycle_ownership(
    manifest: &ContainerSandboxManifest,
    handoff: &RunnerHandoffGuard,
) -> Result<()> {
    converge_runner_ownership_with(
        RunnerOwnershipConvergenceStage::LifecyclePublished,
        || publish_runner_lifecycle_ownership(manifest, handoff),
        |stage, error| wait_for_runner_ownership(manifest, stage, error),
    )
}

fn wait_for_runner_ownership(
    manifest: &ContainerSandboxManifest,
    stage: RunnerOwnershipConvergenceStage,
    error: &SandboxError,
) {
    tracing::warn!(
        sandbox_id = %manifest.handle.id,
        ?stage,
        %error,
        "container runner retains its handoff lock while durable ownership converges"
    );
    thread::sleep(RUNNER_CLEANUP_RETRY_DELAY);
}

pub(super) fn converge_runner_ownership_with(
    stage: RunnerOwnershipConvergenceStage,
    mut transition: impl FnMut() -> Result<()>,
    mut wait: impl FnMut(RunnerOwnershipConvergenceStage, &SandboxError),
) -> Result<()> {
    for attempt in 1..=RUNNER_CONVERGENCE_ATTEMPTS {
        match transition() {
            Ok(()) => return Ok(()),
            Err(error) if attempt < RUNNER_CONVERGENCE_ATTEMPTS => wait(stage, &error),
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner failed to converge {stage:?} after \
                         {RUNNER_CONVERGENCE_ATTEMPTS} attempts: {error}; durable runner handoff \
                         authority remains fenced for inspect-before-retry reconciliation"
                    ),
                });
            }
        }
    }
    unreachable!("runner convergence always attempts at least once")
}

#[cfg(test)]
pub(super) fn converge_runner_launch_result_with<T>(
    state: &mut T,
    launch_result: Result<()>,
    mut cleanup: impl FnMut(&mut T),
    publish: impl FnMut(&T) -> Result<()>,
    wait: impl FnMut(&T, RunnerOwnershipConvergenceStage, &SandboxError),
) -> Result<()> {
    converge_runner_launch_result_with_fallible_cleanup(
        state,
        launch_result,
        |state| {
            cleanup(state);
            Ok(())
        },
        publish,
        wait,
    )
}

fn converge_runner_launch_result_with_fallible_cleanup<T>(
    state: &mut T,
    launch_result: Result<()>,
    mut cleanup: impl FnMut(&mut T) -> Result<()>,
    mut publish: impl FnMut(&T) -> Result<()>,
    mut wait: impl FnMut(&T, RunnerOwnershipConvergenceStage, &SandboxError),
) -> Result<()> {
    let primary = launch_result.err();
    if primary.is_some()
        && let Err(cleanup_error) = cleanup(state)
    {
        return Err(preserve_runner_primary_error(
            primary,
            "initial launch cleanup did not converge",
            cleanup_error,
        ));
    }
    let publication = converge_runner_ownership_with(
        RunnerOwnershipConvergenceStage::LifecyclePublished,
        || publish(state),
        |stage, error| wait(state, stage, error),
    );
    match publication {
        Ok(()) => primary.map_or(Ok(()), Err),
        Err(publication_error) => Err(preserve_runner_primary_error(
            primary,
            "lifecycle publication did not converge",
            publication_error,
        )),
    }
}

pub(super) fn preserve_runner_primary_error(
    primary: Option<SandboxError>,
    convergence_context: &str,
    convergence: SandboxError,
) -> SandboxError {
    match primary {
        Some(primary) => SandboxError::OperationFailed {
            message: format!(
                "{primary}; {convergence_context}: {convergence}; the primary failure and durable \
                 convergence authority are both preserved"
            ),
        },
        None => convergence,
    }
}

#[cfg(test)]
pub(super) fn finalize_runner_exit(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    exit_code: i32,
) -> Result<()> {
    let authority = PublishedRunnerLifecycleAuthority::capture(manifest)?;
    finalize_runner_exit_with_authority(backend, manifest, &authority, exit_code)
}

fn finalize_runner_exit_with_authority(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    authority: &PublishedRunnerLifecycleAuthority,
    exit_code: i32,
) -> Result<()> {
    let terminal_status = if exit_code == 0 {
        crate::instance::SandboxStatus::Stopped
    } else {
        crate::instance::SandboxStatus::Failed
    };
    finalize_published_runner_lifecycle_with(
        backend,
        manifest,
        authority,
        terminal_status,
        Some(exit_code),
        converge_runner_cleanup,
    )
}

fn finalize_published_runner_lifecycle(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    authority: &PublishedRunnerLifecycleAuthority,
    terminal_status: crate::instance::SandboxStatus,
    last_exit_code: Option<i32>,
) -> Result<()> {
    finalize_published_runner_lifecycle_with(
        backend,
        manifest,
        authority,
        terminal_status,
        last_exit_code,
        converge_runner_cleanup,
    )
}

fn finalize_published_runner_lifecycle_with(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    authority: &PublishedRunnerLifecycleAuthority,
    terminal_status: crate::instance::SandboxStatus,
    last_exit_code: Option<i32>,
    converge: impl FnOnce(
        &ContainerSandboxBackend,
        &mut ContainerSandboxManifest,
        crate::instance::SandboxStatus,
        Option<i32>,
    ) -> Result<()>,
) -> Result<()> {
    // The start handoff lock is deliberately released during the workload
    // wait so ordinary stop and inspection remain available. Finalization
    // must re-enter that same lifecycle authority and operate only on the
    // current durable manifest; the pre-wait snapshot is never a write base.
    let _lifecycle = converge_runner_lifecycle_lock(backend, manifest)?;
    let current = read_runner_manifest(&authority.manifest_path)?;
    authority.authenticate(&current)?;
    *manifest = current;
    if manifest.has_terminal_network_finality() {
        return backend.reconcile_terminal_ipam_retirement(manifest);
    }
    converge(backend, manifest, terminal_status, last_exit_code)
}

#[cfg(test)]
pub(super) fn finalize_runner_exit_with_cleanup_for_test(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    exit_code: i32,
    cleanup: impl FnMut(&mut ContainerSandboxManifest) -> Result<()>,
) -> Result<()> {
    let authority = PublishedRunnerLifecycleAuthority::capture(manifest)?;
    let terminal_status = if exit_code == 0 {
        crate::instance::SandboxStatus::Stopped
    } else {
        crate::instance::SandboxStatus::Failed
    };
    finalize_published_runner_lifecycle_with(
        backend,
        manifest,
        &authority,
        terminal_status,
        Some(exit_code),
        |backend, manifest, terminal_status, last_exit_code| {
            try_converge_runner_cleanup_with(
                manifest,
                terminal_status,
                last_exit_code,
                |candidate| backend.write_manifest(candidate),
                cleanup,
                |stage, error| {
                    panic!("deterministic runner cleanup unexpectedly failed at {stage:?}: {error}")
                },
            )
        },
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RunnerCleanupConvergenceStage {
    StoppingPersistence,
    ProviderCleanup,
    TerminalPersistence,
}

pub(super) fn converge_runner_cleanup(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    terminal_status: crate::instance::SandboxStatus,
    last_exit_code: Option<i32>,
) -> Result<()> {
    let sandbox_id = manifest.handle.id.clone();
    try_converge_runner_cleanup_with(
        manifest,
        terminal_status,
        last_exit_code,
        |candidate| backend.write_manifest(candidate),
        |candidate| backend.release_execution_artifacts(candidate),
        |stage, error| {
            tracing::warn!(
                sandbox_id = %sandbox_id,
                ?stage,
                %error,
                "container runner retains lifecycle ownership while cleanup converges"
            );
            thread::sleep(RUNNER_CLEANUP_RETRY_DELAY);
        },
    )
}

pub(super) fn converge_initial_launch_cleanup(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    terminal_status: crate::instance::SandboxStatus,
    cleanup_state: super::direct_execution::InitialLaunchCleanupState,
) -> Result<()> {
    let sandbox_id = manifest.handle.id.clone();
    try_converge_runner_cleanup_with(
        manifest,
        terminal_status,
        None,
        |candidate| backend.write_manifest(candidate),
        |candidate| match cleanup_state {
            super::direct_execution::InitialLaunchCleanupState::Complete => Ok(()),
            super::direct_execution::InitialLaunchCleanupState::UnstartedPending => {
                backend.release_unstarted_launch_artifacts(candidate)
            }
            super::direct_execution::InitialLaunchCleanupState::ProviderPending => {
                backend.release_execution_artifacts(candidate)
            }
        },
        |stage, error| {
            tracing::warn!(
                sandbox_id = %sandbox_id,
                ?stage,
                %error,
                "container runner retains lifecycle ownership while initial-launch cleanup converges"
            );
            thread::sleep(RUNNER_CLEANUP_RETRY_DELAY);
        },
    )
}

#[cfg(test)]
pub(super) fn converge_runner_cleanup_with(
    manifest: &mut ContainerSandboxManifest,
    terminal_status: crate::instance::SandboxStatus,
    last_exit_code: Option<i32>,
    mut persist: impl FnMut(&ContainerSandboxManifest) -> Result<()>,
    mut cleanup: impl FnMut(&mut ContainerSandboxManifest) -> Result<()>,
    mut wait: impl FnMut(RunnerCleanupConvergenceStage, &SandboxError),
) {
    try_converge_runner_cleanup_with(
        manifest,
        terminal_status,
        last_exit_code,
        &mut persist,
        &mut cleanup,
        &mut wait,
    )
    .expect("deterministic runner cleanup fixture should converge");
}

pub(super) fn try_converge_runner_cleanup_with(
    manifest: &mut ContainerSandboxManifest,
    terminal_status: crate::instance::SandboxStatus,
    last_exit_code: Option<i32>,
    mut persist: impl FnMut(&ContainerSandboxManifest) -> Result<()>,
    mut cleanup: impl FnMut(&mut ContainerSandboxManifest) -> Result<()>,
    mut wait: impl FnMut(RunnerCleanupConvergenceStage, &SandboxError),
) -> Result<()> {
    let (terminal_status, last_exit_code) =
        authoritative_runner_cleanup_outcome(manifest, terminal_status, last_exit_code);
    manifest.shutdown_requested = true;
    manifest.last_exit_code = last_exit_code;
    synchronize_handle_status(manifest, crate::instance::SandboxStatus::Stopping);
    converge_runner_cleanup_stage_with(
        RunnerCleanupConvergenceStage::StoppingPersistence,
        || persist(manifest),
        &mut wait,
    )?;

    let mut cleanup_attempt = 1;
    loop {
        match cleanup(manifest) {
            Ok(()) => break,
            Err(error) => {
                if let Err(persistence) = converge_runner_cleanup_stage_with(
                    RunnerCleanupConvergenceStage::StoppingPersistence,
                    || persist(manifest),
                    &mut wait,
                ) {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "container runner provider cleanup failed: {error}; cleanup-progress \
                             persistence also failed: {persistence}; the durable Stopping owner \
                             retains both errors for inspect-before-retry reconciliation"
                        ),
                    });
                }
                if cleanup_attempt == RUNNER_CONVERGENCE_ATTEMPTS {
                    return Err(SandboxError::OperationFailed {
                        message: format!(
                            "container runner failed to converge {:?} after \
                             {RUNNER_CONVERGENCE_ATTEMPTS} attempts: {error}; durable Stopping \
                             ownership remains fenced and terminal lifecycle was not published",
                            RunnerCleanupConvergenceStage::ProviderCleanup
                        ),
                    });
                }
                wait(RunnerCleanupConvergenceStage::ProviderCleanup, &error);
                cleanup_attempt += 1;
            }
        }
    }

    let mut terminal = manifest.clone();
    synchronize_handle_status(&mut terminal, terminal_status);
    converge_runner_cleanup_stage_with(
        RunnerCleanupConvergenceStage::TerminalPersistence,
        || persist(&terminal),
        &mut wait,
    )?;
    *manifest = terminal;
    Ok(())
}

fn authoritative_runner_cleanup_outcome(
    manifest: &ContainerSandboxManifest,
    proposed_status: crate::instance::SandboxStatus,
    proposed_exit_code: Option<i32>,
) -> (crate::instance::SandboxStatus, Option<i32>) {
    if !manifest.shutdown_requested || manifest.status != crate::instance::SandboxStatus::Stopping {
        return (proposed_status, proposed_exit_code);
    }

    let durable_exit_code = manifest.last_exit_code;
    let durable_status = match durable_exit_code {
        Some(0) => crate::instance::SandboxStatus::Stopped,
        Some(_) | None => crate::instance::SandboxStatus::Failed,
    };
    (durable_status, durable_exit_code)
}

fn converge_runner_cleanup_stage_with(
    stage: RunnerCleanupConvergenceStage,
    mut transition: impl FnMut() -> Result<()>,
    mut wait: impl FnMut(RunnerCleanupConvergenceStage, &SandboxError),
) -> Result<()> {
    for attempt in 1..=RUNNER_CONVERGENCE_ATTEMPTS {
        match transition() {
            Ok(()) => return Ok(()),
            Err(error) if attempt < RUNNER_CONVERGENCE_ATTEMPTS => wait(stage, &error),
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "container runner failed to converge {stage:?} after \
                         {RUNNER_CONVERGENCE_ATTEMPTS} attempts: {error}; current durable \
                         lifecycle evidence remains authoritative"
                    ),
                });
            }
        }
    }
    unreachable!("runner cleanup convergence always attempts at least once")
}

pub(super) fn validate_runner_authority_roots(manifest: &ContainerSandboxManifest) -> Result<()> {
    if manifest.runner_config.workload_state_root != manifest.network_layout.workload_state_root {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner workload root {} does not match prepared workload root {}",
                manifest.runner_config.workload_state_root.display(),
                manifest.network_layout.workload_state_root.display()
            ),
        });
    }
    if manifest.runner_config.network_state_root != manifest.network_layout.network_state_root {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner network authority root {} does not match prepared network \
                 authority root {}",
                manifest.runner_config.network_state_root.display(),
                manifest.network_layout.network_state_root.display()
            ),
        });
    }
    Ok(())
}

fn wait_for_container_runner_exit(manifest: &ContainerSandboxManifest) -> Result<i32> {
    poll_until_deadline(None, Duration::from_millis(200), || {
        Ok(manifest
            .conmon_layout
            .exit_status_file
            .exists()
            .then_some(()))
    })?;
    read_exit_code(&manifest.conmon_layout.exit_status_file)
}

fn read_runner_manifest_pointer(bundle_dir: &Path) -> Result<PathBuf> {
    let pointer_path = bundle_dir.join(RUNNER_MANIFEST_POINTER_FILE);
    let contents =
        std::fs::read_to_string(&pointer_path).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to read container runner manifest pointer {}: {error}",
                pointer_path.display()
            ),
        })?;
    let path = contents.trim();
    if path.is_empty() {
        return Err(SandboxError::InvalidSpec {
            message: format!(
                "container runner manifest pointer {} is empty",
                pointer_path.display()
            ),
        });
    }
    Ok(PathBuf::from(path))
}

fn read_runner_manifest(manifest_path: &Path) -> Result<ContainerSandboxManifest> {
    let contents = std::fs::read(manifest_path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to read container runner manifest {}: {error}",
            manifest_path.display()
        ),
    })?;
    serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to parse container runner manifest {}: {error}",
            manifest_path.display()
        ),
    })
}
