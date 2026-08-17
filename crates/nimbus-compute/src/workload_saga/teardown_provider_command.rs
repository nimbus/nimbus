//! Confirmed teardown commands at the provider-owned attempt journal boundary.

use std::future::Future;
use std::pin::Pin;

use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandCurrentExecution, ProviderCommandCurrentInspection,
    ProviderCommandExecutionClaim, ProviderCommandJournalError, ProviderCommandObservation,
    ProviderCommandObservationKind, ProviderCommandOperation, ProviderCommandStartedClaimDecision,
    ProviderCommandStartedExecutionClaim,
};
use nimbus_workloads::{
    WorkloadTeardownCommandMode, WorkloadTeardownDispatchAuthorization, WorkloadTeardownStep,
};

use super::ConfirmedWorkloadTeardownCommand;

/// Exact provider-journal command derived from a compute-confirmed teardown.
///
/// Provider adapters supply only the provider-local effect subject and target
/// digest. This type owns every portable fence and derives the closed operation
/// from the confirmed step, so a caller cannot substitute another operation.
#[derive(Debug, Clone)]
pub struct ConfirmedTeardownProviderCommand {
    mode: WorkloadTeardownCommandMode,
    authorization: WorkloadTeardownDispatchAuthorization,
    claim: ProviderCommandClaim,
}

impl ConfirmedTeardownProviderCommand {
    pub fn new(
        command: &ConfirmedWorkloadTeardownCommand,
        effect_subject: String,
        provider_target_digest: String,
    ) -> Result<Self, ProviderCommandJournalError> {
        let claim = ProviderCommandClaim::new(ProviderCommandClaimInput {
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
            provider_target_digest,
            operation: provider_operation(command.step()),
        })?;
        Ok(Self {
            mode: command.mode(),
            authorization: command.claim().authorization().clone(),
            claim,
        })
    }

    pub const fn mode(&self) -> WorkloadTeardownCommandMode {
        self.mode
    }

    pub fn claim(&self) -> &ProviderCommandClaim {
        &self.claim
    }

    fn authorization(&self) -> &WorkloadTeardownDispatchAuthorization {
        &self.authorization
    }
}

/// Injected access to one provider-owned journal for confirmed teardown work.
///
/// This seam never opens a state root or chooses a provider namespace. The
/// concrete provider remains the sole owner of that durable authority.
#[derive(Debug, Clone)]
pub struct ConfirmedTeardownProviderJournal {
    journal: ProviderCommandAttemptJournal,
}

impl ConfirmedTeardownProviderJournal {
    pub fn new(journal: ProviderCommandAttemptJournal) -> Self {
        Self { journal }
    }

    /// Claim Execute authority or adopt the exact existing attempt.
    pub fn claim_execute(
        &self,
        command: &ConfirmedTeardownProviderCommand,
    ) -> Result<ProviderCommandClaimDecision, ProviderCommandJournalError> {
        require_mode(command, WorkloadTeardownCommandMode::Execute)?;
        match command.authorization() {
            WorkloadTeardownDispatchAuthorization::Initial => {
                self.journal.claim_dispatch_epoch(command.claim())
            }
            WorkloadTeardownDispatchAuthorization::RetryAfterNotCompleted(evidence) => {
                let inspection_evidence = serde_json::to_vec(evidence).map_err(|error| {
                    ProviderCommandJournalError::InvalidClaim {
                        message: format!(
                            "failed to encode confirmed teardown retry evidence: {error}"
                        ),
                    }
                })?;
                self.journal.claim_dispatch_epoch_after_inspected_absence(
                    command.claim(),
                    evidence.dispatch_epoch().as_u64(),
                    &inspection_evidence,
                )
            }
        }
    }

    /// Atomically claim Execute authority with its exact prepared request.
    pub fn claim_execute_started(
        &self,
        command: &ConfirmedTeardownProviderCommand,
        prepared_request: &[u8],
    ) -> Result<ProviderCommandStartedClaimDecision, ProviderCommandJournalError> {
        require_mode(command, WorkloadTeardownCommandMode::Execute)?;
        self.journal
            .claim_dispatch_epoch_started(command.claim(), prepared_request)
    }

    /// Inspect only an exact command that compute confirmed in Inspect mode.
    pub fn adopt_inspect(
        &self,
        command: &ConfirmedTeardownProviderCommand,
    ) -> Result<Option<ProviderCommandObservation>, ProviderCommandJournalError> {
        require_mode(command, WorkloadTeardownCommandMode::Inspect)?;
        self.journal.adopt_exact_attempt(command.claim())
    }

    /// Recover effect authority for the current exact Execute claim.
    pub fn resume_current_claim(
        &self,
        command: &ConfirmedTeardownProviderCommand,
        observation: &ProviderCommandObservation,
    ) -> Result<ProviderCommandExecutionClaim, ProviderCommandJournalError> {
        require_mode(command, WorkloadTeardownCommandMode::Execute)?;
        authenticate_observation(command, observation)?;
        self.journal.resume_current_claim(observation)
    }

    /// Run one read-only inspection while the exact stream stays current.
    pub fn inspect_current_claim<T>(
        &self,
        command: &ConfirmedTeardownProviderCommand,
        observation: &ProviderCommandObservation,
        inspect: impl FnOnce(&ProviderCommandObservation) -> T,
    ) -> Result<T, ProviderCommandJournalError> {
        authenticate_observation(command, observation)?;
        self.journal.inspect_current_claim(observation, inspect)
    }

    /// Send one atomically prepared request while its exact stream stays current.
    pub async fn execute_started_claim_async<T, Execute>(
        &self,
        command: &ConfirmedTeardownProviderCommand,
        execution_claim: ProviderCommandStartedExecutionClaim,
        execute: Execute,
    ) -> Result<(T, ProviderCommandObservation), ProviderCommandJournalError>
    where
        T: Send + 'static,
        Execute: for<'a> FnOnce(
                &'a ProviderCommandCurrentExecution,
            ) -> Pin<
                Box<
                    dyn Future<
                            Output = (T, ProviderCommandObservationKind, Option<String>, Vec<u8>),
                        > + Send
                        + 'a,
                >,
            > + Send
            + 'static,
    {
        require_mode(command, WorkloadTeardownCommandMode::Execute)?;
        if execution_claim.claim() != command.claim() {
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        self.journal
            .execute_started_claim_async(execution_claim, execute)
            .await
    }

    /// Run one parent-local provider effect under its current Execute claim.
    pub async fn execute_current_claim_async<T, Execute>(
        &self,
        command: &ConfirmedTeardownProviderCommand,
        execution_claim: ProviderCommandExecutionClaim,
        execute: Execute,
    ) -> Result<(T, ProviderCommandObservation), ProviderCommandJournalError>
    where
        T: Send + 'static,
        Execute: for<'a> FnOnce(
                &'a ProviderCommandCurrentExecution,
            ) -> Pin<
                Box<
                    dyn Future<
                            Output = (T, ProviderCommandObservationKind, Option<String>, Vec<u8>),
                        > + Send
                        + 'a,
                >,
            > + Send
            + 'static,
    {
        require_mode(command, WorkloadTeardownCommandMode::Execute)?;
        if execution_claim.claim() != command.claim() {
            return Err(ProviderCommandJournalError::CrossedClaim);
        }
        self.journal
            .execute_current_claim_async(execution_claim, execute)
            .await
    }

    /// Await read-only provider inspection while the exact stream stays current.
    pub async fn inspect_current_claim_async<T, Inspect>(
        &self,
        command: &ConfirmedTeardownProviderCommand,
        observation: &ProviderCommandObservation,
        inspect: Inspect,
    ) -> Result<ProviderCommandCurrentInspection<T>, ProviderCommandJournalError>
    where
        Inspect: for<'a> FnOnce(
                &'a ProviderCommandObservation,
            ) -> Pin<Box<dyn Future<Output = T> + Send + 'a>>
            + Send,
    {
        authenticate_observation(command, observation)?;
        self.journal
            .inspect_current_claim_async(observation, inspect)
            .await
    }

    /// Inspect remotely and publish the correlated result under one stream lock.
    pub async fn inspect_current_claim_async_and_publish<T, Inspect>(
        &self,
        command: &ConfirmedTeardownProviderCommand,
        observation: &ProviderCommandObservation,
        inspect: Inspect,
    ) -> Result<(T, ProviderCommandObservation), ProviderCommandJournalError>
    where
        T: Send + 'static,
        Inspect: for<'a> FnOnce(
                &'a ProviderCommandObservation,
            ) -> Pin<
                Box<
                    dyn Future<
                            Output = (T, ProviderCommandObservationKind, Option<String>, Vec<u8>),
                        > + Send
                        + 'a,
                >,
            > + Send
            + 'static,
    {
        authenticate_observation(command, observation)?;
        self.journal
            .inspect_current_claim_async_and_publish(observation, inspect)
            .await
    }

    /// Persist one exact provider observation for this confirmed command.
    pub fn record_observation_with_failure_code(
        &self,
        command: &ConfirmedTeardownProviderCommand,
        kind: ProviderCommandObservationKind,
        failure_code: Option<&str>,
        evidence: &[u8],
    ) -> Result<ProviderCommandObservation, ProviderCommandJournalError> {
        self.journal.record_observation_with_failure_code(
            command.claim(),
            kind,
            failure_code,
            evidence,
        )
    }
}

const fn provider_operation(step: WorkloadTeardownStep) -> ProviderCommandOperation {
    match step {
        WorkloadTeardownStep::WithdrawPublication => {
            ProviderCommandOperation::WithdrawFinalPublication
        }
        WorkloadTeardownStep::DrainExecution => ProviderCommandOperation::DrainExecution,
        WorkloadTeardownStep::StopExecution => ProviderCommandOperation::StopExecution,
        WorkloadTeardownStep::DetachNetwork => ProviderCommandOperation::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork => ProviderCommandOperation::ReleaseNetwork,
    }
}

fn require_mode(
    command: &ConfirmedTeardownProviderCommand,
    required: WorkloadTeardownCommandMode,
) -> Result<(), ProviderCommandJournalError> {
    if command.mode() == required {
        return Ok(());
    }
    Err(ProviderCommandJournalError::InvalidClaim {
        message: format!(
            "confirmed teardown provider journal requires {required:?} mode, got {:?}",
            command.mode()
        ),
    })
}

fn authenticate_observation(
    command: &ConfirmedTeardownProviderCommand,
    observation: &ProviderCommandObservation,
) -> Result<(), ProviderCommandJournalError> {
    if observation.claim() == command.claim() {
        return Ok(());
    }
    Err(ProviderCommandJournalError::CrossedClaim)
}

#[cfg(test)]
#[path = "teardown_provider_command/tests.rs"]
mod tests;
