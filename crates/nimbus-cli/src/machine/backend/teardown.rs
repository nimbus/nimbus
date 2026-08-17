//! Exact parent/guest phase adapter for forwarded-machine teardown.

use std::sync::Arc;

use nimbus::Error;
use nimbus_compute::workload_saga::teardown_provider_command::{
    ConfirmedTeardownProviderCommand, ConfirmedTeardownProviderJournal,
};
use nimbus_compute::workload_saga::{
    ConfirmedWorkloadTeardownCommand, FinalIngressWithdrawalCapability,
    IngressTeardownCapabilities, NetworkAttachmentTeardownCapabilities,
    NetworkDetachmentCapability, NetworkReleaseCapability, WorkloadExecutionDrainCapability,
    WorkloadExecutionStopCapability, WorkloadExecutionTeardownCapabilities,
    WorkloadTeardownCapabilityFuture, WorkloadTeardownExecuteOutcome,
    WorkloadTeardownInspectOutcome, WorkloadTeardownProviderObservation,
    WorkloadTeardownProviderOutcome,
};
use nimbus_machine::api::{
    MachineApiWorkloadTeardownCommandEnvelope, MachineApiWorkloadTeardownCommandEnvelopeInput,
    MachineApiWorkloadTeardownExecuteObservation, MachineApiWorkloadTeardownInspectObservation,
    MachineApiWorkloadTeardownObservation, MachineApiWorkloadTeardownPhaseRequest,
    MachineApiWorkloadTeardownProviderTranslation,
};
use nimbus_sandbox::{
    MachinePortForwardingRetirement, OciMachinePortForwardingRetirement,
    ProviderCommandAttemptJournal, ProviderCommandJournalError, ProviderCommandObservation,
    ProviderCommandObservationKind, ProviderCommandStartedClaimDecision,
};
use nimbus_workloads::{
    WorkloadFailureEvidence, WorkloadOwnerEvidenceDigest, WorkloadTeardownCommandMode,
    WorkloadTeardownProviderTarget, WorkloadTeardownStep, WorkloadTeardownSubjects,
    WorkloadTeardownSuccessEvidence,
};
use serde::Serialize;

use super::super::client::{MachineApiClient, MachineApiWorkloadTeardownTransportOutcome};
use super::super::publication_authority::{
    ConfirmedMachinePublicationRetirement, ConfirmedMachinePublicationRetirementPhase,
};
use super::provision::{ForwardedMachineProvisionAdapter, ForwardedMachineProvisionSourcePlan};

#[cfg(test)]
mod tests;

const PROVIDER_JOURNAL_NAMESPACE: &str = "forwarded-machine-teardown";
const PARENT_WITHDRAWAL_REQUEST_DOMAIN: &str = "nimbus.machine.parent-publication-withdrawal.v1";

pub(crate) struct ForwardedMachineTeardownAdapter {
    provision: Arc<ForwardedMachineProvisionAdapter>,
    phases: ConfirmedTeardownProviderJournal,
    forwarding: Arc<dyn MachinePortForwardingRetirement>,
}

pub(crate) struct ForwardedMachineTeardownRegistrations {
    attachment: NetworkAttachmentTeardownCapabilities,
    execution: WorkloadExecutionTeardownCapabilities,
    ingress: IngressTeardownCapabilities,
}

impl ForwardedMachineTeardownRegistrations {
    pub(crate) fn into_parts(
        self,
    ) -> (
        NetworkAttachmentTeardownCapabilities,
        WorkloadExecutionTeardownCapabilities,
        IngressTeardownCapabilities,
    ) {
        (self.attachment, self.execution, self.ingress)
    }
}

impl ForwardedMachineTeardownAdapter {
    pub(crate) fn new(provision: Arc<ForwardedMachineProvisionAdapter>) -> Result<Self, Error> {
        let journal = ProviderCommandAttemptJournal::open(
            provision.teardown_state_root(),
            PROVIDER_JOURNAL_NAMESPACE,
        )
        .map_err(|error| {
            Error::Internal(format!(
                "failed to open forwarded machine teardown journal: {error}"
            ))
        })?;
        let forwarding = Arc::new(OciMachinePortForwardingRetirement::new(
            provision.teardown_source_plan().forwarder_config().clone(),
        ));
        Self::with_authorities(provision, journal, forwarding)
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(
        provision: Arc<ForwardedMachineProvisionAdapter>,
        forwarding: Arc<dyn MachinePortForwardingRetirement>,
    ) -> Result<Self, Error> {
        let journal = ProviderCommandAttemptJournal::open(
            provision.teardown_state_root(),
            PROVIDER_JOURNAL_NAMESPACE,
        )
        .map_err(|error| {
            Error::Internal(format!(
                "failed to open forwarded machine teardown journal: {error}"
            ))
        })?;
        Self::with_authorities(provision, journal, forwarding)
    }

    pub(crate) fn registrations(self: Arc<Self>) -> ForwardedMachineTeardownRegistrations {
        let source = self.provision.teardown_source_plan();
        ForwardedMachineTeardownRegistrations {
            attachment: NetworkAttachmentTeardownCapabilities::new(
                source.selection().attachment_provider_id().clone(),
                self.clone(),
                self.clone(),
            ),
            execution: WorkloadExecutionTeardownCapabilities::new(
                source.execution_provider_id().clone(),
                self.clone(),
                self.clone(),
            ),
            ingress: IngressTeardownCapabilities::new(
                source.selection().ingress_provider_id().clone(),
                self,
            ),
        }
    }

    fn with_authorities(
        provision: Arc<ForwardedMachineProvisionAdapter>,
        journal: ProviderCommandAttemptJournal,
        forwarding: Arc<dyn MachinePortForwardingRetirement>,
    ) -> Result<Self, Error> {
        let source = provision.teardown_source_plan();
        let client = provision.teardown_client();
        source.authenticate_for_activation(&client)?;
        let authority = client.forwarder_authority()?;
        if forwarding.provider_instance() != authority.provider_instance()
            || forwarding.provider_generation() != authority.generation()
        {
            return Err(Error::PreconditionFailed(
                "forwarded machine teardown forwarding capability is crossed with the parent provider incarnation"
                    .to_owned(),
            ));
        }
        Ok(Self {
            provision,
            phases: ConfirmedTeardownProviderJournal::new(journal),
            forwarding,
        })
    }

    async fn dispatch(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        expected_step: WorkloadTeardownStep,
        expected_mode: WorkloadTeardownCommandMode,
    ) -> WorkloadTeardownProviderObservation {
        let outcome = match self.validate(command, expected_step, expected_mode) {
            Ok(validated) => match expected_mode {
                WorkloadTeardownCommandMode::Execute => self.execute(command, validated).await,
                WorkloadTeardownCommandMode::Inspect => self.inspect(command, validated).await,
            },
            Err(failure) => definite_outcome(expected_mode, failure),
        };
        WorkloadTeardownProviderObservation::for_command(command, outcome)
    }

    fn validate(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        expected_step: WorkloadTeardownStep,
        expected_mode: WorkloadTeardownCommandMode,
    ) -> Result<ValidatedForwardedMachineTeardown, WorkloadFailureEvidence> {
        if command.step() != expected_step || command.mode() != expected_mode {
            return Err(failure(
                "machine_teardown_command_invalid",
                "forwarded teardown capability received the wrong step or mode",
            ));
        }
        let client = self.provision.teardown_client();
        let source_plan = self.provision.teardown_source_plan();
        source_plan
            .authenticate_for_activation(&client)
            .map_err(|error| failure("machine_teardown_provider_crossed", error.to_string()))?;
        let authority = client
            .forwarder_authority()
            .map_err(|error| failure("machine_teardown_forwarder_stale", error.to_string()))?
            .clone();
        validate_source_and_target(command, source_plan, &authority)?;
        validate_subjects(command)?;
        let retirement = self
            .provision
            .authenticate_teardown_retirement(command, &authority)
            .map_err(|error| failure("machine_teardown_order_invalid", error.to_string()))?;
        validate_retirement_order(command.step(), retirement.phase())?;

        let effect_subject = serde_json::to_string(&(
            command.execution_locator(),
            command.subjects(),
            command.prior_receipt_prefix(),
        ))
        .map_err(|error| failure("machine_teardown_command_invalid", error.to_string()))?;
        let target = serde_json::to_vec(command.provider_target())
            .map_err(|error| failure("machine_teardown_command_invalid", error.to_string()))?;
        let provider_command = ConfirmedTeardownProviderCommand::new(
            command,
            effect_subject,
            WorkloadOwnerEvidenceDigest::sha256(target).to_string(),
        )
        .map_err(|error| failure("machine_teardown_command_invalid", error.to_string()))?;

        let remote_request = if command.step() == WorkloadTeardownStep::WithdrawPublication {
            None
        } else {
            Some(
                build_remote_request(command, authority.clone()).map_err(|error| {
                    failure("machine_teardown_command_invalid", error.to_string())
                })?,
            )
        };
        let prepared = match remote_request.as_ref() {
            Some(request) => serde_json::to_vec(request),
            None => serde_json::to_vec(&ParentWithdrawalPreparedRequest {
                domain: PARENT_WITHDRAWAL_REQUEST_DOMAIN,
                command_id: command.command_id(),
                confirmed_revision: command.confirmed_revision(),
                confirmed_transition_id: command.confirmed_transition_id(),
                claim: command.claim(),
                source: command.source(),
                compiled_network_plan: command.compiled_network_plan(),
                execution_locator: command.execution_locator(),
                prior_receipt_prefix: command.prior_receipt_prefix(),
                authority: &authority,
                members: retirement.members(),
            }),
        }
        .map_err(|error| failure("machine_teardown_command_invalid", error.to_string()))?;

        Ok(ValidatedForwardedMachineTeardown {
            provider_command,
            retirement,
            remote_request,
            prepared,
        })
    }

    async fn execute(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        validated: ValidatedForwardedMachineTeardown,
    ) -> WorkloadTeardownProviderOutcome {
        let decision = self
            .phases
            .claim_execute_started(&validated.provider_command, &validated.prepared);
        let execution = match decision {
            Ok(ProviderCommandStartedClaimDecision::ExecuteStarted(execution)) => execution,
            Ok(ProviderCommandStartedClaimDecision::AdoptExactAttempt(observation)) => {
                return provider_outcome(command, &observation);
            }
            Err(error) => return journal_error_outcome(command.mode(), &error),
        };

        let provision = self.provision.clone();
        let forwarding = self.forwarding.clone();
        let client = self.provision.teardown_client();
        let remote_request = validated.remote_request.clone();
        let retirement = validated.retirement.clone();
        let exact_command = command.clone();
        let provider_command = validated.provider_command.clone();
        match self
            .phases
            .execute_started_claim_async(&validated.provider_command, execution, move |current| {
                let prepared = current
                    .observation()
                    .prepared_request()
                    .expect("started execution retains exact prepared bytes")
                    .to_vec();
                Box::pin(async move {
                    tokio::task::spawn_blocking(move || match remote_request {
                        None => local_withdrawal_result(
                            &provision,
                            &exact_command,
                            &provider_command,
                            forwarding.as_ref(),
                            &retirement,
                        ),
                        Some(request) => remote_result(
                            &client,
                            &request,
                            &prepared,
                            RemoteTeardownContext {
                                provision: &provision,
                                command: &exact_command,
                                provider: &provider_command,
                                forwarding: forwarding.as_ref(),
                                retirement: &retirement,
                            },
                        ),
                    })
                    .await
                    .unwrap_or_else(|error| ambiguous_journal_result(error.to_string()))
                })
            })
            .await
        {
            Ok(((), observation)) => provider_outcome(command, &observation),
            Err(error) => journal_error_outcome(command.mode(), &error),
        }
    }

    async fn inspect(
        &self,
        command: &ConfirmedWorkloadTeardownCommand,
        validated: ValidatedForwardedMachineTeardown,
    ) -> WorkloadTeardownProviderOutcome {
        let observation = match self.phases.adopt_inspect(&validated.provider_command) {
            Ok(None) => {
                return WorkloadTeardownProviderOutcome::Inspect(
                    WorkloadTeardownInspectOutcome::NotCompleted(
                        WorkloadOwnerEvidenceDigest::sha256(
                            b"forwarded machine teardown provider command was never claimed",
                        ),
                    ),
                );
            }
            Ok(Some(observation)) if observation.kind().is_terminal_for_adapter() => {
                return provider_outcome(command, &observation);
            }
            Ok(Some(observation)) => observation,
            Err(error) => return journal_error_outcome(command.mode(), &error),
        };

        let provision = self.provision.clone();
        let forwarding = self.forwarding.clone();
        let client = self.provision.teardown_client();
        let remote_request = validated.remote_request.clone();
        let retirement = validated.retirement.clone();
        let exact_command = command.clone();
        let provider_command = validated.provider_command.clone();
        match self
            .phases
            .inspect_current_claim_async_and_publish(
                &validated.provider_command,
                &observation,
                move |_| {
                    Box::pin(async move {
                        tokio::task::spawn_blocking(move || match remote_request {
                            None => local_withdrawal_inspection(
                                &provision,
                                forwarding.as_ref(),
                                &retirement,
                            ),
                            Some(request) => remote_inspection_result(
                                &client,
                                &request,
                                RemoteTeardownContext {
                                    provision: &provision,
                                    command: &exact_command,
                                    provider: &provider_command,
                                    forwarding: forwarding.as_ref(),
                                    retirement: &retirement,
                                },
                            ),
                        })
                        .await
                        .unwrap_or_else(|error| ambiguous_journal_result(error.to_string()))
                    })
                },
            )
            .await
        {
            Ok(((), observation)) => provider_outcome(command, &observation),
            Err(error) => journal_error_outcome(command.mode(), &error),
        }
    }
}

macro_rules! impl_teardown_capability {
    ($trait:ty, $step:expr) => {
        impl $trait for ForwardedMachineTeardownAdapter {
            fn execute<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move {
                    self.dispatch(command, $step, WorkloadTeardownCommandMode::Execute)
                        .await
                })
            }

            fn inspect<'a>(
                &'a self,
                command: &'a ConfirmedWorkloadTeardownCommand,
            ) -> WorkloadTeardownCapabilityFuture<'a> {
                Box::pin(async move {
                    self.dispatch(command, $step, WorkloadTeardownCommandMode::Inspect)
                        .await
                })
            }
        }
    };
}

impl_teardown_capability!(
    FinalIngressWithdrawalCapability,
    WorkloadTeardownStep::WithdrawPublication
);
impl_teardown_capability!(
    WorkloadExecutionDrainCapability,
    WorkloadTeardownStep::DrainExecution
);
impl_teardown_capability!(
    WorkloadExecutionStopCapability,
    WorkloadTeardownStep::StopExecution
);
impl_teardown_capability!(
    NetworkDetachmentCapability,
    WorkloadTeardownStep::DetachNetwork
);
impl_teardown_capability!(
    NetworkReleaseCapability,
    WorkloadTeardownStep::ReleaseNetwork
);

struct ValidatedForwardedMachineTeardown {
    provider_command: ConfirmedTeardownProviderCommand,
    retirement: ConfirmedMachinePublicationRetirement,
    remote_request: Option<MachineApiWorkloadTeardownPhaseRequest>,
    prepared: Vec<u8>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ParentWithdrawalPreparedRequest<'a> {
    domain: &'static str,
    command_id: nimbus_workloads::WorkloadTeardownCommandId,
    confirmed_revision: nimbus_workloads::WorkloadSagaRevision,
    confirmed_transition_id: &'a nimbus_workloads::WorkloadSagaTransitionId,
    claim: &'a nimbus_workloads::WorkloadTeardownClaim,
    source: &'a nimbus_workloads::WorkloadProvisionSourceEvidence,
    compiled_network_plan: &'a nimbus_workloads::CompiledWorkloadNetworkPlan,
    execution_locator: &'a nimbus_workloads::WorkloadExecutionReference,
    prior_receipt_prefix: &'a nimbus_workloads::WorkloadTeardownReceiptPrefix,
    authority: &'a nimbus_machine::MachineForwarderAuthority,
    members: &'a [super::super::publication_authority::ConfirmedMachinePublicationMember],
}

fn build_remote_request(
    command: &ConfirmedWorkloadTeardownCommand,
    authority: nimbus_machine::MachineForwarderAuthority,
) -> Result<
    MachineApiWorkloadTeardownPhaseRequest,
    nimbus_machine::api::MachineApiWorkloadTeardownWireError,
> {
    let provider_translation = match command.step() {
        WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution => {
            MachineApiWorkloadTeardownProviderTranslation::GuestExecutionComposition
        }
        WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork => {
            MachineApiWorkloadTeardownProviderTranslation::GuestContainerAttachment
        }
        WorkloadTeardownStep::WithdrawPublication => unreachable!("withdrawal is parent-local"),
    };
    let envelope = MachineApiWorkloadTeardownCommandEnvelope::new(
        MachineApiWorkloadTeardownCommandEnvelopeInput {
            command_id: command.command_id(),
            confirmed_revision: command.confirmed_revision(),
            confirmed_transition_id: command.confirmed_transition_id().clone(),
            source: command.source().clone(),
            compiled_network_plan: command.compiled_network_plan().clone(),
            execution_locator: command.execution_locator().clone(),
            prior_receipt_prefix: command.prior_receipt_prefix().clone(),
            mode: command.mode(),
            claim: command.claim().clone(),
            machine_forwarder_authority: authority.clone(),
            machine_provider_generation: authority.generation(),
            provider_translation,
        },
    )?;
    MachineApiWorkloadTeardownPhaseRequest::new(authority, envelope)
}

fn validate_source_and_target(
    command: &ConfirmedWorkloadTeardownCommand,
    source: &ForwardedMachineProvisionSourcePlan,
    authority: &nimbus_machine::MachineForwarderAuthority,
) -> Result<(), WorkloadFailureEvidence> {
    let selection = source.selection();
    let selection_digest = source.bundle().selection_evidence().source_digest();
    let target_matches = match (command.step(), command.provider_target()) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownProviderTarget::Ingress {
                provider_id,
                provider_source_digest,
            },
        ) => {
            provider_id == authority.provider_instance().provider_id()
                && *provider_source_digest == selection_digest
        }
        (
            WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution,
            WorkloadTeardownProviderTarget::Execution {
                provider_id,
                provider_source_digest,
            },
        ) => {
            provider_id == source.execution_provider_id()
                && *provider_source_digest == command.source_digest()
        }
        (
            WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork,
            WorkloadTeardownProviderTarget::Attachment {
                provider_id,
                provider_source_digest,
            },
        ) => {
            provider_id == selection.attachment_provider_id()
                && *provider_source_digest == selection_digest
        }
        _ => false,
    };
    if !target_matches
        || command.required_node() != source.node_identity()
        || command.execution_locator().node_identity() != source.node_identity()
        || command.source().execution_provider_id() != source.execution_provider_id()
        || command.source().attachment_provider_id() != selection.attachment_provider_id()
        || command.selection_evidence() != Some(&source.bundle().selection_evidence())
    {
        return Err(failure(
            "machine_teardown_provider_crossed",
            "confirmed teardown command crosses the forwarded machine source plan",
        ));
    }
    Ok(())
}

fn validate_subjects(
    command: &ConfirmedWorkloadTeardownCommand,
) -> Result<(), WorkloadFailureEvidence> {
    let plan = command.compiled_network_plan().plan();
    let matches = match (command.step(), command.subjects()) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => {
            reference.execution() == command.execution_locator()
                && reference.network().plan_id() == plan.plan_id()
                && reference.network().generation() == plan.generation()
                && reference.network().digest() == plan.digest()
        }
        (
            WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution,
            WorkloadTeardownSubjects::Execution(reference),
        ) => reference == command.execution_locator(),
        (
            WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork,
            WorkloadTeardownSubjects::Network(reference),
        ) => {
            reference.plan_id() == plan.plan_id()
                && reference.generation() == plan.generation()
                && reference.digest() == plan.digest()
        }
        _ => false,
    };
    if matches {
        Ok(())
    } else {
        Err(failure(
            "machine_teardown_command_crossed",
            "forwarded machine teardown subjects cross the exact execution or network plan",
        ))
    }
}

fn validate_retirement_order(
    step: WorkloadTeardownStep,
    phase: ConfirmedMachinePublicationRetirementPhase,
) -> Result<(), WorkloadFailureEvidence> {
    let valid = match step {
        WorkloadTeardownStep::WithdrawPublication => matches!(
            phase,
            ConfirmedMachinePublicationRetirementPhase::Active
                | ConfirmedMachinePublicationRetirementPhase::WithdrawalMayExist
                | ConfirmedMachinePublicationRetirementPhase::WithdrawnRetained
                | ConfirmedMachinePublicationRetirementPhase::ReleaseMayExist
                | ConfirmedMachinePublicationRetirementPhase::Released
        ),
        WorkloadTeardownStep::DrainExecution
        | WorkloadTeardownStep::StopExecution
        | WorkloadTeardownStep::DetachNetwork => matches!(
            phase,
            ConfirmedMachinePublicationRetirementPhase::WithdrawnRetained
                | ConfirmedMachinePublicationRetirementPhase::ReleaseMayExist
                | ConfirmedMachinePublicationRetirementPhase::Released
        ),
        WorkloadTeardownStep::ReleaseNetwork => matches!(
            phase,
            ConfirmedMachinePublicationRetirementPhase::WithdrawnRetained
                | ConfirmedMachinePublicationRetirementPhase::ReleaseMayExist
                | ConfirmedMachinePublicationRetirementPhase::Released
        ),
    };
    if valid {
        Ok(())
    } else {
        Err(failure(
            "machine_teardown_order_invalid",
            "parent publication is not withdrawn and retained before the guest phase",
        ))
    }
}

fn local_withdrawal_result(
    provision: &ForwardedMachineProvisionAdapter,
    command: &ConfirmedWorkloadTeardownCommand,
    provider: &ConfirmedTeardownProviderCommand,
    forwarding: &dyn MachinePortForwardingRetirement,
    retirement: &ConfirmedMachinePublicationRetirement,
) -> JournalResult {
    match provision.reconcile_withdrawn_parent_batch(retirement, command, provider, forwarding) {
        Ok(receipts) => success_journal_result(receipts),
        Err(error) => ambiguous_journal_result(error.to_string()),
    }
}

fn local_withdrawal_inspection(
    provision: &ForwardedMachineProvisionAdapter,
    forwarding: &dyn MachinePortForwardingRetirement,
    retirement: &ConfirmedMachinePublicationRetirement,
) -> JournalResult {
    match provision.inspect_parent_withdrawal(retirement, forwarding) {
        Ok((kind, evidence)) => ((), kind, None, evidence),
        Err(error) => ambiguous_journal_result(error.to_string()),
    }
}

struct RemoteTeardownContext<'a> {
    provision: &'a ForwardedMachineProvisionAdapter,
    command: &'a ConfirmedWorkloadTeardownCommand,
    provider: &'a ConfirmedTeardownProviderCommand,
    forwarding: &'a dyn MachinePortForwardingRetirement,
    retirement: &'a ConfirmedMachinePublicationRetirement,
}

fn remote_result(
    client: &MachineApiClient,
    request: &MachineApiWorkloadTeardownPhaseRequest,
    prepared: &[u8],
    context: RemoteTeardownContext<'_>,
) -> JournalResult {
    match client.teardown_workload_phase_prepared(request, prepared) {
        Ok(outcome) => map_remote_outcome(outcome, request, context),
        Err(error) => ambiguous_journal_result(error.to_string()),
    }
}

fn remote_inspection_result(
    client: &MachineApiClient,
    request: &MachineApiWorkloadTeardownPhaseRequest,
    context: RemoteTeardownContext<'_>,
) -> JournalResult {
    match client.teardown_workload_phase(request) {
        Ok(outcome) => map_remote_outcome(outcome, request, context),
        Err(error) => ambiguous_journal_result(error.to_string()),
    }
}

type JournalResult = ((), ProviderCommandObservationKind, Option<String>, Vec<u8>);

fn map_remote_outcome(
    outcome: MachineApiWorkloadTeardownTransportOutcome,
    request: &MachineApiWorkloadTeardownPhaseRequest,
    context: RemoteTeardownContext<'_>,
) -> JournalResult {
    match outcome {
        MachineApiWorkloadTeardownTransportOutcome::Ambiguous { reason } => {
            ambiguous_journal_result(reason)
        }
        MachineApiWorkloadTeardownTransportOutcome::Correlated(response) => {
            let evidence = match serde_json::to_vec(response.as_ref()) {
                Ok(evidence) => evidence,
                Err(error) => return ambiguous_journal_result(error.to_string()),
            };
            match response.observation() {
                MachineApiWorkloadTeardownObservation::Execute(
                    MachineApiWorkloadTeardownExecuteObservation::Succeeded { .. },
                )
                | MachineApiWorkloadTeardownObservation::Inspect(
                    MachineApiWorkloadTeardownInspectObservation::Satisfied { .. },
                ) => {
                    if response.step() == WorkloadTeardownStep::ReleaseNetwork
                        && let Err(error) =
                            context.provision.release_parent_batch_after_guest_release(
                                context.retirement,
                                context.command,
                                context.provider,
                                request,
                                response.as_ref(),
                                context.forwarding,
                            )
                    {
                        return ambiguous_journal_result(error.to_string());
                    }
                    (
                        (),
                        ProviderCommandObservationKind::Succeeded,
                        None,
                        evidence,
                    )
                }
                MachineApiWorkloadTeardownObservation::Execute(
                    MachineApiWorkloadTeardownExecuteObservation::DefiniteFailure { failure },
                )
                | MachineApiWorkloadTeardownObservation::Inspect(
                    MachineApiWorkloadTeardownInspectObservation::DefiniteFailure { failure },
                ) => (
                    (),
                    ProviderCommandObservationKind::DefiniteFailure,
                    Some(failure.code().to_owned()),
                    evidence,
                ),
                MachineApiWorkloadTeardownObservation::Inspect(
                    MachineApiWorkloadTeardownInspectObservation::NotCompleted { .. },
                ) => ((), ProviderCommandObservationKind::Absent, None, evidence),
                MachineApiWorkloadTeardownObservation::Inspect(
                    MachineApiWorkloadTeardownInspectObservation::InProgress { .. },
                ) => (
                    (),
                    ProviderCommandObservationKind::InProgress,
                    None,
                    evidence,
                ),
                MachineApiWorkloadTeardownObservation::Execute(
                    MachineApiWorkloadTeardownExecuteObservation::Ambiguous,
                )
                | MachineApiWorkloadTeardownObservation::Inspect(
                    MachineApiWorkloadTeardownInspectObservation::Ambiguous,
                ) => (
                    (),
                    ProviderCommandObservationKind::Ambiguous,
                    None,
                    evidence,
                ),
            }
        }
    }
}

fn success_journal_result(evidence: impl Serialize) -> JournalResult {
    match serde_json::to_vec(&evidence) {
        Ok(evidence) => (
            (),
            ProviderCommandObservationKind::Succeeded,
            None,
            evidence,
        ),
        Err(error) => ambiguous_journal_result(error.to_string()),
    }
}

fn ambiguous_journal_result(evidence: impl Into<Vec<u8>>) -> JournalResult {
    (
        (),
        ProviderCommandObservationKind::Ambiguous,
        None,
        evidence.into(),
    )
}

trait AdapterObservationKind {
    fn is_terminal_for_adapter(&self) -> bool;
}

impl AdapterObservationKind for ProviderCommandObservationKind {
    fn is_terminal_for_adapter(&self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::DefiniteFailure | Self::Absent | Self::RetryAuthorized
        )
    }
}

fn provider_outcome(
    command: &ConfirmedWorkloadTeardownCommand,
    observation: &ProviderCommandObservation,
) -> WorkloadTeardownProviderOutcome {
    let evidence = provider_evidence(observation);
    match (command.mode(), observation.kind()) {
        (WorkloadTeardownCommandMode::Execute, ProviderCommandObservationKind::Succeeded) => {
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(
                Box::new(success_evidence(command, evidence)),
            ))
        }
        (WorkloadTeardownCommandMode::Inspect, ProviderCommandObservationKind::Succeeded) => {
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Satisfied(
                Box::new(success_evidence(command, evidence)),
            ))
        }
        (_, ProviderCommandObservationKind::DefiniteFailure) => {
            let failure = WorkloadFailureEvidence::new(
                observation
                    .failure_code()
                    .unwrap_or("machine_teardown_provider_failed"),
                evidence,
            )
            .expect("durable provider failure code is validated");
            definite_outcome(command.mode(), failure)
        }
        (
            WorkloadTeardownCommandMode::Inspect,
            ProviderCommandObservationKind::Absent
            | ProviderCommandObservationKind::RetryAuthorized,
        ) => WorkloadTeardownProviderOutcome::Inspect(
            WorkloadTeardownInspectOutcome::NotCompleted(evidence),
        ),
        (WorkloadTeardownCommandMode::Inspect, ProviderCommandObservationKind::Claimed)
        | (WorkloadTeardownCommandMode::Inspect, ProviderCommandObservationKind::InProgress) => {
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::InProgress(
                evidence,
            ))
        }
        (WorkloadTeardownCommandMode::Inspect, ProviderCommandObservationKind::Ambiguous) => {
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous)
        }
        (WorkloadTeardownCommandMode::Execute, _) => {
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
        }
    }
}

fn provider_evidence(observation: &ProviderCommandObservation) -> WorkloadOwnerEvidenceDigest {
    WorkloadOwnerEvidenceDigest::sha256(
        observation
            .evidence_sha256()
            .unwrap_or("provider_command_has_no_outcome_evidence"),
    )
}

fn success_evidence(
    command: &ConfirmedWorkloadTeardownCommand,
    evidence: WorkloadOwnerEvidenceDigest,
) -> WorkloadTeardownSuccessEvidence {
    match (command.step(), command.subjects()) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
            reference: reference.clone(),
            evidence,
        },
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
        _ => unreachable!("validated forwarded teardown subjects match their phase"),
    }
}

fn journal_error_outcome(
    mode: WorkloadTeardownCommandMode,
    error: &ProviderCommandJournalError,
) -> WorkloadTeardownProviderOutcome {
    let code = match error {
        ProviderCommandJournalError::InvalidClaim { .. } => {
            Some("machine_teardown_command_invalid")
        }
        ProviderCommandJournalError::StaleWorkloadGeneration { .. }
        | ProviderCommandJournalError::StaleRestartOrdinal { .. }
        | ProviderCommandJournalError::StaleDispatchEpoch { .. } => {
            Some("machine_teardown_command_stale")
        }
        ProviderCommandJournalError::SkippedRestartOrdinal { .. }
        | ProviderCommandJournalError::SkippedDispatchEpoch { .. }
        | ProviderCommandJournalError::CrossedClaim
        | ProviderCommandJournalError::RetryWithoutAuthority
        | ProviderCommandJournalError::PriorEffectUnresolved => {
            Some("machine_teardown_epoch_invalid")
        }
        ProviderCommandJournalError::Corrupt { .. } | ProviderCommandJournalError::Store { .. } => {
            None
        }
    };
    code.map_or_else(
        || ambiguous_outcome(mode),
        |code| definite_outcome(mode, failure(code, error.to_string())),
    )
}

fn definite_outcome(
    mode: WorkloadTeardownCommandMode,
    failure: WorkloadFailureEvidence,
) -> WorkloadTeardownProviderOutcome {
    match mode {
        WorkloadTeardownCommandMode::Execute => WorkloadTeardownProviderOutcome::Execute(
            WorkloadTeardownExecuteOutcome::DefiniteFailure(failure),
        ),
        WorkloadTeardownCommandMode::Inspect => WorkloadTeardownProviderOutcome::Inspect(
            WorkloadTeardownInspectOutcome::DefiniteFailure(failure),
        ),
    }
}

fn ambiguous_outcome(mode: WorkloadTeardownCommandMode) -> WorkloadTeardownProviderOutcome {
    match mode {
        WorkloadTeardownCommandMode::Execute => {
            WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Ambiguous)
        }
        WorkloadTeardownCommandMode::Inspect => {
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous)
        }
    }
}

fn failure(code: &str, evidence: impl AsRef<[u8]>) -> WorkloadFailureEvidence {
    WorkloadFailureEvidence::new(code, WorkloadOwnerEvidenceDigest::sha256(evidence))
        .expect("static forwarded teardown failure code is valid")
}
