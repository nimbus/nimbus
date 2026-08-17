//! Exact sandbox inputs authenticated from one confirmed restart command.
//!
//! This module translates portable workload identity into the sandbox-owned
//! attempt vocabulary. It grants no provider effect and owns no restart
//! decision.

use nimbus_sandbox::{
    SandboxBackendKind, SandboxError, SandboxExecutionAttemptId, SandboxId,
    SandboxProvisionPhaseObservation, SandboxRestartAttemptFence, SandboxSpec,
};

use nimbus_sandbox::backends::container::ContainerSandboxBackend;
use nimbus_sandbox::backends::krun::KrunSandboxBackend;

use super::provision_provider::ProviderProvisionEffectObservation;
use super::provision_sandbox::{sandbox_execution_provider_id, sandbox_network_plan_for};
use super::restart_provider::{
    NetworkRestartAttachmentCapability, WorkloadExecutionQuiescenceCapability,
    WorkloadRestartActivationCapability, WorkloadRestartActivationPrerequisiteCapability,
    WorkloadRestartCapabilityFuture, WorkloadRestartPreparationCapability,
    WorkloadRestartReadinessCapability,
};
use super::restart_provider_command::ProviderRestartEffectObservation;
use super::{ConfirmedWorkloadRestartCommand, ContainerProvisionAdapter, KrunProvisionAdapter};
use crate::workload_executable::decode_sandbox_spec;

/// Complete sandbox-owned inputs for one exact restart epoch.
pub struct ValidatedSandboxRestartCommand {
    spec: SandboxSpec,
    sandbox_id: SandboxId,
    attempt_fence: SandboxRestartAttemptFence,
    network_plan: nimbus_sandbox::SandboxProvisionNetworkPlan,
}

impl ValidatedSandboxRestartCommand {
    pub fn spec(&self) -> &SandboxSpec {
        &self.spec
    }

    pub fn sandbox_id(&self) -> &SandboxId {
        &self.sandbox_id
    }

    pub fn attempt_fence(&self) -> &SandboxRestartAttemptFence {
        &self.attempt_fence
    }

    pub fn network_plan(&self) -> &nimbus_sandbox::SandboxProvisionNetworkPlan {
        &self.network_plan
    }
}

/// Authenticate one confirmed restart for an exact sandbox backend.
pub fn validate_sandbox_restart_command(
    command: &ConfirmedWorkloadRestartCommand,
    backend: SandboxBackendKind,
) -> Result<ValidatedSandboxRestartCommand, ProviderRestartEffectObservation> {
    let spec = decode_sandbox_spec(command.executable())
        .map_err(|error| definite_failure("invalid executable", error.to_string()))?;
    if spec.backend != backend {
        return Err(definite_failure(
            "execution backend mismatch",
            format!(
                "command selected {backend:?}, but executable requests {:?}",
                spec.backend
            ),
        ));
    }
    if &spec.tenant_id != command.key().tenant_id() {
        return Err(definite_failure(
            "execution tenant mismatch",
            format!(
                "command tenant {} does not match executable tenant {}",
                command.key().tenant_id(),
                spec.tenant_id
            ),
        ));
    }

    let provider_id = sandbox_execution_provider_id(backend);
    if command.provider_selection() != &provider_id
        || command.source().execution_provider_id() != &provider_id
    {
        return Err(definite_failure(
            "execution provider mismatch",
            "confirmed restart is crossed with the sandbox execution provider",
        ));
    }

    let source = command.source_execution();
    let target = command.execution();
    if source.execution_id() != target.execution_id()
        || source.workload_uid() != target.workload_uid()
        || source.node_identity() != target.node_identity()
        || source.generation() != target.generation()
        || source.generation() != command.generation()
        || source.desired_digest() != target.desired_digest()
        || source.desired_digest() != command.desired_digest()
        || source.restart_epoch().checked_next() != Some(target.restart_epoch())
        || target.restart_epoch() != command.restart_epoch()
    {
        return Err(definite_failure(
            "execution attempt chain mismatch",
            "confirmed restart source and target execution references are crossed",
        ));
    }

    let sandbox_id = SandboxId::new(target.execution_id().as_str());
    let source_attempt_id = SandboxExecutionAttemptId::new(source.attempt_id().to_string())
        .map_err(|error| definite_failure("invalid source attempt", error.to_string()))?;
    let attempt_id = SandboxExecutionAttemptId::new(target.attempt_id().to_string())
        .map_err(|error| definite_failure("invalid target attempt", error.to_string()))?;
    let attempt_fence = SandboxRestartAttemptFence::new(
        source_attempt_id,
        attempt_id,
        command.restart_epoch().as_u64(),
    )
    .map_err(|error| definite_failure("invalid restart attempt fence", error.to_string()))?;
    let network_plan =
        sandbox_network_plan_for(command.generation(), command.compiled_network_plan(), &spec)
            .map_err(restart_validation_error)?;

    Ok(ValidatedSandboxRestartCommand {
        spec,
        sandbox_id,
        attempt_fence,
        network_plan,
    })
}

fn restart_validation_error(
    observation: ProviderProvisionEffectObservation,
) -> ProviderRestartEffectObservation {
    match observation {
        ProviderProvisionEffectObservation::Succeeded { evidence } => {
            ProviderRestartEffectObservation::Ambiguous { evidence }
        }
        ProviderProvisionEffectObservation::DefiniteFailure { code, evidence } => {
            definite_failure(code, String::from_utf8_lossy(&evidence))
        }
        ProviderProvisionEffectObservation::Absent { evidence } => {
            ProviderRestartEffectObservation::Absent { evidence }
        }
        ProviderProvisionEffectObservation::InProgress { evidence } => {
            ProviderRestartEffectObservation::InProgress { evidence }
        }
        ProviderProvisionEffectObservation::Ambiguous { evidence } => {
            ProviderRestartEffectObservation::Ambiguous { evidence }
        }
    }
}

fn definite_failure(
    code: impl AsRef<str>,
    evidence: impl AsRef<str>,
) -> ProviderRestartEffectObservation {
    ProviderRestartEffectObservation::DefiniteFailure {
        evidence: format!("{}: {}", code.as_ref(), evidence.as_ref()).into_bytes(),
    }
}

fn phase_result(
    result: Result<SandboxProvisionPhaseObservation, SandboxError>,
) -> ProviderRestartEffectObservation {
    match result {
        Ok(SandboxProvisionPhaseObservation::Succeeded { evidence }) => {
            ProviderRestartEffectObservation::Succeeded { evidence }
        }
        Ok(SandboxProvisionPhaseObservation::Absent { evidence }) => {
            ProviderRestartEffectObservation::Absent { evidence }
        }
        Ok(SandboxProvisionPhaseObservation::InProgress { evidence }) => {
            ProviderRestartEffectObservation::InProgress { evidence }
        }
        Ok(SandboxProvisionPhaseObservation::Ambiguous { evidence }) => {
            ProviderRestartEffectObservation::Ambiguous { evidence }
        }
        Err(error @ (SandboxError::InvalidSpec { .. } | SandboxError::NotFound { .. })) => {
            definite_failure("sandbox restart phase rejected", error.to_string())
        }
        Err(error) => ProviderRestartEffectObservation::Ambiguous {
            evidence: error.to_string().into_bytes(),
        },
    }
}

trait SandboxRestartBackend {
    fn kind(&self) -> SandboxBackendKind;

    fn quiesce_source(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;

    fn inspect_source_quiescence(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;

    fn prepare_target(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;

    fn inspect_target_preparation(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;

    fn attach_retained_network(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;

    fn inspect_retained_network(
        &self,
        sandbox_id: &SandboxId,
        fence: &SandboxRestartAttemptFence,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;

    fn inspect_activation_prerequisites(
        &self,
        sandbox_id: &SandboxId,
        attempt_id: &SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;

    fn activate_target(
        &self,
        sandbox_id: &SandboxId,
        attempt_id: &SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;

    fn inspect_target_activation(
        &self,
        sandbox_id: &SandboxId,
        attempt_id: &SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;

    fn inspect_target_readiness(
        &self,
        sandbox_id: &SandboxId,
        attempt_id: &SandboxExecutionAttemptId,
    ) -> Result<SandboxProvisionPhaseObservation, SandboxError>;
}

macro_rules! impl_restart_backend {
    ($backend:ty, $prepare:ident) => {
        impl SandboxRestartBackend for $backend {
            fn kind(&self) -> SandboxBackendKind {
                nimbus_sandbox::SandboxBackend::kind(self)
            }

            fn quiesce_source(
                &self,
                sandbox_id: &SandboxId,
                fence: &SandboxRestartAttemptFence,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.quiesce_restart_source(sandbox_id, fence)
            }

            fn inspect_source_quiescence(
                &self,
                sandbox_id: &SandboxId,
                fence: &SandboxRestartAttemptFence,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.inspect_restart_source_quiescence(sandbox_id, fence)
            }

            fn prepare_target(
                &self,
                sandbox_id: &SandboxId,
                fence: &SandboxRestartAttemptFence,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.$prepare(sandbox_id, fence)
            }

            fn inspect_target_preparation(
                &self,
                sandbox_id: &SandboxId,
                fence: &SandboxRestartAttemptFence,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.inspect_restart_target_preparation(sandbox_id, fence)
            }

            fn attach_retained_network(
                &self,
                sandbox_id: &SandboxId,
                fence: &SandboxRestartAttemptFence,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.attach_restart_retained_network(sandbox_id, fence)
            }

            fn inspect_retained_network(
                &self,
                sandbox_id: &SandboxId,
                fence: &SandboxRestartAttemptFence,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.inspect_restart_retained_network(sandbox_id, fence)
            }

            fn inspect_activation_prerequisites(
                &self,
                sandbox_id: &SandboxId,
                attempt_id: &SandboxExecutionAttemptId,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.inspect_provision_activation_prerequisites(sandbox_id, attempt_id)
            }

            fn activate_target(
                &self,
                sandbox_id: &SandboxId,
                attempt_id: &SandboxExecutionAttemptId,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.activate_provision_workload(sandbox_id, attempt_id)
            }

            fn inspect_target_activation(
                &self,
                sandbox_id: &SandboxId,
                attempt_id: &SandboxExecutionAttemptId,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.inspect_provision_workload_activation(sandbox_id, attempt_id)
            }

            fn inspect_target_readiness(
                &self,
                sandbox_id: &SandboxId,
                attempt_id: &SandboxExecutionAttemptId,
            ) -> Result<SandboxProvisionPhaseObservation, SandboxError> {
                self.inspect_provision_workload_readiness(sandbox_id, attempt_id)
            }
        }
    };
}

impl_restart_backend!(ContainerSandboxBackend, prepare_restart_target_attempt);
impl_restart_backend!(KrunSandboxBackend, prepare_restart_target);

macro_rules! impl_sandbox_restart_capabilities {
    ($adapter:ty) => {
        impl $adapter {
            fn validated_restart(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> Result<ValidatedSandboxRestartCommand, ProviderRestartEffectObservation> {
                validate_sandbox_restart_command(command, self.backend.kind())
            }
        }

        impl WorkloadExecutionQuiescenceCapability for $adapter {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self.restart_phases.execute(command, || match validated {
                    Ok(validated) => phase_result(
                        self.backend
                            .quiesce_source(validated.sandbox_id(), validated.attempt_fence()),
                    ),
                    Err(observation) => observation,
                });
                Box::pin(std::future::ready(observation))
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self.restart_phases.inspect(command, || match validated {
                    Ok(validated) => phase_result(self.backend.inspect_source_quiescence(
                        validated.sandbox_id(),
                        validated.attempt_fence(),
                    )),
                    Err(observation) => observation,
                });
                Box::pin(std::future::ready(observation))
            }
        }

        impl WorkloadRestartPreparationCapability for $adapter {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self.restart_phases.execute(command, || match validated {
                    Ok(validated) => phase_result(
                        self.backend
                            .prepare_target(validated.sandbox_id(), validated.attempt_fence()),
                    ),
                    Err(observation) => observation,
                });
                Box::pin(std::future::ready(observation))
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self.restart_phases.inspect(command, || match validated {
                    Ok(validated) => phase_result(self.backend.inspect_target_preparation(
                        validated.sandbox_id(),
                        validated.attempt_fence(),
                    )),
                    Err(observation) => observation,
                });
                Box::pin(std::future::ready(observation))
            }
        }

        impl NetworkRestartAttachmentCapability for $adapter {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self.restart_phases.execute(command, || match validated {
                    Ok(validated) => phase_result(self.backend.attach_retained_network(
                        validated.sandbox_id(),
                        validated.attempt_fence(),
                    )),
                    Err(observation) => observation,
                });
                Box::pin(std::future::ready(observation))
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self
                    .restart_phases
                    .inspect_live(command, || match validated {
                        Ok(validated) => phase_result(self.backend.inspect_retained_network(
                            validated.sandbox_id(),
                            validated.attempt_fence(),
                        )),
                        Err(observation) => observation,
                    });
                Box::pin(std::future::ready(observation))
            }
        }

        impl WorkloadRestartActivationPrerequisiteCapability for $adapter {
            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self.restart_phases.inspect(command, || match validated {
                    Ok(validated) => phase_result(self.backend.inspect_activation_prerequisites(
                        validated.sandbox_id(),
                        validated.attempt_fence().attempt_id(),
                    )),
                    Err(observation) => observation,
                });
                Box::pin(std::future::ready(observation))
            }
        }

        impl WorkloadRestartActivationCapability for $adapter {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self.restart_phases.execute(command, || match validated {
                    Ok(validated) => phase_result(self.backend.activate_target(
                        validated.sandbox_id(),
                        validated.attempt_fence().attempt_id(),
                    )),
                    Err(observation) => observation,
                });
                Box::pin(std::future::ready(observation))
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self
                    .restart_phases
                    .inspect_live(command, || match validated {
                        Ok(validated) => phase_result(self.backend.inspect_target_activation(
                            validated.sandbox_id(),
                            validated.attempt_fence().attempt_id(),
                        )),
                        Err(observation) => observation,
                    });
                Box::pin(std::future::ready(observation))
            }
        }

        impl WorkloadRestartReadinessCapability for $adapter {
            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let validated = self.validated_restart(command);
                let observation = self.restart_phases.inspect(command, || match validated {
                    Ok(validated) => phase_result(self.backend.inspect_target_readiness(
                        validated.sandbox_id(),
                        validated.attempt_fence().attempt_id(),
                    )),
                    Err(observation) => observation,
                });
                Box::pin(std::future::ready(observation))
            }
        }
    };
}

impl_sandbox_restart_capabilities!(ContainerProvisionAdapter);
impl_sandbox_restart_capabilities!(KrunProvisionAdapter);

#[cfg(test)]
#[path = "restart_sandbox/tests.rs"]
mod tests;
