use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLifecycleCapabilitySet, NetworkManagementMode, NetworkResourceGeneration,
    NetworkSovereigntyRequirements, PublishedEndpointId,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadEffectReferences,
    WorkloadExecutableEncoding, WorkloadExecutableIntent, WorkloadGeneration,
    WorkloadInspectionRequirement, WorkloadNetworkIntent, WorkloadNetworkPlanContent,
    WorkloadNetworkPlanIdentity, WorkloadOwnerEvidenceDigest, WorkloadOwnerObservation,
    WorkloadPhaseDetail, WorkloadPublicationIntent, WorkloadPublicationReference,
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaFuture, WorkloadSagaIntent,
    WorkloadSagaIntentUpdate, WorkloadSagaKey, WorkloadSagaPage, WorkloadSagaPageRequest,
    WorkloadSagaPhase, WorkloadSagaRecord, WorkloadSagaStore, WorkloadSagaStoreError,
    WorkloadSagaTenantPage, WorkloadSagaTenantPageRequest, WorkloadTerminalEvidenceDigest,
    WorkloadTerminalObservation,
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
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleCapabilitySet::new([]),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        None,
        None,
        [],
        [],
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
    WorkloadSagaIntent::new(
        DesiredWorkloadKind::Sandbox,
        desired_state,
        WorkloadGeneration::new(generation),
        WorkloadExecutableIntent::new(
            WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
            format!(r#"{{"fixtureSeed":{seed}}}"#),
        )
        .expect("fixture executable is valid"),
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
            Some(
                NodeIdentity::new(format!("node-{label}-{generation}"))
                    .expect("fixture node is valid"),
            ),
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

fn provision_references(
    phase: WorkloadSagaPhase,
    intent: &WorkloadSagaIntent,
    publication: &WorkloadPublicationReference,
) -> WorkloadEffectReferences {
    let publication = (intent.publication() == WorkloadPublicationIntent::PublishWhenReady
        && matches!(
            phase,
            WorkloadSagaPhase::Ready | WorkloadSagaPhase::Published | WorkloadSagaPhase::Observed
        ))
    .then(|| publication.clone());
    WorkloadEffectReferences::provision(intent, publication)
        .expect("fixture provision references are valid")
}

fn provision_observations(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
    publication: WorkloadPublicationIntent,
) -> Vec<WorkloadOwnerObservation> {
    let network = references.network().expect("network is retained").clone();
    let execution = references
        .execution()
        .expect("execution is retained")
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
                6
            } else {
                5
            }
        }
        _ => panic!("phase has no provision observations"),
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
                .expect("published phase retains publication")
                .clone(),
            evidence: evidence("publication-present"),
        });
    }
    observations
}

fn advance_provision(
    record: &WorkloadSagaRecord,
    phase: WorkloadSagaPhase,
    publication: &WorkloadPublicationReference,
) -> WorkloadSagaRecord {
    let references = provision_references(phase, record.active_intent(), publication);
    let observations =
        provision_observations(phase, &references, record.active_intent().publication());
    let detail =
        WorkloadPhaseDetail::provision(phase, record.active_intent(), references, observations)
            .expect("fixture phase detail is valid");
    record
        .advance(phase, detail, None)
        .expect("fixture provision transition is valid")
}

fn provision_record(
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

fn terminal_observations(
    phase: WorkloadSagaPhase,
    references: &WorkloadEffectReferences,
) -> Vec<WorkloadTerminalObservation> {
    let rank = match phase {
        WorkloadSagaPhase::WithdrawalCommitted => 0,
        WorkloadSagaPhase::Withdrawn => 1,
        WorkloadSagaPhase::Drained => 2,
        WorkloadSagaPhase::WorkloadStopped => 3,
        WorkloadSagaPhase::NetworkDetached => 4,
        WorkloadSagaPhase::NetworkReleased => 5,
        _ => panic!("phase has no teardown observations"),
    };
    let mut observations = Vec::new();
    if rank >= 1
        && let Some(reference) = references.publication()
    {
        observations.push(WorkloadTerminalObservation::PublicationAbsent {
            reference: reference.clone(),
            evidence: evidence("publication-absent"),
        });
    }
    if rank >= 2
        && let Some(reference) = references.execution()
    {
        observations.push(WorkloadTerminalObservation::ExecutionDrained {
            reference: reference.clone(),
            evidence: evidence("execution-drained"),
        });
    }
    if rank >= 3
        && let Some(reference) = references.execution()
    {
        observations.push(WorkloadTerminalObservation::ExecutionStopped {
            reference: reference.clone(),
            evidence: evidence("execution-stopped"),
        });
    }
    if rank >= 4
        && let Some(reference) = references.network()
    {
        observations.push(WorkloadTerminalObservation::NetworkDetached {
            reference: reference.clone(),
            evidence: evidence("network-detached"),
        });
    }
    if rank >= 5
        && let Some(reference) = references.network()
    {
        observations.push(WorkloadTerminalObservation::NetworkReleased {
            reference: reference.clone(),
            evidence: evidence("network-released"),
        });
    }
    observations
}

fn begin_teardown(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let references = record.phase_detail().references();
    let detail = WorkloadPhaseDetail::teardown(
        WorkloadSagaPhase::WithdrawalCommitted,
        record.active_intent(),
        record.phase(),
        references,
        Vec::new(),
    )
    .expect("fixture withdrawal detail is valid");
    record
        .advance(WorkloadSagaPhase::WithdrawalCommitted, detail, None)
        .expect("fixture withdrawal transition is valid")
}

fn advance_teardown(record: &WorkloadSagaRecord, phase: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let WorkloadPhaseDetail::Teardown(current) = record.phase_detail() else {
        panic!("teardown fixture must carry teardown detail");
    };
    let references = current.retained_references().clone();
    let detail = WorkloadPhaseDetail::teardown(
        phase,
        record.active_intent(),
        current.origin(),
        references.clone(),
        terminal_observations(phase, &references),
    )
    .expect("fixture teardown detail is valid");
    record
        .advance(phase, detail, None)
        .expect("fixture teardown transition is valid")
}

fn finish_teardown(
    mut record: WorkloadSagaRecord,
    target: WorkloadSagaPhase,
) -> WorkloadSagaRecord {
    if target == WorkloadSagaPhase::WithdrawalCommitted {
        return record;
    }
    for phase in [
        WorkloadSagaPhase::Withdrawn,
        WorkloadSagaPhase::Drained,
        WorkloadSagaPhase::WorkloadStopped,
        WorkloadSagaPhase::NetworkDetached,
        WorkloadSagaPhase::NetworkReleased,
    ] {
        record = advance_teardown(&record, phase);
        if target == phase {
            return record;
        }
    }
    let WorkloadPhaseDetail::Teardown(detail) = record.phase_detail() else {
        panic!("network released fixture carries teardown detail");
    };
    let digest = WorkloadTerminalEvidenceDigest::for_observations(detail.terminal_observations())
        .expect("terminal observations can be digested");
    record = record
        .advance(
            WorkloadSagaPhase::Recorded,
            WorkloadPhaseDetail::recorded(record.active_intent(), digest),
            None,
        )
        .expect("fixture recorded transition is valid");
    assert_eq!(target, WorkloadSagaPhase::Recorded);
    record
}

fn teardown_record(label: &str, target: WorkloadSagaPhase) -> WorkloadSagaRecord {
    let observed = provision_record(
        label,
        WorkloadSagaPhase::Observed,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    finish_teardown(begin_teardown(&observed), target)
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
    let references = ready.phase_detail().references();
    let inspections = vec![
        WorkloadInspectionRequirement::Network {
            reference: references.network().expect("network retained").clone(),
            expected_phase: ready.phase(),
        },
        WorkloadInspectionRequirement::Execution {
            reference: references.execution().expect("execution retained").clone(),
            expected_phase: ready.phase(),
        },
        WorkloadInspectionRequirement::Publication {
            reference: references
                .publication()
                .expect("publication retained")
                .clone(),
            expected_phase: ready.phase(),
        },
    ];
    let detail = WorkloadPhaseDetail::cleanup_pending(
        ready.active_intent(),
        ready.phase(),
        references,
        inspections,
    )
    .expect("fixture cleanup detail is valid");
    ready
        .advance(WorkloadSagaPhase::CleanupPending, detail, None)
        .expect("fixture cleanup transition is valid")
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

#[test]
fn selector_covers_every_phase_with_exact_typed_action_and_fences() {
    let intent_committed = provision_record(
        "intent",
        WorkloadSagaPhase::IntentCommitted,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_decision(
        &intent_committed,
        WorkloadSagaPhase::NetworkReserved,
        WorkloadSagaAction::ReserveNetwork {
            reference: nimbus_workloads::WorkloadNetworkReference::for_intent(
                intent_committed.active_intent(),
            ),
            plan: intent_committed
                .active_intent()
                .network()
                .compiled_plan()
                .clone(),
        },
    );

    let network_reserved = provision_record(
        "reserved",
        WorkloadSagaPhase::NetworkReserved,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_decision(
        &network_reserved,
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadSagaAction::PrepareWorkload {
            reference: network_reserved
                .phase_detail()
                .references()
                .execution()
                .expect("execution retained")
                .clone(),
        },
    );

    let workload_prepared = provision_record(
        "prepared",
        WorkloadSagaPhase::WorkloadPrepared,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_decision(
        &workload_prepared,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaAction::AttachNetwork {
            reference: workload_prepared
                .phase_detail()
                .references()
                .network()
                .expect("network retained")
                .clone(),
        },
    );

    let network_attached = provision_record(
        "attached",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_decision(
        &network_attached,
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadSagaAction::ActivateWorkload {
            reference: network_attached
                .phase_detail()
                .references()
                .execution()
                .expect("execution retained")
                .clone(),
        },
    );

    let activated = provision_record(
        "activated",
        WorkloadSagaPhase::WorkloadActivated,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    let activated_refs = activated.phase_detail().references();
    assert_decision(
        &activated,
        WorkloadSagaPhase::Ready,
        WorkloadSagaAction::InspectReadiness {
            network: activated_refs.network().expect("network retained").clone(),
            execution: activated_refs
                .execution()
                .expect("execution retained")
                .clone(),
        },
    );

    let ready = provision_record(
        "ready",
        WorkloadSagaPhase::Ready,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    assert_decision(
        &ready,
        WorkloadSagaPhase::Published,
        WorkloadSagaAction::Publish {
            reference: ready
                .phase_detail()
                .references()
                .publication()
                .expect("publication retained")
                .clone(),
        },
    );

    let published = provision_record(
        "published",
        WorkloadSagaPhase::Published,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    assert_decision(
        &published,
        WorkloadSagaPhase::Observed,
        WorkloadSagaAction::ObservePublication {
            reference: published
                .phase_detail()
                .references()
                .publication()
                .expect("publication retained")
                .clone(),
        },
    );

    let observed = provision_record(
        "observed",
        WorkloadSagaPhase::Observed,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::PublishWhenReady,
    );
    assert_decision(
        &observed,
        WorkloadSagaPhase::Observed,
        WorkloadSagaAction::Quiescent,
    );

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
        WorkloadSagaAction::PromoteSuccessor { intent: successor },
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
        WorkloadPublicationIntent::Withheld,
    );
    let decision = WorkloadSagaDecision::for_record(&record).expect("record is valid");
    let WorkloadSagaAction::ReserveNetwork { reference, plan } = decision.action() else {
        panic!("IntentCommitted must produce one complete pure reservation value");
    };

    assert_eq!(plan, record.active_intent().network().compiled_plan());
    assert_eq!(reference.plan_id(), plan.plan().plan_id());
    assert_eq!(reference.generation(), plan.plan().generation());
    assert_eq!(reference.digest(), plan.plan().digest());
    assert!(record.phase_detail().references().is_empty());
}

#[test]
fn selector_quiesces_prepare_only_withheld_observed_and_terminal_records() {
    let prepare_only = provision_record(
        "prepare-only",
        WorkloadSagaPhase::NetworkAttached,
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
    );
    assert_decision(
        &prepare_only,
        WorkloadSagaPhase::NetworkAttached,
        WorkloadSagaAction::Quiescent,
    );

    let withheld_ready = provision_record(
        "withheld",
        WorkloadSagaPhase::Ready,
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    );
    assert_decision(
        &withheld_ready,
        WorkloadSagaPhase::Observed,
        WorkloadSagaAction::AdvanceWithoutEffect,
    );

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
        WorkloadSagaAction::PromoteSuccessor { intent: running },
    );

    let stopped = stopped_intent("successor-stopped", 2);
    let stopped_record = recorded_with_successor("successor-stopped", stopped.clone());
    assert_decision(
        &stopped_record,
        WorkloadSagaPhase::Recorded,
        WorkloadSagaAction::PromoteSuccessor { intent: stopped },
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
