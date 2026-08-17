//! Exact Container substitution for execution drain and stop capabilities.
//!
//! Compute authenticates workload-owned vocabulary and lowers it to the
//! workload-neutral sandbox contract before it can claim provider authority.

use std::sync::Arc;

use nimbus_network::NetworkProviderId;
use nimbus_sandbox::backends::container::ContainerSandboxBackend;
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaimDecision, ProviderCommandExecutionClaim,
    ProviderCommandJournalError, ProviderCommandObservation, ProviderCommandObservationKind,
    SandboxBackendKind, SandboxExecutionAttemptId, SandboxExecutionTeardownCommand,
    SandboxExecutionTeardownObservation, SandboxExecutionTeardownOperation, SandboxId,
    SandboxNetworkTeardownObservation, sandbox_network_plan_requirements,
};
use nimbus_workloads::{
    WorkloadExecutionProviderId, WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest,
    WorkloadTeardownCommandMode, WorkloadTeardownProviderTarget, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};

use super::provision_sandbox::sandbox_execution_provider_id;
use super::teardown_provider_command::{
    ConfirmedTeardownProviderCommand, ConfirmedTeardownProviderJournal,
};
use super::{
    ConfirmedWorkloadTeardownCommand, NetworkAttachmentTeardownCapabilities,
    NetworkDetachmentCapability, NetworkReleaseCapability, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome,
};

mod attachment;
pub mod krun;

const CONTAINER_EXECUTION_PROVIDER_KEY: &str = "nimbus-sandbox.container-execution";

/// Validated lower command plus its exact provider-journal claim.
pub struct ValidatedSandboxTeardownCommand {
    sandbox_command: SandboxExecutionTeardownCommand,
    provider_command: ConfirmedTeardownProviderCommand,
}

impl ValidatedSandboxTeardownCommand {
    pub fn sandbox_command(&self) -> &SandboxExecutionTeardownCommand {
        &self.sandbox_command
    }

    fn provider_command(&self) -> &ConfirmedTeardownProviderCommand {
        &self.provider_command
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

    let effect_subject = serde_json::to_string(&(command.execution_locator(), command.subjects()))
        .map_err(|error| invalid_command_failure(error.to_string()))?;
    let provider_target = serde_json::to_vec(command.provider_target())
        .map_err(|error| invalid_command_failure(error.to_string()))?;
    let provider_command = ConfirmedTeardownProviderCommand::new(
        command,
        effect_subject,
        WorkloadOwnerEvidenceDigest::sha256(provider_target).to_string(),
    )
    .map_err(|error| invalid_command_failure(error.to_string()))?;
    if provider_command.claim().operation() != operation.provider_operation() {
        return Err(invalid_command_failure(
            "sandbox execution teardown operation crosses the confirmed provider operation",
        ));
    }
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
        provider_command.claim().clone(),
    )
    .map_err(|error| invalid_command_failure(error.to_string()))?;
    Ok(ValidatedSandboxTeardownCommand {
        sandbox_command,
        provider_command,
    })
}

struct ProviderTeardownPhaseAdapter {
    journal: ConfirmedTeardownProviderJournal,
}

impl ProviderTeardownPhaseAdapter {
    fn new(journal: ProviderCommandAttemptJournal) -> Self {
        Self {
            journal: ConfirmedTeardownProviderJournal::new(journal),
        }
    }

    fn execute(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        validated: &ValidatedSandboxTeardownCommand,
        effect: impl FnOnce(
            ProviderCommandExecutionClaim,
        ) -> Result<ProviderCommandObservation, ProviderCommandJournalError>,
    ) -> WorkloadTeardownProviderOutcome {
        let provider_command = validated.provider_command();
        match self.journal.claim_execute(provider_command) {
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
        let provider_command = validated.provider_command();
        match self.journal.adopt_inspect(provider_command) {
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
            Ok(Some(observation)) => {
                match self
                    .journal
                    .inspect_current_claim(provider_command, &observation, inspect)
                {
                    Ok(inspected) => self.record(command, provider_command, inspected),
                    Err(error) => journal_error_outcome(command.mode(), &error),
                }
            }
            Err(error) => journal_error_outcome(command.mode(), &error),
        }
    }

    fn record(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        provider_command: &ConfirmedTeardownProviderCommand,
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
            provider_command,
            kind,
            observation.failure_code(),
            observation.evidence(),
        ) {
            Ok(observation) => provider_outcome(command, &observation),
            Err(error) => journal_error_outcome(command.mode(), &error),
        }
    }

    fn execute_network(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        validated: &attachment::ValidatedSandboxNetworkTeardownCommand,
        effect: impl FnOnce(
            ProviderCommandExecutionClaim,
        ) -> Result<ProviderCommandObservation, ProviderCommandJournalError>,
        inspect: impl FnOnce(&ProviderCommandObservation) -> SandboxNetworkTeardownObservation,
    ) -> WorkloadTeardownProviderOutcome {
        let provider_command = validated.provider_command();
        match self.journal.claim_execute(provider_command) {
            Ok(ProviderCommandClaimDecision::ExecuteClaimed(execution_claim)) => {
                match effect(execution_claim) {
                    Ok(observation) => provider_outcome(command, &observation),
                    Err(error) => journal_error_outcome(command.mode(), &error),
                }
            }
            Ok(ProviderCommandClaimDecision::AdoptExactAttempt(observation))
                if observation.kind() == ProviderCommandObservationKind::Claimed =>
            {
                match self
                    .journal
                    .resume_current_claim(provider_command, &observation)
                {
                    Ok(execution_claim) => match effect(execution_claim) {
                        Ok(observation) => provider_outcome(command, &observation),
                        Err(error) => journal_error_outcome(command.mode(), &error),
                    },
                    Err(error) => journal_error_outcome(command.mode(), &error),
                }
            }
            Ok(ProviderCommandClaimDecision::AdoptExactAttempt(observation))
                if matches!(
                    observation.kind(),
                    ProviderCommandObservationKind::InProgress
                        | ProviderCommandObservationKind::Ambiguous
                ) =>
            {
                let inspected = inspect(&observation);
                match self.journal.record_observation_with_failure_code(
                    provider_command,
                    network_observation_kind(&inspected),
                    inspected.failure_code(),
                    inspected.evidence(),
                ) {
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

    fn inspect_network(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        validated: &attachment::ValidatedSandboxNetworkTeardownCommand,
        inspect: impl FnOnce(&ProviderCommandObservation) -> SandboxNetworkTeardownObservation,
    ) -> WorkloadTeardownProviderOutcome {
        let provider_command = validated.provider_command();
        match self.journal.adopt_inspect(provider_command) {
            Ok(None) => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::NotCompleted(WorkloadOwnerEvidenceDigest::sha256(
                    b"sandbox network provider command was never claimed",
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
            Ok(Some(observation)) => network_inspect_outcome(inspect(&observation)),
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

/// Real Container substitution for host-managed detach and release.
pub struct ContainerAttachmentTeardownAdapter {
    backend: Arc<ContainerSandboxBackend>,
    phases: ProviderTeardownPhaseAdapter,
    provider_id: NetworkProviderId,
}

impl ContainerAttachmentTeardownAdapter {
    pub fn new(backend: Arc<ContainerSandboxBackend>) -> Result<Self, ProviderCommandJournalError> {
        let journal = backend.attempt_idempotency_journal()?;
        Ok(Self {
            backend,
            phases: ProviderTeardownPhaseAdapter::new(journal),
            provider_id: sandbox_network_plan_requirements(SandboxBackendKind::Container)
                .required_attachment_provider_id()
                .clone(),
        })
    }

    pub fn capabilities(self: Arc<Self>) -> NetworkAttachmentTeardownCapabilities {
        NetworkAttachmentTeardownCapabilities::new(self.provider_id.clone(), self.clone(), self)
    }

    fn validated(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> Result<attachment::ValidatedSandboxNetworkTeardownCommand, WorkloadFailureEvidence> {
        attachment::validate_sandbox_network_teardown_command(
            command,
            SandboxBackendKind::Container,
        )
    }

    fn execute_network(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderOutcome {
        match self.validated(command) {
            Ok(validated) => match self
                .backend
                .preflight_network_teardown_command(validated.sandbox_command())
            {
                Ok(()) => self.phases.execute_network(
                    command,
                    &validated,
                    |execution_claim| {
                        self.backend.execute_network_teardown_with_claim(
                            validated.sandbox_command(),
                            execution_claim,
                        )
                    },
                    |observation| {
                        self.backend.inspect_network_teardown_with_observation(
                            validated.sandbox_command(),
                            observation,
                        )
                    },
                ),
                Err(observation) => network_preflight_failure_outcome(command, observation),
            },
            Err(failure) => WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::DefiniteFailure(failure),
            ),
        }
    }

    fn inspect_network(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderOutcome {
        match self.validated(command) {
            Ok(validated) => match self
                .backend
                .preflight_network_teardown_command(validated.sandbox_command())
            {
                Ok(()) => self
                    .phases
                    .inspect_network(command, &validated, |observation| {
                        self.backend.inspect_network_teardown_with_observation(
                            validated.sandbox_command(),
                            observation,
                        )
                    }),
                Err(observation) => network_preflight_failure_outcome(command, observation),
            },
            Err(failure) => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
            ),
        }
    }
}

impl NetworkDetachmentCapability for ContainerAttachmentTeardownAdapter {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            WorkloadTeardownProviderObservation::for_command(command, self.execute_network(command))
        })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            WorkloadTeardownProviderObservation::for_command(command, self.inspect_network(command))
        })
    }
}

impl NetworkReleaseCapability for ContainerAttachmentTeardownAdapter {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        <Self as NetworkDetachmentCapability>::execute(self, command)
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        <Self as NetworkDetachmentCapability>::inspect(self, command)
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

fn network_inspect_outcome(
    observation: SandboxNetworkTeardownObservation,
) -> WorkloadTeardownProviderOutcome {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(observation.evidence());
    WorkloadTeardownProviderOutcome::Inspect(match observation {
        SandboxNetworkTeardownObservation::Succeeded { .. } => {
            WorkloadTeardownInspectOutcome::InProgress(evidence)
        }
        SandboxNetworkTeardownObservation::DefiniteFailure { code, .. } => {
            WorkloadTeardownInspectOutcome::DefiniteFailure(provider_failure(&code, evidence))
        }
        SandboxNetworkTeardownObservation::Absent { .. }
        | SandboxNetworkTeardownObservation::RetryAuthorized { .. } => {
            WorkloadTeardownInspectOutcome::NotCompleted(evidence)
        }
        SandboxNetworkTeardownObservation::InProgress { .. } => {
            WorkloadTeardownInspectOutcome::InProgress(evidence)
        }
        SandboxNetworkTeardownObservation::Ambiguous { .. } => {
            WorkloadTeardownInspectOutcome::Ambiguous
        }
    })
}

fn network_preflight_failure_outcome(
    command: &ConfirmedWorkloadTeardownCommand,
    observation: SandboxNetworkTeardownObservation,
) -> WorkloadTeardownProviderOutcome {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(observation.evidence());
    match (command.mode(), observation) {
        (
            WorkloadTeardownCommandMode::Execute,
            SandboxNetworkTeardownObservation::DefiniteFailure { code, .. },
        ) => WorkloadTeardownProviderOutcome::Execute(
            WorkloadTeardownExecuteOutcome::DefiniteFailure(provider_failure(&code, evidence)),
        ),
        (
            WorkloadTeardownCommandMode::Inspect,
            SandboxNetworkTeardownObservation::DefiniteFailure { code, .. },
        ) => WorkloadTeardownProviderOutcome::Inspect(
            WorkloadTeardownInspectOutcome::DefiniteFailure(provider_failure(&code, evidence)),
        ),
        (
            WorkloadTeardownCommandMode::Execute,
            SandboxNetworkTeardownObservation::Ambiguous { .. },
        ) => WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous),
        (
            WorkloadTeardownCommandMode::Inspect,
            SandboxNetworkTeardownObservation::Ambiguous { .. },
        ) => WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous),
        (WorkloadTeardownCommandMode::Execute, _) => {
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
        }
        (WorkloadTeardownCommandMode::Inspect, _) => {
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous)
        }
    }
}

fn network_observation_kind(
    observation: &SandboxNetworkTeardownObservation,
) -> ProviderCommandObservationKind {
    match observation {
        SandboxNetworkTeardownObservation::Succeeded { .. } => {
            ProviderCommandObservationKind::Succeeded
        }
        SandboxNetworkTeardownObservation::DefiniteFailure { .. } => {
            ProviderCommandObservationKind::DefiniteFailure
        }
        SandboxNetworkTeardownObservation::Absent { .. } => ProviderCommandObservationKind::Absent,
        SandboxNetworkTeardownObservation::RetryAuthorized { .. } => {
            ProviderCommandObservationKind::RetryAuthorized
        }
        SandboxNetworkTeardownObservation::InProgress { .. } => {
            ProviderCommandObservationKind::InProgress
        }
        SandboxNetworkTeardownObservation::Ambiguous { .. } => {
            ProviderCommandObservationKind::Ambiguous
        }
    }
}

fn success_evidence(
    command: &ConfirmedWorkloadTeardownCommand,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadTeardownSuccessEvidence {
    match (command.step(), command.subjects()) {
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence,
            }
        }
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence,
            }
        }
        _ => unreachable!("validated sandbox teardown command has matching subjects"),
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
