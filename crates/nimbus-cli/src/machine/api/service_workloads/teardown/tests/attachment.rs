use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use nimbus_core::{Error, TenantId, WorkloadId};
use nimbus_machine::api::MachineApiWorkloadTeardownCommandEnvelopeInput;
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration, NetworkCapabilityBundle,
    NetworkCapabilityRequirements, NetworkCapabilitySelection, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkManagementMode,
    NetworkProviderId, NetworkResourceGeneration, NetworkSovereigntyCapabilities,
    NetworkSovereigntyRequirements, PublishedEndpointId,
};
use nimbus_node::{
    HostExecutionDrainProvider, HostExecutionStopProvider, HostLifecycleBackend,
    HostLifecycleFuture, HostLifecyclePlan, HostLifecycleRequest, HostLifecycleStatus,
    HostTeardownExecuteClaim, HostTeardownExecuteObservation, HostTeardownFuture,
    HostTeardownInspectClaim, HostTeardownInspectObservation,
};
use nimbus_sandbox::{
    ProviderCommandAttemptJournal, ProviderCommandClaim, ProviderCommandClaimDecision,
    ProviderCommandObservation, ProviderCommandObservationKind, ProviderCommandOperation,
    backends::container::{
        ContainerSandboxBackend, ContainerSandboxBackendConfig, OciMachinePortForwarderConfig,
    },
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState,
    LocalEnforcementBinding, NodeIdentity, WorkloadActivationIntent, WorkloadAdmissionEvidence,
    WorkloadDesiredDigest, WorkloadExecutableEncoding, WorkloadExecutableIntent,
    WorkloadExecutionReference, WorkloadGeneration, WorkloadNetworkAttachmentBlueprint,
    WorkloadNetworkIntent, WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity,
    WorkloadNetworkReference, WorkloadOwnerEvidenceDigest, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadPublicationIntent, WorkloadSagaIntent,
    WorkloadSagaKey, WorkloadSagaRevision, WorkloadSagaTransitionId, WorkloadTeardownAttempt,
    WorkloadTeardownAttemptInput, WorkloadTeardownClaim, WorkloadTeardownCommandId,
    WorkloadTeardownCommandMode, WorkloadTeardownProviderTarget, WorkloadTeardownReceipt,
    WorkloadTeardownReceiptPrefix, WorkloadTeardownResultConfirmation, WorkloadTeardownStep,
    WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
};
use serde_json::json;

use super::super::attachment::{
    effect_can_still_start_observation_for_test, expected_forwarder_for_test,
    lower_attachment_command_for_test, prior_claim_for_test,
    require_prior_journal_success_for_test,
};
use super::super::*;

struct AttachmentFixture {
    intent: WorkloadSagaIntent,
    key: WorkloadSagaKey,
    execution: WorkloadExecutionReference,
    network: WorkloadNetworkReference,
    node: NodeIdentity,
    forwarder: MachineForwarderAuthority,
}

impl AttachmentFixture {
    fn new() -> Self {
        Self::with_attachment_provider(
            crate::machine::backend::provision::forwarded_machine_attachment_provider_id(),
        )
    }

    fn with_attachment_provider(attachment_provider: NetworkProviderId) -> Self {
        let tenant_id = TenantId::new("tenant-guest-attachment").unwrap();
        let generation = WorkloadGeneration::new(3);
        let node = NodeIdentity::new("node-guest-attachment").unwrap();
        let ingress_provider = NetworkProviderId::for_registration_key("guest-attachment-ingress");
        let selection =
            NetworkCapabilitySelection::new(attachment_provider.clone(), ingress_provider.clone());
        let selection_evidence = NetworkCapabilityBundle::new(
            NetworkAttachmentProviderRegistration::new(
                attachment_provider.clone(),
                NetworkAttachmentCapabilitySet::new(NetworkManagementMode::ProviderManaged, [], []),
                [],
                NetworkLifecycleCapabilitySet::new([]),
                NetworkSovereigntyCapabilities::new(
                    NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
            ),
            NetworkIngressProviderRegistration::new(
                ingress_provider,
                NetworkEndpointCapabilitySet::new([], [], [], [], []),
                NetworkIngressCapabilitySet::new([]),
                NetworkForwardingCapabilitySet::new([]),
                NetworkLifecycleCapabilitySet::new([]),
                NetworkSovereigntyCapabilities::new(
                    NetworkControlPlaneLocality::LocalOnly,
                    [],
                    true,
                ),
            ),
        )
        .selection_evidence();
        let identity = WorkloadNetworkPlanIdentity::new(
            tenant_id.clone(),
            "guest-attachment-incarnation",
            NetworkResourceGeneration::new(generation.as_u64()),
        )
        .unwrap();
        let requirements = NetworkCapabilityRequirements::new(
            NetworkAttachmentCapabilitySet::new(NetworkManagementMode::ProviderManaged, [], []),
            NetworkEndpointCapabilitySet::new([], [], [], [], []),
            NetworkIngressCapabilitySet::new([]),
            NetworkForwardingCapabilitySet::new([]),
            nimbus_network::NetworkLifecycleRequirements::new(
                NetworkLifecycleCapabilitySet::new([]),
                NetworkLifecycleCapabilitySet::new([]),
            ),
            NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
        );
        let content = WorkloadNetworkPlanContent::new(
            identity.clone(),
            requirements,
            Some(selection),
            Some(selection_evidence),
            Some(WorkloadNetworkAttachmentBlueprint::new(&identity, "primary").unwrap()),
            [],
            [],
            [],
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        )
        .unwrap();
        let compiled = CompiledWorkloadNetworkPlan::from_content(content).unwrap();
        let executable = WorkloadExecutableIntent::new(
            WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            r#"{"fixture":"guest-attachment"}"#,
        )
        .unwrap();
        let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
            WorkloadProvisionSourceIdentity::standalone_sandbox(
                "guest-attachment-workload",
                "guest-attachment-profile",
            )
            .unwrap(),
            WorkloadProvisionSourceGeneration::new(1),
            WorkloadProvisionSourceResourceVersion::new("guest-attachment-version").unwrap(),
            executable.content_digest(),
            attachment_provider,
            crate::machine::backend::provision::forwarded_machine_execution_provider_id(),
        )
        .unwrap();
        let intent = WorkloadSagaIntent::new_without_automatic_restart(
            DesiredWorkloadKind::Sandbox,
            DesiredWorkloadState::Running,
            generation,
            executable,
            source,
            WorkloadNetworkIntent::new(compiled),
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
            WorkloadAdmissionEvidence::new(
                format!("tid_{}", "c".repeat(64)).try_into().unwrap(),
                format!("twu_{}", "d".repeat(64)).try_into().unwrap(),
                node.clone(),
            ),
        )
        .unwrap();
        let key = WorkloadSagaKey::new(
            tenant_id,
            WorkloadId::new("guest-attachment-workload").unwrap(),
        );
        let execution = WorkloadExecutionReference::for_intent(&intent);
        let network = WorkloadNetworkReference::for_intent(&intent);
        let forwarder = MachineForwarderAuthority::new(
            OciMachinePortForwarderConfig::gvproxy_provider_handle("guest-attachment-forwarder")
                .unwrap(),
            NetworkResourceGeneration::new(11),
        );
        Self {
            intent,
            key,
            execution,
            network,
            node,
            forwarder,
        }
    }

    fn transition(&self, ordinal: usize) -> WorkloadSagaTransitionId {
        let value = (b'a' + u8::try_from(ordinal % 6).unwrap()) as char;
        format!("wst_{}", value.to_string().repeat(64))
            .try_into()
            .unwrap()
    }

    fn attempt(&self, step: WorkloadTeardownStep) -> WorkloadTeardownAttempt {
        let index = step_index(step);
        let (source_phase, target_phase) = step.phases();
        WorkloadTeardownAttempt::new(WorkloadTeardownAttemptInput {
            key: self.key.clone(),
            saga_id: self.key.saga_id(),
            issuing_revision: WorkloadSagaRevision::new((index * 2 + 1) as u64),
            issuing_transition_id: self.transition(index * 2),
            generation: self.intent.generation(),
            desired_digest: self.intent.desired_digest(),
            required_node: self.node.clone(),
            source_digest: self.intent.source().source_digest(),
            execution_provider_id: self.intent.source().execution_provider_id().clone(),
            network_plan_digest: self.intent.network().digest(),
            selection_evidence: self
                .intent
                .network()
                .compiled_plan()
                .content()
                .capability_selection_evidence()
                .cloned(),
            cause: nimbus_workloads::WorkloadTeardownCause::Successor {
                generation: WorkloadGeneration::new(self.intent.generation().as_u64() + 1),
                desired_digest: WorkloadDesiredDigest::sha256("guest-attachment-successor"),
            },
            successor_fence: None,
            source_phase,
            target_phase,
            step,
            subjects: match step {
                WorkloadTeardownStep::WithdrawPublication => WorkloadTeardownSubjects::Publication(
                    nimbus_workloads::WorkloadPublicationReference::new(
                        [PublishedEndpointId::for_workload_endpoint(
                            "guest-attachment-incarnation",
                            "api",
                        )],
                        &self.intent,
                    )
                    .unwrap(),
                ),
                WorkloadTeardownStep::DrainExecution | WorkloadTeardownStep::StopExecution => {
                    WorkloadTeardownSubjects::Execution(self.execution.clone())
                }
                WorkloadTeardownStep::DetachNetwork | WorkloadTeardownStep::ReleaseNetwork => {
                    WorkloadTeardownSubjects::Network(self.network.clone())
                }
            },
        })
        .unwrap()
    }

    fn claim(&self, step: WorkloadTeardownStep) -> WorkloadTeardownClaim {
        let attempt = self.attempt(step);
        let provider_target = WorkloadTeardownProviderTarget::for_attempt(&attempt)
            .unwrap()
            .unwrap();
        serde_json::from_value(json!({
            "attempt": attempt,
            "claimedRevision": attempt.issuing_revision().checked_next().unwrap(),
            "dispatchEpoch": "0",
            "providerTarget": provider_target,
            "authorization": { "kind": "initial" },
        }))
        .unwrap()
    }

    fn receipt(
        &self,
        step: WorkloadTeardownStep,
        override_evidence: Option<(WorkloadTeardownStep, WorkloadOwnerEvidenceDigest)>,
    ) -> WorkloadTeardownReceipt {
        let claim = self.claim(step);
        let digest = override_evidence
            .filter(|(candidate, _)| *candidate == step)
            .map_or_else(
                || WorkloadOwnerEvidenceDigest::sha256(format!("prior-{step:?}")),
                |(_, evidence)| evidence,
            );
        let evidence = match claim.attempt().subjects() {
            WorkloadTeardownSubjects::Publication(reference) => {
                WorkloadTeardownSuccessEvidence::PublicationAbsent {
                    reference: reference.clone(),
                    evidence: digest,
                }
            }
            WorkloadTeardownSubjects::Execution(reference) => match step {
                WorkloadTeardownStep::DrainExecution => {
                    WorkloadTeardownSuccessEvidence::ExecutionDrained {
                        reference: reference.clone(),
                        evidence: digest,
                    }
                }
                WorkloadTeardownStep::StopExecution => {
                    WorkloadTeardownSuccessEvidence::ExecutionStopped {
                        reference: reference.clone(),
                        evidence: digest,
                    }
                }
                _ => unreachable!(),
            },
            WorkloadTeardownSubjects::Network(reference) => match step {
                WorkloadTeardownStep::DetachNetwork => {
                    WorkloadTeardownSuccessEvidence::NetworkDetached {
                        reference: reference.clone(),
                        evidence: digest,
                    }
                }
                WorkloadTeardownStep::ReleaseNetwork => {
                    WorkloadTeardownSuccessEvidence::NetworkReleased {
                        reference: reference.clone(),
                        evidence: digest,
                    }
                }
                _ => unreachable!(),
            },
        };
        serde_json::from_value(json!({
            "claim": claim,
            "evidence": evidence,
            "confirmation": WorkloadTeardownResultConfirmation::Dispatch,
        }))
        .unwrap()
    }

    fn command(
        &self,
        step: WorkloadTeardownStep,
        mode: WorkloadTeardownCommandMode,
        override_evidence: Option<(WorkloadTeardownStep, WorkloadOwnerEvidenceDigest)>,
    ) -> MachineApiWorkloadTeardownCommandEnvelope {
        let claim = self.claim(step);
        let confirmed_revision = match mode {
            WorkloadTeardownCommandMode::Execute => claim.claimed_revision(),
            WorkloadTeardownCommandMode::Inspect => {
                claim.claimed_revision().checked_next().unwrap()
            }
        };
        let confirmed_transition_id = self.transition(step_index(step) * 2 + 1);
        let command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
            &claim,
            confirmed_revision,
            &confirmed_transition_id,
            mode,
        )
        .unwrap();
        let receipts: Vec<_> = all_steps()
            .into_iter()
            .take(step_index(step))
            .map(|prior| self.receipt(prior, override_evidence))
            .collect();
        let prior_receipt_prefix: WorkloadTeardownReceiptPrefix =
            serde_json::from_value(json!({ "receipts": receipts })).unwrap();
        MachineApiWorkloadTeardownCommandEnvelope::new(
            MachineApiWorkloadTeardownCommandEnvelopeInput {
                command_id,
                confirmed_revision,
                confirmed_transition_id,
                source: self.intent.source().clone(),
                compiled_network_plan: self.intent.network().compiled_plan().clone(),
                execution_locator: self.execution.clone(),
                prior_receipt_prefix,
                mode,
                claim,
                machine_forwarder_authority: self.forwarder.clone(),
                machine_provider_generation: self.forwarder.generation(),
                provider_translation:
                    MachineApiWorkloadTeardownProviderTranslation::GuestContainerAttachment,
            },
        )
        .unwrap()
    }
}

fn all_steps() -> [WorkloadTeardownStep; 5] {
    [
        WorkloadTeardownStep::WithdrawPublication,
        WorkloadTeardownStep::DrainExecution,
        WorkloadTeardownStep::StopExecution,
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownStep::ReleaseNetwork,
    ]
}

fn step_index(step: WorkloadTeardownStep) -> usize {
    all_steps()
        .iter()
        .position(|candidate| *candidate == step)
        .unwrap()
}

fn journal(root: &Path) -> ProviderCommandAttemptJournal {
    ProviderCommandAttemptJournal::open(root, "container-runtime").unwrap()
}

fn claim_and_record(
    journal: &ProviderCommandAttemptJournal,
    claim: &ProviderCommandClaim,
    kind: ProviderCommandObservationKind,
    evidence: &[u8],
) -> ProviderCommandObservation {
    assert!(matches!(
        journal.claim_dispatch_epoch(claim),
        Ok(ProviderCommandClaimDecision::ExecuteClaimed(_))
    ));
    if kind == ProviderCommandObservationKind::DefiniteFailure {
        journal
            .record_observation_with_failure_code(
                claim,
                kind,
                Some("sandbox_teardown_fixture_failure"),
                evidence,
            )
            .unwrap()
    } else {
        journal.record_observation(claim, kind, evidence).unwrap()
    }
}

fn composite_stop_evidence(claim: &ProviderCommandClaim) -> Vec<u8> {
    let systemd = ChildObservationEvidence {
        owner: "systemd",
        kind: ChildObservationKind::Succeeded,
        failure_code: None,
        evidence_sha256: WorkloadOwnerEvidenceDigest::sha256("attachment-systemd-stop"),
    };
    let container = ChildObservationEvidence {
        owner: "container",
        kind: ChildObservationKind::Succeeded,
        failure_code: None,
        evidence_sha256: WorkloadOwnerEvidenceDigest::sha256("attachment-container-stop"),
    };
    composite_evidence(
        "attachment-stop-command",
        WorkloadTeardownStep::StopExecution,
        claim,
        &systemd,
        Some(&container),
    )
    .unwrap()
}

fn assert_ambiguous(observation: &MachineApiWorkloadTeardownObservation) {
    assert!(matches!(
        observation,
        MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::Ambiguous
        ) | MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::Ambiguous
        )
    ));
}

fn assert_definite(observation: &MachineApiWorkloadTeardownObservation) {
    assert!(matches!(
        observation,
        MachineApiWorkloadTeardownObservation::Execute(
            MachineApiWorkloadTeardownExecuteObservation::DefiniteFailure { .. }
        ) | MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::DefiniteFailure { .. }
        )
    ));
}

#[test]
fn guest_attachment_lowering_binds_current_and_prior_claims_independently() {
    let fixture = AttachmentFixture::new();
    for (step, expected_operation) in [
        (
            WorkloadTeardownStep::DetachNetwork,
            ProviderCommandOperation::DetachNetwork,
        ),
        (
            WorkloadTeardownStep::ReleaseNetwork,
            ProviderCommandOperation::ReleaseNetwork,
        ),
    ] {
        let command = fixture.command(step, WorkloadTeardownCommandMode::Execute, None);
        let lowered = lower_attachment_command_for_test(&command, command.claim(), &fixture.node)
            .expect("the exact attachment claim should lower");
        assert_eq!(lowered.provider_claim().operation(), expected_operation);
        assert_eq!(lowered.tenant_id(), fixture.key.tenant_id());
        assert_eq!(
            lowered.network_plan(),
            command.compiled_network_plan().plan()
        );
        assert_eq!(
            lowered.provider_registration_key(),
            nimbus_sandbox::backends::CONTAINER_HOST_MANAGED_ATTACHMENT_PROVIDER_KEY
        );
    }

    let detach = fixture.command(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Execute,
        None,
    );
    let stop = prior_claim_for_test(&detach, &fixture.forwarder, &fixture.node).unwrap();
    assert_eq!(stop.operation(), ProviderCommandOperation::StopExecution);
    assert_eq!(
        stop.attempt_id(),
        detach
            .prior_receipt_prefix()
            .receipt_for(WorkloadTeardownStep::StopExecution)
            .unwrap()
            .claim()
            .attempt()
            .attempt_id()
            .as_str()
    );
    assert_ne!(stop.attempt_id(), detach.attempt_id().as_str());

    let release = fixture.command(
        WorkloadTeardownStep::ReleaseNetwork,
        WorkloadTeardownCommandMode::Execute,
        None,
    );
    let prior_detach = prior_claim_for_test(&release, &fixture.forwarder, &fixture.node).unwrap();
    assert_eq!(
        prior_detach.operation(),
        ProviderCommandOperation::DetachNetwork
    );
    assert_ne!(prior_detach.attempt_id(), release.attempt_id().as_str());
}

#[test]
fn guest_attachment_correlates_real_composite_stop_and_detach_results() {
    let fixture = AttachmentFixture::new();
    let root = tempfile::tempdir().unwrap();
    let journal = journal(root.path());

    let placeholder_detach = fixture.command(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Execute,
        None,
    );
    let stop_claim =
        prior_claim_for_test(&placeholder_detach, &fixture.forwarder, &fixture.node).unwrap();
    let stop_observation = claim_and_record(
        &journal,
        &stop_claim,
        ProviderCommandObservationKind::Succeeded,
        &composite_stop_evidence(&stop_claim),
    );
    let stop_digest = exact_provider_evidence(&stop_observation).unwrap();
    let detach = fixture.command(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Execute,
        Some((WorkloadTeardownStep::StopExecution, stop_digest)),
    );
    let stop_receipt = detach
        .prior_receipt_prefix()
        .receipt_for(WorkloadTeardownStep::StopExecution)
        .unwrap();
    assert_eq!(
        require_prior_journal_success_for_test(&detach, stop_receipt, &stop_claim, &journal)
            .unwrap(),
        stop_observation
    );

    let placeholder_release = fixture.command(
        WorkloadTeardownStep::ReleaseNetwork,
        WorkloadTeardownCommandMode::Execute,
        None,
    );
    let detach_claim =
        prior_claim_for_test(&placeholder_release, &fixture.forwarder, &fixture.node).unwrap();
    let detach_observation = claim_and_record(
        &journal,
        &detach_claim,
        ProviderCommandObservationKind::Succeeded,
        b"exact forwarded detached evidence",
    );
    let detach_digest = exact_provider_evidence(&detach_observation).unwrap();
    let release = fixture.command(
        WorkloadTeardownStep::ReleaseNetwork,
        WorkloadTeardownCommandMode::Execute,
        Some((WorkloadTeardownStep::DetachNetwork, detach_digest)),
    );
    let detach_receipt = release
        .prior_receipt_prefix()
        .receipt_for(WorkloadTeardownStep::DetachNetwork)
        .unwrap();
    assert_eq!(
        require_prior_journal_success_for_test(&release, detach_receipt, &detach_claim, &journal)
            .unwrap(),
        detach_observation
    );
    let records: Vec<_> = snapshot(root.path())
        .into_keys()
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    assert_eq!(records.len(), 2, "Stop and Detach need two exact streams");
    assert_eq!(
        records[0].parent(),
        records[1].parent(),
        "both streams must remain in the one Container-rooted namespace"
    );
}

#[test]
fn guest_attachment_prior_journal_states_fail_closed() {
    for kind in [
        ProviderCommandObservationKind::Claimed,
        ProviderCommandObservationKind::InProgress,
        ProviderCommandObservationKind::Ambiguous,
        ProviderCommandObservationKind::DefiniteFailure,
        ProviderCommandObservationKind::Absent,
        ProviderCommandObservationKind::RetryAuthorized,
    ] {
        let fixture = AttachmentFixture::new();
        let root = tempfile::tempdir().unwrap();
        let journal = journal(root.path());
        let command = fixture.command(
            WorkloadTeardownStep::DetachNetwork,
            WorkloadTeardownCommandMode::Execute,
            None,
        );
        let claim = prior_claim_for_test(&command, &fixture.forwarder, &fixture.node).unwrap();
        if kind == ProviderCommandObservationKind::Claimed {
            assert!(matches!(
                journal.claim_dispatch_epoch(&claim),
                Ok(ProviderCommandClaimDecision::ExecuteClaimed(_))
            ));
        } else {
            claim_and_record(&journal, &claim, kind, b"non-success prior state");
        }
        let receipt = command
            .prior_receipt_prefix()
            .receipt_for(WorkloadTeardownStep::StopExecution)
            .unwrap();
        let error = require_prior_journal_success_for_test(&command, receipt, &claim, &journal)
            .unwrap_err();
        if matches!(
            kind,
            ProviderCommandObservationKind::Claimed
                | ProviderCommandObservationKind::InProgress
                | ProviderCommandObservationKind::Ambiguous
        ) {
            assert_ambiguous(&error);
        } else {
            assert_definite(&error);
        }
    }

    let fixture = AttachmentFixture::new();
    let root = tempfile::tempdir().unwrap();
    let missing_journal = journal(root.path());
    let command = fixture.command(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Execute,
        None,
    );
    let claim = prior_claim_for_test(&command, &fixture.forwarder, &fixture.node).unwrap();
    let receipt = command
        .prior_receipt_prefix()
        .receipt_for(WorkloadTeardownStep::StopExecution)
        .unwrap();
    assert_ambiguous(
        &require_prior_journal_success_for_test(&command, receipt, &claim, &missing_journal)
            .unwrap_err(),
    );

    let corrupt_root = tempfile::tempdir().unwrap();
    let corrupt_journal = journal(corrupt_root.path());
    claim_and_record(
        &corrupt_journal,
        &claim,
        ProviderCommandObservationKind::Succeeded,
        b"terminal evidence that will lose its digest",
    );
    let record = snapshot(corrupt_root.path())
        .into_keys()
        .find(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .unwrap();
    let record = corrupt_root.path().join(record);
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&record).unwrap()).unwrap();
    value["observation"]
        .as_object_mut()
        .unwrap()
        .remove("evidenceSha256");
    fs::write(&record, serde_json::to_vec_pretty(&value).unwrap()).unwrap();
    assert_ambiguous(
        &require_prior_journal_success_for_test(&command, receipt, &claim, &corrupt_journal)
            .unwrap_err(),
    );
}

#[test]
fn guest_attachment_rejects_crossed_receipt_provider_forwarder_node_and_step() {
    let fixture = AttachmentFixture::new();
    let root = tempfile::tempdir().unwrap();
    let journal = journal(root.path());
    let placeholder = fixture.command(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Execute,
        None,
    );
    let prior = prior_claim_for_test(&placeholder, &fixture.forwarder, &fixture.node).unwrap();
    claim_and_record(
        &journal,
        &prior,
        ProviderCommandObservationKind::Succeeded,
        b"exact prior success with another receipt digest",
    );
    let receipt = placeholder
        .prior_receipt_prefix()
        .receipt_for(WorkloadTeardownStep::StopExecution)
        .unwrap();
    assert_definite(
        &require_prior_journal_success_for_test(&placeholder, receipt, &prior, &journal)
            .unwrap_err(),
    );

    let crossed_provider = AttachmentFixture::with_attachment_provider(
        NetworkProviderId::for_registration_key("crossed-parent-attachment"),
    );
    let crossed_command = crossed_provider.command(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Execute,
        None,
    );
    assert_definite(
        &lower_attachment_command_for_test(
            &crossed_command,
            crossed_command.claim(),
            &crossed_provider.node,
        )
        .unwrap_err(),
    );

    let stale_forwarder = MachineForwarderAuthority::new(
        fixture.forwarder.provider_instance().clone(),
        NetworkResourceGeneration::new(fixture.forwarder.generation().as_u64() + 1),
    );
    assert_definite(&expected_forwarder_for_test(&placeholder, &stale_forwarder).unwrap_err());
    assert_definite(
        &lower_attachment_command_for_test(
            &placeholder,
            placeholder.claim(),
            &NodeIdentity::new("crossed-attachment-node").unwrap(),
        )
        .unwrap_err(),
    );
    let stop_claim = placeholder
        .prior_receipt_prefix()
        .receipt_for(WorkloadTeardownStep::StopExecution)
        .unwrap()
        .claim();
    assert_definite(
        &lower_attachment_command_for_test(&placeholder, stop_claim, &fixture.node).unwrap_err(),
    );
}

#[test]
fn guest_attachment_live_claim_inspection_is_in_progress() {
    let fixture = AttachmentFixture::new();
    let root = tempfile::tempdir().unwrap();
    let journal = journal(root.path());
    let command = fixture.command(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Inspect,
        None,
    );
    let lowered = lower_attachment_command_for_test(&command, command.claim(), &fixture.node)
        .expect("the exact Inspect command should lower");
    let decision = journal
        .claim_dispatch_epoch(lowered.provider_claim())
        .expect("the fixture current claim should persist");
    let ProviderCommandClaimDecision::ExecuteClaimed(execution) = decision else {
        panic!("the fresh stream must grant one execute claim");
    };
    let observation = effect_can_still_start_observation_for_test(execution.observation());
    assert!(matches!(
        observation,
        MachineApiWorkloadTeardownObservation::Inspect(
            MachineApiWorkloadTeardownInspectObservation::InProgress { .. }
        )
    ));
}

#[tokio::test]
async fn guest_attachment_preflight_failure_never_claims_current_stream() {
    let fixture = AttachmentFixture::new();
    let root = tempfile::tempdir().unwrap();
    let state_root = root.path().join("state");
    let backend = Arc::new(ContainerSandboxBackend::new(
        ContainerSandboxBackendConfig::plan_only(root.path().join("bundles"), &state_root),
    ));
    let journal = backend.attempt_idempotency_journal().unwrap();
    let placeholder = fixture.command(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Execute,
        None,
    );
    let prior = prior_claim_for_test(&placeholder, &fixture.forwarder, &fixture.node).unwrap();
    let prior_observation = claim_and_record(
        &journal,
        &prior,
        ProviderCommandObservationKind::Succeeded,
        &composite_stop_evidence(&prior),
    );
    let command = fixture.command(
        WorkloadTeardownStep::DetachNetwork,
        WorkloadTeardownCommandMode::Execute,
        Some((
            WorkloadTeardownStep::StopExecution,
            exact_provider_evidence(&prior_observation).unwrap(),
        )),
    );
    let provider = Arc::new(NoopHostProvider);
    let service = GuestNodeWorkloadService::new_for_teardown_test(
        fixture.node.clone(),
        provider,
        Arc::clone(&backend),
        &state_root,
    );
    let before = snapshot(&state_root);
    let observation = super::super::dispatch(&service, &command, &fixture.forwarder)
        .await
        .unwrap();
    assert_ambiguous(&observation);
    assert_eq!(
        snapshot(&state_root),
        before,
        "missing manifest preflight must run before current claim creation"
    );
    let current = lower_attachment_command_for_test(&command, command.claim(), &fixture.node)
        .expect("the current command should lower");
    assert!(
        journal
            .adopt_exact_attempt(current.provider_claim())
            .unwrap()
            .is_none(),
        "preflight uncertainty must leave the current stream absent"
    );
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn collect(root: &Path, path: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return;
        };
        if metadata.is_file() {
            out.insert(
                path.strip_prefix(root).unwrap().to_owned(),
                fs::read(path).unwrap(),
            );
        } else if metadata.is_dir() {
            let mut children: Vec<_> = fs::read_dir(path)
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect();
            children.sort();
            for child in children {
                collect(root, &child, out);
            }
        }
    }
    let mut result = BTreeMap::new();
    collect(root, root, &mut result);
    result
}

struct NoopHostProvider;

impl HostLifecycleBackend for NoopHostProvider {
    fn validate(
        &self,
        _binding: &LocalEnforcementBinding,
        _request: HostLifecycleRequest,
    ) -> nimbus_core::Result<HostLifecyclePlan> {
        Err(Error::PermissionDenied(
            "attachment preflight must not call the host provider".to_owned(),
        ))
    }

    fn stop<'a>(
        &'a self,
        _execution_id: nimbus_workloads::WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "attachment preflight must not call coarse stop".to_owned(),
            ))
        })
    }

    fn inspect<'a>(
        &'a self,
        _execution_id: nimbus_workloads::WorkloadExecutionId,
    ) -> HostLifecycleFuture<'a, HostLifecycleStatus> {
        Box::pin(async {
            Err(Error::PermissionDenied(
                "attachment preflight must not call host inspection".to_owned(),
            ))
        })
    }
}

impl HostExecutionDrainProvider for NoopHostProvider {
    fn execute_drain<'a>(
        &'a self,
        _claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation> {
        Box::pin(async { HostTeardownExecuteObservation::Ambiguous })
    }

    fn inspect_drain<'a>(
        &'a self,
        _claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation> {
        Box::pin(async { HostTeardownInspectObservation::Ambiguous })
    }
}

impl HostExecutionStopProvider for NoopHostProvider {
    fn execute_stop<'a>(
        &'a self,
        _claim: HostTeardownExecuteClaim,
    ) -> HostTeardownFuture<'a, HostTeardownExecuteObservation> {
        Box::pin(async { HostTeardownExecuteObservation::Ambiguous })
    }

    fn inspect_stop<'a>(
        &'a self,
        _claim: HostTeardownInspectClaim,
    ) -> HostTeardownFuture<'a, HostTeardownInspectObservation> {
        Box::pin(async { HostTeardownInspectObservation::Ambiguous })
    }
}
