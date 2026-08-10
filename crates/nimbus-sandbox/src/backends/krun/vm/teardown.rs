//! Exact Krun execution drain and stop state machine.
//!
//! This module owns execution-only teardown. It never detaches networking,
//! stops the PEP, releases listeners, or releases allocation authority.

use std::time::Duration;

use serde::Serialize;

use crate::backends::conmon::runtime_process::{
    RuntimeProcessIdentityObservation, RuntimeProcessSignal, RuntimeProcessSignalOutcome,
};
use crate::{
    ProviderCommandClaim, ProviderCommandExecutionClaim, ProviderCommandJournalError,
    ProviderCommandObservation, ProviderCommandObservationKind, SandboxError,
    SandboxExecutionTeardownCommand, SandboxExecutionTeardownObservation,
    SandboxExecutionTeardownOperation,
};

use super::{
    KrunCreatorHandoffState, KrunLaunchAuthority, KrunSandboxBackend, KrunSandboxManifest,
    KrunStartMode,
};

pub(super) mod effects;
pub(super) mod state;

use effects::{KrunExecutionTeardownRuntime, KrunExecutionTerminalObservation};
use state::{KrunDrainProgress, KrunStopProgress};

const KRUN_EXECUTION_PROVIDER_KEY: &str = "nimbus-sandbox.krun-execution";
const KILL_REDELIVERY_DELAY: Duration = Duration::from_secs(1);

impl KrunSandboxBackend {
    /// Execute after the one provider journal authenticates the current claim.
    pub fn execute_execution_teardown_with_claim(
        &self,
        command: &SandboxExecutionTeardownCommand,
        execution_claim: ProviderCommandExecutionClaim,
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        if execution_claim.claim() != command.provider_claim()
            || execution_claim.observation().kind() != ProviderCommandObservationKind::Claimed
        {
            return Err(ProviderCommandJournalError::InvalidClaim {
                message: "Krun execution authorization crossed its provider command".to_owned(),
            });
        }
        let journal = self.attempt_idempotency_journal()?;
        let (_, provider_observation) =
            journal.execute_current_claim(execution_claim, |current_claim| {
                let sandbox_observation = match self
                    .execute_execution_teardown_inner_with_runtime_and_authorization(
                        command,
                        self.teardown_runtime_provider.as_ref(),
                        Some(current_claim.observation()),
                    ) {
                    Ok(observation) => observation,
                    Err(error @ SandboxError::InvalidSpec { .. }) => {
                        definite_failure("sandbox_teardown_command_crossed", error.to_string())
                    }
                    Err(error @ SandboxError::NotFound { .. }) => {
                        ambiguous(format!("exact Krun manifest is absent: {error}"))
                    }
                    Err(error) => ambiguous(error.to_string()),
                };
                let kind = execution_observation_kind(&sandbox_observation);
                let failure_code = sandbox_observation.failure_code().map(str::to_owned);
                let evidence = sandbox_observation.evidence().to_vec();
                (sandbox_observation, kind, failure_code, evidence)
            })?;
        Ok(provider_observation)
    }

    /// Inspect after the one provider journal authenticates the current claim.
    pub fn inspect_execution_teardown_with_observation(
        &self,
        command: &SandboxExecutionTeardownCommand,
        provider_observation: &ProviderCommandObservation,
    ) -> SandboxExecutionTeardownObservation {
        if provider_observation.claim() != command.provider_claim()
            || !matches!(
                provider_observation.kind(),
                ProviderCommandObservationKind::Claimed
                    | ProviderCommandObservationKind::InProgress
                    | ProviderCommandObservationKind::Ambiguous
            )
        {
            return definite_failure(
                "sandbox_teardown_command_crossed",
                "Krun inspection authorization crossed its provider command",
            );
        }
        match self.inspect_execution_teardown_inner_with_runtime_and_authorization(
            command,
            self.teardown_runtime_provider.as_ref(),
            Some(provider_observation),
        ) {
            Ok(observation) => observation,
            Err(error @ SandboxError::InvalidSpec { .. }) => {
                definite_failure("sandbox_teardown_command_crossed", error.to_string())
            }
            Err(error @ SandboxError::NotFound { .. }) => {
                ambiguous(format!("exact Krun manifest is absent: {error}"))
            }
            Err(error) => ambiguous(error.to_string()),
        }
    }

    #[cfg(test)]
    pub(super) fn execute_execution_teardown(
        &self,
        command: &SandboxExecutionTeardownCommand,
    ) -> SandboxExecutionTeardownObservation {
        match self.execute_execution_teardown_inner_with_runtime_and_authorization(
            command,
            self.teardown_runtime_provider.as_ref(),
            None,
        ) {
            Ok(observation) => observation,
            Err(error @ SandboxError::InvalidSpec { .. }) => {
                definite_failure("sandbox_teardown_command_crossed", error.to_string())
            }
            Err(error @ SandboxError::NotFound { .. }) => {
                ambiguous(format!("exact Krun manifest is absent: {error}"))
            }
            Err(error) => ambiguous(error.to_string()),
        }
    }

    #[cfg(test)]
    pub(super) fn inspect_execution_teardown(
        &self,
        command: &SandboxExecutionTeardownCommand,
    ) -> SandboxExecutionTeardownObservation {
        match self.inspect_execution_teardown_inner_with_runtime_and_authorization(
            command,
            self.teardown_runtime_provider.as_ref(),
            None,
        ) {
            Ok(observation) => observation,
            Err(error @ SandboxError::InvalidSpec { .. }) => {
                definite_failure("sandbox_teardown_command_crossed", error.to_string())
            }
            Err(error @ SandboxError::NotFound { .. }) => {
                ambiguous(format!("exact Krun manifest is absent: {error}"))
            }
            Err(error) => ambiguous(error.to_string()),
        }
    }

    fn execute_execution_teardown_inner_with_runtime_and_authorization(
        &self,
        command: &SandboxExecutionTeardownCommand,
        runtime: &dyn KrunExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.validate_teardown_command_shape(command)?;
        let Some(snapshot) = self.read_manifest(command.sandbox_id())? else {
            return Err(SandboxError::NotFound {
                sandbox_id: command.sandbox_id().as_str().to_owned(),
            });
        };
        // Reject crossed durable identity before creating or taking provider
        // synchronization state. Reauthenticate after the lock to close the
        // read-to-lock race against a successor manifest.
        self.authenticate_teardown_manifest(command, &snapshot)?;
        let _lifecycle = self.lock_launch_lifecycle(&snapshot)?;
        let Some(mut manifest) = self.read_manifest(command.sandbox_id())? else {
            return Err(SandboxError::NotFound {
                sandbox_id: command.sandbox_id().as_str().to_owned(),
            });
        };
        self.authenticate_teardown_manifest(command, &manifest)?;
        match command.operation() {
            SandboxExecutionTeardownOperation::Drain => self.execute_drain(
                command.provider_claim(),
                &mut manifest,
                journal_authorization,
            ),
            SandboxExecutionTeardownOperation::Stop => self.execute_stop_execution(
                command.provider_claim(),
                &mut manifest,
                runtime,
                journal_authorization,
            ),
        }
    }

    fn inspect_execution_teardown_inner_with_runtime_and_authorization(
        &self,
        command: &SandboxExecutionTeardownCommand,
        runtime: &dyn KrunExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.validate_teardown_command_shape(command)?;
        let Some(snapshot) = self.read_manifest(command.sandbox_id())? else {
            return Err(SandboxError::NotFound {
                sandbox_id: command.sandbox_id().as_str().to_owned(),
            });
        };
        let (_inspection, manifest) = self.lock_current_inspection(&snapshot)?;
        self.authenticate_teardown_manifest(command, &manifest)?;
        match command.operation() {
            SandboxExecutionTeardownOperation::Drain => {
                self.inspect_drain(command.provider_claim(), &manifest, journal_authorization)
            }
            SandboxExecutionTeardownOperation::Stop => self.inspect_stop_execution(
                command.provider_claim(),
                &manifest,
                runtime,
                journal_authorization,
            ),
        }
    }

    fn validate_teardown_command_shape(
        &self,
        command: &SandboxExecutionTeardownCommand,
    ) -> crate::Result<()> {
        if self.config.start_mode != KrunStartMode::Execute {
            return Err(SandboxError::InvalidSpec {
                message: "Krun execution teardown requires an Execute backend".to_owned(),
            });
        }
        if command.provider_registration_key() != KRUN_EXECUTION_PROVIDER_KEY {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "Krun teardown provider key {:?} is crossed with {:?}",
                    command.provider_registration_key(),
                    KRUN_EXECUTION_PROVIDER_KEY,
                ),
            });
        }
        if command.provider_claim().operation() != command.operation().provider_operation() {
            return Err(SandboxError::InvalidSpec {
                message: "Krun teardown command operation is crossed with its provider claim"
                    .to_owned(),
            });
        }
        Ok(())
    }

    fn authenticate_teardown_manifest(
        &self,
        command: &SandboxExecutionTeardownCommand,
        manifest: &KrunSandboxManifest,
    ) -> crate::Result<()> {
        if &manifest.spec.tenant_id != command.tenant_id()
            || &manifest.handle.id != command.sandbox_id()
        {
            return Err(SandboxError::InvalidSpec {
                message: "Krun teardown command crossed tenant or sandbox identity".to_owned(),
            });
        }
        manifest
            .require_execution_attempt(command.execution_attempt_id(), "Krun execution teardown")?;
        let plan = manifest.provision_network_plan.as_ref().ok_or_else(|| {
            SandboxError::OperationFailed {
                message: format!(
                    "Krun execution teardown for {} lacks its exact compiled network plan",
                    manifest.handle.id
                ),
            }
        })?;
        let claim = command.provider_claim();
        if plan.tenant_id() != command.tenant_id()
            || plan.generation().as_u64() != claim.workload_generation()
            || plan.network_plan().digest().to_string() != claim.network_plan_digest()
        {
            return Err(SandboxError::InvalidSpec {
                message: "Krun teardown command crossed durable plan or generation".to_owned(),
            });
        }
        Ok(())
    }

    fn execute_drain(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &mut KrunSandboxManifest,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        match manifest.execution_teardown.drain() {
            KrunDrainProgress::Drained { fence, evidence } if fence == claim => {
                return Ok(succeeded(evidence.clone()));
            }
            KrunDrainProgress::BarrierPersisted { fence } if fence == claim => {}
            KrunDrainProgress::BarrierPersisted { fence }
                if journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                manifest
                    .execution_teardown
                    .set_drain(KrunDrainProgress::BarrierPersisted {
                        fence: claim.clone(),
                    });
                self.persist_effect_barrier(manifest, "krun execution drain retry barrier")?;
            }
            KrunDrainProgress::Open => {
                manifest
                    .execution_teardown
                    .set_drain(KrunDrainProgress::BarrierPersisted {
                        fence: claim.clone(),
                    });
                self.persist_effect_barrier(manifest, "krun execution drain barrier")?;
            }
            KrunDrainProgress::BarrierPersisted { fence }
            | KrunDrainProgress::Drained { fence, .. }
                if same_command_attempt(fence, claim) =>
            {
                return Ok(invalid_epoch(
                    "drain progress is not from the adjacent retry epoch",
                ));
            }
            _ => return Err(crossed_progress("drain", manifest)),
        }

        if let Some(evidence) = self.admitted_work_evidence(manifest)? {
            return Ok(SandboxExecutionTeardownObservation::InProgress { evidence });
        }
        let evidence = teardown_evidence("krun_execution_drained", manifest, claim)?;
        manifest
            .execution_teardown
            .set_drain(KrunDrainProgress::Drained {
                fence: claim.clone(),
                evidence: evidence.clone(),
            });
        self.persist_effect_barrier(manifest, "krun execution drained")?;
        Ok(succeeded(evidence))
    }

    fn inspect_drain(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &KrunSandboxManifest,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        match manifest.execution_teardown.drain() {
            KrunDrainProgress::Open => Ok(absent("drain barrier is not durable")),
            KrunDrainProgress::BarrierPersisted { fence }
                if fence == claim
                    || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                if let Some(evidence) = self.admitted_work_evidence(manifest)? {
                    Ok(SandboxExecutionTeardownObservation::InProgress { evidence })
                } else {
                    Ok(absent("drain completion is not durable"))
                }
            }
            KrunDrainProgress::Drained { fence, evidence }
                if fence == claim
                    || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                Ok(succeeded(evidence.clone()))
            }
            KrunDrainProgress::BarrierPersisted { fence }
            | KrunDrainProgress::Drained { fence, .. }
                if same_command_attempt(fence, claim) =>
            {
                Ok(invalid_epoch(
                    "drain inspection crossed a non-adjacent retry epoch",
                ))
            }
            _ => Err(crossed_progress("drain inspection", manifest)),
        }
    }

    fn execute_stop_execution(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &mut KrunSandboxManifest,
        runtime: &dyn KrunExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        require_matching_drain(manifest, claim)?;
        match manifest.execution_teardown.stop().clone() {
            KrunStopProgress::ExecutionStopped { fence, evidence } if fence == *claim => {
                Ok(succeeded(evidence))
            }
            KrunStopProgress::NotRequested => {
                manifest.shutdown_requested = true;
                super::readiness::synchronize_handle_status(
                    manifest,
                    crate::instance::SandboxStatus::Stopping,
                );
                manifest
                    .execution_teardown
                    .set_stop(KrunStopProgress::IntentPersisted {
                        fence: claim.clone(),
                    });
                self.persist_effect_barrier(manifest, "krun execution stop intent")?;
                self.advance_stop_after_intent(claim, manifest, runtime)
            }
            KrunStopProgress::IntentPersisted { fence } if same_command_epoch(&fence, claim) => {
                self.advance_stop_after_intent(claim, manifest, runtime)
            }
            KrunStopProgress::IntentPersisted { fence }
                if journal_authorizes_older_progress(&fence, claim, journal_authorization) =>
            {
                manifest
                    .execution_teardown
                    .set_stop(KrunStopProgress::IntentPersisted {
                        fence: claim.clone(),
                    });
                self.persist_effect_barrier(manifest, "krun execution stop retry intent")?;
                self.advance_stop_after_intent(claim, manifest, runtime)
            }
            KrunStopProgress::GracefulSignalMayExist {
                fence,
                process,
                graceful_signal,
                grace_deadline_unix_millis,
            } if journal_authorizes_older_progress(&fence, claim, journal_authorization) => {
                match runtime.observe_execution_terminal(manifest)? {
                    KrunExecutionTerminalObservation::ExactExit { exit_code } => {
                        return self.persist_execution_stopped(
                            claim,
                            manifest,
                            "graceful_exit_receipt_observed",
                            Some(exit_code),
                        );
                    }
                    KrunExecutionTerminalObservation::ExplicitAbsence => {
                        return self.persist_execution_stopped(
                            claim,
                            manifest,
                            "graceful_provider_absence_observed",
                            None,
                        );
                    }
                    KrunExecutionTerminalObservation::NotObserved => {}
                }
                match runtime.inspect_process(manifest, &process)? {
                    RuntimeProcessIdentityObservation::ExplicitlyAbsent => self
                        .persist_execution_stopped(
                            claim,
                            manifest,
                            "graceful_observed_absent",
                            None,
                        ),
                    RuntimeProcessIdentityObservation::ExactLive
                        if runtime.now_unix_millis()? >= grace_deadline_unix_millis =>
                    {
                        manifest
                            .execution_teardown
                            .set_stop(KrunStopProgress::KillMayExist {
                                fence: claim.clone(),
                                process: process.clone(),
                                redelivery_not_before_unix_millis: kill_redelivery_deadline(
                                    runtime,
                                )?,
                            });
                        self.persist_effect_barrier(manifest, "krun execution KILL may exist")?;
                        let _ = runtime.signal_process(
                            manifest,
                            &process,
                            RuntimeProcessSignal::kill(),
                        )?;
                        Ok(in_progress(
                            "KILL may exist; exact terminality is not observed",
                        ))
                    }
                    RuntimeProcessIdentityObservation::ExactLive => {
                        manifest.execution_teardown.set_stop(
                            KrunStopProgress::GracefulSignalMayExist {
                                fence: claim.clone(),
                                process,
                                graceful_signal,
                                grace_deadline_unix_millis,
                            },
                        );
                        self.persist_effect_barrier(
                            manifest,
                            "krun execution graceful retry adoption",
                        )?;
                        Ok(in_progress(
                            "graceful signal deadline has not elapsed for the exact process",
                        ))
                    }
                }
            }
            KrunStopProgress::KillMayExist {
                fence,
                process,
                redelivery_not_before_unix_millis,
            } if journal_authorizes_older_progress(&fence, claim, journal_authorization) => {
                match runtime.observe_execution_terminal(manifest)? {
                    KrunExecutionTerminalObservation::ExactExit { exit_code } => {
                        return self.persist_execution_stopped(
                            claim,
                            manifest,
                            "kill_exit_receipt_observed",
                            Some(exit_code),
                        );
                    }
                    KrunExecutionTerminalObservation::ExplicitAbsence => {
                        return self.persist_execution_stopped(
                            claim,
                            manifest,
                            "kill_provider_absence_observed",
                            None,
                        );
                    }
                    KrunExecutionTerminalObservation::NotObserved => {}
                }
                match runtime.inspect_process(manifest, &process)? {
                    RuntimeProcessIdentityObservation::ExplicitlyAbsent => self
                        .persist_execution_stopped(claim, manifest, "kill_observed_absent", None),
                    RuntimeProcessIdentityObservation::ExactLive => {
                        let now = runtime.now_unix_millis()?;
                        let redelivery_is_due = now >= redelivery_not_before_unix_millis;
                        let next_deadline = if redelivery_is_due {
                            now.checked_add(duration_millis(KILL_REDELIVERY_DELAY)?)
                                .ok_or_else(|| SandboxError::OperationFailed {
                                    message: "Krun KILL redelivery deadline overflowed".to_owned(),
                                })?
                        } else {
                            redelivery_not_before_unix_millis
                        };
                        manifest
                            .execution_teardown
                            .set_stop(KrunStopProgress::KillMayExist {
                                fence: claim.clone(),
                                process: process.clone(),
                                redelivery_not_before_unix_millis: next_deadline,
                            });
                        self.persist_effect_barrier(
                            manifest,
                            "krun execution KILL reconciliation",
                        )?;
                        if redelivery_is_due {
                            let _ = runtime.signal_process(
                                manifest,
                                &process,
                                RuntimeProcessSignal::kill(),
                            )?;
                            Ok(in_progress(
                                "authenticated KILL redelivery may exist; exact terminality is not observed",
                            ))
                        } else {
                            Ok(in_progress(
                                "exact process remains live before authenticated KILL redelivery",
                            ))
                        }
                    }
                }
            }
            KrunStopProgress::GracefulSignalMayExist { fence, .. }
            | KrunStopProgress::KillMayExist { fence, .. }
            | KrunStopProgress::IntentPersisted { fence }
            | KrunStopProgress::ExecutionStopped { fence, .. }
                if fence == *claim =>
            {
                Ok(in_progress(
                    "exact stop effect is already durable and requires inspection",
                ))
            }
            KrunStopProgress::GracefulSignalMayExist { fence, .. }
            | KrunStopProgress::KillMayExist { fence, .. }
            | KrunStopProgress::IntentPersisted { fence }
            | KrunStopProgress::ExecutionStopped { fence, .. }
                if same_command_attempt(&fence, claim) =>
            {
                Ok(invalid_epoch(
                    "stop progress is not from the adjacent retry epoch",
                ))
            }
            _ => Err(crossed_progress("stop", manifest)),
        }
    }

    fn advance_stop_after_intent(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &mut KrunSandboxManifest,
        runtime: &dyn KrunExecutionTeardownRuntime,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        match runtime.observe_execution_terminal(manifest)? {
            KrunExecutionTerminalObservation::ExactExit { exit_code } => {
                return self.persist_execution_stopped(
                    claim,
                    manifest,
                    "terminal_before_signal",
                    Some(exit_code),
                );
            }
            KrunExecutionTerminalObservation::ExplicitAbsence => {
                return self.persist_execution_stopped(
                    claim,
                    manifest,
                    "provider_absence_before_signal",
                    None,
                );
            }
            KrunExecutionTerminalObservation::NotObserved => {}
        }
        let process = runtime.capture_process(manifest)?;
        let graceful_signal = crate::backends::conmon::lifecycle::configured_stop_signal(
            manifest.image_metadata.stop_signal.as_deref(),
        );
        let parsed_signal = RuntimeProcessSignal::parse(&graceful_signal)?;
        let timeout = crate::backends::conmon::lifecycle::configured_stop_timeout(
            &manifest.spec,
            self.config.stop_timeout,
        );
        let grace_deadline_unix_millis = runtime
            .now_unix_millis()?
            .checked_add(duration_millis(timeout)?)
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "Krun stop grace deadline overflowed".to_owned(),
            })?;
        manifest
            .execution_teardown
            .set_stop(KrunStopProgress::GracefulSignalMayExist {
                fence: claim.clone(),
                process: process.clone(),
                graceful_signal,
                grace_deadline_unix_millis,
            });
        self.persist_effect_barrier(manifest, "krun graceful signal may exist")?;
        match runtime.signal_process(manifest, &process, parsed_signal)? {
            RuntimeProcessSignalOutcome::Delivered => Ok(in_progress(
                "graceful signal may exist; exact terminality is not observed",
            )),
            RuntimeProcessSignalOutcome::AlreadyAbsent => Ok(in_progress(
                "process disappeared during graceful signal; inspect exact runtime state",
            )),
        }
    }

    fn inspect_stop_execution(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &KrunSandboxManifest,
        runtime: &dyn KrunExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        require_matching_drain(manifest, claim)?;
        match manifest.execution_teardown.stop() {
            KrunStopProgress::NotRequested => Ok(absent("stop intent is not durable")),
            KrunStopProgress::IntentPersisted { fence }
                if same_command_epoch(fence, claim)
                    || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                if !matches!(
                    runtime.observe_execution_terminal(manifest)?,
                    KrunExecutionTerminalObservation::NotObserved
                ) {
                    Ok(absent(
                        "terminal execution evidence awaits durable stop completion",
                    ))
                } else {
                    Ok(absent("stop effect is not durable"))
                }
            }
            KrunStopProgress::GracefulSignalMayExist {
                fence,
                process,
                grace_deadline_unix_millis,
                ..
            } if same_command_epoch(fence, claim)
                || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                if !matches!(
                    runtime.observe_execution_terminal(manifest)?,
                    KrunExecutionTerminalObservation::NotObserved
                ) {
                    return Ok(retry_authorized(
                        "exact exit receipt awaits durable stop completion",
                    ));
                }
                match runtime.inspect_process(manifest, process)? {
                    RuntimeProcessIdentityObservation::ExplicitlyAbsent => Ok(retry_authorized(
                        "graceful signal completed; durable stop completion is pending",
                    )),
                    RuntimeProcessIdentityObservation::ExactLive
                        if runtime.now_unix_millis()? >= *grace_deadline_unix_millis =>
                    {
                        Ok(retry_authorized(
                            "graceful signal deadline elapsed; forced-stop dispatch is pending",
                        ))
                    }
                    RuntimeProcessIdentityObservation::ExactLive => Ok(in_progress(
                        "exact process remains live before the graceful signal deadline",
                    )),
                }
            }
            KrunStopProgress::KillMayExist {
                fence,
                process,
                redelivery_not_before_unix_millis,
            } if same_command_epoch(fence, claim)
                || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                if !matches!(
                    runtime.observe_execution_terminal(manifest)?,
                    KrunExecutionTerminalObservation::NotObserved
                ) {
                    return Ok(retry_authorized(
                        "exact exit receipt awaits durable stop completion",
                    ));
                }
                match runtime.inspect_process(manifest, process)? {
                    RuntimeProcessIdentityObservation::ExplicitlyAbsent => Ok(retry_authorized(
                        "KILL completed; durable stop completion is pending",
                    )),
                    RuntimeProcessIdentityObservation::ExactLive
                        if runtime.now_unix_millis()? >= *redelivery_not_before_unix_millis =>
                    {
                        Ok(retry_authorized(
                            "exact process remains live; authenticated KILL redelivery is pending",
                        ))
                    }
                    RuntimeProcessIdentityObservation::ExactLive => Ok(in_progress(
                        "exact process remains live before KILL reconciliation deadline",
                    )),
                }
            }
            KrunStopProgress::ExecutionStopped { fence, evidence }
                if same_command_epoch(fence, claim)
                    || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                Ok(succeeded(evidence.clone()))
            }
            KrunStopProgress::IntentPersisted { fence }
            | KrunStopProgress::GracefulSignalMayExist { fence, .. }
            | KrunStopProgress::KillMayExist { fence, .. }
            | KrunStopProgress::ExecutionStopped { fence, .. }
                if same_command_attempt(fence, claim) =>
            {
                Ok(invalid_epoch(
                    "stop inspection crossed a non-adjacent retry epoch",
                ))
            }
            _ => Err(crossed_progress("stop inspection", manifest)),
        }
    }

    fn persist_execution_stopped(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &mut KrunSandboxManifest,
        reason: &str,
        exit_code: Option<i32>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        if let Some(exit_code) = exit_code {
            manifest.last_exit_code = Some(exit_code);
        }
        let evidence = teardown_evidence(reason, manifest, claim)?;
        manifest
            .execution_teardown
            .set_stop(KrunStopProgress::ExecutionStopped {
                fence: claim.clone(),
                evidence: evidence.clone(),
            });
        super::readiness::synchronize_handle_status(
            manifest,
            crate::instance::SandboxStatus::Stopping,
        );
        self.persist_effect_barrier(manifest, "krun execution stopped")?;
        Ok(succeeded(evidence))
    }

    fn admitted_work_evidence(
        &self,
        manifest: &KrunSandboxManifest,
    ) -> crate::Result<Option<Vec<u8>>> {
        if matches!(
            manifest.creator_handoff,
            KrunCreatorHandoffState::SpawnIntent { .. } | KrunCreatorHandoffState::Pending { .. }
        ) {
            return Ok(Some(b"Krun creator handoff remains in progress".to_vec()));
        }
        if manifest.creator_handoff == KrunCreatorHandoffState::NotSpawned
            && matches!(
                manifest.launch_authority,
                KrunLaunchAuthority::ProviderOwned
            )
        {
            return Ok(Some(
                b"Krun provider-owned execution lacks authenticated creator evidence".to_vec(),
            ));
        }
        if manifest.provider_failure_cleanup.is_active() {
            return Ok(Some(
                b"Krun provider-failure cleanup remains in progress".to_vec(),
            ));
        }
        if !manifest.provision_prepared
            || !matches!(
                manifest.launch_authority,
                KrunLaunchAuthority::ProviderOwned
            )
        {
            return Ok(Some(
                format!(
                    "Krun activation remains incomplete: provision_prepared={}, launch_authority={:?}",
                    manifest.provision_prepared, manifest.launch_authority,
                )
                .into_bytes(),
            ));
        }
        self.execution_drain_pending_restart_evidence(manifest)
    }
}

fn require_matching_drain(
    manifest: &KrunSandboxManifest,
    stop_claim: &ProviderCommandClaim,
) -> crate::Result<()> {
    let KrunDrainProgress::Drained { fence, .. } = manifest.execution_teardown.drain() else {
        return Err(SandboxError::InvalidSpec {
            message: "Krun execution stop requires exact durable drain completion".to_owned(),
        });
    };
    if same_workload_fence(fence, stop_claim) {
        Ok(())
    } else {
        Err(SandboxError::InvalidSpec {
            message: "Krun execution stop is crossed with the durable drain fence".to_owned(),
        })
    }
}

fn same_workload_fence(left: &ProviderCommandClaim, right: &ProviderCommandClaim) -> bool {
    left.authority_id() == right.authority_id()
        && left.effect_subject() == right.effect_subject()
        && left.source_attempt_id() == right.source_attempt_id()
        && left.attempt_id() == right.attempt_id()
        && left.workload_generation() == right.workload_generation()
        && left.restart_ordinal() == right.restart_ordinal()
        && left.desired_digest() == right.desired_digest()
        && left.source_digest() == right.source_digest()
        && left.network_plan_digest() == right.network_plan_digest()
        && left.provider_target_digest() == right.provider_target_digest()
}

fn same_command_attempt(left: &ProviderCommandClaim, right: &ProviderCommandClaim) -> bool {
    same_workload_fence(left, right)
        && left.attempt_id() == right.attempt_id()
        && left.operation() == right.operation()
}

fn same_command_epoch(left: &ProviderCommandClaim, right: &ProviderCommandClaim) -> bool {
    same_command_attempt(left, right) && left.dispatch_epoch() == right.dispatch_epoch()
}

fn journal_authorizes_older_progress(
    progress: &ProviderCommandClaim,
    current: &ProviderCommandClaim,
    authorization: Option<&ProviderCommandObservation>,
) -> bool {
    let Some(authorization) = authorization else {
        return false;
    };
    authorization.claim() == current
        && matches!(
            authorization.kind(),
            ProviderCommandObservationKind::Claimed
                | ProviderCommandObservationKind::InProgress
                | ProviderCommandObservationKind::Ambiguous
        )
        && same_command_attempt(progress, current)
        && progress.dispatch_epoch().checked_add(1) == Some(current.dispatch_epoch())
        && authorization.authenticates_retry_progress(progress)
}

fn kill_redelivery_deadline(runtime: &dyn KrunExecutionTeardownRuntime) -> crate::Result<u64> {
    runtime
        .now_unix_millis()?
        .checked_add(duration_millis(KILL_REDELIVERY_DELAY)?)
        .ok_or_else(|| SandboxError::OperationFailed {
            message: "Krun KILL redelivery deadline overflowed".to_owned(),
        })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TeardownEvidence<'a> {
    kind: &'a str,
    tenant_id: &'a str,
    sandbox_id: &'a str,
    execution_attempt_id: &'a str,
    authority_id: &'a str,
    dispatch_epoch: u64,
    launch_authority: &'a KrunLaunchAuthority,
}

fn teardown_evidence(
    kind: &str,
    manifest: &KrunSandboxManifest,
    claim: &ProviderCommandClaim,
) -> crate::Result<Vec<u8>> {
    serde_json::to_vec(&TeardownEvidence {
        kind,
        tenant_id: manifest.spec.tenant_id.as_str(),
        sandbox_id: manifest.handle.id.as_str(),
        execution_attempt_id: manifest.execution_attempt_id.as_str(),
        authority_id: claim.authority_id(),
        dispatch_epoch: claim.dispatch_epoch(),
        launch_authority: &manifest.launch_authority,
    })
    .map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to encode Krun execution teardown evidence: {error}"),
    })
}

fn duration_millis(duration: Duration) -> crate::Result<u64> {
    u64::try_from(duration.as_millis()).map_err(|_| SandboxError::OperationFailed {
        message: "Krun stop timeout cannot be represented in milliseconds".to_owned(),
    })
}

fn crossed_progress(operation: &str, manifest: &KrunSandboxManifest) -> SandboxError {
    SandboxError::InvalidSpec {
        message: format!(
            "Krun {operation} command is crossed with durable execution teardown progress for {}",
            manifest.handle.id
        ),
    }
}

fn succeeded(evidence: Vec<u8>) -> SandboxExecutionTeardownObservation {
    SandboxExecutionTeardownObservation::Succeeded { evidence }
}

fn absent(message: impl Into<Vec<u8>>) -> SandboxExecutionTeardownObservation {
    SandboxExecutionTeardownObservation::Absent {
        evidence: message.into(),
    }
}

fn retry_authorized(message: impl Into<Vec<u8>>) -> SandboxExecutionTeardownObservation {
    SandboxExecutionTeardownObservation::RetryAuthorized {
        evidence: message.into(),
    }
}

fn in_progress(message: impl Into<Vec<u8>>) -> SandboxExecutionTeardownObservation {
    SandboxExecutionTeardownObservation::InProgress {
        evidence: message.into(),
    }
}

fn ambiguous(message: impl Into<Vec<u8>>) -> SandboxExecutionTeardownObservation {
    SandboxExecutionTeardownObservation::Ambiguous {
        evidence: message.into(),
    }
}

fn definite_failure(
    code: &str,
    message: impl Into<Vec<u8>>,
) -> SandboxExecutionTeardownObservation {
    SandboxExecutionTeardownObservation::DefiniteFailure {
        code: code.to_owned(),
        evidence: message.into(),
    }
}

fn invalid_epoch(message: impl Into<Vec<u8>>) -> SandboxExecutionTeardownObservation {
    definite_failure("sandbox_teardown_epoch_invalid", message)
}

fn execution_observation_kind(
    observation: &SandboxExecutionTeardownObservation,
) -> ProviderCommandObservationKind {
    match observation {
        SandboxExecutionTeardownObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        SandboxExecutionTeardownObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        SandboxExecutionTeardownObservation::Absent { .. }
        | SandboxExecutionTeardownObservation::RetryAuthorized { .. }
        | SandboxExecutionTeardownObservation::Ambiguous { .. } => {
            ProviderCommandObservationKind::Ambiguous
        }
        SandboxExecutionTeardownObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
    }
}

#[cfg(test)]
#[path = "teardown/tests.rs"]
mod tests;
