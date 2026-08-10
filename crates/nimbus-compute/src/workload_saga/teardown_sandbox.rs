//! Exact Container substitution for execution drain and stop capabilities.
//!
//! Compute authenticates workload-owned vocabulary and lowers it to the
//! workload-neutral sandbox contract before it can claim provider authority.

use std::sync::Arc;

use nimbus_sandbox::backends::container::ContainerSandboxBackend;
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandExecutionClaim, ProviderCommandJournalError,
    ProviderCommandObservation, ProviderCommandObservationKind, ProviderCommandOperation,
    SandboxBackendKind, SandboxExecutionAttemptId, SandboxExecutionTeardownCommand,
    SandboxExecutionTeardownObservation, SandboxExecutionTeardownOperation, SandboxId,
};
use nimbus_workloads::{
    WorkloadExecutionProviderId, WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest,
    WorkloadTeardownCommandMode, WorkloadTeardownProviderTarget, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};

use super::provision_sandbox::sandbox_execution_provider_id;
use super::{
    ConfirmedWorkloadTeardownCommand, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome,
};

pub mod krun;

const CONTAINER_EXECUTION_PROVIDER_KEY: &str = "nimbus-sandbox.container-execution";

/// Validated lower command plus its exact provider-journal claim.
pub struct ValidatedSandboxTeardownCommand {
    sandbox_command: SandboxExecutionTeardownCommand,
}

impl ValidatedSandboxTeardownCommand {
    pub fn sandbox_command(&self) -> &SandboxExecutionTeardownCommand {
        &self.sandbox_command
    }
}

/// Authenticate and lower one compute-confirmed execution teardown command.
pub fn validate_sandbox_teardown_command(
    command: &ConfirmedWorkloadTeardownCommand,
    backend: SandboxBackendKind,
) -> Result<ValidatedSandboxTeardownCommand, WorkloadFailureEvidence> {
    let expected_provider = sandbox_execution_provider_id(backend);
    let WorkloadTeardownProviderTarget::Execution {
        provider_id,
        provider_source_digest,
    } = command.provider_target()
    else {
        return Err(invalid_command_failure(
            "sandbox execution teardown requires an execution provider target",
        ));
    };
    if provider_id != &expected_provider
        || command.source().execution_provider_id() != &expected_provider
        || *provider_source_digest != command.source_digest()
    {
        return Err(crossed_command_failure(
            "sandbox execution provider target is crossed with confirmed source evidence",
        ));
    }

    let operation = match command.step() {
        WorkloadTeardownStep::DrainExecution => SandboxExecutionTeardownOperation::Drain,
        WorkloadTeardownStep::StopExecution => SandboxExecutionTeardownOperation::Stop,
        _ => {
            return Err(invalid_command_failure(
                "sandbox execution teardown supports only drain and stop",
            ));
        }
    };
    let WorkloadTeardownSubjects::Execution(subject) = command.subjects() else {
        return Err(invalid_command_failure(
            "sandbox execution teardown requires an execution subject",
        ));
    };
    let locator = command.execution_locator();
    if subject != locator
        || locator.node_identity() != command.required_node()
        || locator.generation() != command.generation()
        || locator.desired_digest() != command.desired_digest()
    {
        return Err(crossed_command_failure(
            "sandbox execution locator is crossed with confirmed command fences",
        ));
    }

    let provider_claim = provider_claim(command, operation.provider_operation())?;
    let provider_key = match backend {
        SandboxBackendKind::Container => CONTAINER_EXECUTION_PROVIDER_KEY,
        SandboxBackendKind::Krun => "nimbus-sandbox.krun-execution",
    };
    let sandbox_command = SandboxExecutionTeardownCommand::new(
        command.key().tenant_id().clone(),
        SandboxId::new(locator.execution_id().as_str()),
        SandboxExecutionAttemptId::new(locator.attempt_id().to_string())
            .map_err(|error| invalid_command_failure(error.to_string()))?,
        provider_key,
        operation,
        provider_claim,
    )
    .map_err(|error| invalid_command_failure(error.to_string()))?;
    Ok(ValidatedSandboxTeardownCommand { sandbox_command })
}

fn provider_claim(
    command: &ConfirmedWorkloadTeardownCommand,
    operation: ProviderCommandOperation,
) -> Result<ProviderCommandClaim, WorkloadFailureEvidence> {
    // The operation is a separate provider-journal domain. Keep the effect
    // subject stable across drain and stop so the provider can prove that both
    // phases refer to the same retained execution.
    let effect_subject = serde_json::to_string(&(command.execution_locator(), command.subjects()))
        .map_err(|error| invalid_command_failure(error.to_string()))?;
    let provider_target = serde_json::to_vec(command.provider_target())
        .map_err(|error| invalid_command_failure(error.to_string()))?;
    ProviderCommandClaim::new(ProviderCommandClaimInput {
        authority_id: command.saga_id().as_str().to_owned(),
        effect_subject,
        source_attempt_id: None,
        attempt_id: command.attempt_id().as_str().to_owned(),
        dispatch_epoch: command.dispatch_epoch().as_u64(),
        workload_generation: command.generation().as_u64(),
        restart_ordinal: 0,
        desired_digest: command.desired_digest().to_string(),
        source_digest: command.source_digest().to_string(),
        network_plan_digest: command.network_plan_digest().to_string(),
        provider_target_digest: WorkloadOwnerEvidenceDigest::sha256(provider_target).to_string(),
        operation,
    })
    .map_err(|error| invalid_command_failure(error.to_string()))
}

struct ProviderTeardownPhaseAdapter {
    journal: ProviderCommandAttemptJournal,
}

impl ProviderTeardownPhaseAdapter {
    fn new(journal: ProviderCommandAttemptJournal) -> Self {
        Self { journal }
    }

    fn execute(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        validated: &ValidatedSandboxTeardownCommand,
        effect: impl FnOnce(
            ProviderCommandExecutionClaim,
        ) -> Result<ProviderCommandObservation, ProviderCommandJournalError>,
    ) -> WorkloadTeardownProviderOutcome {
        let claim = validated.sandbox_command().provider_claim();
        match self.journal.claim_dispatch_epoch(claim) {
            Ok(ProviderCommandClaimDecision::ExecuteClaimed(execution_claim)) => {
                match effect(execution_claim) {
                    Ok(observation) => provider_outcome(command, &observation),
                    Err(error) => journal_error_outcome(command.mode(), &error),
                }
            }
            Ok(ProviderCommandClaimDecision::AdoptExactAttempt(observation)) => {
                provider_outcome(command, &observation)
            }
            Err(error) => journal_error_outcome(command.mode(), &error),
        }
    }

    fn inspect(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        validated: &ValidatedSandboxTeardownCommand,
        inspect: impl FnOnce(&ProviderCommandObservation) -> SandboxExecutionTeardownObservation,
    ) -> WorkloadTeardownProviderOutcome {
        let claim = validated.sandbox_command().provider_claim();
        match self.journal.adopt_exact_attempt(claim) {
            Ok(None) => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::NotCompleted(WorkloadOwnerEvidenceDigest::sha256(
                    b"sandbox provider command was never claimed",
                )),
            ),
            Ok(Some(observation))
                if matches!(
                    observation.kind(),
                    ProviderCommandObservationKind::Succeeded
                        | ProviderCommandObservationKind::DefiniteFailure
                        | ProviderCommandObservationKind::Absent
                        | ProviderCommandObservationKind::RetryAuthorized
                ) =>
            {
                provider_outcome(command, &observation)
            }
            Ok(Some(observation)) => self.record(command, claim, inspect(&observation)),
            Err(error) => journal_error_outcome(command.mode(), &error),
        }
    }

    fn record(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        claim: &ProviderCommandClaim,
        observation: SandboxExecutionTeardownObservation,
    ) -> WorkloadTeardownProviderOutcome {
        let kind = match (&observation, command.mode()) {
            (SandboxExecutionTeardownObservation::Succeeded { .. }, _) => {
                ProviderCommandObservationKind::Succeeded
            }
            (SandboxExecutionTeardownObservation::DefiniteFailure { .. }, _) => {
                ProviderCommandObservationKind::DefiniteFailure
            }
            (
                SandboxExecutionTeardownObservation::Absent { .. },
                WorkloadTeardownCommandMode::Execute,
            ) => ProviderCommandObservationKind::Ambiguous,
            (
                SandboxExecutionTeardownObservation::Absent { .. },
                WorkloadTeardownCommandMode::Inspect,
            ) => ProviderCommandObservationKind::Absent,
            (
                SandboxExecutionTeardownObservation::RetryAuthorized { .. },
                WorkloadTeardownCommandMode::Inspect,
            ) => ProviderCommandObservationKind::RetryAuthorized,
            (
                SandboxExecutionTeardownObservation::RetryAuthorized { .. },
                WorkloadTeardownCommandMode::Execute,
            ) => ProviderCommandObservationKind::Ambiguous,
            (SandboxExecutionTeardownObservation::InProgress { .. }, _) => {
                ProviderCommandObservationKind::InProgress
            }
            (SandboxExecutionTeardownObservation::Ambiguous { .. }, _) => {
                ProviderCommandObservationKind::Ambiguous
            }
        };
        match self.journal.record_observation_with_failure_code(
            claim,
            kind,
            observation.failure_code(),
            observation.evidence(),
        ) {
            Ok(observation) => provider_outcome(command, &observation),
            Err(error) => journal_error_outcome(command.mode(), &error),
        }
    }
}

/// Real Container adapter for the two execution-only teardown capabilities.
pub struct ContainerTeardownAdapter {
    backend: Arc<ContainerSandboxBackend>,
    phases: ProviderTeardownPhaseAdapter,
    provider_id: WorkloadExecutionProviderId,
}

impl ContainerTeardownAdapter {
    pub fn new(backend: Arc<ContainerSandboxBackend>) -> Result<Self, ProviderCommandJournalError> {
        let journal = backend.attempt_idempotency_journal()?;
        Ok(Self {
            backend,
            phases: ProviderTeardownPhaseAdapter::new(journal),
            provider_id: sandbox_execution_provider_id(SandboxBackendKind::Container),
        })
    }

    pub fn capabilities(self: Arc<Self>) -> WorkloadExecutionTeardownCapabilities {
        WorkloadExecutionTeardownCapabilities::new(self.provider_id.clone(), self.clone(), self)
    }

    fn validated(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> Result<ValidatedSandboxTeardownCommand, WorkloadFailureEvidence> {
        validate_sandbox_teardown_command(command, SandboxBackendKind::Container)
    }
}

impl WorkloadExecutionDrainCapability for ContainerTeardownAdapter {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            let outcome = match self.validated(command) {
                Ok(validated) => self.phases.execute(command, &validated, |execution_claim| {
                    self.backend.execute_execution_teardown_with_claim(
                        validated.sandbox_command(),
                        execution_claim,
                    )
                }),
                Err(failure) => WorkloadTeardownProviderOutcome::Execute(
                    WorkloadTeardownExecuteOutcome::DefiniteFailure(failure),
                ),
            };
            WorkloadTeardownProviderObservation::for_command(command, outcome)
        })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            let outcome = match self.validated(command) {
                Ok(validated) => self.phases.inspect(command, &validated, |observation| {
                    self.backend.inspect_execution_teardown_with_observation(
                        validated.sandbox_command(),
                        observation,
                    )
                }),
                Err(failure) => WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
                ),
            };
            WorkloadTeardownProviderObservation::for_command(command, outcome)
        })
    }
}

impl WorkloadExecutionStopCapability for ContainerTeardownAdapter {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        <Self as WorkloadExecutionDrainCapability>::execute(self, command)
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        <Self as WorkloadExecutionDrainCapability>::inspect(self, command)
    }
}

fn provider_outcome(
    command: &ConfirmedWorkloadTeardownCommand,
    observation: &ProviderCommandObservation,
) -> WorkloadTeardownProviderOutcome {
    let evidence = provider_evidence(observation);
    match command.mode() {
        WorkloadTeardownCommandMode::Execute => {
            WorkloadTeardownProviderOutcome::Execute(match observation.kind() {
                ProviderCommandObservationKind::Succeeded => {
                    WorkloadTeardownExecuteOutcome::Succeeded(
                        success_evidence(command, evidence).into(),
                    )
                }
                ProviderCommandObservationKind::DefiniteFailure => {
                    WorkloadTeardownExecuteOutcome::DefiniteFailure(provider_failure(
                        observation
                            .failure_code()
                            .expect("validated teardown failure has a durable code"),
                        evidence,
                    ))
                }
                ProviderCommandObservationKind::Claimed
                | ProviderCommandObservationKind::Absent
                | ProviderCommandObservationKind::RetryAuthorized
                | ProviderCommandObservationKind::InProgress
                | ProviderCommandObservationKind::Ambiguous => {
                    WorkloadTeardownExecuteOutcome::Ambiguous
                }
            })
        }
        WorkloadTeardownCommandMode::Inspect => {
            WorkloadTeardownProviderOutcome::Inspect(match observation.kind() {
                ProviderCommandObservationKind::Succeeded => {
                    WorkloadTeardownInspectOutcome::Satisfied(
                        success_evidence(command, evidence).into(),
                    )
                }
                ProviderCommandObservationKind::DefiniteFailure => {
                    WorkloadTeardownInspectOutcome::DefiniteFailure(provider_failure(
                        observation
                            .failure_code()
                            .expect("validated teardown failure has a durable code"),
                        evidence,
                    ))
                }
                ProviderCommandObservationKind::Absent => {
                    WorkloadTeardownInspectOutcome::NotCompleted(evidence)
                }
                ProviderCommandObservationKind::RetryAuthorized => {
                    WorkloadTeardownInspectOutcome::NotCompleted(evidence)
                }
                ProviderCommandObservationKind::Claimed
                | ProviderCommandObservationKind::InProgress => {
                    WorkloadTeardownInspectOutcome::InProgress(evidence)
                }
                ProviderCommandObservationKind::Ambiguous => {
                    WorkloadTeardownInspectOutcome::Ambiguous
                }
            })
        }
    }
}

fn success_evidence(
    command: &ConfirmedWorkloadTeardownCommand,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadTeardownSuccessEvidence {
    let WorkloadTeardownSubjects::Execution(reference) = command.subjects() else {
        unreachable!("validated Container teardown command has an execution subject")
    };
    match command.step() {
        WorkloadTeardownStep::DrainExecution => WorkloadTeardownSuccessEvidence::ExecutionDrained {
            reference: reference.clone(),
            evidence,
        },
        WorkloadTeardownStep::StopExecution => WorkloadTeardownSuccessEvidence::ExecutionStopped {
            reference: reference.clone(),
            evidence,
        },
        _ => unreachable!("validated Container teardown command is drain or stop"),
    }
}

fn provider_evidence(observation: &ProviderCommandObservation) -> WorkloadOwnerEvidenceDigest {
    let durable = observation
        .evidence_sha256()
        .unwrap_or("provider_claimed_without_outcome_evidence");
    WorkloadOwnerEvidenceDigest::sha256(
        [
            b"nimbus.compute.provider-teardown.observation.v1\0".as_slice(),
            durable.as_bytes(),
        ]
        .concat(),
    )
}

fn journal_error_outcome(
    mode: WorkloadTeardownCommandMode,
    error: &ProviderCommandJournalError,
) -> WorkloadTeardownProviderOutcome {
    let code = match error {
        ProviderCommandJournalError::InvalidClaim { .. } => {
            Some("sandbox_teardown_command_invalid")
        }
        ProviderCommandJournalError::StaleWorkloadGeneration { .. }
        | ProviderCommandJournalError::StaleRestartOrdinal { .. }
        | ProviderCommandJournalError::StaleDispatchEpoch { .. } => {
            Some("sandbox_teardown_command_stale")
        }
        ProviderCommandJournalError::SkippedRestartOrdinal { .. }
        | ProviderCommandJournalError::SkippedDispatchEpoch { .. }
        | ProviderCommandJournalError::CrossedClaim
        | ProviderCommandJournalError::RetryWithoutAuthority
        | ProviderCommandJournalError::PriorEffectUnresolved => {
            Some("sandbox_teardown_epoch_invalid")
        }
        ProviderCommandJournalError::Corrupt { .. } | ProviderCommandJournalError::Store { .. } => {
            None
        }
    };
    if let Some(code) = code {
        let failure =
            provider_failure(code, WorkloadOwnerEvidenceDigest::sha256(error.to_string()));
        return match mode {
            WorkloadTeardownCommandMode::Execute => WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::DefiniteFailure(failure),
            ),
            WorkloadTeardownCommandMode::Inspect => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
            ),
        };
    }
    match mode {
        WorkloadTeardownCommandMode::Execute => {
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
        }
        WorkloadTeardownCommandMode::Inspect => {
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous)
        }
    }
}

fn invalid_command_failure(message: impl AsRef<str>) -> WorkloadFailureEvidence {
    provider_failure(
        "sandbox_teardown_command_invalid",
        WorkloadOwnerEvidenceDigest::sha256(message.as_ref()),
    )
}

fn crossed_command_failure(message: impl AsRef<str>) -> WorkloadFailureEvidence {
    provider_failure(
        "sandbox_teardown_command_crossed",
        WorkloadOwnerEvidenceDigest::sha256(message.as_ref()),
    )
}

fn provider_failure(code: &str, evidence: WorkloadOwnerEvidenceDigest) -> WorkloadFailureEvidence {
    WorkloadFailureEvidence::new(code, evidence)
        .expect("static sandbox teardown failure code is valid")
}

#[cfg(test)]
#[path = "teardown_sandbox/tests.rs"]
mod tests;
