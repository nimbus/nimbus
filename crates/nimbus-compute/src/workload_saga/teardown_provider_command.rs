//! Confirmed teardown commands at the provider-owned attempt journal boundary.

use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandClaimInput, ProviderCommandExecutionClaim, ProviderCommandJournalError,
    ProviderCommandObservation, ProviderCommandObservationKind, ProviderCommandOperation,
};
use nimbus_workloads::{WorkloadTeardownCommandMode, WorkloadTeardownStep};

use super::ConfirmedWorkloadTeardownCommand;

/// Exact provider-journal command derived from a compute-confirmed teardown.
///
/// Provider adapters supply only the provider-local effect subject and target
/// digest. This type owns every portable fence and derives the closed operation
/// from the confirmed step, so a caller cannot substitute another operation.
#[derive(Debug, Clone)]
pub struct ConfirmedTeardownProviderCommand {
    mode: WorkloadTeardownCommandMode,
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
            claim,
        })
    }

    pub const fn mode(&self) -> WorkloadTeardownCommandMode {
        self.mode
    }

    pub fn claim(&self) -> &ProviderCommandClaim {
        &self.claim
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
        self.journal.claim_dispatch_epoch(command.claim())
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
