//! Parent-side adapter for one compute-confirmed forwarded restart phase.
//!
//! The compute saga owns order and retry. This adapter authenticates the
//! complete command, retains one provider-local attempt journal, forwards one
//! exact guest phase, and reconciles only the parent host-port authority that
//! belongs to publication.

use nimbus::SandboxBackendKind;
use nimbus_compute::workload_executable::decode_sandbox_spec;
use nimbus_compute::workload_saga::restart_provider_command::ProviderRestartEffectObservation;
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadRestartCommand, NetworkRestartAttachmentCapability,
    RestartPublicationCapability, RestartPublicationObservationCapability,
    RestartPublicationWithdrawalCapability, WorkloadExecutionQuiescenceCapability,
    WorkloadRestartActivationCapability, WorkloadRestartActivationPrerequisiteCapability,
    WorkloadRestartCapabilityFuture, WorkloadRestartCommandMode,
    WorkloadRestartPreparationCapability, WorkloadRestartReadinessCapability,
};
use nimbus_machine::api::{
    MachineApiWorkloadRestartCommandEnvelope, MachineApiWorkloadRestartCommandMode,
    MachineApiWorkloadRestartObservation,
};
use nimbus_network::{NetworkPlanId, PortLeasePhase};
use nimbus_workloads::WorkloadRestartStep;

use super::{
    ConfirmedMachinePublicationMember, ForwardedMachineProvisionAdapter,
    ProviderProvisionEffectObservation, canonical_machine_restart_publication_members,
};

struct ValidatedForwardedRestart {
    envelope: MachineApiWorkloadRestartCommandEnvelope,
    plan_id: NetworkPlanId,
    members: Vec<ConfirmedMachinePublicationMember>,
}

impl ForwardedMachineProvisionAdapter {
    fn inspect_parent_batch_for(
        &self,
        plan_id: &NetworkPlanId,
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<bool, ProviderRestartEffectObservation> {
        if members.is_empty() {
            return Ok(true);
        }
        let records = self.port_leases.list_plan(plan_id).map_err(|error| {
            restart_ambiguous(format!("parent port lease inspection failed: {error}"))
        })?;
        if records.is_empty() {
            return Ok(false);
        }
        let requests = super::publication_requests(members);
        super::authenticate_exact_durable_plan(&requests, &records).map_err(|error| {
            restart_definite_failure(format!("parent publication plan is crossed: {error}"))
        })?;
        let batches = self.live.lock().map_err(|_| {
            restart_ambiguous("forwarded machine publication runtime registry is poisoned")
        })?;
        let Some(live) = batches.get(plan_id) else {
            drop(batches);
            if super::exact_active_batch(&records, members) {
                // A transient recovery guard proves that the recorded owner is
                // dead. Dropping it here leaves durable state unchanged; only
                // a later Execute command may reclaim the publication.
                let recoveries = super::recover_dead_batch(&self.port_leases, &requests)
                    .map_err(|error| restart_ambiguous(error.to_string()))?;
                drop(recoveries);
            }
            return Ok(false);
        };
        if live.members != members {
            return Err(restart_definite_failure(
                "live parent publication members differ from the canonical restart command",
            ));
        }
        Ok(super::exact_active_batch(&records, members))
    }

    fn inspect_parent_absence_for(
        &self,
        plan_id: &NetworkPlanId,
        members: &[ConfirmedMachinePublicationMember],
    ) -> Result<bool, ProviderRestartEffectObservation> {
        if members.is_empty() {
            return Ok(true);
        }
        if self
            .live
            .lock()
            .map_err(|_| {
                restart_ambiguous("forwarded machine publication runtime registry is poisoned")
            })?
            .contains_key(plan_id)
        {
            return Ok(false);
        }
        let records = self.port_leases.list_plan(plan_id).map_err(|error| {
            restart_ambiguous(format!("parent port lease inspection failed: {error}"))
        })?;
        if records.is_empty() {
            return Ok(true);
        }
        let requests = super::publication_requests(members);
        super::authenticate_exact_durable_plan(&requests, &records).map_err(|error| {
            restart_definite_failure(format!("parent publication plan is crossed: {error}"))
        })?;
        Ok(records.iter().all(|record| {
            record.phase() == PortLeasePhase::Reserved
                && record.binding().is_none()
                && record.bind_claim().is_none()
        }))
    }

    fn validate_restart_phase(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        expected_step: WorkloadRestartStep,
        expected_mode: WorkloadRestartCommandMode,
    ) -> Result<ValidatedForwardedRestart, ProviderRestartEffectObservation> {
        if command.step() != expected_step || command.mode() != expected_mode {
            return Err(restart_definite_failure(
                "forwarded machine restart capability received the wrong step or command mode",
            ));
        }
        let authority = self.client.forwarder_authority().map_err(|error| {
            restart_definite_failure(format!(
                "machine forwarder authority is unavailable: {error}"
            ))
        })?;
        let spec = decode_sandbox_spec(command.executable()).map_err(|error| {
            restart_definite_failure(format!("restart executable is invalid: {error}"))
        })?;
        let content = command.compiled_network_plan().content();
        if spec.backend != SandboxBackendKind::Container
            || spec.tenant_id != *command.key().tenant_id()
            || command.source_execution().node_identity() != self.source_plan.node_identity()
            || command.execution().node_identity() != self.source_plan.node_identity()
            || command.provider_selection() != self.source_plan.execution_provider_id()
            || command.source().execution_provider_id() != self.source_plan.execution_provider_id()
            || content.capability_selection() != Some(self.source_plan.selection())
        {
            return Err(restart_definite_failure(
                "confirmed restart is crossed with the forwarded machine provider realm",
            ));
        }
        let mode = match command.mode() {
            WorkloadRestartCommandMode::Execute => MachineApiWorkloadRestartCommandMode::Execute,
            WorkloadRestartCommandMode::Inspect => MachineApiWorkloadRestartCommandMode::Inspect,
        };
        let envelope = MachineApiWorkloadRestartCommandEnvelope::new(
            command.command_id().clone(),
            command.key().clone(),
            command.saga_id().clone(),
            command.transition_id().clone(),
            command.generation(),
            command.desired_digest(),
            command.source().clone(),
            command.source_execution().clone(),
            command.execution().clone(),
            command.source_attempt_id().clone(),
            command.attempt_id().clone(),
            command.restart_epoch(),
            command.dispatch_epoch(),
            command.request_id().clone(),
            command.issuing_revision(),
            command.confirmed_revision(),
            command.inspection_version(),
            command.provider_selection().clone(),
            command.step(),
            mode,
            command.successor_veto_generation(),
            command.claim().clone(),
            command.executable().clone(),
            command.network_plan_digest(),
            command.compiled_network_plan().clone(),
            authority.clone(),
            authority.generation(),
        )
        .map_err(|error| {
            restart_definite_failure(format!("machine restart envelope is crossed: {error}"))
        })?;
        let members = canonical_machine_restart_publication_members(&envelope, authority)
            .map_err(|error| restart_definite_failure(error.to_string()))?;
        Ok(ValidatedForwardedRestart {
            plan_id: command.compiled_network_plan().plan().plan_id().clone(),
            envelope,
            members,
        })
    }

    fn forward_restart_phase(
        &self,
        validated: &ValidatedForwardedRestart,
    ) -> ProviderRestartEffectObservation {
        match self
            .client
            .restart_workload_phase(validated.envelope.clone())
        {
            Ok(response) => match response.observation() {
                MachineApiWorkloadRestartObservation::Succeeded { evidence } => {
                    ProviderRestartEffectObservation::Succeeded {
                        evidence: evidence.to_string().into_bytes(),
                    }
                }
                MachineApiWorkloadRestartObservation::AuthenticatedAbsent { evidence } => {
                    ProviderRestartEffectObservation::Absent {
                        evidence: evidence.to_string().into_bytes(),
                    }
                }
                MachineApiWorkloadRestartObservation::DefiniteFailure { evidence } => {
                    ProviderRestartEffectObservation::DefiniteFailure {
                        evidence: evidence.to_string().into_bytes(),
                    }
                }
                MachineApiWorkloadRestartObservation::InProgress { evidence } => {
                    ProviderRestartEffectObservation::InProgress {
                        evidence: evidence.to_string().into_bytes(),
                    }
                }
                MachineApiWorkloadRestartObservation::Ambiguous => {
                    ProviderRestartEffectObservation::Ambiguous {
                        evidence: b"forwarded machine restart outcome is ambiguous".to_vec(),
                    }
                }
            },
            Err(error) => ProviderRestartEffectObservation::Ambiguous {
                evidence: error.to_string().into_bytes(),
            },
        }
    }

    fn execute_restart_phase(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        step: WorkloadRestartStep,
    ) -> nimbus_compute::workload_saga::WorkloadRestartProviderObservation {
        let validated =
            match self.validate_restart_phase(command, step, WorkloadRestartCommandMode::Execute) {
                Ok(validated) => validated,
                Err(error) => return self.restart_phases.execute(command, || error),
            };
        self.restart_phases.execute(command, || match step {
            WorkloadRestartStep::WithdrawPublication => {
                let remote = self.forward_restart_phase(&validated);
                if matches!(
                    remote,
                    ProviderRestartEffectObservation::Succeeded { .. }
                        | ProviderRestartEffectObservation::Absent { .. }
                ) && let Err(error) =
                    self.reconcile_parent_absence_for(&validated.plan_id, &validated.members)
                {
                    return restart_post_remote_error(error);
                }
                remote
            }
            WorkloadRestartStep::Publish => {
                if let Err(error) =
                    self.reserve_parent_batch_for(&validated.plan_id, &validated.members)
                {
                    return restart_parent_error(error);
                }
                let remote = self.forward_restart_phase(&validated);
                match &remote {
                    ProviderRestartEffectObservation::Succeeded { .. } => {
                        if let Err(error) =
                            self.activate_parent_batch_for(&validated.plan_id, &validated.members)
                        {
                            return restart_post_remote_error(error);
                        }
                    }
                    ProviderRestartEffectObservation::Absent { .. }
                    | ProviderRestartEffectObservation::DefiniteFailure { .. } => {
                        if let Err(error) = self
                            .reconcile_parent_absence_for(&validated.plan_id, &validated.members)
                        {
                            return restart_post_remote_error(error);
                        }
                    }
                    ProviderRestartEffectObservation::InProgress { .. }
                    | ProviderRestartEffectObservation::Ambiguous { .. } => {}
                }
                remote
            }
            _ => self.forward_restart_phase(&validated),
        })
    }

    fn inspect_restart_phase(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
        step: WorkloadRestartStep,
    ) -> nimbus_compute::workload_saga::WorkloadRestartProviderObservation {
        let validated =
            match self.validate_restart_phase(command, step, WorkloadRestartCommandMode::Inspect) {
                Ok(validated) => validated,
                Err(error) => return self.restart_phases.inspect(command, || error),
            };
        let inspect = || {
            let remote = self.forward_restart_phase(&validated);
            let parent_observed = match (&step, &remote) {
                (
                    WorkloadRestartStep::WithdrawPublication,
                    ProviderRestartEffectObservation::Succeeded { .. },
                ) => self.inspect_parent_absence_for(&validated.plan_id, &validated.members),
                (
                    WorkloadRestartStep::Publish | WorkloadRestartStep::ObservePublication,
                    ProviderRestartEffectObservation::Succeeded { .. },
                ) => self.inspect_parent_batch_for(&validated.plan_id, &validated.members),
                _ => return remote,
            };
            match parent_observed {
                Ok(true) => remote,
                Ok(false) => ProviderRestartEffectObservation::Absent {
                    evidence: format!(
                        "parent publication state is absent for plan {}",
                        validated.plan_id.as_str()
                    )
                    .into_bytes(),
                },
                Err(error) => error,
            }
        };
        if requires_live_reconciliation(step) {
            self.restart_phases.inspect_live(command, inspect)
        } else {
            self.restart_phases.inspect(command, inspect)
        }
    }
}

macro_rules! impl_forwarded_restart_effect_capability {
    ($capability:ty, $step:expr) => {
        impl $capability for ForwardedMachineProvisionAdapter {
            fn execute(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let observation = self.execute_restart_phase(command, $step);
                Box::pin(std::future::ready(observation))
            }

            fn inspect(
                &self,
                command: &ConfirmedWorkloadRestartCommand,
            ) -> WorkloadRestartCapabilityFuture<'_> {
                let observation = self.inspect_restart_phase(command, $step);
                Box::pin(std::future::ready(observation))
            }
        }
    };
}

impl_forwarded_restart_effect_capability!(
    RestartPublicationWithdrawalCapability,
    WorkloadRestartStep::WithdrawPublication
);
impl_forwarded_restart_effect_capability!(
    WorkloadExecutionQuiescenceCapability,
    WorkloadRestartStep::QuiesceExecution
);
impl_forwarded_restart_effect_capability!(
    WorkloadRestartPreparationCapability,
    WorkloadRestartStep::PrepareExecution
);
impl_forwarded_restart_effect_capability!(
    NetworkRestartAttachmentCapability,
    WorkloadRestartStep::AttachNetwork
);
impl_forwarded_restart_effect_capability!(
    WorkloadRestartActivationCapability,
    WorkloadRestartStep::ActivateExecution
);
impl_forwarded_restart_effect_capability!(
    RestartPublicationCapability,
    WorkloadRestartStep::Publish
);

impl WorkloadRestartActivationPrerequisiteCapability for ForwardedMachineProvisionAdapter {
    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        let observation = self
            .inspect_restart_phase(command, WorkloadRestartStep::InspectActivationPrerequisites);
        Box::pin(std::future::ready(observation))
    }
}

impl WorkloadRestartReadinessCapability for ForwardedMachineProvisionAdapter {
    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        let observation =
            self.inspect_restart_phase(command, WorkloadRestartStep::InspectReadiness);
        Box::pin(std::future::ready(observation))
    }
}

impl RestartPublicationObservationCapability for ForwardedMachineProvisionAdapter {
    fn inspect(
        &self,
        command: &ConfirmedWorkloadRestartCommand,
    ) -> WorkloadRestartCapabilityFuture<'_> {
        let observation =
            self.inspect_restart_phase(command, WorkloadRestartStep::ObservePublication);
        Box::pin(std::future::ready(observation))
    }
}

fn restart_parent_error(
    error: ProviderProvisionEffectObservation,
) -> ProviderRestartEffectObservation {
    match error {
        ProviderProvisionEffectObservation::DefiniteFailure { code, evidence } => {
            let mut combined = code.into_bytes();
            combined.push(b':');
            combined.extend(evidence);
            ProviderRestartEffectObservation::DefiniteFailure { evidence: combined }
        }
        ProviderProvisionEffectObservation::Absent { evidence } => {
            ProviderRestartEffectObservation::Absent { evidence }
        }
        ProviderProvisionEffectObservation::InProgress { evidence } => {
            ProviderRestartEffectObservation::InProgress { evidence }
        }
        ProviderProvisionEffectObservation::Succeeded { evidence } => {
            ProviderRestartEffectObservation::Succeeded { evidence }
        }
        ProviderProvisionEffectObservation::Ambiguous { evidence } => {
            ProviderRestartEffectObservation::Ambiguous { evidence }
        }
    }
}

fn restart_post_remote_error(
    error: ProviderProvisionEffectObservation,
) -> ProviderRestartEffectObservation {
    let (kind, code, detail) = match error {
        ProviderProvisionEffectObservation::DefiniteFailure { code, evidence } => {
            ("definite_failure", Some(code), evidence)
        }
        ProviderProvisionEffectObservation::Absent { evidence } => ("absent", None, evidence),
        ProviderProvisionEffectObservation::InProgress { evidence } => {
            ("in_progress", None, evidence)
        }
        ProviderProvisionEffectObservation::Succeeded { evidence } => ("succeeded", None, evidence),
        ProviderProvisionEffectObservation::Ambiguous { evidence } => ("ambiguous", None, evidence),
    };
    let mut evidence =
        format!("parent publication reconciliation after remote {kind}").into_bytes();
    if let Some(code) = code {
        evidence.extend_from_slice(b":");
        evidence.extend_from_slice(code.as_bytes());
    }
    evidence.extend_from_slice(b":");
    evidence.extend(detail);
    ProviderRestartEffectObservation::Ambiguous { evidence }
}

fn restart_definite_failure(evidence: impl Into<Vec<u8>>) -> ProviderRestartEffectObservation {
    ProviderRestartEffectObservation::DefiniteFailure {
        evidence: evidence.into(),
    }
}

fn restart_ambiguous(evidence: impl Into<Vec<u8>>) -> ProviderRestartEffectObservation {
    ProviderRestartEffectObservation::Ambiguous {
        evidence: evidence.into(),
    }
}

const fn requires_live_reconciliation(step: WorkloadRestartStep) -> bool {
    matches!(
        step,
        WorkloadRestartStep::AttachNetwork
            | WorkloadRestartStep::ActivateExecution
            | WorkloadRestartStep::Publish
            | WorkloadRestartStep::ObservePublication
    )
}

#[cfg(test)]
#[path = "restart/tests.rs"]
pub(crate) mod tests;
