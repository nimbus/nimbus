use std::net::{IpAddr, Ipv4Addr};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    EndpointProtocol, NetworkAddressFamily, NetworkAttachmentCapabilitySet,
    NetworkAttachmentProviderRegistration, NetworkBindRealmKind, NetworkCapabilityBundle,
    NetworkCapabilityRequirements, NetworkControlPlaneLocality, NetworkEndpointCapabilitySet,
    NetworkExposure, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkIngressProviderRegistration, NetworkLifecycleCapabilitySet, NetworkManagementMode,
    NetworkPortAssignmentMode, NetworkProviderId, NetworkResourceGeneration,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements, NetworkTlsBehavior,
    PortProtocol, PublishedEndpointId,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    ProposedWorkloadTeardownTransition, WorkloadActivationIntent, WorkloadAdmissionEvidence,
    WorkloadExecutableEncoding, WorkloadExecutableIntent, WorkloadFailureEvidence,
    WorkloadGeneration, WorkloadNetworkEndpointSemantics, WorkloadNetworkForwardingBehavior,
    WorkloadNetworkIntent, WorkloadNetworkListenerBlueprint, WorkloadNetworkPlanContent,
    WorkloadNetworkPlanIdentity, WorkloadNetworkPortRequestMode, WorkloadOwnerEvidenceDigest,
    WorkloadPhaseDetail, WorkloadProvisionSourceEvidence, WorkloadProvisionSourceGeneration,
    WorkloadProvisionSourceIdentity, WorkloadProvisionSourceResourceVersion,
    WorkloadPublicationIntent, WorkloadPublicationReference, WorkloadSagaCommit,
    WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaIntent, WorkloadSagaIntentUpdate,
    WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest, WorkloadSagaPhase,
    WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError, WorkloadSagaTenantPage,
    WorkloadSagaTenantPageRequest, WorkloadTeardownDecision, WorkloadTeardownEffectResult,
    WorkloadTeardownStep, WorkloadTeardownSubjects, WorkloadTeardownSuccessEvidence,
    WorkloadTerminalEvidenceDigest,
};

use super::{WorkloadSagaAction, WorkloadSagaCoordinator, WorkloadSagaDecision};

fn tenant(label: &str) -> TenantId {
    TenantId::new(format!("tenant-{label}")).expect("fixture tenant is valid")
}

fn key(label: &str) -> WorkloadSagaKey {
    WorkloadSagaKey::new(
        tenant(label),
        WorkloadId::new(format!("workload-{label}")).expect("fixture workload is valid"),
    )
}

fn compiled_plan(
    tenant_id: &TenantId,
    label: &str,
    generation: u64,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        format!("fixture-{label}"),
        NetworkResourceGeneration::new(generation),
    )
    .expect("fixture network identity is valid");
    let attachment =
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []);
    let endpoint = NetworkEndpointCapabilitySet::new(
        [NetworkAddressFamily::Ipv4],
        [NetworkBindRealmKind::Host],
        [NetworkExposure::Loopback],
        [PortProtocol::Tcp],
        [NetworkPortAssignmentMode::ProviderAssigned],
    );
    let ingress = NetworkIngressCapabilitySet::new([]);
    let forwarding = NetworkForwardingCapabilitySet::new([]);
    let lifecycle = NetworkLifecycleCapabilitySet::new([]);
    let sovereignty =
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true);
    let requirements = NetworkCapabilityRequirements::new(
        attachment.clone(),
        endpoint.clone(),
        ingress.clone(),
        forwarding.clone(),
        nimbus_network::NetworkLifecycleRequirements::new(lifecycle.clone(), lifecycle.clone()),
        sovereignty,
    );
    let (selection, selection_evidence, listeners) =
        if publication == WorkloadPublicationIntent::PublishWhenReady {
            let attachment_provider = NetworkProviderId::for_registration_key("fixture-attachment");
            let ingress_provider = NetworkProviderId::for_registration_key("fixture-ingress");
            let bundle = NetworkCapabilityBundle::new(
                NetworkAttachmentProviderRegistration::new(
                    attachment_provider,
                    attachment,
                    [NetworkAddressFamily::Ipv4],
                    lifecycle.clone(),
                    NetworkSovereigntyCapabilities::new(
                        NetworkControlPlaneLocality::LocalOnly,
                        [],
                        true,
                    ),
                ),
                NetworkIngressProviderRegistration::new(
                    ingress_provider,
                    endpoint,
                    ingress,
                    forwarding,
                    lifecycle,
                    NetworkSovereigntyCapabilities::new(
                        NetworkControlPlaneLocality::LocalOnly,
                        [],
                        true,
                    ),
                ),
            );
            let listener = WorkloadNetworkListenerBlueprint::new(
                &identity,
                "api",
                EndpointProtocol::Http,
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                WorkloadNetworkPortRequestMode::ProviderAssigned,
                WorkloadNetworkEndpointSemantics::new(
                    WorkloadNetworkForwardingBehavior::None,
                    NetworkTlsBehavior::Disabled,
                ),
                None,
            )
            .expect("fixture listener should validate");
            (
                Some(bundle.selection()),
                Some(bundle.selection_evidence()),
                vec![listener],
            )
        } else {
            (None, None, Vec::new())
        };
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        selection,
        selection_evidence,
        None,
        [],
        listeners,
        [],
        activation,
        publication,
    )
    .expect("fixture network content is valid");
    CompiledWorkloadNetworkPlan::from_content(content)
        .expect("fixture compiled network plan is valid")
}

fn intent(
    label: &str,
    generation: u64,
    desired_state: DesiredWorkloadState,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
    seed: u8,
) -> WorkloadSagaIntent {
    let tenant_id = tenant(label);
    let executable = WorkloadExecutableIntent::new(
        WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"fixtureSeed":{seed}}}"#),
    )
    .expect("fixture executable is valid");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(label, label)
            .expect("fixture source identity is valid"),
        WorkloadProvisionSourceGeneration::new(generation),
        WorkloadProvisionSourceResourceVersion::new(format!("fixture-{seed}"))
            .expect("fixture source version is valid"),
        executable.content_digest(),
        NetworkProviderId::for_registration_key("fixture-attachment"),
        nimbus_workloads::WorkloadExecutionProviderId::for_registration_key("fixture-execution"),
    )
    .expect("fixture source evidence is valid");
    WorkloadSagaIntent::new_without_automatic_restart(
        DesiredWorkloadKind::Sandbox,
        desired_state,
        WorkloadGeneration::new(generation),
        executable,
        source,
        WorkloadNetworkIntent::new(compiled_plan(
            &tenant_id,
            label,
            generation,
            activation,
            publication,
        )),
        activation,
        publication,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", format!("{seed:02x}").repeat(32))
                .try_into()
                .expect("fixture decision id is valid"),
            format!("twu_{}", format!("{:02x}", seed.wrapping_add(1)).repeat(32))
                .try_into()
                .expect("fixture workload uid is valid"),
            NodeIdentity::new(format!("node-{label}-{generation}")).expect("fixture node is valid"),
        ),
    )
    .expect("fixture intent is valid")
}

fn running_intent(
    label: &str,
    generation: u64,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
) -> WorkloadSagaIntent {
    intent(
        label,
        generation,
        DesiredWorkloadState::Running,
        activation,
        publication,
        u8::try_from(generation + 10).expect("fixture generation fits a byte"),
    )
}

fn stopped_intent(label: &str, generation: u64) -> WorkloadSagaIntent {
    intent(
        label,
        generation,
        DesiredWorkloadState::Stopped,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        u8::try_from(generation + 40).expect("fixture generation fits a byte"),
    )
}

fn evidence(label: &str) -> WorkloadOwnerEvidenceDigest {
    WorkloadOwnerEvidenceDigest::sha256(label)
}

fn publication_reference(intent: &WorkloadSagaIntent) -> WorkloadPublicationReference {
    WorkloadPublicationReference::new([PublishedEndpointId::generate()], intent)
        .expect("fixture publication reference is valid")
}

fn advance_provision(
    record: &WorkloadSagaRecord,
    phase: WorkloadSagaPhase,
    publication: &WorkloadPublicationReference,
) -> WorkloadSagaRecord {
    let _ = publication;
    let candidate = crate::workload_saga::test_support::confirmed_provision(record);
    assert_eq!(
        candidate.phase(),
        phase,
        "fixture should reach its target phase"
    );
    candidate
}

pub(crate) fn provision_record(
    label: &str,
    target: WorkloadSagaPhase,
    activation: WorkloadActivationIntent,
    publication: WorkloadPublicationIntent,
) -> WorkloadSagaRecord {
    let intent = running_intent(label, 1, activation, publication);
    let publication_reference = publication_reference(&intent);
    let mut record = WorkloadSagaRecord::new(key(label), intent).expect("fixture record is valid");
    if target == WorkloadSagaPhase::IntentCommitted {
        return record;
    }
    for phase in [
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaPhase::Ready,
    ] {
        record = advance_provision(&record, phase, &publication_reference);
        if phase == target {
            return record;
        }
    }
    if publication == WorkloadPublicationIntent::PublishWhenReady {
        record = advance_provision(
            &record,
            WorkloadSagaPhase::Published,
            &publication_reference,
        );
        if target == WorkloadSagaPhase::Published {
            return record;
        }
    }
    record = advance_provision(&record, WorkloadSagaPhase::Observed, &publication_reference);
    assert_eq!(target, WorkloadSagaPhase::Observed);
    record
}

fn begin_teardown(
    record: &WorkloadSagaRecord,
    successor: WorkloadSagaIntent,
) -> WorkloadSagaRecord {
    let WorkloadSagaIntentUpdate::Transition(candidate) = record
        .apply_intent(successor)
        .expect("higher generation fixture intent should commit teardown")
    else {
        panic!("higher generation fixture intent should transition");
    };
    *candidate
}

fn teardown_success_evidence(
    step: WorkloadTeardownStep,
    subjects: &WorkloadTeardownSubjects,
) -> WorkloadTeardownSuccessEvidence {
    match (step, subjects) {
        (
            WorkloadTeardownStep::WithdrawPublication,
            WorkloadTeardownSubjects::Publication(reference),
        ) => WorkloadTeardownSuccessEvidence::PublicationAbsent {
            reference: reference.clone(),
            evidence: evidence("publication-absent"),
        },
        (WorkloadTeardownStep::DrainExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionDrained {
                reference: reference.clone(),
                evidence: evidence("execution-drained"),
            }
        }
        (WorkloadTeardownStep::StopExecution, WorkloadTeardownSubjects::Execution(reference)) => {
            WorkloadTeardownSuccessEvidence::ExecutionStopped {
                reference: reference.clone(),
                evidence: evidence("execution-stopped"),
            }
        }
        (WorkloadTeardownStep::DetachNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkDetached {
                reference: reference.clone(),
                evidence: evidence("network-detached"),
            }
        }
        (WorkloadTeardownStep::ReleaseNetwork, WorkloadTeardownSubjects::Network(reference)) => {
            WorkloadTeardownSuccessEvidence::NetworkReleased {
                reference: reference.clone(),
                evidence: evidence("network-released"),
            }
        }
        _ => panic!("teardown fixture step and subjects should match"),
    }
}

fn advance_teardown(record: WorkloadSagaRecord) -> WorkloadSagaRecord {
    match record
        .decide_teardown()
        .expect("teardown fixture should reduce")
    {
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::ResourceFree { step, .. },
        ) => record
            .record_resource_free_teardown_step(step)
            .expect("resource-free teardown fixture step should persist"),
        WorkloadTeardownDecision::PersistCandidate(ProposedWorkloadTeardownTransition::Claim {
            attempt,
            provider_target,
        }) => {
            let success = teardown_success_evidence(attempt.step(), attempt.subjects());
            let claimed = record
                .claim_teardown(*attempt, provider_target)
                .expect("teardown fixture claim should persist");
            let claim = claimed
                .teardown_disposition()
                .and_then(|disposition| disposition.claim())
                .expect("claimed teardown fixture should retain its claim")
                .clone();
            claimed
                .apply_teardown_effect_result(
                    &claim,
                    WorkloadTeardownEffectResult::Succeeded {
                        attempt_id: claim.attempt().attempt_id().clone(),
                        dispatch_epoch: claim.dispatch_epoch(),
                        provider_target: claim.provider_target().clone(),
                        evidence: Box::new(success),
                    },
                )
                .expect("exact teardown fixture success should persist")
        }
        WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::RecordTerminal,
        ) => record
            .record_terminal_teardown()
            .expect("terminal teardown fixture should persist"),
        decision => panic!("teardown fixture cannot advance decision {decision:?}"),
    }
}

fn finish_teardown(
    mut record: WorkloadSagaRecord,
    target: WorkloadSagaPhase,
) -> WorkloadSagaRecord {
    for _ in 0..=6 {
        if record.phase() == target {
            return record;
        }
        record = advance_teardown(record);
    }
    panic!("teardown fixture exceeded its reducer decision bound")
}

fn teardown_record(label: &str, target: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let observed = provision_record(
        label,
        WorkloadSagaPhase::Observed,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    finish_teardown(begin_teardown(&observed, stopped_intent(label, 2)), target)
}

fn no_reference_teardown_record(label: &str, target: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let current = provision_record(
        label,
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let WorkloadSagaIntentUpdate::Transition(with_successor) = current
        .apply_intent(stopped_intent(label, 2))
        .expect("higher generation queues a successor")
    else {
        panic!("higher generation must transition");
    };
    finish_teardown(*with_successor, target)
}

fn recorded_with_successor(label: &str, successor: WorkloadSagaIntent) -> WorkloadSagaRecord {
    let observed = provision_record(
        label,
        WorkloadSagaPhase::Observed,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let WorkloadSagaIntentUpdate::Transition(with_successor) = observed
        .apply_intent(successor)
        .expect("higher generation queues a successor")
    else {
        panic!("higher generation must transition");
    };
    finish_teardown(*with_successor, WorkloadSagaPhase::Recorded)
}

fn cleanup_pending_record(label: &str) -> WorkloadSagaRecord {
    let ready = provision_record(
        label,
        WorkloadSagaPhase::Ready,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let mut record = begin_teardown(&ready, stopped_intent(label, 2));
    for _ in 0..5 {
        match record
            .decide_teardown()
            .expect("cleanup fixture should reduce")
        {
            WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::ResourceFree { step, .. },
            ) => {
                record = record
                    .record_resource_free_teardown_step(step)
                    .expect("resource-free cleanup fixture step should persist");
            }
            WorkloadTeardownDecision::PersistCandidate(
                ProposedWorkloadTeardownTransition::Claim {
                    attempt,
                    provider_target,
                },
            ) => {
                let claimed = record
                    .claim_teardown(*attempt, provider_target)
                    .expect("cleanup fixture claim should persist");
                let claim = claimed
                    .teardown_disposition()
                    .and_then(|disposition| disposition.claim())
                    .expect("cleanup fixture should retain its exact claim")
                    .clone();
                return claimed
                    .apply_teardown_effect_result(
                        &claim,
                        WorkloadTeardownEffectResult::DefiniteFailure {
                            attempt_id: claim.attempt().attempt_id().clone(),
                            dispatch_epoch: claim.dispatch_epoch(),
                            provider_target: claim.provider_target().clone(),
                            failure: WorkloadFailureEvidence::new(
                                "fixture_teardown_failed",
                                evidence("fixture-teardown-failed"),
                            )
                            .expect("cleanup fixture failure should validate"),
                        },
                    )
                    .expect("exact teardown fixture failure should enter cleanup");
            }
            decision => panic!("cleanup fixture cannot advance decision {decision:?}"),
        }
    }
    panic!("cleanup fixture did not reach an effectful teardown step")
}

fn assert_decision(
    record: &WorkloadSagaRecord,
    target: WorkloadSagaPhase,
    action: WorkloadSagaAction,
) {
    let decision = WorkloadSagaDecision::for_record(record).expect("valid record is selectable");
    assert_eq!(decision.key(), record.key());
    assert_eq!(decision.saga_id(), record.saga_id());
    assert_eq!(decision.revision(), record.revision());
    assert_eq!(
        decision.active_generation(),
        record.active_intent().generation()
    );
    assert_eq!(decision.target_phase(), target);
    assert_eq!(decision.action(), &action);
}

fn assert_provision_decision(record: &WorkloadSagaRecord, target: WorkloadSagaPhase) {
    let provision = super::super::WorkloadProvisionDecision::plan(record)
        .expect("valid provision record is reducible");
    assert_decision(record, target, WorkloadSagaAction::Provision(provision));
}

#[test]
fn selector_covers_every_phase_with_exact_typed_action_and_fences() {
    let intent_committed = provision_record(
        "intent",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_provision_decision(&intent_committed, WorkloadSagaPhase::NetworkReserved);

    let network_reserved = provision_record(
        "reserved",
        WorkloadSagaPhase::NetworkReserved,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_provision_decision(&network_reserved, WorkloadSagaPhase::WorkloadPrepared);

    let workload_prepared = provision_record(
        "prepared",
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_provision_decision(&workload_prepared, WorkloadSagaPhase::NetworkAttached);

    let network_attached = provision_record(
        "attached",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_provision_decision(&network_attached, WorkloadSagaPhase::NetworkAttached);

    let activated = provision_record(
        "activated",
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_provision_decision(&activated, WorkloadSagaPhase::Ready);

    let ready = provision_record(
        "ready",
        WorkloadSagaPhase::Ready,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    assert_provision_decision(&ready, WorkloadSagaPhase::Published);

    let published = provision_record(
        "published",
        WorkloadSagaPhase::Published,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    assert_provision_decision(&published, WorkloadSagaPhase::Observed);

    let observed = provision_record(
        "observed",
        WorkloadSagaPhase::Observed,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    assert_provision_decision(&observed, WorkloadSagaPhase::Observed);

    let withdrawal = teardown_record("withdrawal", WorkloadSagaPhase::WithdrawalCommitted);
    assert_decision(
        &withdrawal,
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaAction::WithdrawPublication {
            reference: withdrawal
                .phase_detail()
                .references()
                .publication()
                .expect("publication retained")
                .clone(),
        },
    );

    let withdrawn = teardown_record("withdrawn", WorkloadSagaPhase::Withdrawn);
    assert_decision(
        &withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaAction::DrainWorkload {
            reference: withdrawn
                .phase_detail()
                .references()
                .execution()
                .expect("execution retained")
                .clone(),
        },
    );

    let drained = teardown_record("drained", WorkloadSagaPhase::Drained);
    assert_decision(
        &drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaAction::StopWorkload {
            reference: drained
                .phase_detail()
                .references()
                .execution()
                .expect("execution retained")
                .clone(),
        },
    );

    let stopped = teardown_record("stopped", WorkloadSagaPhase::WorkloadStopped);
    assert_decision(
        &stopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaAction::DetachNetwork {
            reference: stopped
                .phase_detail()
                .references()
                .network()
                .expect("network retained")
                .clone(),
        },
    );

    let detached = teardown_record("detached", WorkloadSagaPhase::NetworkDetached);
    assert_decision(
        &detached,
        WorkloadSagaPhase::NetworkReleased,
        WorkloadSagaAction::ReleaseNetwork {
            reference: detached
                .phase_detail()
                .references()
                .network()
                .expect("network retained")
                .clone(),
        },
    );

    let released = teardown_record("released", WorkloadSagaPhase::NetworkReleased);
    let WorkloadPhaseDetail::Teardown(released_detail) = released.phase_detail() else {
        panic!("network released carries teardown evidence");
    };
    assert_decision(
        &released,
        WorkloadSagaPhase::Recorded,
        WorkloadSagaAction::RecordTerminalEvidence {
            digest: WorkloadTerminalEvidenceDigest::for_observations(
                released_detail.terminal_observations(),
            )
            .expect("terminal evidence can be digested"),
        },
    );

    let successor = running_intent(
        "recorded",
        2,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let recorded = recorded_with_successor("recorded", successor.clone());
    assert_decision(
        &recorded,
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaAction::PromoteSuccessor {
            intent: Box::new(successor),
        },
    );

    let cleanup = cleanup_pending_record("cleanup");
    let WorkloadPhaseDetail::CleanupPending(cleanup_detail) = cleanup.phase_detail() else {
        panic!("cleanup fixture carries cleanup detail");
    };
    assert_decision(
        &cleanup,
        WorkloadSagaPhase::CleanupPending,
        WorkloadSagaAction::InspectCleanup {
            last_safe_phase: cleanup_detail.last_safe_phase(),
            retained_references: cleanup_detail.retained_references().clone(),
            inspections: cleanup_detail.inspections().to_vec(),
        },
    );
}

#[test]
fn intent_committed_decision_carries_exact_compiled_network_plan() {
    let record = provision_record(
        "complete-reservation",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    let decision = WorkloadSagaDecision::for_record(&record).expect("record is valid");
    let WorkloadSagaAction::Provision(super::super::WorkloadProvisionDecision::Proposed(proposed)) =
        decision.action()
    else {
        panic!("IntentCommitted must produce one complete pure reservation value");
    };
    let Some(nimbus_workloads::WorkloadProvisionDisposition::DispatchPending(claim)) =
        proposed.candidate().provision_disposition()
    else {
        panic!("reservation proposal must retain the exact pending dispatch claim");
    };
    let attempt = claim.attempt();
    let nimbus_workloads::WorkloadProvisionSubjects::Network(reference) = attempt.subjects() else {
        panic!("reservation attempt must carry its network subject");
    };
    let plan = proposed
        .candidate()
        .active_intent()
        .network()
        .compiled_plan();

    assert_eq!(plan, record.active_intent().network().compiled_plan());
    assert_eq!(reference.plan_id(), plan.plan().plan_id());
    assert_eq!(reference.generation(), plan.plan().generation());
    assert_eq!(reference.digest(), plan.plan().digest());
    assert!(record.phase_detail().references().is_empty());
}

#[test]
fn selector_delegates_all_provision_phases_and_quiesces_terminal_records() {
    let prepare_only = provision_record(
        "prepare-only",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    assert_provision_decision(&prepare_only, WorkloadSagaPhase::NetworkAttached);

    let withheld_ready = provision_record(
        "withheld",
        WorkloadSagaPhase::Ready,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_provision_decision(&withheld_ready, WorkloadSagaPhase::Observed);

    let recorded = WorkloadSagaRecord::new(key("terminal"), stopped_intent("terminal", 1))
        .expect("stopped intent is terminal");
    assert_decision(
        &recorded,
        WorkloadSagaPhase::Recorded,
        WorkloadSagaAction::Quiescent,
    );
}

#[test]
fn selector_advances_teardown_without_effect_when_origin_retained_no_reference() {
    for (phase, target) in [
        (
            WorkloadSagaPhase::WithdrawalCommitted,
            WorkloadSagaPhase::Withdrawn,
        ),
        (WorkloadSagaPhase::Withdrawn, WorkloadSagaPhase::Drained),
        (
            WorkloadSagaPhase::Drained,
            WorkloadSagaPhase::WorkloadStopped,
        ),
        (
            WorkloadSagaPhase::WorkloadStopped,
            WorkloadSagaPhase::NetworkDetached,
        ),
        (
            WorkloadSagaPhase::NetworkDetached,
            WorkloadSagaPhase::NetworkReleased,
        ),
    ] {
        let record = no_reference_teardown_record(&format!("no-ref-{phase:?}"), phase);
        assert!(record.phase_detail().references().is_empty());
        assert_decision(&record, target, WorkloadSagaAction::AdvanceWithoutEffect);
    }
}

#[test]
fn recorded_promotes_only_the_exact_queued_successor_and_exact_target() {
    let running = running_intent(
        "successor-running",
        2,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let running_record = recorded_with_successor("successor-running", running.clone());
    assert_decision(
        &running_record,
        WorkloadSagaPhase::IntentCommitted,
        WorkloadSagaAction::PromoteSuccessor {
            intent: Box::new(running),
        },
    );

    let stopped = stopped_intent("successor-stopped", 2);
    let stopped_record = recorded_with_successor("successor-stopped", stopped.clone());
    assert_decision(
        &stopped_record,
        WorkloadSagaPhase::Recorded,
        WorkloadSagaAction::PromoteSuccessor {
            intent: Box::new(stopped),
        },
    );
}

#[derive(Debug)]
struct DecisionStore {
    recovery_result: Result<WorkloadSagaPage, WorkloadSagaStoreError>,
    recovery_reads: AtomicUsize,
    mutation_or_other_reads: AtomicUsize,
}

impl DecisionStore {
    fn new(recovery_result: Result<WorkloadSagaPage, WorkloadSagaStoreError>) -> Self {
        Self {
            recovery_result,
            recovery_reads: AtomicUsize::new(0),
            mutation_or_other_reads: AtomicUsize::new(0),
        }
    }
}

impl WorkloadSagaStore for DecisionStore {
    fn load<'a>(
        &'a self,
        _key: &'a WorkloadSagaKey,
    ) -> WorkloadSagaFuture<'a, Option<WorkloadSagaRecord>> {
        Box::pin(async move {
            self.mutation_or_other_reads.fetch_add(1, Ordering::SeqCst);
            Err(WorkloadSagaStoreError::Unavailable)
        })
    }

    fn compare_and_swap<'a>(
        &'a self,
        _expected: WorkloadSagaExpected,
        _next: WorkloadSagaRecord,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaCommit> {
        Box::pin(async move {
            self.mutation_or_other_reads.fetch_add(1, Ordering::SeqCst);
            Err(WorkloadSagaStoreError::Unavailable)
        })
    }

    fn list_recoverable<'a>(
        &'a self,
        _request: WorkloadSagaPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaPage> {
        Box::pin(async move {
            self.recovery_reads.fetch_add(1, Ordering::SeqCst);
            self.recovery_result.clone()
        })
    }

    fn list_restart_candidates<'a>(
        &'a self,
        request: nimbus_workloads::WorkloadRestartCandidatePageRequest,
    ) -> nimbus_workloads::WorkloadSagaFuture<'a, nimbus_workloads::WorkloadRestartCandidatePage>
    {
        Box::pin(async move {
            nimbus_workloads::WorkloadRestartCandidatePage::new(&request, Vec::new(), false)
        })
    }

    fn list_for_tenant<'a>(
        &'a self,
        _tenant_id: &'a TenantId,
        _request: WorkloadSagaTenantPageRequest,
    ) -> WorkloadSagaFuture<'a, WorkloadSagaTenantPage> {
        Box::pin(async move {
            self.mutation_or_other_reads.fetch_add(1, Ordering::SeqCst);
            Err(WorkloadSagaStoreError::Unavailable)
        })
    }
}

#[tokio::test]
async fn bounded_reader_preserves_order_cardinality_and_exact_store_cursor() {
    let request = WorkloadSagaPageRequest::new(None, 2).expect("fixture request is valid");
    let mut records = vec![
        provision_record(
            "page-a",
            WorkloadSagaPhase::IntentCommitted,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        ),
        provision_record(
            "page-b",
            WorkloadSagaPhase::NetworkReserved,
            WorkloadActivationIntent::ActivateWhenAttached,
            WorkloadPublicationIntent::Withheld,
        ),
    ];
    records.sort_by(|left, right| left.saga_id().cmp(right.saga_id()));
    let expected_ids: Vec<_> = records
        .iter()
        .map(|record| record.saga_id().clone())
        .collect();
    let page = WorkloadSagaPage::new(&request, records, true).expect("fixture page is valid");
    let expected_cursor = page.next_cursor().cloned();
    let store = Arc::new(DecisionStore::new(Ok(page)));
    let coordinator = WorkloadSagaCoordinator::new(store.clone());

    let planned = coordinator
        .plan_recoverable_page(request)
        .await
        .expect("valid page is planned");

    assert_eq!(planned.decisions().len(), expected_ids.len());
    assert_eq!(
        planned
            .decisions()
            .iter()
            .map(|decision| decision.saga_id().clone())
            .collect::<Vec<_>>(),
        expected_ids
    );
    assert_eq!(planned.next_cursor(), expected_cursor.as_ref());
    assert_eq!(store.recovery_reads.load(Ordering::SeqCst), 1);
    assert_eq!(store.mutation_or_other_reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn bounded_reader_fails_closed_on_store_errors_without_other_authority_calls() {
    for expected in [
        WorkloadSagaStoreError::Unavailable,
        WorkloadSagaStoreError::Corrupt,
    ] {
        let store = Arc::new(DecisionStore::new(Err(expected.clone())));
        let coordinator = WorkloadSagaCoordinator::new(store.clone());
        let request = WorkloadSagaPageRequest::new(None, 17).expect("fixture request is valid");

        let result = coordinator.plan_recoverable_page(request).await;

        assert_eq!(result, Err(expected));
        assert_eq!(store.recovery_reads.load(Ordering::SeqCst), 1);
        assert_eq!(store.mutation_or_other_reads.load(Ordering::SeqCst), 0);
    }
}
