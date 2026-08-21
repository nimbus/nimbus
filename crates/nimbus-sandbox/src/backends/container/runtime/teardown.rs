//! Exact Container execution drain and stop state machine.
//!
//! This module owns execution-only teardown. It does not detach the network,
//! stop the PEP, release a listener, or release any allocation authority.

use std::time::Duration;

use nimbus_core::TenantId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::backends::conmon::lifecycle::read_exit_receipt;
use crate::backends::conmon::runtime_process::{
    RuntimeProcessIdentityObservation, RuntimeProcessSignal, RuntimeProcessSignalOutcome,
};
use crate::{
    ProviderCommandClaim, ProviderCommandCurrentExecution, ProviderCommandExecutionClaim,
    ProviderCommandJournalError, ProviderCommandObservation, ProviderCommandObservationKind,
    ProviderCommandOperation, SandboxError, SandboxExecutionAttemptId,
    SandboxExecutionTeardownCommand, SandboxExecutionTeardownObservation,
    SandboxExecutionTeardownOperation, SandboxId,
};

use super::manifest::{ContainerCreatorHandoffState, ContainerSandboxManifest};
use super::{ContainerSandboxBackend, ContainerStartMode};

pub(super) mod effects;
pub(super) mod state;

use effects::ContainerExecutionTeardownRuntime;
use state::{ContainerDrainProgress, ContainerStopProgress};

/// Provider registration key for the Container execution-teardown child.
pub const CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY: &str = "nimbus-sandbox.container-execution";
const KILL_REDELIVERY_DELAY: Duration = Duration::from_secs(1);
const HOST_TERMINAL_EVIDENCE_DOMAIN: &[u8] =
    b"nimbus.sandbox.container.host-terminal-evidence.v1\0";

#[derive(Clone, Copy)]
enum TeardownCaller {
    ExecuteAdapter,
    CompositeSubstep,
}

/// Exact Systemd terminal evidence admitted by the guest composition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerHostTerminalEvidence {
    tenant_id: TenantId,
    sandbox_id: SandboxId,
    execution_attempt_id: SandboxExecutionAttemptId,
    provider_claim: ProviderCommandClaim,
    evidence_sha256: String,
}

impl ContainerHostTerminalEvidence {
    /// Construct evidence for one exact host-terminal execution.
    pub fn new(
        tenant_id: TenantId,
        sandbox_id: SandboxId,
        execution_attempt_id: SandboxExecutionAttemptId,
        provider_claim: ProviderCommandClaim,
        evidence: Vec<u8>,
    ) -> crate::Result<Self> {
        if provider_claim.operation() != ProviderCommandOperation::StopExecution {
            return Err(SandboxError::InvalidSpec {
                message: "Container host terminal evidence requires a stop claim".to_owned(),
            });
        }
        if evidence.is_empty() {
            return Err(SandboxError::InvalidSpec {
                message: "Container host terminal evidence cannot be empty".to_owned(),
            });
        }
        let mut hasher = Sha256::new();
        hasher.update(HOST_TERMINAL_EVIDENCE_DOMAIN);
        hasher.update(&evidence);
        Ok(Self {
            tenant_id,
            sandbox_id,
            execution_attempt_id,
            provider_claim,
            evidence_sha256: format!("{:x}", hasher.finalize()),
        })
    }

    pub fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    pub fn execution_attempt_id(&self) -> &SandboxExecutionAttemptId {
        &self.execution_attempt_id
    }

    pub fn provider_claim(&self) -> &ProviderCommandClaim {
        &self.provider_claim
    }

    /// Domain-separated digest of the exact serialized Systemd evidence.
    pub fn evidence_sha256(&self) -> &str {
        &self.evidence_sha256
    }

    fn authenticates(&self, command: &SandboxExecutionTeardownCommand) -> bool {
        self.tenant_id() == command.tenant_id()
            && self.sandbox_id() == command.sandbox_id()
            && self.execution_attempt_id() == command.execution_attempt_id()
            && self.provider_claim() == command.provider_claim()
    }
}

impl ContainerSandboxBackend {
    /// Execute one exact drain or stop transition without provider authority.
    #[cfg(test)]
    fn execute_execution_teardown(
        &self,
        command: &SandboxExecutionTeardownCommand,
    ) -> SandboxExecutionTeardownObservation {
        match self.execute_execution_teardown_inner(command) {
            Ok(observation) => observation,
            Err(error @ SandboxError::InvalidSpec { .. }) => {
                definite_failure("sandbox_teardown_command_crossed", error.to_string())
            }
            Err(error @ SandboxError::NotFound { .. }) => {
                ambiguous(format!("exact Container manifest is absent: {error}"))
            }
            Err(error) => ambiguous(error.to_string()),
        }
    }

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
                message: "Container execution authorization crossed its provider command"
                    .to_owned(),
            });
        }
        let journal = self.attempt_idempotency_journal()?;
        let (_, provider_observation) =
            journal.execute_current_claim(execution_claim, |current_claim| {
                let sandbox_observation = execution_result(
                    self.execute_execution_teardown_inner_with_runtime_and_authorization_for_caller(
                        command,
                        self.teardown_runtime_provider.as_ref(),
                        Some(current_claim.observation()),
                        TeardownCaller::ExecuteAdapter,
                    ),
                );
                let kind = execution_observation_kind(&sandbox_observation);
                let failure_code = sandbox_observation.failure_code().map(str::to_owned);
                let evidence = sandbox_observation.evidence().to_vec();
                (sandbox_observation, kind, failure_code, evidence)
            })?;
        Ok(provider_observation)
    }

    /// Run one Container child effect under a caller-owned exact stream lock.
    ///
    /// This method never opens or publishes the provider journal. The caller
    /// retains the generic result authority until all sibling effects settle.
    #[doc(hidden)]
    pub fn execute_execution_teardown_substep(
        &self,
        command: &SandboxExecutionTeardownCommand,
        current_execution: &ProviderCommandCurrentExecution,
    ) -> SandboxExecutionTeardownObservation {
        if !current_execution.authenticates(command.provider_claim()) {
            return definite_failure(
                "sandbox_teardown_command_crossed",
                "Container execution authorization crossed its provider command",
            );
        }
        execution_result(
            self.execute_execution_teardown_inner_with_runtime_and_authorization_for_caller(
                command,
                self.teardown_runtime_provider.as_ref(),
                Some(current_execution.observation()),
                TeardownCaller::CompositeSubstep,
            ),
        )
    }

    /// Record an exact externally stopped PlanOnly execution.
    ///
    /// The guest composition must first prove Systemd unit absence. This
    /// method independently inspects Container runtime terminality and never
    /// sends a signal or publishes the generic provider result.
    #[doc(hidden)]
    pub fn record_externally_stopped_execution_substep(
        &self,
        command: &SandboxExecutionTeardownCommand,
        current_execution: &ProviderCommandCurrentExecution,
        host_terminal: &ContainerHostTerminalEvidence,
    ) -> SandboxExecutionTeardownObservation {
        self.record_externally_stopped_execution_substep_with_runtime_inner(
            command,
            current_execution,
            host_terminal,
            self.teardown_runtime_provider.as_ref(),
        )
    }

    #[cfg(test)]
    fn record_externally_stopped_execution_substep_with_runtime(
        &self,
        command: &SandboxExecutionTeardownCommand,
        current_execution: &ProviderCommandCurrentExecution,
        host_terminal: &ContainerHostTerminalEvidence,
        runtime: &dyn ContainerExecutionTeardownRuntime,
    ) -> SandboxExecutionTeardownObservation {
        self.record_externally_stopped_execution_substep_with_runtime_inner(
            command,
            current_execution,
            host_terminal,
            runtime,
        )
    }

    fn record_externally_stopped_execution_substep_with_runtime_inner(
        &self,
        command: &SandboxExecutionTeardownCommand,
        current_execution: &ProviderCommandCurrentExecution,
        host_terminal: &ContainerHostTerminalEvidence,
        runtime: &dyn ContainerExecutionTeardownRuntime,
    ) -> SandboxExecutionTeardownObservation {
        if !current_execution.authenticates(command.provider_claim())
            || !host_terminal.authenticates(command)
        {
            return definite_failure(
                "sandbox_teardown_command_crossed",
                "Container externally stopped evidence crossed its provider command",
            );
        }
        execution_result(self.record_externally_stopped_execution_inner(
            command,
            current_execution.observation(),
            host_terminal.evidence_sha256(),
            runtime,
        ))
    }

    fn record_externally_stopped_execution_inner(
        &self,
        command: &SandboxExecutionTeardownCommand,
        journal_authorization: &ProviderCommandObservation,
        host_terminal_evidence_sha256: &str,
        runtime: &dyn ContainerExecutionTeardownRuntime,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        if self.config.start_mode != ContainerStartMode::PlanOnly
            || command.operation() != SandboxExecutionTeardownOperation::Stop
        {
            return Err(SandboxError::InvalidSpec {
                message: "Container externally stopped execution requires a PlanOnly stop substep"
                    .to_owned(),
            });
        }
        self.validate_teardown_provider_command_shape(command)?;
        let Some(snapshot) = self.read_manifest(command.sandbox_id())? else {
            return Err(SandboxError::NotFound {
                sandbox_id: command.sandbox_id().as_str().to_owned(),
            });
        };
        let (_guard, mut manifest) =
            super::runner::lock_current_provision_lifecycle_for_backend(self, &snapshot)?;
        self.authenticate_teardown_mode(&manifest)?;
        self.authenticate_teardown_manifest(command, &manifest)?;
        require_matching_drain(&manifest, command.provider_claim())?;
        match manifest.execution_teardown.stop() {
            ContainerStopProgress::ExecutionStopped { fence, evidence }
                if fence == command.provider_claim() =>
            {
                authenticate_host_terminal_binding(evidence, host_terminal_evidence_sha256)?;
                return Ok(succeeded(evidence.clone()));
            }
            ContainerStopProgress::ExecutionStopped { fence, .. }
                if journal_authorizes_older_progress(
                    fence,
                    command.provider_claim(),
                    Some(journal_authorization),
                ) => {}
            ContainerStopProgress::NotRequested => {}
            _ => return Err(crossed_progress("externally stopped execution", &manifest)),
        }
        if !runtime.execution_is_terminal(&manifest)? {
            return Ok(in_progress(
                "Systemd is terminal, but exact Container runtime remains live",
            ));
        }
        manifest.shutdown_requested = true;
        self.persist_execution_stopped_with_host_terminal(
            command.provider_claim(),
            &mut manifest,
            "container_execution_externally_stopped",
            Some(host_terminal_evidence_sha256),
        )
    }

    /// Inspect one externally stopped PlanOnly child without an effect or write.
    ///
    /// The caller must hold the exact generic provider stream inspection lock
    /// and supply the exact current Systemd terminal evidence. A live runtime
    /// remains in progress. Terminality without a durable Container fence is
    /// an exact absent child effect that authorizes an adjacent Execute.
    #[doc(hidden)]
    pub fn inspect_externally_stopped_execution_substep(
        &self,
        command: &SandboxExecutionTeardownCommand,
        provider_observation: &ProviderCommandObservation,
        host_terminal: &ContainerHostTerminalEvidence,
    ) -> SandboxExecutionTeardownObservation {
        self.inspect_externally_stopped_execution_substep_with_runtime_inner(
            command,
            provider_observation,
            host_terminal,
            self.teardown_runtime_provider.as_ref(),
        )
    }

    #[cfg(test)]
    fn inspect_externally_stopped_execution_substep_with_runtime(
        &self,
        command: &SandboxExecutionTeardownCommand,
        provider_observation: &ProviderCommandObservation,
        host_terminal: &ContainerHostTerminalEvidence,
        runtime: &dyn ContainerExecutionTeardownRuntime,
    ) -> SandboxExecutionTeardownObservation {
        self.inspect_externally_stopped_execution_substep_with_runtime_inner(
            command,
            provider_observation,
            host_terminal,
            runtime,
        )
    }

    fn inspect_externally_stopped_execution_substep_with_runtime_inner(
        &self,
        command: &SandboxExecutionTeardownCommand,
        provider_observation: &ProviderCommandObservation,
        host_terminal: &ContainerHostTerminalEvidence,
        runtime: &dyn ContainerExecutionTeardownRuntime,
    ) -> SandboxExecutionTeardownObservation {
        if provider_observation.claim() != command.provider_claim()
            || !matches!(
                provider_observation.kind(),
                ProviderCommandObservationKind::Claimed
                    | ProviderCommandObservationKind::InProgress
                    | ProviderCommandObservationKind::Ambiguous
            )
            || !host_terminal.authenticates(command)
        {
            return definite_failure(
                "sandbox_teardown_command_crossed",
                "Container externally stopped inspection crossed its provider command",
            );
        }
        execution_result(self.inspect_externally_stopped_execution_inner(
            command,
            provider_observation,
            host_terminal.evidence_sha256(),
            runtime,
        ))
    }

    fn inspect_externally_stopped_execution_inner(
        &self,
        command: &SandboxExecutionTeardownCommand,
        journal_authorization: &ProviderCommandObservation,
        host_terminal_evidence_sha256: &str,
        runtime: &dyn ContainerExecutionTeardownRuntime,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        if self.config.start_mode != ContainerStartMode::PlanOnly
            || command.operation() != SandboxExecutionTeardownOperation::Stop
        {
            return Err(SandboxError::InvalidSpec {
                message: "Container externally stopped inspection requires a PlanOnly stop substep"
                    .to_owned(),
            });
        }
        self.validate_teardown_provider_command_shape(command)?;
        let Some(snapshot) = self.read_manifest(command.sandbox_id())? else {
            return Err(SandboxError::NotFound {
                sandbox_id: command.sandbox_id().as_str().to_owned(),
            });
        };
        let (_guard, manifest) =
            super::runner::lock_current_inspection_for_backend(self, &snapshot)?;
        self.authenticate_teardown_mode(&manifest)?;
        self.authenticate_teardown_manifest(command, &manifest)?;
        require_matching_drain(&manifest, command.provider_claim())?;
        match manifest.execution_teardown.stop() {
            ContainerStopProgress::ExecutionStopped { fence, evidence }
                if fence == command.provider_claim()
                    || journal_authorizes_older_progress(
                        fence,
                        command.provider_claim(),
                        Some(journal_authorization),
                    ) =>
            {
                authenticate_host_terminal_binding(evidence, host_terminal_evidence_sha256)?;
                Ok(succeeded(evidence.clone()))
            }
            ContainerStopProgress::NotRequested => {
                if runtime.execution_is_terminal(&manifest)? {
                    Ok(absent(
                        "exact Container runtime is terminal and no Container stop effect is durable",
                    ))
                } else {
                    Ok(in_progress(
                        "Systemd is terminal, but exact Container runtime remains live",
                    ))
                }
            }
            _ => Err(crossed_progress(
                "externally stopped execution inspection",
                &manifest,
            )),
        }
    }

    /// Inspect exact drain or stop progress without a provider effect or write.
    pub fn inspect_execution_teardown(
        &self,
        command: &SandboxExecutionTeardownCommand,
    ) -> SandboxExecutionTeardownObservation {
        match self.inspect_execution_teardown_inner(command) {
            Ok(observation) => observation,
            Err(error @ SandboxError::InvalidSpec { .. }) => {
                definite_failure("sandbox_teardown_command_crossed", error.to_string())
            }
            Err(error @ SandboxError::NotFound { .. }) => {
                ambiguous(format!("exact Container manifest is absent: {error}"))
            }
            Err(error) => ambiguous(error.to_string()),
        }
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
                "Container inspection authorization crossed its provider command",
            );
        }
        match self.inspect_execution_teardown_inner_with_runtime_and_authorization_for_caller(
            command,
            self.teardown_runtime_provider.as_ref(),
            Some(provider_observation),
            TeardownCaller::ExecuteAdapter,
        ) {
            Ok(observation) => observation,
            Err(error @ SandboxError::InvalidSpec { .. }) => {
                definite_failure("sandbox_teardown_command_crossed", error.to_string())
            }
            Err(error @ SandboxError::NotFound { .. }) => {
                ambiguous(format!("exact Container manifest is absent: {error}"))
            }
            Err(error) => ambiguous(error.to_string()),
        }
    }

    /// Inspect one Container child without publishing the generic result.
    #[doc(hidden)]
    pub fn inspect_execution_teardown_substep(
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
                "Container inspection authorization crossed its provider command",
            );
        }
        match self.inspect_execution_teardown_inner_with_runtime_and_authorization_for_caller(
            command,
            self.teardown_runtime_provider.as_ref(),
            Some(provider_observation),
            TeardownCaller::CompositeSubstep,
        ) {
            Ok(observation) => observation,
            Err(error @ SandboxError::InvalidSpec { .. }) => {
                definite_failure("sandbox_teardown_command_crossed", error.to_string())
            }
            Err(error @ SandboxError::NotFound { .. }) => {
                ambiguous(format!("exact Container manifest is absent: {error}"))
            }
            Err(error) => ambiguous(error.to_string()),
        }
    }

    #[cfg(test)]
    fn execute_execution_teardown_inner(
        &self,
        command: &SandboxExecutionTeardownCommand,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.execute_execution_teardown_inner_with_runtime(
            command,
            self.teardown_runtime_provider.as_ref(),
        )
    }

    #[cfg(test)]
    fn execute_execution_teardown_inner_with_runtime(
        &self,
        command: &SandboxExecutionTeardownCommand,
        runtime: &dyn ContainerExecutionTeardownRuntime,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.execute_execution_teardown_inner_with_runtime_and_authorization(command, runtime, None)
    }

    #[cfg(test)]
    fn execute_execution_teardown_inner_with_runtime_and_authorization(
        &self,
        command: &SandboxExecutionTeardownCommand,
        runtime: &dyn ContainerExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.execute_execution_teardown_inner_with_runtime_and_authorization_for_caller(
            command,
            runtime,
            journal_authorization,
            TeardownCaller::ExecuteAdapter,
        )
    }

    fn execute_execution_teardown_inner_with_runtime_and_authorization_for_caller(
        &self,
        command: &SandboxExecutionTeardownCommand,
        runtime: &dyn ContainerExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
        caller: TeardownCaller,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.validate_teardown_command_shape(command, caller)?;
        let Some(snapshot) = self.read_manifest(command.sandbox_id())? else {
            return Err(SandboxError::NotFound {
                sandbox_id: command.sandbox_id().as_str().to_owned(),
            });
        };
        let (_guard, mut manifest) = match caller {
            TeardownCaller::CompositeSubstep
                if self.config.start_mode == ContainerStartMode::PlanOnly =>
            {
                super::runner::lock_current_provision_lifecycle_for_backend(self, &snapshot)?
            }
            TeardownCaller::ExecuteAdapter | TeardownCaller::CompositeSubstep => {
                super::runner::lock_current_execute_lifecycle_for_backend(self, &snapshot)?
            }
        };
        self.authenticate_teardown_mode(&manifest)?;
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

    fn inspect_execution_teardown_inner(
        &self,
        command: &SandboxExecutionTeardownCommand,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.inspect_execution_teardown_inner_with_runtime(
            command,
            self.teardown_runtime_provider.as_ref(),
        )
    }

    fn inspect_execution_teardown_inner_with_runtime(
        &self,
        command: &SandboxExecutionTeardownCommand,
        runtime: &dyn ContainerExecutionTeardownRuntime,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.inspect_execution_teardown_inner_with_runtime_and_authorization(command, runtime, None)
    }

    fn inspect_execution_teardown_inner_with_runtime_and_authorization(
        &self,
        command: &SandboxExecutionTeardownCommand,
        runtime: &dyn ContainerExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.inspect_execution_teardown_inner_with_runtime_and_authorization_for_caller(
            command,
            runtime,
            journal_authorization,
            TeardownCaller::ExecuteAdapter,
        )
    }

    fn inspect_execution_teardown_inner_with_runtime_and_authorization_for_caller(
        &self,
        command: &SandboxExecutionTeardownCommand,
        runtime: &dyn ContainerExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
        caller: TeardownCaller,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.validate_teardown_command_shape(command, caller)?;
        let Some(snapshot) = self.read_manifest(command.sandbox_id())? else {
            return Err(SandboxError::NotFound {
                sandbox_id: command.sandbox_id().as_str().to_owned(),
            });
        };
        let (_guard, manifest) =
            super::runner::lock_current_inspection_for_backend(self, &snapshot)?;
        self.authenticate_teardown_mode(&manifest)?;
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
        caller: TeardownCaller,
    ) -> crate::Result<()> {
        match (caller, self.config.start_mode, command.operation()) {
            (TeardownCaller::ExecuteAdapter, ContainerStartMode::PlanOnly, _) => {
                return Err(SandboxError::InvalidSpec {
                    message: "Container execution teardown requires an Execute backend".to_owned(),
                });
            }
            (
                TeardownCaller::CompositeSubstep,
                ContainerStartMode::PlanOnly,
                SandboxExecutionTeardownOperation::Stop,
            ) => {
                return Err(SandboxError::InvalidSpec {
                    message: "Container PlanOnly composite teardown permits drain only".to_owned(),
                });
            }
            _ => {}
        }
        self.validate_teardown_provider_command_shape(command)
    }

    fn validate_teardown_provider_command_shape(
        &self,
        command: &SandboxExecutionTeardownCommand,
    ) -> crate::Result<()> {
        if command.provider_registration_key() != CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "Container teardown provider key {:?} is crossed with {:?}",
                    command.provider_registration_key(),
                    CONTAINER_EXECUTION_TEARDOWN_PROVIDER_KEY
                ),
            });
        }
        if command.provider_claim().operation() != command.operation().provider_operation() {
            return Err(SandboxError::InvalidSpec {
                message: "Container teardown command operation is crossed with its provider claim"
                    .to_owned(),
            });
        }
        Ok(())
    }

    fn authenticate_teardown_mode(&self, manifest: &ContainerSandboxManifest) -> crate::Result<()> {
        if manifest.start_mode != self.config.start_mode {
            return Err(SandboxError::InvalidSpec {
                message: format!(
                    "Container teardown backend mode {:?} is crossed with manifest mode {:?}",
                    self.config.start_mode, manifest.start_mode
                ),
            });
        }
        Ok(())
    }

    fn authenticate_teardown_manifest(
        &self,
        command: &SandboxExecutionTeardownCommand,
        manifest: &ContainerSandboxManifest,
    ) -> crate::Result<()> {
        if &manifest.spec.tenant_id != command.tenant_id()
            || &manifest.handle.id != command.sandbox_id()
        {
            return Err(SandboxError::InvalidSpec {
                message: "Container teardown command crossed tenant or sandbox identity".to_owned(),
            });
        }
        manifest.require_execution_attempt(
            command.execution_attempt_id(),
            "Container execution teardown",
        )?;
        let plan =
            manifest
                .provision_network_plan
                .as_ref()
                .ok_or_else(|| SandboxError::InvalidSpec {
                    message: format!(
                        "Container execution teardown for {} lacks its exact compiled network plan",
                        manifest.handle.id
                    ),
                })?;
        let claim = command.provider_claim();
        if plan.tenant_id() != command.tenant_id()
            || plan.generation().as_u64() != claim.workload_generation()
            || plan.network_plan().digest().to_string() != claim.network_plan_digest()
        {
            return Err(SandboxError::InvalidSpec {
                message: "Container teardown command crossed durable plan or generation".to_owned(),
            });
        }
        Ok(())
    }

    fn execute_drain(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &mut ContainerSandboxManifest,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        match manifest.execution_teardown.drain() {
            ContainerDrainProgress::Drained { fence, evidence } if fence == claim => {
                return Ok(succeeded(evidence.clone()));
            }
            ContainerDrainProgress::BarrierPersisted { fence } if fence == claim => {}
            ContainerDrainProgress::BarrierPersisted { fence }
                if journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                manifest
                    .execution_teardown
                    .set_drain(ContainerDrainProgress::BarrierPersisted {
                        fence: claim.clone(),
                    });
                self.write_existing_workload_manifest(manifest)?;
            }
            ContainerDrainProgress::Open => {
                manifest
                    .execution_teardown
                    .set_drain(ContainerDrainProgress::BarrierPersisted {
                        fence: claim.clone(),
                    });
                self.write_existing_workload_manifest(manifest)?;
            }
            _ => return Err(crossed_progress("drain", manifest)),
        }

        if let Some(evidence) = admitted_work_evidence(manifest)? {
            return Ok(SandboxExecutionTeardownObservation::InProgress { evidence });
        }
        let evidence = teardown_evidence("container_execution_drained", manifest, claim)?;
        manifest
            .execution_teardown
            .set_drain(ContainerDrainProgress::Drained {
                fence: claim.clone(),
                evidence: evidence.clone(),
            });
        self.write_existing_workload_manifest(manifest)?;
        Ok(succeeded(evidence))
    }

    fn inspect_drain(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &ContainerSandboxManifest,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        match manifest.execution_teardown.drain() {
            ContainerDrainProgress::Open => Ok(absent("drain barrier is not durable")),
            ContainerDrainProgress::BarrierPersisted { fence }
                if fence == claim
                    || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                if let Some(evidence) = admitted_work_evidence(manifest)? {
                    Ok(SandboxExecutionTeardownObservation::InProgress { evidence })
                } else {
                    Ok(absent("drain completion is not durable"))
                }
            }
            ContainerDrainProgress::Drained { fence, evidence }
                if fence == claim
                    || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                Ok(succeeded(evidence.clone()))
            }
            _ => Err(crossed_progress("drain inspection", manifest)),
        }
    }

    fn execute_stop_execution(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &mut ContainerSandboxManifest,
        runtime: &dyn ContainerExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        require_matching_drain(manifest, claim)?;
        let current = manifest.execution_teardown.stop().clone();
        match current {
            ContainerStopProgress::ExecutionStopped { fence, evidence } if fence == *claim => {
                Ok(succeeded(evidence))
            }
            ContainerStopProgress::NotRequested => {
                manifest.shutdown_requested = true;
                super::synchronize_handle_status(
                    manifest,
                    crate::instance::SandboxStatus::Stopping,
                );
                manifest
                    .execution_teardown
                    .set_stop(ContainerStopProgress::IntentPersisted {
                        fence: claim.clone(),
                    });
                self.write_existing_workload_manifest(manifest)?;
                self.advance_stop_after_intent(claim, manifest, runtime)
            }
            ContainerStopProgress::IntentPersisted { fence }
                if same_command_epoch(&fence, claim) =>
            {
                self.advance_stop_after_intent(claim, manifest, runtime)
            }
            ContainerStopProgress::IntentPersisted { fence }
                if journal_authorizes_older_progress(&fence, claim, journal_authorization) =>
            {
                manifest
                    .execution_teardown
                    .set_stop(ContainerStopProgress::IntentPersisted {
                        fence: claim.clone(),
                    });
                self.write_existing_workload_manifest(manifest)?;
                self.advance_stop_after_intent(claim, manifest, runtime)
            }
            ContainerStopProgress::TermMayExist {
                fence,
                process,
                grace_deadline_unix_millis,
            } if journal_authorizes_older_progress(&fence, claim, journal_authorization) => {
                if runtime.execution_is_terminal(manifest)? {
                    return self.persist_execution_stopped(
                        claim,
                        manifest,
                        "term_exit_receipt_observed",
                    );
                }
                match runtime.inspect_process(manifest, &process)? {
                    RuntimeProcessIdentityObservation::ExplicitlyAbsent => {
                        self.persist_execution_stopped(claim, manifest, "term_observed_absent")
                    }
                    RuntimeProcessIdentityObservation::ExactLive
                        if runtime.now_unix_millis()? >= grace_deadline_unix_millis =>
                    {
                        manifest
                            .execution_teardown
                            .set_stop(ContainerStopProgress::KillMayExist {
                                fence: claim.clone(),
                                process: process.clone(),
                                redelivery_not_before_unix_millis: kill_redelivery_deadline(
                                    runtime,
                                )?,
                            });
                        self.write_existing_workload_manifest(manifest)?;
                        let _ = runtime.signal_process(
                            manifest,
                            &process,
                            RuntimeProcessSignal::kill(),
                        )?;
                        Ok(in_progress(
                            "KILL may exist; exact terminality is not observed",
                        ))
                    }
                    RuntimeProcessIdentityObservation::ExactLive => Ok(in_progress(
                        "TERM grace deadline has not elapsed for the exact process",
                    )),
                }
            }
            ContainerStopProgress::KillMayExist {
                fence,
                process,
                redelivery_not_before_unix_millis,
            } if journal_authorizes_older_progress(&fence, claim, journal_authorization) => {
                if runtime.execution_is_terminal(manifest)? {
                    return self.persist_execution_stopped(
                        claim,
                        manifest,
                        "kill_exit_receipt_observed",
                    );
                }
                match runtime.inspect_process(manifest, &process)? {
                    RuntimeProcessIdentityObservation::ExplicitlyAbsent => {
                        self.persist_execution_stopped(claim, manifest, "kill_observed_absent")
                    }
                    RuntimeProcessIdentityObservation::ExactLive => {
                        let now_unix_millis = runtime.now_unix_millis()?;
                        let redelivery_is_due =
                            now_unix_millis >= redelivery_not_before_unix_millis;
                        let next_deadline = if redelivery_is_due {
                            now_unix_millis
                                .checked_add(duration_millis(KILL_REDELIVERY_DELAY)?)
                                .ok_or_else(|| SandboxError::OperationFailed {
                                    message: "Container KILL redelivery deadline overflowed"
                                        .to_owned(),
                                })?
                        } else {
                            redelivery_not_before_unix_millis
                        };
                        manifest
                            .execution_teardown
                            .set_stop(ContainerStopProgress::KillMayExist {
                                fence: claim.clone(),
                                process: process.clone(),
                                redelivery_not_before_unix_millis: next_deadline,
                            });
                        self.write_existing_workload_manifest(manifest)?;
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
            ContainerStopProgress::TermMayExist { fence, .. }
            | ContainerStopProgress::KillMayExist { fence, .. }
            | ContainerStopProgress::IntentPersisted { fence }
            | ContainerStopProgress::ExecutionStopped { fence, .. }
                if fence == *claim =>
            {
                Ok(in_progress(
                    "exact stop effect is already durable and requires inspection",
                ))
            }
            _ => Err(crossed_progress("stop", manifest)),
        }
    }

    fn advance_stop_after_intent(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &mut ContainerSandboxManifest,
        runtime: &dyn ContainerExecutionTeardownRuntime,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        if runtime.execution_is_terminal(manifest)? {
            return self.persist_execution_stopped(claim, manifest, "terminal_before_signal");
        }
        let process = runtime.capture_process(manifest)?;
        let signal = RuntimeProcessSignal::parse(
            &crate::backends::conmon::lifecycle::configured_stop_signal(
                manifest.image_metadata.stop_signal.as_deref(),
            ),
        )?;
        let timeout = crate::backends::conmon::lifecycle::configured_stop_timeout(
            &manifest.spec,
            self.config.stop_timeout,
        );
        let grace_deadline_unix_millis = runtime
            .now_unix_millis()?
            .checked_add(duration_millis(timeout)?)
            .ok_or_else(|| SandboxError::OperationFailed {
                message: "Container stop grace deadline overflowed".to_owned(),
            })?;
        manifest
            .execution_teardown
            .set_stop(ContainerStopProgress::TermMayExist {
                fence: claim.clone(),
                process: process.clone(),
                grace_deadline_unix_millis,
            });
        self.write_existing_workload_manifest(manifest)?;
        match runtime.signal_process(manifest, &process, signal)? {
            RuntimeProcessSignalOutcome::Delivered => Ok(in_progress(
                "TERM may exist; exact terminality is not observed",
            )),
            RuntimeProcessSignalOutcome::AlreadyAbsent => Ok(in_progress(
                "process disappeared during TERM; inspect exact runtime state",
            )),
        }
    }

    fn inspect_stop_execution(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &ContainerSandboxManifest,
        runtime: &dyn ContainerExecutionTeardownRuntime,
        journal_authorization: Option<&ProviderCommandObservation>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        require_matching_drain(manifest, claim)?;
        match manifest.execution_teardown.stop() {
            ContainerStopProgress::NotRequested => Ok(absent("stop intent is not durable")),
            ContainerStopProgress::IntentPersisted { fence }
                if same_command_epoch(fence, claim)
                    || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                if runtime.execution_is_terminal(manifest)? {
                    Ok(absent(
                        "terminal execution evidence awaits durable stop completion",
                    ))
                } else {
                    Ok(absent("stop effect is not durable"))
                }
            }
            ContainerStopProgress::TermMayExist {
                fence,
                process,
                grace_deadline_unix_millis,
            } if same_command_epoch(fence, claim)
                || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                if runtime.execution_is_terminal(manifest)? {
                    return Ok(retry_authorized(
                        "exact exit receipt awaits durable stop completion",
                    ));
                }
                match runtime.inspect_process(manifest, process)? {
                    RuntimeProcessIdentityObservation::ExplicitlyAbsent => Ok(retry_authorized(
                        "TERM completed; durable stop completion is pending",
                    )),
                    RuntimeProcessIdentityObservation::ExactLive
                        if runtime.now_unix_millis()? >= *grace_deadline_unix_millis =>
                    {
                        Ok(retry_authorized(
                            "TERM deadline elapsed; forced-stop dispatch is pending",
                        ))
                    }
                    RuntimeProcessIdentityObservation::ExactLive => Ok(in_progress(
                        "exact process remains live before the TERM deadline",
                    )),
                }
            }
            ContainerStopProgress::KillMayExist {
                fence,
                process,
                redelivery_not_before_unix_millis,
            } if same_command_epoch(fence, claim)
                || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                if runtime.execution_is_terminal(manifest)? {
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
            ContainerStopProgress::ExecutionStopped { fence, evidence }
                if same_command_epoch(fence, claim)
                    || journal_authorizes_older_progress(fence, claim, journal_authorization) =>
            {
                Ok(succeeded(evidence.clone()))
            }
            _ => Err(crossed_progress("stop inspection", manifest)),
        }
    }

    fn persist_execution_stopped(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &mut ContainerSandboxManifest,
        reason: &str,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        self.persist_execution_stopped_with_host_terminal(claim, manifest, reason, None)
    }

    fn persist_execution_stopped_with_host_terminal(
        &self,
        claim: &ProviderCommandClaim,
        manifest: &mut ContainerSandboxManifest,
        reason: &str,
        host_terminal_evidence_sha256: Option<&str>,
    ) -> crate::Result<SandboxExecutionTeardownObservation> {
        // Record the exit code when conmon has published one. A receipt still
        // mid-publication must not fail a stop that has already concluded, and
        // `last_exit_code` already represents "not known".
        if let Some(exit_code) =
            read_exit_receipt(&manifest.conmon_layout.exit_status_file)?.exit_code()
        {
            manifest.last_exit_code = Some(exit_code);
        }
        let evidence = teardown_evidence_with_host_terminal(
            reason,
            manifest,
            claim,
            host_terminal_evidence_sha256,
        )?;
        manifest
            .execution_teardown
            .set_stop(ContainerStopProgress::ExecutionStopped {
                fence: claim.clone(),
                evidence: evidence.clone(),
            });
        // Execution terminality is not network finality. Keep the overall
        // manifest in Stopping until the attachment release owner completes.
        super::synchronize_handle_status(manifest, crate::instance::SandboxStatus::Stopping);
        self.write_existing_workload_manifest(manifest)?;
        Ok(succeeded(evidence))
    }
}

fn require_matching_drain(
    manifest: &ContainerSandboxManifest,
    stop_claim: &ProviderCommandClaim,
) -> crate::Result<()> {
    let ContainerDrainProgress::Drained { fence, .. } = manifest.execution_teardown.drain() else {
        return Err(SandboxError::InvalidSpec {
            message: "Container execution stop requires exact durable drain completion".to_owned(),
        });
    };
    if same_workload_fence(fence, stop_claim) {
        Ok(())
    } else {
        Err(SandboxError::InvalidSpec {
            message: "Container execution stop is crossed with the durable drain fence".to_owned(),
        })
    }
}

fn same_workload_fence(left: &ProviderCommandClaim, right: &ProviderCommandClaim) -> bool {
    left.authority_id() == right.authority_id()
        && left.effect_subject() == right.effect_subject()
        && left.source_attempt_id() == right.source_attempt_id()
        && left.workload_generation() == right.workload_generation()
        && left.restart_ordinal() == right.restart_ordinal()
        && left.desired_digest() == right.desired_digest()
        && left.source_digest() == right.source_digest()
        && left.network_plan_digest() == right.network_plan_digest()
        && left.provider_target_digest() == right.provider_target_digest()
}

fn same_command_attempt(left: &ProviderCommandClaim, right: &ProviderCommandClaim) -> bool {
    same_workload_fence(left, right)
        && left.effect_subject() == right.effect_subject()
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
        && authorization.authenticates_retry_progress(progress)
}

fn kill_redelivery_deadline(runtime: &dyn ContainerExecutionTeardownRuntime) -> crate::Result<u64> {
    runtime
        .now_unix_millis()?
        .checked_add(duration_millis(KILL_REDELIVERY_DELAY)?)
        .ok_or_else(|| SandboxError::OperationFailed {
            message: "Container KILL redelivery deadline overflowed".to_owned(),
        })
}

fn admitted_work_evidence(manifest: &ContainerSandboxManifest) -> crate::Result<Option<Vec<u8>>> {
    if matches!(
        manifest.creator_handoff,
        ContainerCreatorHandoffState::SpawnIntent { .. }
            | ContainerCreatorHandoffState::Pending { .. }
    ) {
        return Ok(Some(
            b"Container creator handoff remains in progress".to_vec(),
        ));
    }
    if let Some(phase) = super::runner::execute_handoff_phase(manifest)? {
        // The lifecycle lock proves that no runner still owns this transition.
        // A pre-effect claim is therefore a settled no-effect admission once
        // the durable drain barrier exists. Any later recovery is fenced at
        // the effect boundary. EffectsStarted remains ambiguous until its
        // existing inspect-before-retry owner settles it.
        if phase == super::runner::RunnerHandoffPhase::ClaimedBeforeEffects {
            return Ok(None);
        }
        return Ok(Some(
            format!("Container runner handoff remains {phase:?}").into_bytes(),
        ));
    }
    Ok(None)
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
    network_cleanup_complete: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    host_terminal_evidence_sha256: Option<&'a str>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredHostTerminalBinding {
    host_terminal_evidence_sha256: Option<String>,
}

fn teardown_evidence(
    kind: &str,
    manifest: &ContainerSandboxManifest,
    claim: &ProviderCommandClaim,
) -> crate::Result<Vec<u8>> {
    teardown_evidence_with_host_terminal(kind, manifest, claim, None)
}

fn teardown_evidence_with_host_terminal(
    kind: &str,
    manifest: &ContainerSandboxManifest,
    claim: &ProviderCommandClaim,
    host_terminal_evidence_sha256: Option<&str>,
) -> crate::Result<Vec<u8>> {
    serde_json::to_vec(&TeardownEvidence {
        kind,
        tenant_id: manifest.spec.tenant_id.as_str(),
        sandbox_id: manifest.handle.id.as_str(),
        execution_attempt_id: manifest.execution_attempt_id.as_str(),
        authority_id: claim.authority_id(),
        dispatch_epoch: claim.dispatch_epoch(),
        network_cleanup_complete: manifest.network_cleanup_complete,
        host_terminal_evidence_sha256,
    })
    .map_err(|error| SandboxError::OperationFailed {
        message: format!("failed to encode Container execution teardown evidence: {error}"),
    })
}

fn authenticate_host_terminal_binding(evidence: &[u8], expected_sha256: &str) -> crate::Result<()> {
    let binding: StoredHostTerminalBinding =
        serde_json::from_slice(evidence).map_err(|error| SandboxError::OperationFailed {
            message: format!("failed to decode durable Container external-stop evidence: {error}"),
        })?;
    if binding.host_terminal_evidence_sha256.as_deref() == Some(expected_sha256) {
        Ok(())
    } else {
        Err(SandboxError::InvalidSpec {
            message: "Container external-stop replay crossed its durable host evidence".to_owned(),
        })
    }
}

fn duration_millis(duration: Duration) -> crate::Result<u64> {
    u64::try_from(duration.as_millis()).map_err(|_| SandboxError::OperationFailed {
        message: "Container stop timeout cannot be represented in milliseconds".to_owned(),
    })
}

fn crossed_progress(operation: &str, manifest: &ContainerSandboxManifest) -> SandboxError {
    SandboxError::InvalidSpec {
        message: format!(
            "Container {operation} command is crossed with durable execution teardown progress for {}",
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

fn execution_result(
    result: crate::Result<SandboxExecutionTeardownObservation>,
) -> SandboxExecutionTeardownObservation {
    match result {
        Ok(observation) => observation,
        Err(error @ SandboxError::InvalidSpec { .. }) => {
            definite_failure("sandbox_teardown_command_crossed", error.to_string())
        }
        Err(error @ SandboxError::NotFound { .. }) => {
            ambiguous(format!("exact Container manifest is absent: {error}"))
        }
        Err(error) => ambiguous(error.to_string()),
    }
}

#[cfg(test)]
#[path = "teardown/tests.rs"]
mod tests;
