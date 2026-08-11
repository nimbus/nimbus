use std::net::{IpAddr, Ipv4Addr};
use std::num::NonZeroU16;

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    EndpointProtocol, NetworkAttachmentCapabilitySet, NetworkAttachmentProviderRegistration,
    NetworkCapabilityBundle, NetworkCapabilityRequirements, NetworkCapabilitySelection,
    NetworkControlPlaneLocality, NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet,
    NetworkIngressCapabilitySet, NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet,
    NetworkManagementMode, NetworkProviderId, NetworkResourceGeneration,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements, NetworkTlsBehavior,
    PublishedEndpointId,
};
use serde_json::json;

use super::*;
use crate::{
    CompiledWorkloadNetworkPlan, WorkloadNetworkAttachmentBlueprint,
    WorkloadNetworkDependencyListenerBlueprint, WorkloadNetworkEndpointSemantics,
    WorkloadNetworkForwardingBehavior, WorkloadNetworkListenerBlueprint,
    WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity, WorkloadNetworkPortRequestMode,
};

const TWO_TO_53: u64 = 9_007_199_254_740_992;

fn tenant(label: &str) -> TenantId {
    TenantId::new(label).expect("fixture tenant should validate")
}

fn key(tenant_label: &str, workload_label: &str) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant(tenant_label),
        WorkloadId::new(workload_label).expect("fixture workload should validate"),
    )
}

fn digest_text(byte: u8) -> String {
    format!("{byte:02x}").repeat(32)
}

fn decision(byte: u8) -> TenantIsolationDecisionId {
    format!("tid_{}", digest_text(byte))
        .try_into()
        .expect("fixture decision id should validate")
}

fn workload_uid(byte: u8) -> TenantWorkloadUid {
    format!("twu_{}", digest_text(byte))
        .try_into()
        .expect("fixture workload uid should validate")
}

fn compiled_network_plan(
    tenant_id: &TenantId,
    workload_label: &str,
    generation: u64,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    seed: u8,
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        workload_label,
        NetworkResourceGeneration::new(generation),
    )
    .expect("network identity should validate");
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        nimbus_network::NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let dependency = WorkloadNetworkDependencyListenerBlueprint::new(
        &identity,
        format!("dependency-{seed}"),
        NetworkProviderId::for_registration_key(&format!("provider-{seed}")),
    )
    .expect("network dependency should validate");
    let (selection, selection_evidence, attachment, listeners) = if publication
        == WorkloadPublicationIntent::PublishWhenReady
    {
        let attachment_provider =
            NetworkProviderId::for_registration_key(&format!("provider-{seed}"));
        let ingress_provider = NetworkProviderId::for_registration_key(&format!("ingress-{seed}"));
        let selection =
            NetworkCapabilitySelection::new(attachment_provider.clone(), ingress_provider.clone());
        let selection_evidence = NetworkCapabilityBundle::new(
            NetworkAttachmentProviderRegistration::new(
                attachment_provider,
                NetworkAttachmentCapabilitySet::new(
                    NetworkManagementMode::NimbusHostManaged,
                    [],
                    [],
                ),
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
        let attachment = WorkloadNetworkAttachmentBlueprint::new(&identity, "default")
            .expect("attachment should validate");
        let listener = WorkloadNetworkListenerBlueprint::new(
            &identity,
            "http",
            EndpointProtocol::Http,
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            WorkloadNetworkPortRequestMode::exact(
                NonZeroU16::new(8_080).expect("fixture port is non-zero"),
            ),
            WorkloadNetworkEndpointSemantics::new(
                WorkloadNetworkForwardingBehavior::None,
                NetworkTlsBehavior::Disabled,
            ),
            None,
        )
        .expect("listener should validate");
        (
            Some(selection),
            Some(selection_evidence),
            Some(attachment),
            vec![listener],
        )
    } else {
        (None, None, None, Vec::new())
    };
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        selection,
        selection_evidence,
        attachment,
        [],
        listeners,
        [dependency],
        activation,
        publication,
    )
    .expect("network content should validate");
    CompiledWorkloadNetworkPlan::from_content(content).expect("network plan should compile")
}

fn intent_with(
    tenant_label: &str,
    workload_label: &str,
    generation: u64,
    desired_state: DesiredWorkloadState,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    seed: u8,
) -> WorkloadSagaIntent {
    intent_with_restart_policy(
        tenant_label,
        workload_label,
        generation,
        desired_state,
        activation,
        publication,
        seed,
        WorkloadRestartPolicy::Never,
    )
}

#[allow(clippy::too_many_arguments)]
fn intent_with_restart_policy(
    tenant_label: &str,
    workload_label: &str,
    generation: u64,
    desired_state: DesiredWorkloadState,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    seed: u8,
    restart_policy: WorkloadRestartPolicy,
) -> WorkloadSagaIntent {
    let tenant_id = tenant(tenant_label);
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"fixtureSeed":{seed}}}"#),
    )
    .expect("fixture executable should validate");
    let source_identity = if seed.is_multiple_of(2) {
        WorkloadProvisionSourceIdentity::sandbox_backed_service(workload_label)
            .expect("service source identity should validate")
    } else {
        WorkloadProvisionSourceIdentity::standalone_sandbox(workload_label, workload_label)
            .expect("sandbox source identity should validate")
    };
    let source_version = WorkloadProvisionSourceResourceVersion::new(format!("fixture-{seed}"))
        .expect("source version should validate");
    let attachment_provider = NetworkProviderId::for_registration_key(&format!("provider-{seed}"));
    let execution_provider =
        WorkloadExecutionProviderId::for_registration_key(&format!("execution-{seed}"));
    let source = if seed.is_multiple_of(2) {
        WorkloadProvisionSourceEvidence::sandbox_backed_service(
            source_identity,
            WorkloadProvisionSourceGeneration::new(generation),
            source_version,
            executable.content_digest(),
            attachment_provider,
            execution_provider,
        )
    } else {
        WorkloadProvisionSourceEvidence::standalone_sandbox(
            source_identity,
            WorkloadProvisionSourceGeneration::new(generation),
            source_version,
            executable.content_digest(),
            attachment_provider,
            execution_provider,
        )
    }
    .expect("source evidence should validate");
    WorkloadSagaIntent::new_with_restart_policy(
        if seed.is_multiple_of(2) {
            DesiredWorkloadKind::Service
        } else {
            DesiredWorkloadKind::Sandbox
        },
        desired_state,
        WorkloadGeneration::new(generation),
        executable,
        source,
        restart_policy,
        WorkloadNetworkIntent::new(compiled_network_plan(
            &tenant_id,
            workload_label,
            generation,
            activation,
            publication,
            seed,
        )),
        activation,
        publication,
        WorkloadAdmissionEvidence::new(
            decision(seed),
            workload_uid(seed),
            NodeIdentity::new(format!("node-{seed}")).expect("node should validate"),
        ),
    )
    .expect("fixture intent should validate")
}

fn running_intent(generation: u64, publication: WorkloadPublicationIntent) -> WorkloadSagaIntent {
    intent_with(
        "tenant-a",
        "workload-a",
        generation,
        DesiredWorkloadState::Running,
        WorkloadActivationIntent::ActivateWhenAttached,
        publication,
        u8::try_from(generation % 200 + 1).expect("fixture seed should fit"),
    )
}

fn stopped_intent(generation: u64) -> WorkloadSagaIntent {
    intent_with(
        "tenant-a",
        "workload-a",
        generation,
        DesiredWorkloadState::Stopped,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        u8::try_from(generation % 200 + 1).expect("fixture seed should fit"),
    )
}

fn evidence(label: &str) -> WorkloadOwnerEvidenceDigest {
    WorkloadOwnerEvidenceDigest::sha256(label)
}

fn publication_reference(
    intent: &WorkloadSagaIntent,
    _fixture_discriminator: u128,
) -> WorkloadPublicationReference {
    WorkloadPublicationReference::new([PublishedEndpointId::generate()], intent)
        .expect("publication reference should validate")
}

fn provision_references(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    publication: Option<&WorkloadPublicationReference>,
) -> WorkloadEffectReferences {
    let settled_publication = matches!(
        phase,
        WorkloadSagaPhase::Ready | WorkloadSagaPhase::Published | WorkloadSagaPhase::Observed
    );
    let publication = if settled_publication
        && intent.publication() == WorkloadPublicationIntent::PublishWhenReady
    {
        publication.cloned()
    } else if settled_publication
        && intent.publication() == WorkloadPublicationIntent::Withheld
        && intent
            .network()
            .compiled_plan()
            .content()
            .listeners()
            .is_empty()
    {
        Some(
            WorkloadPublicationReference::new([], intent)
                .expect("zero-listener fixture needs explicit empty publication authority"),
        )
    } else {
        None
    };
    WorkloadEffectReferences::provision(intent, publication)
        .expect("fixture references should validate")
}

fn provision_observations(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
    publication: WorkloadPublicationIntent,
) -> Vec<WorkloadOwnerObservation> {
    let network = references
        .network()
        .expect("provision fixture needs network")
        .clone();
    let execution = references
        .execution()
        .expect("provision fixture needs execution")
        .clone();
    let rank = match phase {
        WorkloadSagaPhase::NetworkReserved => 1,
        WorkloadSagaPhase::WorkloadPrepared => 2,
        WorkloadSagaPhase::NetworkAttached => 3,
        WorkloadSagaPhase::WorkloadActivated => 4,
        WorkloadSagaPhase::Ready => 5,
        WorkloadSagaPhase::Published => 6,
        WorkloadSagaPhase::Observed => {
            if publication == WorkloadPublicationIntent::PublishWhenReady {
                7
            } else {
                5
            }
        }
        _ => panic!("fixture phase is not provision evidence"),
    };
    let mut observations = Vec::new();
    if rank >= 1 {
        observations.push(WorkloadOwnerObservation::NetworkReserved {
            reference: network.clone(),
            evidence: evidence("network-reserved"),
        });
    }
    if rank >= 2 {
        observations.push(WorkloadOwnerObservation::ExecutionPrepared {
            reference: execution.clone(),
            evidence: evidence("execution-prepared"),
        });
    }
    if rank >= 3 {
        observations.push(WorkloadOwnerObservation::NetworkAttached {
            reference: network.clone(),
            evidence: evidence("network-attached"),
        });
    }
    if rank >= 4 {
        observations.push(WorkloadOwnerObservation::ExecutionActivated {
            reference: execution.clone(),
            evidence: evidence("execution-activated"),
        });
    }
    if rank >= 5 {
        observations.push(WorkloadOwnerObservation::Ready {
            network,
            execution,
            evidence: evidence("ready"),
        });
    }
    if rank >= 6 {
        observations.push(WorkloadOwnerObservation::PublicationPresent {
            reference: references
                .publication()
                .expect("published fixture needs publication")
                .clone(),
            evidence: evidence("publication-present"),
        });
    }
    if rank >= 7 {
        observations.push(WorkloadOwnerObservation::PublicationObserved {
            reference: references
                .publication()
                .expect("observed fixture needs publication")
                .clone(),
            evidence: evidence("publication-observed"),
        });
    }
    observations
}

fn provision_detail(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    publication: Option<&WorkloadPublicationReference>,
) -> WorkloadPhaseDetail {
    let references = provision_references(phase, intent, publication);
    let observations = provision_observations(phase, &references, intent.publication());
    WorkloadPhaseDetail::provision(phase, intent, references, observations)
        .expect("provision fixture should validate")
}

fn advance_provision(
    record: &WorkloadSagaRecord,
    phase: WorkloadSagaPhase,
    publication: Option<&WorkloadPublicationReference>,
) -> WorkloadSagaRecord {
    let detail = provision_detail(phase, record.active_intent(), publication);
    confirm_provision_fixture(record, phase, detail)
}

fn provision_attempt_fixture(
    record: &WorkloadSagaRecord,
    step: WorkloadProvisionStep,
    target_phase: WorkloadSagaPhase,
    subjects: WorkloadProvisionSubjects,
    prerequisite: Option<WorkloadProvisionPrerequisiteEvidence>,
) -> WorkloadProvisionAttempt {
    super::test_support::provision_attempt(record, step, target_phase, subjects, prerequisite)
}

fn persist_attempt_fixture(
    record: &WorkloadSagaRecord,
    attempt: WorkloadProvisionAttempt,
) -> WorkloadSagaRecord {
    super::test_support::persist_attempt(record, attempt)
}

fn provider_target_fixture(attempt: &WorkloadProvisionAttempt) -> WorkloadProvisionProviderTarget {
    WorkloadProvisionProviderTarget::for_attempt(attempt)
        .expect("fixture provider target should validate")
        .expect("effectful fixture attempt should name a provider target")
}

fn confirm_provision_fixture(
    record: &WorkloadSagaRecord,
    target_phase: WorkloadSagaPhase,
    detail: WorkloadPhaseDetail,
) -> WorkloadSagaRecord {
    super::test_support::confirmed_provision(record, target_phase, detail)
}

fn record_at_ready(publication: WorkloadPublicationIntent) -> WorkloadSagaRecord {
    let intent = running_intent(1, publication);
    let publication_reference = publication_reference(&intent, 11);
    let mut record = WorkloadSagaRecord::new(key("tenant-a", "workload-a"), intent)
        .expect("record should initialize");
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
    ] {
        record = advance_provision(&record, phase, Some(&publication_reference));
    }
    record
}

fn terminal_observations(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    origin: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
) -> Vec<WorkloadTerminalObservation> {
    let rank = match phase {
        WorkloadSagaPhase::WithdrawalCommitted => 0,
        WorkloadSagaPhase::Withdrawn => 1,
        WorkloadSagaPhase::Drained => 2,
        WorkloadSagaPhase::WorkloadStopped => 3,
        WorkloadSagaPhase::NetworkDetached => 4,
        WorkloadSagaPhase::NetworkReleased => 5,
        _ => panic!("fixture phase is not teardown evidence"),
    };
    let mut observations = Vec::new();
    let origin_rank = origin.recovery_order();
    let provider_managed_network = intent
        .network()
        .compiled_plan()
        .content()
        .capability_selection_evidence()
        .is_some();
    if rank >= 1
        && let Some(reference) = references.publication()
        && origin_rank >= WorkloadSagaPhase::Published.recovery_order()
    {
        observations.push(WorkloadTerminalObservation::PublicationAbsent {
            reference: reference.clone(),
            evidence: evidence("publication-absent"),
        });
    }
    if rank >= 2
        && let Some(reference) = references.execution()
        && origin_rank >= WorkloadSagaPhase::WorkloadActivated.recovery_order()
    {
        observations.push(WorkloadTerminalObservation::ExecutionDrained {
            reference: reference.clone(),
            evidence: evidence("execution-drained"),
        });
    }
    if rank >= 3
        && let Some(reference) = references.execution()
        && origin_rank >= WorkloadSagaPhase::WorkloadPrepared.recovery_order()
    {
        observations.push(WorkloadTerminalObservation::ExecutionStopped {
            reference: reference.clone(),
            evidence: evidence("execution-stopped"),
        });
    }
    if rank >= 4
        && let Some(reference) = references.network()
        && provider_managed_network
        && origin_rank >= WorkloadSagaPhase::NetworkAttached.recovery_order()
    {
        observations.push(WorkloadTerminalObservation::NetworkDetached {
            reference: reference.clone(),
            evidence: evidence("network-detached"),
        });
    }
    if rank >= 5
        && let Some(reference) = references.network()
        && provider_managed_network
        && origin_rank >= WorkloadSagaPhase::NetworkReserved.recovery_order()
    {
        observations.push(WorkloadTerminalObservation::NetworkReleased {
            reference: reference.clone(),
            evidence: evidence("network-released"),
        });
    }
    observations
}

fn begin_teardown(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let successor = stopped_intent(
        record
            .successor_intent()
            .map_or(
                record.active_intent().generation(),
                WorkloadSagaIntent::generation,
            )
            .checked_next()
            .expect("fixture successor generation should not overflow")
            .as_u64(),
    );
    let WorkloadSagaIntentUpdate::Transition(withdrawal) = record
        .apply_intent(successor)
        .expect("fixture successor should commit withdrawal")
    else {
        panic!("fixture successor must produce one withdrawal transition");
    };
    *withdrawal
}

fn advance_teardown(record: &WorkloadSagaRecord, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let mut current = record.clone();
    while current.phase().recovery_order() < phase.recovery_order() {
        current = match current
            .decide_teardown()
            .expect("fixture teardown decision should validate")
        {
            WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::ResourceFree { step, .. },
            ) => current
                .record_resource_free_teardown_step(step)
                .expect("resource-free fixture step should persist"),
            WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::Claim {
                    attempt,
                    provider_target,
                },
            ) => {
                let pending = current
                    .claim_teardown(*attempt, provider_target)
                    .expect("fixture teardown claim should persist");
                let claim = pending
                    .teardown_disposition()
                    .and_then(WorkloadTeardownDisposition::claim)
                    .expect("fixture pending teardown should retain its claim")
                    .clone();
                let evidence = match (claim.attempt().step(), claim.attempt().subjects()) {
                    (
                        WorkloadTeardownStep::WithdrawPublication,
                        WorkloadTeardownSubjects::Publication(reference),
                    ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
                        reference: reference.clone(),
                        evidence: evidence("publication-absent"),
                    },
                    (
                        WorkloadTeardownStep::DrainExecution,
                        WorkloadTeardownSubjects::Execution(reference),
                    ) => WorkloadTeardownSuccessEvidence::ExecutionDrained {
                        reference: reference.clone(),
                        evidence: evidence("execution-drained"),
                    },
                    (
                        WorkloadTeardownStep::StopExecution,
                        WorkloadTeardownSubjects::Execution(reference),
                    ) => WorkloadTeardownSuccessEvidence::ExecutionStopped {
                        reference: reference.clone(),
                        evidence: evidence("execution-stopped"),
                    },
                    (
                        WorkloadTeardownStep::DetachNetwork,
                        WorkloadTeardownSubjects::Network(reference),
                    ) => WorkloadTeardownSuccessEvidence::NetworkDetached {
                        reference: reference.clone(),
                        evidence: evidence("network-detached"),
                    },
                    (
                        WorkloadTeardownStep::ReleaseNetwork,
                        WorkloadTeardownSubjects::Network(reference),
                    ) => WorkloadTeardownSuccessEvidence::NetworkReleased {
                        reference: reference.clone(),
                        evidence: evidence("network-released"),
                    },
                    _ => panic!("fixture teardown claim has a crossed step and subject"),
                };
                pending
                    .apply_teardown_effect_result(
                        &claim,
                        WorkloadTeardownEffectResult::Succeeded {
                            attempt_id: claim.attempt().attempt_id().clone(),
                            dispatch_epoch: claim.dispatch_epoch(),
                            provider_target: claim.provider_target().clone(),
                            evidence: Box::new(evidence),
                        },
                    )
                    .expect("fixture teardown success should persist")
            }
            decision => panic!("unexpected fixture teardown decision {decision:?}"),
        };
    }
    current
}

fn fail_current_teardown(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let WorkloadTeardownDecision::PersistCandidate(ProposedWorkloadTeardownTransition::Claim {
        attempt,
        provider_target,
    }) = record
        .decide_teardown()
        .expect("fixture teardown decision should validate")
    else {
        panic!("fixture teardown failure requires an effectful step");
    };
    let pending = record
        .claim_teardown(*attempt, provider_target)
        .expect("fixture teardown claim should persist");
    let claim = pending
        .teardown_disposition()
        .and_then(WorkloadTeardownDisposition::claim)
        .expect("fixture teardown claim should remain durable")
        .clone();
    pending
        .apply_teardown_effect_result(
            &claim,
            WorkloadTeardownEffectResult::DefiniteFailure {
                attempt_id: claim.attempt().attempt_id().clone(),
                dispatch_epoch: claim.dispatch_epoch(),
                provider_target: claim.provider_target().clone(),
                failure: WorkloadFailureEvidence::new(
                    "fixture_teardown_failure",
                    evidence("fixture-teardown-failure"),
                )
                .expect("fixture failure should validate"),
            },
        )
        .expect("fixture teardown failure should enter cleanup")
}

fn finish_teardown(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let mut record = record.clone();
    for phase in [
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
    ] {
        if phase.recovery_order() > record.phase().recovery_order() {
            record = advance_teardown(&record, phase);
        }
    }
    record
        .record_terminal_teardown()
        .expect("recorded transition should validate")
}

fn transition_id(record: &WorkloadSagaRecord) -> WorkloadSagaTransitionId {
    record.last_transition().transition_id().clone()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RehashedTransitionPayload {
    saga_id: WorkloadSagaId,
    expected_revision: Option<WorkloadSagaRevision>,
    resulting_revision: WorkloadSagaRevision,
    source_phase: Option<WorkloadSagaPhase>,
    target_phase: WorkloadSagaPhase,
    active_intent: WorkloadSagaIntent,
    successor_intent: Option<WorkloadSagaIntent>,
    phase_detail: WorkloadPhaseDetail,
    provision_disposition: Option<WorkloadProvisionDisposition>,
    teardown_disposition: Option<WorkloadTeardownDisposition>,
    restart: WorkloadRestartState,
    failure: Option<WorkloadFailureEvidence>,
}

fn rehash_encoded_record(record: &mut serde_json::Value) {
    let resulting_revision: WorkloadSagaRevision =
        serde_json::from_value(record["revision"].clone()).unwrap();
    let expected_revision = resulting_revision
        .as_u64()
        .checked_sub(1)
        .map(WorkloadSagaRevision::new);
    let payload = RehashedTransitionPayload {
        saga_id: serde_json::from_value(record["sagaId"].clone()).unwrap(),
        expected_revision,
        resulting_revision,
        source_phase: serde_json::from_value(record["lastTransition"]["sourcePhase"].clone())
            .unwrap(),
        target_phase: serde_json::from_value(record["phase"].clone()).unwrap(),
        active_intent: serde_json::from_value(record["activeIntent"].clone()).unwrap(),
        successor_intent: serde_json::from_value(record["successorIntent"].clone()).unwrap(),
        phase_detail: serde_json::from_value(record["phaseDetail"].clone()).unwrap(),
        provision_disposition: serde_json::from_value(record["provisionDisposition"].clone())
            .unwrap(),
        teardown_disposition: record
            .get("teardownDisposition")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .unwrap(),
        restart: serde_json::from_value(record["restart"].clone()).unwrap(),
        failure: serde_json::from_value(record["failure"].clone()).unwrap(),
    };
    let encoded_payload = serde_json::to_vec(&payload).unwrap();
    record["lastTransition"]["transitionId"] = json!(derive_id(
        WorkloadSagaTransitionId::PREFIX,
        b"nimbus.workloads.saga.transition.v5",
        &[std::str::from_utf8(&encoded_payload).unwrap()],
    ));
}

#[test]
fn running_and_stopped_intents_initialize_to_exact_terminality() {
    let running = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .unwrap();
    assert_eq!(running.revision(), WorkloadSagaRevision::new(0));
    assert_eq!(running.phase(), WorkloadSagaPhase::IntentCommitted);
    assert_eq!(running.last_transition().source_phase(), None);
    assert!(running.requires_recovery());

    let stopped =
        WorkloadSagaRecord::new(key("tenant-a", "workload-a"), stopped_intent(1)).unwrap();
    assert_eq!(stopped.phase(), WorkloadSagaPhase::Recorded);
    assert!(!stopped.requires_recovery());
    assert!(stopped.phase_detail().references().is_empty());
}

#[test]
fn provision_matrix_accepts_every_allowed_publication_branch() {
    for publication in [
        WorkloadPublicationIntent::Withheld,
        WorkloadPublicationIntent::PublishWhenReady,
    ] {
        let mut record = record_at_ready(publication);
        if publication == WorkloadPublicationIntent::PublishWhenReady {
            let publication_reference = record
                .phase_detail()
                .references()
                .publication()
                .unwrap()
                .clone();
            record = advance_provision(
                &record,
                WorkloadSagaPhase::Published,
                Some(&publication_reference),
            );
            record = advance_provision(
                &record,
                WorkloadSagaPhase::Observed,
                Some(&publication_reference),
            );
        } else {
            record = advance_provision(&record, WorkloadSagaPhase::Observed, None);
        }
        assert_eq!(record.phase(), WorkloadSagaPhase::Observed);
        assert!(!record.requires_recovery());
    }
}

#[test]
fn prepare_only_is_quiescent_at_attached_and_cannot_activate() {
    let intent = intent_with(
        "tenant-a",
        "workload-a",
        1,
        DesiredWorkloadState::Running,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        7,
    );
    let mut record = WorkloadSagaRecord::new(key("tenant-a", "workload-a"), intent).unwrap();
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
    ] {
        record = advance_provision(&record, phase, None);
    }
    assert!(!record.requires_recovery());
    let references = record.phase_detail().references();
    let observations = provision_observations(
        WorkloadSagaPhase::WorkloadActivated,
        &references,
        WorkloadPublicationIntent::Withheld,
    );
    assert!(
        WorkloadPhaseDetail::provision(
            WorkloadSagaPhase::WorkloadActivated,
            record.active_intent(),
            references,
            observations,
        )
        .is_err()
    );
}

#[test]
fn provision_evidence_rejects_missing_extra_duplicate_crossed_and_out_of_order() {
    let intent = running_intent(1, WorkloadPublicationIntent::Withheld);
    let references = provision_references(WorkloadSagaPhase::WorkloadPrepared, &intent, None);
    let valid = provision_observations(
        WorkloadSagaPhase::WorkloadPrepared,
        &references,
        intent.publication(),
    );

    assert!(
        WorkloadPhaseDetail::provision(
            WorkloadSagaPhase::WorkloadPrepared,
            &intent,
            references.clone(),
            valid[..1].to_vec(),
        )
        .is_err()
    );
    let mut duplicate = valid.clone();
    duplicate.push(valid[1].clone());
    assert!(
        WorkloadPhaseDetail::provision(
            WorkloadSagaPhase::WorkloadPrepared,
            &intent,
            references.clone(),
            duplicate,
        )
        .is_err()
    );
    let mut out_of_order = valid.clone();
    out_of_order.reverse();
    assert!(
        WorkloadPhaseDetail::provision(
            WorkloadSagaPhase::WorkloadPrepared,
            &intent,
            references.clone(),
            out_of_order,
        )
        .is_err()
    );
    let crossed = running_intent(2, WorkloadPublicationIntent::Withheld);
    assert!(
        WorkloadPhaseDetail::provision(
            WorkloadSagaPhase::WorkloadPrepared,
            &intent,
            provision_references(WorkloadSagaPhase::WorkloadPrepared, &crossed, None),
            valid,
        )
        .is_err()
    );
    let forbidden_publication = publication_reference(&intent, 99);
    assert!(
        WorkloadPhaseDetail::provision(
            WorkloadSagaPhase::Ready,
            &intent,
            WorkloadEffectReferences::provision(&intent, Some(forbidden_publication)).unwrap(),
            Vec::new(),
        )
        .is_err()
    );
}

#[test]
fn established_publication_reference_cannot_change_between_phases() {
    let ready = record_at_ready(WorkloadPublicationIntent::PublishWhenReady);
    let replacement = publication_reference(ready.active_intent(), 100);
    let attempt = provision_attempt_fixture(
        &ready,
        WorkloadProvisionStep::Publish,
        WorkloadSagaPhase::Published,
        WorkloadProvisionSubjects::Publication(
            ready
                .phase_detail()
                .references()
                .publication()
                .expect("ready fixture should retain publication")
                .clone(),
        ),
        None,
    );
    let pending = persist_attempt_fixture(&ready, attempt);
    let error = pending
        .dispatch_to_success(
            WorkloadSagaPhase::Published,
            provision_detail(
                WorkloadSagaPhase::Published,
                ready.active_intent(),
                Some(&replacement),
            ),
        )
        .unwrap_err();
    assert!(matches!(error, WorkloadSagaError::InvalidEvidence(_)));
}

#[test]
fn teardown_matrix_accepts_every_origin_and_no_op_step() {
    let intent_only = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .unwrap();
    let recorded = finish_teardown(&begin_teardown(&intent_only));
    assert_eq!(recorded.phase(), WorkloadSagaPhase::Recorded);

    for publication in [
        WorkloadPublicationIntent::Withheld,
        WorkloadPublicationIntent::PublishWhenReady,
    ] {
        let mut origin = WorkloadSagaRecord::new(
            key("tenant-a", "workload-a"),
            running_intent(1, publication),
        )
        .unwrap();
        let publication_reference = publication_reference(origin.active_intent(), 1);
        let mut origins = Vec::new();
        for phase in [
            WorkloadSagaPhase::NetworkReserved,
            WorkloadSagaPhase::WorkloadPrepared,
            WorkloadSagaPhase::NetworkAttached,
            WorkloadSagaPhase::WorkloadActivated,
            WorkloadSagaPhase::Ready,
        ] {
            origin = advance_provision(&origin, phase, Some(&publication_reference));
            origins.push(origin.clone());
        }
        if publication == WorkloadPublicationIntent::PublishWhenReady {
            origin = advance_provision(
                &origin,
                WorkloadSagaPhase::Published,
                Some(&publication_reference),
            );
            origins.push(origin.clone());
        }
        origin = advance_provision(
            &origin,
            WorkloadSagaPhase::Observed,
            Some(&publication_reference),
        );
        origins.push(origin);

        for origin in origins {
            let references = origin.phase_detail().references();
            let withdrawn =
                advance_teardown(&begin_teardown(&origin), WorkloadSagaPhase::Withdrawn);
            let WorkloadPhaseDetail::Teardown(detail) = withdrawn.phase_detail() else {
                panic!("withdrawn phase should carry teardown detail");
            };
            assert_eq!(
                !detail.terminal_observations().is_empty(),
                references.publication().is_some()
                    && origin
                        .active_intent()
                        .network()
                        .compiled_plan()
                        .content()
                        .capability_selection_evidence()
                        .is_some()
                    && origin.phase().recovery_order()
                        >= WorkloadSagaPhase::Published.recovery_order()
            );
            assert_eq!(
                finish_teardown(&withdrawn).phase(),
                WorkloadSagaPhase::Recorded
            );
        }
    }
}

#[test]
fn teardown_rejects_reference_rewrite_missing_duplicate_and_early_release() {
    let ready = record_at_ready(WorkloadPublicationIntent::PublishWhenReady);
    let withdrawal = begin_teardown(&ready);
    let WorkloadPhaseDetail::Teardown(detail) = withdrawal.phase_detail() else {
        panic!("withdrawal should carry teardown detail");
    };
    let references = detail.retained_references().clone();
    let valid = terminal_observations(
        WorkloadSagaPhase::Drained,
        ready.active_intent(),
        detail.origin(),
        &references,
    );

    assert!(
        WorkloadPhaseDetail::teardown(
            WorkloadSagaPhase::Drained,
            ready.active_intent(),
            detail.origin(),
            references.clone(),
            Vec::new(),
        )
        .is_err()
    );
    let mut duplicate = valid.clone();
    duplicate.push(valid[0].clone());
    assert!(
        WorkloadPhaseDetail::teardown(
            WorkloadSagaPhase::Drained,
            ready.active_intent(),
            detail.origin(),
            references.clone(),
            duplicate,
        )
        .is_err()
    );
    assert!(
        WorkloadPhaseDetail::teardown(
            WorkloadSagaPhase::NetworkReleased,
            ready.active_intent(),
            detail.origin(),
            references.clone(),
            terminal_observations(
                WorkloadSagaPhase::NetworkDetached,
                ready.active_intent(),
                detail.origin(),
                &references,
            ),
        )
        .is_err()
    );

    let crossed_intent = running_intent(2, WorkloadPublicationIntent::PublishWhenReady);
    let crossed = provision_references(
        WorkloadSagaPhase::Ready,
        &crossed_intent,
        Some(&publication_reference(&crossed_intent, 7)),
    );
    assert!(
        WorkloadPhaseDetail::teardown(
            WorkloadSagaPhase::WithdrawalCommitted,
            ready.active_intent(),
            ready.phase(),
            crossed,
            Vec::new(),
        )
        .is_err()
    );
}

#[test]
fn teardown_transition_retains_exact_origin_references() {
    let ready = record_at_ready(WorkloadPublicationIntent::PublishWhenReady);
    let withdrawal = begin_teardown(&ready);
    let WorkloadPhaseDetail::Teardown(_) = withdrawal.phase_detail() else {
        panic!("withdrawal should carry teardown detail");
    };
    let crossed_intent = running_intent(2, WorkloadPublicationIntent::PublishWhenReady);
    let replacement = WorkloadEffectReferences::provision(
        &crossed_intent,
        Some(publication_reference(&crossed_intent, 777)),
    )
    .unwrap();
    let mut crossed = serde_json::to_value(&withdrawal).unwrap();
    crossed["phaseDetail"]["value"]["retainedReferences"] =
        serde_json::to_value(replacement).unwrap();
    rehash_encoded_record(&mut crossed);
    assert!(serde_json::from_value::<WorkloadSagaRecord>(crossed).is_err());
}

fn cleanup_inspections(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
) -> Vec<WorkloadInspectionRequirement> {
    let mut inspections = Vec::new();
    if let Some(reference) = references.network() {
        inspections.push(WorkloadInspectionRequirement::Network {
            reference: reference.clone(),
            expected_phase: phase,
        });
    }
    if let Some(reference) = references.execution() {
        inspections.push(WorkloadInspectionRequirement::Execution {
            reference: reference.clone(),
            expected_phase: phase,
        });
    }
    if let Some(reference) = references.publication() {
        inspections.push(WorkloadInspectionRequirement::Publication {
            reference: reference.clone(),
            expected_phase: phase,
        });
    }
    inspections
}

#[test]
fn cleanup_pending_requires_exact_one_to_one_inspection_and_retains_fences() {
    let ready = record_at_ready(WorkloadPublicationIntent::PublishWhenReady);
    let references = ready.phase_detail().references();
    let inspections = cleanup_inspections(ready.phase(), &references);
    let detail = WorkloadPhaseDetail::cleanup_pending(
        ready.active_intent(),
        ready.phase(),
        references.clone(),
        inspections.clone(),
    )
    .unwrap();
    let cleanup = ready
        .advance(
            WorkloadSagaPhase::CleanupPending,
            detail,
            Some(WorkloadFailureEvidence::new("provider_timeout", evidence("failure")).unwrap()),
        )
        .unwrap();
    assert!(cleanup.requires_recovery());
    assert_eq!(cleanup.phase_detail().references(), references);
    assert!(cleanup.promote_successor().is_err());
    assert!(
        cleanup
            .record_resource_free_teardown_step(WorkloadTeardownStep::ReleaseNetwork)
            .is_err()
    );

    for invalid in [
        Vec::new(),
        inspections[..2].to_vec(),
        {
            let mut duplicate = inspections.clone();
            duplicate[2] = duplicate[1].clone();
            duplicate
        },
        {
            let mut out_of_order = inspections.clone();
            out_of_order.swap(0, 1);
            out_of_order
        },
    ] {
        assert!(
            WorkloadPhaseDetail::cleanup_pending(
                ready.active_intent(),
                ready.phase(),
                references.clone(),
                invalid,
            )
            .is_err()
        );
    }
    assert!(
        WorkloadPhaseDetail::cleanup_pending(
            ready.active_intent(),
            ready.phase(),
            WorkloadEffectReferences::default(),
            Vec::new(),
        )
        .is_err()
    );
}

#[test]
fn desired_generation_rules_distinguish_replay_divergence_stale_and_replacement() {
    let record = record_at_ready(WorkloadPublicationIntent::Withheld);
    assert_eq!(
        record.apply_intent(record.active_intent().clone()).unwrap(),
        WorkloadSagaIntentUpdate::Unchanged
    );
    let divergent = intent_with(
        "tenant-a",
        "workload-a",
        1,
        DesiredWorkloadState::Running,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        99,
    );
    assert!(matches!(
        record.apply_intent(divergent),
        Err(WorkloadSagaError::EqualGenerationConflict(_))
    ));
    assert!(matches!(
        record.apply_intent(stopped_intent(0)),
        Err(WorkloadSagaError::StaleGeneration { .. })
    ));

    let WorkloadSagaIntentUpdate::Transition(with_successor) =
        record.apply_intent(stopped_intent(2)).unwrap()
    else {
        panic!("higher generation should transition");
    };
    assert_eq!(
        with_successor.phase(),
        WorkloadSagaPhase::WithdrawalCommitted
    );
    assert_eq!(
        with_successor.successor_intent().unwrap().generation(),
        WorkloadGeneration::new(2)
    );
    let WorkloadSagaIntentUpdate::Transition(replaced) =
        with_successor.apply_intent(stopped_intent(3)).unwrap()
    else {
        panic!("still-higher generation should replace successor");
    };
    assert_eq!(
        replaced.successor_intent().unwrap().generation(),
        WorkloadGeneration::new(3)
    );
    assert_eq!(replaced.phase_detail(), with_successor.phase_detail());
}

#[test]
fn desired_digest_binds_complete_intent() {
    let intent = running_intent(1, WorkloadPublicationIntent::Withheld);
    let replacement = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        r#"{"fixture":"crossed-executable"}"#,
    )
    .expect("replacement executable should validate");
    let replacement_source = match intent.source() {
        WorkloadProvisionSourceEvidence::StandaloneSandbox { .. } => {
            WorkloadProvisionSourceEvidence::standalone_sandbox(
                intent.source().source_identity().clone(),
                intent.source().source_generation(),
                intent.source().resource_version().clone(),
                replacement.content_digest(),
                intent.source().attachment_provider_id().clone(),
                intent.source().execution_provider_id().clone(),
            )
        }
        WorkloadProvisionSourceEvidence::SandboxBackedService { .. } => {
            WorkloadProvisionSourceEvidence::sandbox_backed_service(
                intent.source().source_identity().clone(),
                intent.source().source_generation(),
                intent.source().resource_version().clone(),
                replacement.content_digest(),
                intent.source().attachment_provider_id().clone(),
                intent.source().execution_provider_id().clone(),
            )
        }
    }
    .expect("replacement source evidence should validate");
    let divergent = WorkloadSagaIntent::new_without_automatic_restart(
        intent.kind(),
        intent.desired_state(),
        intent.generation(),
        replacement,
        replacement_source,
        intent.network().clone(),
        intent.activation(),
        intent.publication(),
        intent.admission().clone(),
    )
    .expect("divergent complete desired intent should validate independently");

    let mut crossed_executable = serde_json::to_value(&divergent).unwrap();
    crossed_executable["desiredDigest"] = json!(intent.desired_digest());
    let error = serde_json::from_value::<WorkloadSagaIntent>(crossed_executable)
        .expect_err("stale digest must reject a complete executable/source replacement");
    assert!(
        error
            .to_string()
            .contains("workload desired digest does not match complete desired intent")
    );

    let mut crossed_admission = serde_json::to_value(&intent).unwrap();
    crossed_admission["admission"]["assignedNode"] = json!("node-crossed-admission");
    let error = serde_json::from_value::<WorkloadSagaIntent>(crossed_admission)
        .expect_err("crossed admission must invalidate the complete desired digest");
    assert!(
        error
            .to_string()
            .contains("workload desired digest does not match complete desired intent")
    );

    assert_ne!(divergent.desired_digest(), intent.desired_digest());
    let record = WorkloadSagaRecord::new(key("tenant-a", "workload-a"), intent).unwrap();
    assert!(matches!(
        record.apply_intent(divergent),
        Err(WorkloadSagaError::EqualGenerationConflict(_))
    ));
}

#[test]
fn exact_successor_retains_executable() {
    let active = record_at_ready(WorkloadPublicationIntent::Withheld);
    let successor = stopped_intent(2);
    assert_ne!(
        active.active_intent().executable(),
        successor.executable(),
        "fixture must distinguish active and successor executable content"
    );

    let WorkloadSagaIntentUpdate::Transition(with_successor) =
        active.apply_intent(successor.clone()).unwrap()
    else {
        panic!("higher generation should queue one exact successor");
    };
    let durable: WorkloadSagaRecord =
        serde_json::from_value(serde_json::to_value(with_successor).unwrap())
            .expect("queued successor should round-trip through strict durable shape");
    let retained = durable
        .successor_intent()
        .expect("successor should remain queued");
    assert_eq!(retained.executable(), successor.executable());
    assert_eq!(
        retained.executable().canonical_content(),
        successor.executable().canonical_content()
    );
    assert_eq!(
        retained.executable().content_digest(),
        successor.executable().content_digest()
    );

    let mut crossed = serde_json::to_value(&durable).unwrap();
    crossed["successorIntent"]["executable"] = serde_json::to_value(
        WorkloadExecutableIntent::new(
            WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            r#"{"fixture":"crossed-successor"}"#,
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        serde_json::from_value::<WorkloadSagaRecord>(crossed).is_err(),
        "a crossed queued successor must fail before a transition exists"
    );

    let mut unknown = serde_json::to_value(&durable).unwrap();
    unknown["successorIntent"]["compatibilityCache"] = json!("forbidden");
    assert!(
        serde_json::from_value::<WorkloadSagaRecord>(unknown).is_err(),
        "an unknown queued-successor field must fail strict intent decoding"
    );

    let promoted = finish_teardown(&durable).promote_successor().unwrap();
    assert_eq!(promoted.active_intent(), &successor);
    assert_eq!(
        promoted.active_intent().executable(),
        successor.executable()
    );
}

#[test]
fn recorded_promotes_exact_running_or_stopped_successor_without_old_fences() {
    for successor in [
        running_intent(2, WorkloadPublicationIntent::Withheld),
        stopped_intent(2),
    ] {
        let active = record_at_ready(WorkloadPublicationIntent::Withheld);
        let WorkloadSagaIntentUpdate::Transition(with_successor) =
            active.apply_intent(successor.clone()).unwrap()
        else {
            panic!("successor should queue");
        };
        let recorded = finish_teardown(&with_successor);
        assert!(recorded.phase_detail().references().is_empty());
        let promoted = recorded.promote_successor().unwrap();
        assert_eq!(promoted.active_intent(), &successor);
        assert!(promoted.successor_intent().is_none());
        assert!(promoted.phase_detail().references().is_empty());
        assert_eq!(
            promoted.phase(),
            if successor.desired_state() == DesiredWorkloadState::Running {
                WorkloadSagaPhase::IntentCommitted
            } else {
                WorkloadSagaPhase::Recorded
            }
        );
    }
}

#[test]
fn recorded_accepts_direct_higher_intent_and_queued_successor_cannot_be_skipped() {
    let recorded =
        WorkloadSagaRecord::new(key("tenant-a", "workload-a"), stopped_intent(1)).unwrap();
    let WorkloadSagaIntentUpdate::Transition(direct) = recorded
        .apply_intent(running_intent(2, WorkloadPublicationIntent::Withheld))
        .unwrap()
    else {
        panic!("direct higher intent should promote");
    };
    assert_eq!(direct.phase(), WorkloadSagaPhase::IntentCommitted);

    let active = record_at_ready(WorkloadPublicationIntent::Withheld);
    let WorkloadSagaIntentUpdate::Transition(queued) =
        active.apply_intent(stopped_intent(2)).unwrap()
    else {
        panic!("successor should queue");
    };
    let recorded = finish_teardown(&queued);
    let WorkloadSagaIntentUpdate::Transition(replaced) = recorded
        .apply_intent(running_intent(3, WorkloadPublicationIntent::Withheld))
        .unwrap()
    else {
        panic!("later successor should replace without skipping lifecycle state");
    };
    assert_eq!(replaced.phase(), WorkloadSagaPhase::Recorded);
    assert_eq!(replaced.active_intent(), recorded.active_intent());
    assert_eq!(
        replaced.successor_intent().unwrap().generation(),
        WorkloadGeneration::new(3)
    );
}

#[test]
fn recorded_successor_remains_recoverable_across_promotion_crash_window() {
    let active = record_at_ready(WorkloadPublicationIntent::Withheld);
    let WorkloadSagaIntentUpdate::Transition(queued) =
        active.apply_intent(stopped_intent(2)).unwrap()
    else {
        panic!("successor should queue");
    };
    let recorded = finish_teardown(&queued);
    assert_eq!(recorded.phase(), WorkloadSagaPhase::Recorded);
    assert!(recorded.successor_intent().is_some());
    assert!(recorded.requires_recovery());

    let cursor = crate::WorkloadSagaRecoveryCursor::for_record(&recorded).unwrap();
    let request = crate::WorkloadSagaPageRequest::new(None, 1).unwrap();
    let page = crate::WorkloadSagaPage::new(&request, vec![recorded.clone()], true).unwrap();
    assert_eq!(page.next_cursor(), Some(&cursor));

    let promoted = recorded.promote_successor().unwrap();
    assert!(promoted.successor_intent().is_none());
}

#[test]
fn validated_record_rejects_cleanup_successor_replacement() {
    let active = record_at_ready(WorkloadPublicationIntent::Withheld);
    let WorkloadSagaIntentUpdate::Transition(queued) =
        active.apply_intent(stopped_intent(2)).unwrap()
    else {
        panic!("successor should queue");
    };
    let withdrawn = advance_teardown(&queued, WorkloadSagaPhase::Withdrawn);
    let cleanup = fail_current_teardown(&withdrawn);
    assert!(cleanup.apply_intent(stopped_intent(3)).is_err());

    let mut forged = serde_json::to_value(&cleanup).unwrap();
    forged["successorIntent"] = serde_json::to_value(stopped_intent(3)).unwrap();
    forged["lastTransition"]["successorGeneration"] = json!("3");
    rehash_encoded_record(&mut forged);
    assert!(serde_json::from_value::<WorkloadSagaRecord>(forged).is_err());
}

#[test]
fn validated_record_binds_withdrawal_and_cleanup_details_to_source_phase() {
    let ready = record_at_ready(WorkloadPublicationIntent::Withheld);
    let withdrawal = begin_teardown(&ready);
    let mut crossed_withdrawal = serde_json::to_value(&withdrawal).unwrap();
    crossed_withdrawal["phaseDetail"]["value"]["origin"] = json!("network_attached");
    rehash_encoded_record(&mut crossed_withdrawal);
    assert!(serde_json::from_value::<WorkloadSagaRecord>(crossed_withdrawal).is_err());

    let retained = ready.phase_detail().references();
    let cleanup_detail = WorkloadPhaseDetail::cleanup_pending(
        ready.active_intent(),
        ready.phase(),
        retained.clone(),
        cleanup_inspections(ready.phase(), &retained),
    )
    .unwrap();
    let cleanup = ready
        .advance(WorkloadSagaPhase::CleanupPending, cleanup_detail, None)
        .unwrap();
    let mut crossed_cleanup = serde_json::to_value(&cleanup).unwrap();
    crossed_cleanup["phaseDetail"]["value"]["lastSafePhase"] = json!("network_attached");
    for inspection in crossed_cleanup["phaseDetail"]["value"]["inspections"]
        .as_array_mut()
        .unwrap()
    {
        inspection["expected_phase"] = json!("network_attached");
    }
    rehash_encoded_record(&mut crossed_cleanup);
    assert!(serde_json::from_value::<WorkloadSagaRecord>(crossed_cleanup).is_err());
}

#[test]
fn validated_record_rejects_successor_while_provision_remains_active() {
    let ready = record_at_ready(WorkloadPublicationIntent::Withheld);
    let successor = stopped_intent(2);
    let mut forged = serde_json::to_value(&ready).unwrap();
    forged["successorIntent"] = serde_json::to_value(&successor).unwrap();
    forged["lastTransition"]["successorGeneration"] = json!("2");
    rehash_encoded_record(&mut forged);
    assert!(serde_json::from_value::<WorkloadSagaRecord>(forged).is_err());
}

#[test]
fn effect_reference_deserialization_and_context_validation_enforce_invariants() {
    let intent = running_intent(1, WorkloadPublicationIntent::PublishWhenReady);
    let references = WorkloadEffectReferences::provision(&intent, None).unwrap();
    let execution = references.execution().unwrap();
    let mut crossed_execution = serde_json::to_value(execution).unwrap();
    crossed_execution["executionId"] = json!(WorkloadExecutionId::for_execution(
        execution.workload_uid(),
        execution.node_identity(),
        WorkloadGeneration::new(2),
    ));
    assert!(serde_json::from_value::<WorkloadExecutionReference>(crossed_execution).is_err());

    let publication = WorkloadPublicationReference::new(
        [
            PublishedEndpointId::generate(),
            PublishedEndpointId::generate(),
        ],
        &intent,
    )
    .unwrap();
    let mut empty = serde_json::to_value(&publication).unwrap();
    empty["endpoints"] = json!([]);
    let empty: WorkloadPublicationReference =
        serde_json::from_value(empty).expect("an empty endpoint set is intrinsically ordered");
    assert!(
        empty.validate_for(&intent).is_err(),
        "an empty endpoint set must reject a publishable nonempty intent"
    );

    let zero_listener = stopped_intent(2);
    let explicit_empty = WorkloadPublicationReference::new([], &zero_listener)
        .expect("a withheld zero-listener intent should retain explicit empty authority");
    assert!(explicit_empty.endpoints().is_empty());
    let round_trip: WorkloadPublicationReference = serde_json::from_value(
        serde_json::to_value(&explicit_empty).expect("empty reference should encode"),
    )
    .expect("empty reference should decode");
    assert_eq!(round_trip, explicit_empty);
    round_trip
        .validate_for(&zero_listener)
        .expect("empty reference should authenticate its exact zero-listener intent");

    let endpoint = publication.endpoints()[0].clone();
    let mut duplicate = serde_json::to_value(&publication).unwrap();
    duplicate["endpoints"] = json!([endpoint.clone(), endpoint]);
    assert!(serde_json::from_value::<WorkloadPublicationReference>(duplicate).is_err());

    let mut unsorted = publication.endpoints().to_vec();
    unsorted.reverse();
    let mut unsorted_value = serde_json::to_value(&publication).unwrap();
    unsorted_value["endpoints"] = serde_json::to_value(unsorted).unwrap();
    assert!(serde_json::from_value::<WorkloadPublicationReference>(unsorted_value).is_err());
}

#[test]
fn transition_id_is_exact_replay_stable_and_binds_semantic_payload() {
    let current = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .unwrap();
    let first = advance_provision(&current, WorkloadSagaPhase::NetworkReserved, None);
    let replay = advance_provision(&current, WorkloadSagaPhase::NetworkReserved, None);
    assert_eq!(transition_id(&first), transition_id(&replay));

    let references = provision_references(
        WorkloadSagaPhase::NetworkReserved,
        current.active_intent(),
        None,
    );
    let changed_evidence = WorkloadPhaseDetail::provision(
        WorkloadSagaPhase::NetworkReserved,
        current.active_intent(),
        references.clone(),
        vec![WorkloadOwnerObservation::NetworkReserved {
            reference: references.network().unwrap().clone(),
            evidence: evidence("different-evidence"),
        }],
    )
    .unwrap();
    let changed = current
        .record_resource_free_network_step(
            WorkloadProvisionStep::ReserveNetwork,
            WorkloadSagaPhase::NetworkReserved,
            changed_evidence,
        )
        .unwrap();
    assert_ne!(transition_id(&first), transition_id(&changed));

    let withdrawal = begin_teardown(&first);
    assert_ne!(transition_id(&first), transition_id(&withdrawal));
    let successor_a = first.apply_intent(stopped_intent(2)).unwrap();
    let successor_b = first.apply_intent(stopped_intent(3)).unwrap();
    let WorkloadSagaIntentUpdate::Transition(successor_a) = successor_a else {
        panic!("successor should transition");
    };
    let WorkloadSagaIntentUpdate::Transition(successor_b) = successor_b else {
        panic!("successor should transition");
    };
    assert_ne!(transition_id(&successor_a), transition_id(&successor_b));

    let cleanup_a = first
        .advance(
            WorkloadSagaPhase::CleanupPending,
            WorkloadPhaseDetail::cleanup_pending(
                first.active_intent(),
                first.phase(),
                first.phase_detail().references(),
                cleanup_inspections(first.phase(), &first.phase_detail().references()),
            )
            .unwrap(),
            Some(WorkloadFailureEvidence::new("failed_a", evidence("failure-a")).unwrap()),
        )
        .unwrap();
    let cleanup_b = first
        .advance(
            WorkloadSagaPhase::CleanupPending,
            WorkloadPhaseDetail::cleanup_pending(
                first.active_intent(),
                first.phase(),
                first.phase_detail().references(),
                cleanup_inspections(first.phase(), &first.phase_detail().references()),
            )
            .unwrap(),
            Some(WorkloadFailureEvidence::new("failed_b", evidence("failure-b")).unwrap()),
        )
        .unwrap();
    assert_ne!(transition_id(&cleanup_a), transition_id(&cleanup_b));
}

#[test]
fn transition_id_binds_every_active_intent_network_and_revision_field() {
    let base = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .unwrap();
    let encoded = serde_json::to_value(&base).unwrap();
    let alternate_intent = intent_with(
        "tenant-a",
        "workload-b",
        2,
        DesiredWorkloadState::Running,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::PublishWhenReady,
        3,
    );
    let alternate = serde_json::to_value(&alternate_intent).unwrap();
    let reject = |candidate| {
        assert!(serde_json::from_value::<WorkloadSagaRecord>(candidate).is_err());
    };

    let mut candidate = encoded.clone();
    candidate["activeIntent"]["kind"] = alternate["kind"].clone();
    reject(candidate);

    let mut candidate = encoded.clone();
    candidate["activeIntent"]["generation"] = alternate["generation"].clone();
    candidate["lastTransition"]["activeGeneration"] = alternate["generation"].clone();
    reject(candidate);

    let mut candidate = encoded.clone();
    candidate["activeIntent"]["desiredDigest"] = alternate["desiredDigest"].clone();
    reject(candidate);

    for path in [
        "/plan/plan_id",
        "/plan/generation",
        "/plan/content_digest",
        "/content/identity/generation",
        "/content/dependencyListeners",
    ] {
        let mut candidate = encoded.clone();
        *candidate["activeIntent"]["network"]
            .pointer_mut(path)
            .expect("network field should exist") = alternate["network"]
            .pointer(path)
            .expect("alternate network field should exist")
            .clone();
        reject(candidate);
    }

    let mut candidate = encoded.clone();
    candidate["activeIntent"]["activation"] = alternate["activation"].clone();
    reject(candidate);

    let mut candidate = encoded.clone();
    candidate["activeIntent"]["publication"] = alternate["publication"].clone();
    reject(candidate);

    for field in ["decisionId", "workloadUid", "assignedNode"] {
        let mut candidate = encoded.clone();
        candidate["activeIntent"]["admission"][field] = alternate["admission"][field].clone();
        reject(candidate);
    }

    let progressed = advance_provision(&base, WorkloadSagaPhase::NetworkReserved, None);
    let mut candidate = serde_json::to_value(&progressed).unwrap();
    let crossed_revision = progressed
        .revision()
        .checked_next()
        .expect("fixture revision should have room");
    candidate["revision"] = json!(crossed_revision);
    candidate["lastTransition"]["resultingRevision"] = json!(crossed_revision);
    reject(candidate);

    let stopped =
        WorkloadSagaRecord::new(key("tenant-a", "workload-a"), stopped_intent(1)).unwrap();
    assert_ne!(transition_id(&base), transition_id(&stopped));
}

#[test]
fn deserialization_revalidates_nested_evidence_unknown_fields_and_transition_binding() {
    let record = record_at_ready(WorkloadPublicationIntent::PublishWhenReady);
    let encoded = serde_json::to_value(&record).unwrap();
    assert_eq!(
        serde_json::from_value::<WorkloadSagaRecord>(encoded.clone()).unwrap(),
        record
    );

    let mut unknown = encoded.clone();
    unknown["activeIntent"]["unknown"] = json!(true);
    assert!(serde_json::from_value::<WorkloadSagaRecord>(unknown).is_err());

    let mut unknown_observation = encoded.clone();
    unknown_observation["phaseDetail"]["value"]["observations"][0]["unknown"] = json!(true);
    assert!(serde_json::from_value::<WorkloadSagaRecord>(unknown_observation).is_err());

    let mut unknown_tag = encoded.clone();
    unknown_tag["phaseDetail"]["value"]["observations"][0]["kind"] = json!("invented");
    assert!(serde_json::from_value::<WorkloadSagaRecord>(unknown_tag).is_err());

    let mut crossed = encoded.clone();
    crossed["phaseDetail"]["value"]["observations"][0]["evidence"] =
        json!(WorkloadOwnerEvidenceDigest::sha256("tampered").to_string());
    assert!(serde_json::from_value::<WorkloadSagaRecord>(crossed).is_err());

    let mut wrong_saga = encoded;
    wrong_saga["sagaId"] = json!(key("tenant-b", "workload-a").saga_id().to_string());
    assert!(serde_json::from_value::<WorkloadSagaRecord>(wrong_saga).is_err());
}

#[test]
fn nested_value_deserialization_enforces_constructor_invariants() {
    let stopped = stopped_intent(1);
    let mut invalid_stopped = serde_json::to_value(&stopped).unwrap();
    invalid_stopped["activation"] = json!("activate_when_attached");
    assert!(serde_json::from_value::<WorkloadSagaIntent>(invalid_stopped).is_err());

    let failure = WorkloadFailureEvidence::new("provider_failed", evidence("failure")).unwrap();
    let mut invalid_failure = serde_json::to_value(&failure).unwrap();
    invalid_failure["code"] = json!("Provider Failed");
    assert!(serde_json::from_value::<WorkloadFailureEvidence>(invalid_failure).is_err());
}

#[test]
fn phase_progression_cannot_rewrite_prior_observation_evidence() {
    let initial = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .unwrap();
    let reserved = advance_provision(&initial, WorkloadSagaPhase::NetworkReserved, None);
    let prepared_references = provision_references(
        WorkloadSagaPhase::WorkloadPrepared,
        reserved.active_intent(),
        None,
    );
    let mut rewritten_provision = provision_observations(
        WorkloadSagaPhase::WorkloadPrepared,
        &prepared_references,
        WorkloadPublicationIntent::Withheld,
    );
    rewritten_provision[0] = WorkloadOwnerObservation::NetworkReserved {
        reference: prepared_references.network().unwrap().clone(),
        evidence: evidence("rewritten-reservation"),
    };
    let rewritten_provision = WorkloadPhaseDetail::provision(
        WorkloadSagaPhase::WorkloadPrepared,
        reserved.active_intent(),
        prepared_references,
        rewritten_provision,
    )
    .unwrap();
    assert!(
        reserved
            .advance(
                WorkloadSagaPhase::WorkloadPrepared,
                rewritten_provision,
                None,
            )
            .is_err()
    );

    let ready = record_at_ready(WorkloadPublicationIntent::PublishWhenReady);
    let withdrawal = begin_teardown(&ready);
    let withdrawn = advance_teardown(&withdrawal, WorkloadSagaPhase::Withdrawn);
    let WorkloadPhaseDetail::Teardown(withdrawn_detail) = withdrawn.phase_detail() else {
        panic!("withdrawn fixture must carry teardown detail");
    };
    let mut rewritten_terminal = terminal_observations(
        WorkloadSagaPhase::Drained,
        ready.active_intent(),
        withdrawn_detail.origin(),
        withdrawn_detail.retained_references(),
    );
    rewritten_terminal[0] = WorkloadTerminalObservation::PublicationAbsent {
        reference: withdrawn_detail
            .retained_references()
            .publication()
            .unwrap()
            .clone(),
        evidence: evidence("rewritten-withdrawal"),
    };
    let rewritten_teardown = WorkloadPhaseDetail::teardown(
        WorkloadSagaPhase::Drained,
        withdrawn.active_intent(),
        withdrawn_detail.origin(),
        withdrawn_detail.retained_references().clone(),
        rewritten_terminal,
    );
    assert!(rewritten_teardown.is_err());
}

#[test]
fn record_deserialization_rejects_forged_illegal_source_edge() {
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct ForgedTransitionPayload<'a> {
        saga_id: &'a WorkloadSagaId,
        expected_revision: Option<WorkloadSagaRevision>,
        resulting_revision: WorkloadSagaRevision,
        source_phase: Option<WorkloadSagaPhase>,
        target_phase: WorkloadSagaPhase,
        active_intent: &'a WorkloadSagaIntent,
        successor_intent: Option<&'a WorkloadSagaIntent>,
        phase_detail: &'a WorkloadPhaseDetail,
        provision_disposition: Option<&'a WorkloadProvisionDisposition>,
        teardown_disposition: Option<&'a WorkloadTeardownDisposition>,
        restart: &'a WorkloadRestartState,
        failure: Option<&'a WorkloadFailureEvidence>,
    }

    let initial = WorkloadSagaRecord::new(
        key("tenant-a", "workload-a"),
        running_intent(1, WorkloadPublicationIntent::Withheld),
    )
    .unwrap();
    let reserved = advance_provision(&initial, WorkloadSagaPhase::NetworkReserved, None);
    let forged_source = WorkloadSagaPhase::Published;
    let payload = ForgedTransitionPayload {
        saga_id: reserved.saga_id(),
        expected_revision: Some(initial.revision()),
        resulting_revision: reserved.revision(),
        source_phase: Some(forged_source),
        target_phase: reserved.phase(),
        active_intent: reserved.active_intent(),
        successor_intent: reserved.successor_intent(),
        phase_detail: reserved.phase_detail(),
        provision_disposition: reserved.provision_disposition(),
        teardown_disposition: reserved.teardown_disposition(),
        restart: reserved.restart_state(),
        failure: reserved.failure(),
    };
    let encoded_payload = serde_json::to_vec(&payload).unwrap();
    let transition_id = derive_id(
        WorkloadSagaTransitionId::PREFIX,
        b"nimbus.workloads.saga.transition.v5",
        &[std::str::from_utf8(&encoded_payload).unwrap()],
    );
    let mut forged_record = serde_json::to_value(&reserved).unwrap();
    forged_record["lastTransition"]["sourcePhase"] = json!(forged_source);
    forged_record["lastTransition"]["transitionId"] = json!(transition_id);

    assert!(serde_json::from_value::<WorkloadSagaRecord>(forged_record).is_err());
}

#[test]
fn failure_evidence_is_stable_bounded_and_cleanup_only() {
    assert!(WorkloadFailureEvidence::new("", evidence("empty")).is_err());
    assert!(WorkloadFailureEvidence::new("Bad", evidence("uppercase")).is_err());
    assert!(WorkloadFailureEvidence::new("x".repeat(97), evidence("long")).is_err());

    let ready = record_at_ready(WorkloadPublicationIntent::Withheld);
    assert!(
        ready
            .advance(
                WorkloadSagaPhase::Observed,
                provision_detail(WorkloadSagaPhase::Observed, ready.active_intent(), None,),
                Some(WorkloadFailureEvidence::new("failed", evidence("failed")).unwrap()),
            )
            .is_err()
    );
}

#[path = "tests/provision_state.rs"]
mod provision_state;
#[path = "tests/restart_state.rs"]
mod restart_state;
#[path = "tests/teardown_state.rs"]
mod teardown_state;

#[path = "tests/wire_primitives.rs"]
mod wire_primitives;
