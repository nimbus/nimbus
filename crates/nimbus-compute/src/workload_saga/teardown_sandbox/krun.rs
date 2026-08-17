//! Exact Krun substitution for execution drain and stop capabilities.

use std::sync::Arc;

use nimbus_network::NetworkProviderId;
use nimbus_sandbox::backends::krun::KrunSandboxBackend;
use nimbus_sandbox::{
    ProviderCommandJournalError, SandboxBackendKind, sandbox_network_plan_requirements,
};
use nimbus_workloads::{WorkloadExecutionProviderId, WorkloadFailureEvidence};

use super::{
    ConfirmedWorkloadTeardownCommand, NetworkAttachmentTeardownCapabilities,
    NetworkDetachmentCapability, NetworkReleaseCapability, ProviderTeardownPhaseAdapter,
    ValidatedSandboxTeardownCommand, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome, attachment, validate_sandbox_teardown_command,
};
use crate::workload_saga::provision_sandbox::sandbox_execution_provider_id;

/// Real Krun adapter for the two execution-only teardown capabilities.
pub struct KrunTeardownAdapter {
    backend: Arc<KrunSandboxBackend>,
    phases: ProviderTeardownPhaseAdapter,
    provider_id: WorkloadExecutionProviderId,
}

impl KrunTeardownAdapter {
    pub fn new(backend: Arc<KrunSandboxBackend>) -> Result<Self, ProviderCommandJournalError> {
        let journal = backend.attempt_idempotency_journal()?;
        Ok(Self {
            backend,
            phases: ProviderTeardownPhaseAdapter::new(journal),
            provider_id: sandbox_execution_provider_id(SandboxBackendKind::Krun),
        })
    }

    pub fn capabilities(self: Arc<Self>) -> WorkloadExecutionTeardownCapabilities {
        WorkloadExecutionTeardownCapabilities::new(self.provider_id.clone(), self.clone(), self)
    }

    fn validated(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> Result<ValidatedSandboxTeardownCommand, WorkloadFailureEvidence> {
        validate_sandbox_teardown_command(command, SandboxBackendKind::Krun)
    }

    fn execute(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderOutcome {
        match self.validated(command) {
            Ok(validated) => self.phases.execute(command, &validated, |execution_claim| {
                self.backend.execute_execution_teardown_with_claim(
                    validated.sandbox_command(),
                    execution_claim,
                )
            }),
            Err(failure) => WorkloadTeardownProviderOutcome::Execute(
                WorkloadTeardownExecuteOutcome::DefiniteFailure(failure),
            ),
        }
    }

    fn inspect(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownProviderOutcome {
        match self.validated(command) {
            Ok(validated) => self.phases.inspect(command, &validated, |observation| {
                self.backend.inspect_execution_teardown_with_observation(
                    validated.sandbox_command(),
                    observation,
                )
            }),
            Err(failure) => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
            ),
        }
    }
}

impl WorkloadExecutionDrainCapability for KrunTeardownAdapter {
    fn execute<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            WorkloadTeardownProviderObservation::for_command(command, self.execute(command))
        })
    }

    fn inspect<'a>(
        &'a self,
        command: &'a ConfirmedWorkloadTeardownCommand,
    ) -> WorkloadTeardownCapabilityFuture<'a> {
        Box::pin(async move {
            WorkloadTeardownProviderObservation::for_command(command, self.inspect(command))
        })
    }
}

impl WorkloadExecutionStopCapability for KrunTeardownAdapter {
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

/// Real Krun substitution for host-managed detach and release.
pub struct KrunAttachmentTeardownAdapter {
    backend: Arc<KrunSandboxBackend>,
    phases: ProviderTeardownPhaseAdapter,
    provider_id: NetworkProviderId,
}

impl KrunAttachmentTeardownAdapter {
    pub fn new(backend: Arc<KrunSandboxBackend>) -> Result<Self, ProviderCommandJournalError> {
        let journal = backend.attempt_idempotency_journal()?;
        Ok(Self {
            backend,
            phases: ProviderTeardownPhaseAdapter::new(journal),
            provider_id: sandbox_network_plan_requirements(SandboxBackendKind::Krun)
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
        attachment::validate_sandbox_network_teardown_command(command, SandboxBackendKind::Krun)
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
                Err(observation) => super::network_preflight_failure_outcome(command, observation),
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
                Err(observation) => super::network_preflight_failure_outcome(command, observation),
            },
            Err(failure) => WorkloadTeardownProviderOutcome::Inspect(
                WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
            ),
        }
    }
}

impl NetworkDetachmentCapability for KrunAttachmentTeardownAdapter {
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

impl NetworkReleaseCapability for KrunAttachmentTeardownAdapter {
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
