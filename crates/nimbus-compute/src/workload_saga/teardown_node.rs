//! Exact translation between compute-confirmed teardown commands and node ports.

use std::sync::Arc;

use nimbus_node::{
    HostExecutionDrainProvider, HostExecutionStopProvider, HostTeardownExecuteClaim,
    HostTeardownExecuteObservation, HostTeardownInspectClaim, HostTeardownInspectObservation,
    HostTeardownProviderClaimInput,
};
use nimbus_workloads::{
    WorkloadExecutionProviderId, WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest,
    WorkloadTeardownProviderTarget, WorkloadTeardownSubjects,
};

use super::{
    ConfirmedWorkloadTeardownCommand, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, WorkloadTeardownCapabilityFuture,
    WorkloadTeardownExecuteOutcome, WorkloadTeardownInspectOutcome,
    WorkloadTeardownProviderObservation, WorkloadTeardownProviderOutcome,
};

/// Compute-side adapter for one admitted node execution provider.
pub struct NodeExecutionTeardownAdapter<Provider> {
    provider_id: WorkloadExecutionProviderId,
    provider: Arc<Provider>,
}

impl<Provider> NodeExecutionTeardownAdapter<Provider> {
    pub fn new(provider_id: WorkloadExecutionProviderId, provider: Arc<Provider>) -> Self {
        Self {
            provider_id,
            provider,
        }
    }

    fn claim_input(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> Result<HostTeardownProviderClaimInput, WorkloadFailureEvidence> {
        let WorkloadTeardownProviderTarget::Execution { provider_id, .. } =
            command.provider_target()
        else {
            return Err(invalid_command_failure(
                "node teardown adapter requires an execution provider target",
            ));
        };
        if provider_id != &self.provider_id {
            return Err(invalid_command_failure(
                "node teardown adapter provider ID is crossed with the admitted registration",
            ));
        }
        let WorkloadTeardownSubjects::Execution(execution) = command.subjects() else {
            return Err(invalid_command_failure(
                "node teardown adapter requires an execution subject",
            ));
        };
        Ok(HostTeardownProviderClaimInput {
            claim: command.claim().clone(),
            command_id: command.command_id(),
            confirmed_revision: command.confirmed_revision(),
            confirmed_transition_id: command.confirmed_transition_id().clone(),
            source: command.source().clone(),
            execution: execution.clone(),
            provider_target: command.provider_target().clone(),
            prior_receipt_prefix: command.prior_receipt_prefix().clone(),
        })
    }

    fn execute_claim(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> Result<HostTeardownExecuteClaim, WorkloadFailureEvidence> {
        HostTeardownExecuteClaim::new(self.claim_input(command)?)
            .map_err(|error| invalid_command_failure(error.to_string()))
    }

    fn inspect_claim(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> Result<HostTeardownInspectClaim, WorkloadFailureEvidence> {
        HostTeardownInspectClaim::new(self.claim_input(command)?)
            .map_err(|error| invalid_command_failure(error.to_string()))
    }
}

impl<Provider> WorkloadExecutionDrainCapability for NodeExecutionTeardownAdapter<Provider>
where
    Provider: HostExecutionDrainProvider + 'static,
{
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            let outcome = match self.execute_claim(command) {
                Ok(claim) => map_execute_observation(self.provider.execute_drain(claim).await),
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
            let outcome = match self.inspect_claim(command) {
                Ok(claim) => map_inspect_observation(self.provider.inspect_drain(claim).await),
                Err(failure) => WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
                ),
            };
            WorkloadTeardownProviderObservation::for_command(command, outcome)
        })
    }
}

impl<Provider> WorkloadExecutionStopCapability for NodeExecutionTeardownAdapter<Provider>
where
    Provider: HostExecutionStopProvider + 'static,
{
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            let outcome = match self.execute_claim(command) {
                Ok(claim) => map_execute_observation(self.provider.execute_stop(claim).await),
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
            let outcome = match self.inspect_claim(command) {
                Ok(claim) => map_inspect_observation(self.provider.inspect_stop(claim).await),
                Err(failure) => WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
                ),
            };
            WorkloadTeardownProviderObservation::for_command(command, outcome)
        })
    }
}

fn map_execute_observation(
    observation: HostTeardownExecuteObservation,
) -> WorkloadTeardownProviderOutcome {
    WorkloadTeardownProviderOutcome::Execute(match observation {
        HostTeardownExecuteObservation::Succeeded(evidence) => {
            WorkloadTeardownExecuteOutcome::Succeeded(evidence)
        }
        HostTeardownExecuteObservation::DefiniteFailure(failure) => {
            WorkloadTeardownExecuteOutcome::DefiniteFailure(failure)
        }
        HostTeardownExecuteObservation::Ambiguous => WorkloadTeardownExecuteOutcome::Ambiguous,
    })
}

fn map_inspect_observation(
    observation: HostTeardownInspectObservation,
) -> WorkloadTeardownProviderOutcome {
    WorkloadTeardownProviderOutcome::Inspect(match observation {
        HostTeardownInspectObservation::Satisfied(evidence) => {
            WorkloadTeardownInspectOutcome::Satisfied(evidence)
        }
        HostTeardownInspectObservation::NotCompleted(evidence) => {
            WorkloadTeardownInspectOutcome::NotCompleted(evidence)
        }
        HostTeardownInspectObservation::DefiniteFailure(failure) => {
            WorkloadTeardownInspectOutcome::DefiniteFailure(failure)
        }
        HostTeardownInspectObservation::InProgress(evidence) => {
            WorkloadTeardownInspectOutcome::InProgress(evidence)
        }
        HostTeardownInspectObservation::Ambiguous => WorkloadTeardownInspectOutcome::Ambiguous,
    })
}

fn invalid_command_failure(message: impl AsRef<str>) -> WorkloadFailureEvidence {
    WorkloadFailureEvidence::new(
        "node_teardown_command_invalid",
        WorkloadOwnerEvidenceDigest::sha256(message.as_ref()),
    )
    .expect("static node teardown failure code is valid")
}

#[cfg(test)]
#[path = "teardown_node/tests.rs"]
mod tests;
