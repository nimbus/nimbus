//! Generation-bound provider-effect receipts and restart reconciliation.
//!
//! Provider observation remains in the container/OCI adapters. This child owns
//! only the runner's exact handoff/result binding and composes those adapter
//! outcomes while the parent holds the one lifecycle lock.

use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::backends::conmon::lifecycle::{
    RuntimeStateObservation, runtime_state, runtime_state_for_creator_attempt,
};
use crate::backends::oci::network::{
    MachinePortPreparationReleaseAuthority, authenticate_container_network_generation,
};
use crate::backends::oci::port_lifecycle::LaunchPortBatchState;
use crate::error::{Result, SandboxError};
use crate::instance::SandboxStatus;

use super::super::manifest::ContainerCreatorHandoffState;
use super::identity::result_manifest_sha256;
use super::*;

pub(in crate::backends::container::runtime) const RUNNER_RESULT_ANCHOR_FILE: &str =
    ".nimbus-runner-result.json";
const RUNNER_RESULT_ANCHOR_STAGE_FILE: &str = ".nimbus-runner-result.stage";
const RUNNER_RESULT_ANCHOR_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(in crate::backends::container::runtime) enum RunnerEffectOutcome {
    Present,
    Absent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct RunnerEffectReceipt {
    handoff_id: RunnerHandoffId,
    outcome: RunnerEffectOutcome,
    result_manifest_sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct RunnerResultAnchor {
    version: u32,
    handoff_id: RunnerHandoffId,
    outcome: RunnerEffectOutcome,
    result_manifest_sha256: String,
}

impl RunnerResultAnchor {
    fn from_receipt(receipt: &RunnerEffectReceipt) -> Self {
        Self {
            version: RUNNER_RESULT_ANCHOR_VERSION,
            handoff_id: receipt.handoff_id.clone(),
            outcome: receipt.outcome,
            result_manifest_sha256: receipt.result_manifest_sha256.clone(),
        }
    }

    fn receipt(&self) -> RunnerEffectReceipt {
        RunnerEffectReceipt {
            handoff_id: self.handoff_id.clone(),
            outcome: self.outcome,
            result_manifest_sha256: self.result_manifest_sha256.clone(),
        }
    }
}

pub(in crate::backends::container::runtime) fn record_runner_effect_outcome(
    result_manifest: &ContainerSandboxManifest,
    outcome: RunnerEffectOutcome,
    _handoff: &RunnerHandoffGuard,
) -> Result<()> {
    require_execute_handoff_source(
        result_manifest,
        "container runner effect-result publication",
    )?;
    let persisted = read_runner_manifest(&result_manifest.conmon_layout.manifest_path)?;
    if persisted != *result_manifest {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "cannot publish container runner effect receipt for {} before its exact result \
                 manifest is durable",
                result_manifest.handle.id
            ),
        });
    }
    require_outcome_shape(result_manifest, outcome)?;

    let prepared = prepared_projection(result_manifest);
    let decision_path = runner_handoff_decision_path(&prepared);
    let mut decision = read_runner_handoff_decision(&decision_path)?;
    validate_runner_handoff_decision(&prepared, &decision)?;
    if decision.decision != RunnerHandoffDecision::Execute
        || decision.phase != RunnerHandoffPhase::EffectsStarted
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container runner cannot publish an effect receipt from {:?}/{:?} for {}",
                decision.decision, decision.phase, result_manifest.handle.id
            ),
        });
    }
    let receipt = RunnerEffectReceipt {
        handoff_id: decision.decision_id.clone(),
        outcome,
        result_manifest_sha256: result_manifest_sha256(result_manifest)?,
    };
    publish_runner_result_anchor(result_manifest, &receipt)?;
    match decision.effect_receipt.as_ref() {
        Some(existing) if existing == &receipt => {
            return sync_runner_handoff_parent(result_manifest, &decision_path);
        }
        Some(_) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container runner effect receipt for {} conflicts with the exact durable \
                     handoff/result generation; lifecycle promotion remains fenced",
                    result_manifest.handle.id
                ),
            });
        }
        None => {}
    }
    decision.effect_receipt = Some(receipt);
    durably_replace_runner_handoff_decision(result_manifest, &decision_path, &decision)
}

/// Reconcile the ambiguous post-fence window without replaying initial launch.
///
/// The caller owns the exact Execute lifecycle lock. Provider observation
/// stays in the container/OCI adapters; this coordinator promotes only an
/// authenticated live generation or compensates only explicit absence.
pub(in crate::backends::container::runtime) fn reconcile_runner_effects_started(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
    handoff: &RunnerHandoffGuard,
) -> Result<RunnerEffectOutcome> {
    require_execute_handoff_source(manifest, "container runner effect reconciliation")?;
    let decision_path = runner_handoff_decision_path(manifest);
    let decision = read_runner_handoff_decision(&decision_path)?;
    validate_runner_handoff_decision(&prepared_projection(manifest), &decision)?;
    if decision.decision != RunnerHandoffDecision::Execute
        || decision.phase != RunnerHandoffPhase::EffectsStarted
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "container runner effect reconciliation requires Execute/EffectsStarted for {}; \
                 found {:?}/{:?}",
                manifest.handle.id, decision.decision, decision.phase
            ),
        });
    }

    if let Some(receipt) = decision.effect_receipt.as_ref() {
        let outcome = receipt.outcome;
        publish_runner_lifecycle_ownership(manifest, handoff)?;
        return Ok(outcome);
    }
    if let Some(anchor) = read_optional_runner_result_anchor(manifest)? {
        let outcome = anchor.outcome;
        record_runner_effect_outcome(manifest, outcome, handoff)?;
        publish_runner_lifecycle_ownership(manifest, handoff)?;
        return Ok(outcome);
    }

    let observation = observe_exact_runner_effect(backend, manifest).map_err(|error| {
        SandboxError::OperationFailed {
            message: format!(
                "cannot reconcile container runner effects for {}: {error}; the exact \
                 EffectsStarted handoff and provider authority remain fenced",
                manifest.handle.id
            ),
        }
    })?;
    let outcome = match observation {
        RuntimeStateObservation::Present(_) => {
            authenticate_present_runner_effects(backend, manifest)?;
            RunnerEffectOutcome::Present
        }
        RuntimeStateObservation::ExplicitlyAbsent => {
            let terminal_status = match manifest.status {
                SandboxStatus::Stopped => SandboxStatus::Stopped,
                SandboxStatus::Failed => SandboxStatus::Failed,
                _ => SandboxStatus::Failed,
            };
            let cleanup_state = classify_explicitly_absent_cleanup(backend, manifest)?;
            let first_cleanup =
                converge_initial_launch_cleanup(backend, manifest, terminal_status, cleanup_state);
            if let Err(no_effect_error) = first_cleanup {
                if cleanup_state
                    != super::super::direct_execution::InitialLaunchCleanupState::UnstartedPending
                {
                    return Err(no_effect_error);
                }
                let claim = manifest.launch_reservation_claim.as_ref().ok_or_else(|| {
                    SandboxError::OperationFailed {
                        message: format!(
                            "container runner no-effect cleanup for {} failed after retiring its \
                             launch claim: {no_effect_error}",
                            manifest.handle.id
                        ),
                    }
                })?;
                let attachment_id = manifest.require_network_config()?.attachment_id.clone();
                backend
                    .segment_allocator
                    .adopt_reserved_attachment(&manifest.spec.tenant_id, &attachment_id, claim)
                    .map_err(|adoption_error| SandboxError::OperationFailed {
                        message: format!(
                            "container runner no-effect cleanup for {} did not converge: \
                             {no_effect_error}; exact attachment adoption for provider cleanup \
                             also failed: {adoption_error}",
                            manifest.handle.id
                        ),
                    })?;
                converge_initial_launch_cleanup(
                    backend,
                    manifest,
                    terminal_status,
                    super::super::direct_execution::InitialLaunchCleanupState::ProviderPending,
                )?;
            }
            RunnerEffectOutcome::Absent
        }
    };
    record_runner_effect_outcome(manifest, outcome, handoff)?;
    publish_runner_lifecycle_ownership(manifest, handoff)?;
    Ok(outcome)
}

fn observe_exact_runner_effect(
    backend: &ContainerSandboxBackend,
    manifest: &mut ContainerSandboxManifest,
) -> Result<RuntimeStateObservation> {
    let creator_was_pending = matches!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::Pending { .. }
    );
    backend.reconcile_pending_creator_before_cleanup(manifest)?;
    match &manifest.creator_handoff {
        ContainerCreatorHandoffState::RuntimeObserved { receipt } => {
            runtime_state_for_creator_attempt(
                &manifest.conmon_launch.state_command,
                manifest.handle.id.as_str(),
                receipt.attempt_id(),
            )
        }
        ContainerCreatorHandoffState::Quiesced { .. } if creator_was_pending => {
            Ok(RuntimeStateObservation::ExplicitlyAbsent)
        }
        ContainerCreatorHandoffState::Pending { .. } if creator_was_pending => {
            Err(SandboxError::OperationFailed {
                message: format!(
                    "container creator reconciliation for {} did not publish an exact runtime \
                     outcome; runner effects remain fenced",
                    manifest.handle.id
                ),
            })
        }
        _ => runtime_state(
            &manifest.conmon_launch.state_command,
            manifest.handle.id.as_str(),
        ),
    }
}

fn classify_explicitly_absent_cleanup(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<super::super::direct_execution::InitialLaunchCleanupState> {
    use super::super::direct_execution::InitialLaunchCleanupState;

    if manifest.has_terminal_network_finality() {
        return Ok(InitialLaunchCleanupState::Complete);
    }
    let Some(claim) = manifest.launch_reservation_claim.as_ref() else {
        return Ok(InitialLaunchCleanupState::ProviderPending);
    };
    let mut launch_batch = manifest.port_leases.clone();
    if let Some(assignment) = manifest.egress_proxy.as_ref() {
        launch_batch.push(assignment.port_lease.clone());
    }
    let ports = backend
        .port_lease_coordinator_for_manifest(manifest)?
        .classify_launch_port_batch(&launch_batch, claim)?;
    let status_absent = path_is_absent(
        &manifest.network_layout.status_path,
        "Netavark status projection",
    )?;
    let netns_absent = path_is_absent(
        &manifest.network_layout.netns_path,
        "persistent network namespace",
    )?;
    if ports == LaunchPortBatchState::NeverBound && status_absent && netns_absent {
        Ok(InitialLaunchCleanupState::UnstartedPending)
    } else {
        Ok(InitialLaunchCleanupState::ProviderPending)
    }
}

fn path_is_absent(path: &std::path::Path, artifact: &str) -> Result<bool> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to inspect {artifact} {} during container runner recovery: {error}; \
                 cleanup remains fenced",
                path.display()
            ),
        }),
    }
}

fn authenticate_present_runner_effects(
    backend: &ContainerSandboxBackend,
    manifest: &ContainerSandboxManifest,
) -> Result<()> {
    backend.validate_manifest_execution_context(manifest)?;
    if manifest.launch_reservation_claim.is_some()
        || manifest.shutdown_requested
        || manifest.network_cleanup_complete
        || !matches!(
            manifest.creator_handoff,
            ContainerCreatorHandoffState::RuntimeObserved { .. }
        )
    {
        return Err(SandboxError::OperationFailed {
            message: format!(
                "live container runtime {} lacks a complete post-launch manifest generation; \
                 promotion remains fenced",
                manifest.handle.id
            ),
        });
    }
    let network_config = manifest.require_network_config()?;
    let assigned_ips = authenticate_container_network_generation(
        &backend.ipam_authority,
        &manifest.network_layout,
        network_config,
        &manifest.handle.id,
    )?;
    match std::fs::symlink_metadata(&manifest.network_layout.status_path) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container runner {} has no regular Netavark status projection for its live \
                     attachment generation",
                    manifest.handle.id
                ),
            });
        }
        Err(error) => {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to authenticate Netavark status projection {} for live container {}: \
                     {error}",
                    manifest.network_layout.status_path.display(),
                    manifest.handle.id
                ),
            });
        }
    }

    let port_lease_coordinator = backend.port_lease_coordinator_for_manifest(manifest)?;
    if manifest
        .runner_config
        .validated_machine_port_forwarder(&manifest.handle.id)?
        .is_some()
    {
        backend.ensure_machine_port_proxies_running_with_publication(
            &manifest.handle.id,
            &assigned_ips,
            manifest,
            MachinePortPreparationReleaseAuthority::Retain,
            || Ok(()),
        )?;
    } else {
        let state = port_lease_coordinator.classify_netavark_cleanup_batch(
            &manifest.spec.tenant_id,
            &manifest.handle.id,
            &manifest.spec.port_bindings,
            &manifest.port_leases,
            None,
        )?;
        let empty_terminal_batch = manifest.spec.port_bindings.is_empty()
            && manifest.port_leases.is_empty()
            && state == LaunchPortBatchState::TerminalNoEffect;
        if state != LaunchPortBatchState::ProviderOwned && !empty_terminal_batch {
            return Err(SandboxError::OperationFailed {
                message: format!(
                    "container runner {} cannot promote live runtime effects from port authority \
                     state {state:?}",
                    manifest.handle.id
                ),
            });
        }
    }
    backend.ensure_egress_proxy_running(manifest)
}

pub(super) fn validate_runner_effect_receipt(
    manifest: &ContainerSandboxManifest,
    decision: &RunnerHandoffDecisionRecord,
) -> Result<()> {
    let invalid = || SandboxError::OperationFailed {
        message: format!(
            "durable container runner effect receipt does not authenticate the exact handoff and \
             result generation for {}; lifecycle mutation remains fenced",
            manifest.handle.id
        ),
    };
    match (decision.phase, decision.effect_receipt.as_ref()) {
        (RunnerHandoffPhase::ClaimedBeforeEffects | RunnerHandoffPhase::Cancelled, None) => {
            require_no_runner_result_anchor(manifest, &invalid)
        }
        (RunnerHandoffPhase::EffectsStarted, None) => {
            let Some(anchor) = read_optional_runner_result_anchor(manifest)? else {
                return Ok(());
            };
            let receipt = anchor.receipt();
            let mut effect_result = manifest.clone();
            effect_result.start_mode = ContainerStartMode::Execute;
            if receipt.handoff_id != decision.decision_id
                || !is_canonical_sha256(&receipt.result_manifest_sha256)
                || receipt.result_manifest_sha256 != result_manifest_sha256(&effect_result)?
                || require_outcome_shape(manifest, receipt.outcome).is_err()
            {
                return Err(invalid());
            }
            Ok(())
        }
        (RunnerHandoffPhase::EffectsStarted, Some(receipt)) => {
            let mut effect_result = manifest.clone();
            effect_result.start_mode = ContainerStartMode::Execute;
            if receipt.handoff_id != decision.decision_id
                || !is_canonical_sha256(&receipt.result_manifest_sha256)
                || receipt.result_manifest_sha256 != result_manifest_sha256(&effect_result)?
                || require_outcome_shape(manifest, receipt.outcome).is_err()
                || read_runner_result_anchor(manifest)? != RunnerResultAnchor::from_receipt(receipt)
            {
                return Err(invalid());
            }
            Ok(())
        }
        (RunnerHandoffPhase::LifecyclePublished, Some(receipt)) => {
            if receipt.handoff_id != decision.decision_id
                || !is_canonical_sha256(&receipt.result_manifest_sha256)
                || read_runner_result_anchor(manifest)? != RunnerResultAnchor::from_receipt(receipt)
            {
                return Err(invalid());
            }
            Ok(())
        }
        (RunnerHandoffPhase::LifecyclePublished, None)
            if manifest.has_terminal_network_finality() =>
        {
            // The only effect-free LifecyclePublished path transitions directly
            // from ClaimedBeforeEffects after exact no-provider cleanup.
            require_no_runner_result_anchor(manifest, &invalid)
        }
        _ => Err(invalid()),
    }
}

fn runner_result_anchor_path(manifest: &ContainerSandboxManifest) -> PathBuf {
    manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_RESULT_ANCHOR_FILE)
}

fn read_runner_result_anchor(manifest: &ContainerSandboxManifest) -> Result<RunnerResultAnchor> {
    let path = runner_result_anchor_path(manifest);
    let contents = std::fs::read(&path).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to read immutable container runner result anchor {}: {error}; lifecycle \
             mutation remains fenced",
            path.display()
        ),
    })?;
    serde_json::from_slice(&contents).map_err(|error| SandboxError::OperationFailed {
        message: format!(
            "failed to parse immutable container runner result anchor {}: {error}; lifecycle \
             mutation remains fenced",
            path.display()
        ),
    })
}

fn read_optional_runner_result_anchor(
    manifest: &ContainerSandboxManifest,
) -> Result<Option<RunnerResultAnchor>> {
    let path = runner_result_anchor_path(manifest);
    match std::fs::read(&path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map(Some)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to parse immutable container runner result anchor {}: {error}; \
                     lifecycle mutation remains fenced",
                    path.display()
                ),
            }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to read immutable container runner result anchor {}: {error}; lifecycle \
                 mutation remains fenced",
                path.display()
            ),
        }),
    }
}

fn require_no_runner_result_anchor(
    manifest: &ContainerSandboxManifest,
    invalid: &impl Fn() -> SandboxError,
) -> Result<()> {
    if read_optional_runner_result_anchor(manifest)?.is_none() {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn publish_runner_result_anchor(
    manifest: &ContainerSandboxManifest,
    receipt: &RunnerEffectReceipt,
) -> Result<()> {
    let anchor = RunnerResultAnchor::from_receipt(receipt);
    let anchor_path = runner_result_anchor_path(manifest);
    let staged_path = manifest
        .conmon_layout
        .container_state_dir
        .join(RUNNER_RESULT_ANCHOR_STAGE_FILE);
    reconcile_runner_result_anchor_stage(manifest, &staged_path)?;
    if anchor_path.exists() {
        return require_exact_runner_result_anchor(manifest, &anchor);
    }

    let mut rendered =
        serde_json::to_vec_pretty(&anchor).map_err(|error| SandboxError::OperationFailed {
            message: format!(
                "failed to serialize immutable container runner result anchor: {error}"
            ),
        })?;
    rendered.push(b'\n');
    let publish = (|| -> Result<()> {
        let mut staged = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged_path)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to create staged container runner result anchor {}: {error}",
                    staged_path.display()
                ),
            })?;
        staged
            .write_all(&rendered)
            .and_then(|()| staged.sync_all())
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to durably stage container runner result anchor {}: {error}",
                    staged_path.display()
                ),
            })?;
        match std::fs::hard_link(&staged_path, &anchor_path) {
            Ok(()) => sync_runner_handoff_parent(manifest, &anchor_path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                require_exact_runner_result_anchor(manifest, &anchor)
            }
            Err(error) => Err(SandboxError::OperationFailed {
                message: format!(
                    "failed to exclusively publish immutable container runner result anchor {}: \
                     {error}",
                    anchor_path.display()
                ),
            }),
        }
    })();
    let cleanup = reconcile_runner_result_anchor_stage(manifest, &staged_path);
    match (publish, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(cleanup)) => Err(cleanup),
        (Err(error), Err(cleanup)) => Err(SandboxError::OperationFailed {
            message: format!("{error}; staged runner result anchor cleanup also failed: {cleanup}"),
        }),
    }
}

fn require_exact_runner_result_anchor(
    manifest: &ContainerSandboxManifest,
    expected: &RunnerResultAnchor,
) -> Result<()> {
    let observed = read_runner_result_anchor(manifest)?;
    if &observed == expected {
        return sync_runner_handoff_parent(manifest, &runner_result_anchor_path(manifest));
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "immutable container runner result anchor conflicts with handoff/result generation for \
             {}; lifecycle mutation remains fenced",
            manifest.handle.id
        ),
    })
}

fn reconcile_runner_result_anchor_stage(
    manifest: &ContainerSandboxManifest,
    staged_path: &Path,
) -> Result<()> {
    match std::fs::remove_file(staged_path) {
        Ok(()) => sync_runner_handoff_parent(manifest, staged_path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(SandboxError::OperationFailed {
            message: format!(
                "failed to remove staged container runner result anchor {}: {error}",
                staged_path.display()
            ),
        }),
    }
}

fn require_outcome_shape(
    manifest: &ContainerSandboxManifest,
    outcome: RunnerEffectOutcome,
) -> Result<()> {
    let matches = match outcome {
        RunnerEffectOutcome::Present => {
            !manifest.shutdown_requested
                && !matches!(
                    manifest.status,
                    crate::instance::SandboxStatus::Stopped
                        | crate::instance::SandboxStatus::Failed
                )
                && !manifest.network_cleanup_complete
        }
        RunnerEffectOutcome::Absent => manifest.has_terminal_network_finality(),
    };
    if matches {
        return Ok(());
    }
    Err(SandboxError::OperationFailed {
        message: format!(
            "container runner {:?} effect outcome contradicts durable lifecycle state for {}",
            outcome, manifest.handle.id
        ),
    })
}

fn is_canonical_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
