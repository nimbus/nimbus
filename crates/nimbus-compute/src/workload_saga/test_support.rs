//! Test-only histories driven through the real pure provision reducer.

use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{
    NetworkAttachmentCapabilitySet, NetworkCapabilityRequirements, NetworkControlPlaneLocality,
    NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet, NetworkIngressCapabilitySet,
    NetworkLifecycleCapabilitySet, NetworkLifecycleRequirements, NetworkManagementMode,
    NetworkProviderId, NetworkResourceGeneration, NetworkSovereigntyRequirements,
};
use nimbus_workloads::{
    CompiledWorkloadNetworkPlan, DesiredWorkloadKind, DesiredWorkloadState, NodeIdentity,
    WorkloadActivationIntent, WorkloadAdmissionEvidence, WorkloadInspectionVersion,
    WorkloadNetworkIntent, WorkloadNetworkPlanContent, WorkloadNetworkPlanIdentity,
    WorkloadOwnerEvidenceDigest, WorkloadPhaseDetail, WorkloadProvisionAttempt,
    WorkloadProvisionDisposition, WorkloadProvisionEffectResult, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadPublicationIntent, WorkloadRestartAdmissionInput,
    WorkloadRestartAdmissionUpdate, WorkloadRestartEffectResult,
    WorkloadRestartNotBeforeUnixMillis, WorkloadRestartPolicy, WorkloadRestartRequestId,
    WorkloadRestartTrigger, WorkloadSagaIntent, WorkloadSagaIntentUpdate, WorkloadSagaKey,
    WorkloadSagaPhase, WorkloadSagaRecord,
};

use super::WorkloadProvisionDecision;

pub(crate) fn success_for(attempt: &WorkloadProvisionAttempt) -> WorkloadProvisionSuccessEvidence {
    let evidence = WorkloadOwnerEvidenceDigest::sha256(format!("{:?}", attempt.step()));
    match (attempt.step(), attempt.subjects()) {
        (WorkloadProvisionStep::ReserveNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkReserved {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::PrepareWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadPrepared {
            reference: reference.clone(),
            evidence,
        },
        (WorkloadProvisionStep::AttachNetwork, WorkloadProvisionSubjects::Network(reference)) => {
            WorkloadProvisionSuccessEvidence::NetworkAttached {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::InspectActivationPrerequisites,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::ActivateWorkload,
            WorkloadProvisionSubjects::Execution(reference),
        ) => WorkloadProvisionSuccessEvidence::WorkloadActivated {
            reference: reference.clone(),
            evidence,
        },
        (
            WorkloadProvisionStep::InspectWorkloadReadiness,
            WorkloadProvisionSubjects::Readiness { network, execution },
        ) => WorkloadProvisionSuccessEvidence::WorkloadReady {
            network: network.clone(),
            execution: execution.clone(),
            evidence,
        },
        (WorkloadProvisionStep::Publish, WorkloadProvisionSubjects::Publication(reference)) => {
            WorkloadProvisionSuccessEvidence::Published {
                reference: reference.clone(),
                evidence,
            }
        }
        (
            WorkloadProvisionStep::ObservePublication,
            WorkloadProvisionSubjects::Publication(reference),
        ) => WorkloadProvisionSuccessEvidence::PublicationObserved {
            reference: reference.clone(),
            evidence,
        },
        _ => panic!("fixture attempt step and typed subject must remain correlated"),
    }
}

pub(crate) fn provision_candidates(record: &WorkloadSagaRecord) -> Vec<WorkloadSagaRecord> {
    let WorkloadProvisionDecision::Proposed(proposed) =
        WorkloadProvisionDecision::plan(record).expect("fixture phase should be reducible")
    else {
        panic!("fixture phase should produce a provision proposal");
    };
    let mut candidate = proposed.into_candidate();
    let mut candidates = vec![candidate.clone()];
    while let Some(WorkloadProvisionDisposition::DispatchPending(claim)) =
        candidate.provision_disposition()
    {
        let attempt = claim.attempt();
        let result = WorkloadProvisionEffectResult::Succeeded {
            attempt_id: attempt.attempt_id().clone(),
            evidence: success_for(attempt),
        };
        let WorkloadProvisionDecision::Proposed(proposed) =
            WorkloadProvisionDecision::reduce(&candidate, result)
                .expect("fixture success should reduce")
        else {
            panic!("fixture success should produce a durable candidate");
        };
        candidate = proposed.into_candidate();
        candidates.push(candidate.clone());
    }
    candidates
}

pub(crate) fn confirmed_provision(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    provision_candidates(record)
        .pop()
        .expect("fixture provision decision should produce a candidate")
}

pub(crate) fn first_proposed_candidate(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    provision_candidates(record)
        .into_iter()
        .next()
        .expect("fixture provision decision should produce a first candidate")
}

fn restart_compiled_plan(
    tenant_id: &TenantId,
    label: &str,
    generation: u64,
) -> CompiledWorkloadNetworkPlan {
    let identity = WorkloadNetworkPlanIdentity::new(
        tenant_id.clone(),
        format!("restart-{label}"),
        NetworkResourceGeneration::new(generation),
    )
    .expect("restart fixture network identity is valid");
    let requirements = NetworkCapabilityRequirements::new(
        NetworkAttachmentCapabilitySet::new(NetworkManagementMode::NimbusHostManaged, [], []),
        NetworkEndpointCapabilitySet::new([], [], [], [], []),
        NetworkIngressCapabilitySet::new([]),
        NetworkForwardingCapabilitySet::new([]),
        NetworkLifecycleRequirements::new(
            NetworkLifecycleCapabilitySet::new([]),
            NetworkLifecycleCapabilitySet::new([]),
        ),
        NetworkSovereigntyRequirements::new(NetworkControlPlaneLocality::LocalOnly, [], true),
    );
    let content = WorkloadNetworkPlanContent::new(
        identity,
        requirements,
        None,
        None,
        None,
        [],
        [],
        [],
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
    )
    .expect("restart fixture network content is valid");
    CompiledWorkloadNetworkPlan::from_content(content)
        .expect("restart fixture compiled network plan is valid")
}

fn restart_intent_for_tenant(
    tenant_id: &TenantId,
    label: &str,
    generation: u64,
    policy: WorkloadRestartPolicy,
) -> WorkloadSagaIntent {
    let executable = nimbus_workloads::WorkloadExecutableIntent::new(
        nimbus_workloads::WorkloadExecutableEncoding::SandboxSpecCanonicalJsonV1,
        format!(r#"{{"fixture":"restart-{label}-{generation}"}}"#),
    )
    .expect("restart fixture executable is valid");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(
            format!("workload-{label}"),
            format!("revision-{generation}"),
        )
        .expect("restart fixture source identity is valid"),
        WorkloadProvisionSourceGeneration::new(generation),
        WorkloadProvisionSourceResourceVersion::new(format!("restart-{label}-{generation}"))
            .expect("restart fixture source version is valid"),
        executable.content_digest(),
        NetworkProviderId::for_registration_key("restart-attachment"),
        nimbus_workloads::WorkloadExecutionProviderId::for_registration_key("restart-execution"),
    )
    .expect("restart fixture source evidence is valid");
    WorkloadSagaIntent::new_with_restart_policy(
        DesiredWorkloadKind::Sandbox,
        DesiredWorkloadState::Running,
        nimbus_workloads::WorkloadGeneration::new(generation),
        executable,
        source,
        policy,
        WorkloadNetworkIntent::new(restart_compiled_plan(tenant_id, label, generation)),
        WorkloadActivationIntent::ActivateWhenAttached,
        WorkloadPublicationIntent::Withheld,
        WorkloadAdmissionEvidence::new(
            format!("tid_{}", "3".repeat(64))
                .try_into()
                .expect("restart fixture decision ID is valid"),
            format!("twu_{}", "4".repeat(64))
                .try_into()
                .expect("restart fixture workload UID is valid"),
            NodeIdentity::new(format!("node-{label}")).expect("restart fixture node is valid"),
        ),
    )
    .expect("restart fixture intent is valid")
}

pub(crate) fn restart_intent(
    label: &str,
    generation: u64,
    policy: WorkloadRestartPolicy,
) -> WorkloadSagaIntent {
    let tenant_id =
        TenantId::new(format!("tenant-{label}")).expect("restart fixture tenant is valid");
    restart_intent_for_tenant(&tenant_id, label, generation, policy)
}

pub(crate) fn restart_observed_record(
    label: &str,
    policy: WorkloadRestartPolicy,
) -> WorkloadSagaRecord {
    let tenant_id =
        TenantId::new(format!("tenant-{label}")).expect("restart fixture tenant is valid");
    let key = WorkloadSagaKey::new(
        tenant_id,
        WorkloadId::new(format!("workload-{label}")).expect("restart fixture workload is valid"),
    );
    let initial = WorkloadSagaRecord::new(key, restart_intent(label, 1, policy))
        .expect("restart fixture record is valid");
    let mut record = initial;
    for _ in 0..32 {
        match WorkloadProvisionDecision::plan(&record)
            .expect("restart fixture provision state should reduce")
        {
            WorkloadProvisionDecision::Wait => {
                assert_eq!(record.phase(), WorkloadSagaPhase::Observed);
                return record;
            }
            WorkloadProvisionDecision::Proposed(proposed) => {
                record = proposed.into_candidate();
                if let Some(WorkloadProvisionDisposition::DispatchPending(claim)) =
                    record.provision_disposition()
                {
                    let result = WorkloadProvisionEffectResult::Succeeded {
                        attempt_id: claim.attempt().attempt_id().clone(),
                        evidence: success_for(claim.attempt()),
                    };
                    let WorkloadProvisionDecision::Proposed(completed) =
                        WorkloadProvisionDecision::reduce(&record, result)
                            .expect("restart fixture provision result should reduce")
                    else {
                        panic!("restart fixture result should produce a candidate");
                    };
                    record = completed.into_candidate();
                }
            }
            WorkloadProvisionDecision::InspectExact(claim) => {
                let result = WorkloadProvisionEffectResult::Succeeded {
                    attempt_id: claim.attempt().attempt_id().clone(),
                    evidence: success_for(claim.attempt()),
                };
                let WorkloadProvisionDecision::Proposed(completed) =
                    WorkloadProvisionDecision::reduce(&record, result)
                        .expect("restart fixture inspection result should reduce")
                else {
                    panic!("restart fixture inspection should produce a candidate");
                };
                record = completed.into_candidate();
            }
            WorkloadProvisionDecision::DefiniteFailure => {
                panic!("restart fixture has no provision failure");
            }
        }
    }
    panic!("restart fixture exceeded its provision decision bound")
}

pub(crate) fn withdrawn_record(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let detail = WorkloadPhaseDetail::teardown(
        WorkloadSagaPhase::WithdrawalCommitted,
        record.active_intent(),
        record.phase(),
        record.phase_detail().references(),
        Vec::new(),
    )
    .expect("restart fixture withdrawal detail is valid");
    record
        .advance(WorkloadSagaPhase::WithdrawalCommitted, detail, None)
        .expect("restart fixture withdrawal is valid")
}

pub(crate) fn record_with_successor(
    record: &WorkloadSagaRecord,
    label: &str,
) -> WorkloadSagaRecord {
    let WorkloadSagaIntentUpdate::Transition(candidate) = record
        .apply_intent(restart_intent_for_tenant(
            record.key().tenant_id(),
            label,
            2,
            record.active_intent().restart_policy(),
        ))
        .expect("restart fixture successor should queue")
    else {
        panic!("higher generation restart fixture should transition");
    };
    *candidate
}

fn succeed_restart_command(record: WorkloadSagaRecord, label: &str) -> WorkloadSagaRecord {
    let active = record
        .restart_state()
        .active()
        .expect("restart fixture should be active");
    let request_id = active.admission().request_id().clone();
    let claimed = record
        .claim_restart_command(&request_id)
        .expect("restart fixture command should claim");
    let claim = claimed
        .restart_state()
        .active()
        .and_then(|active| active.disposition().claim())
        .expect("restart fixture should retain its claim")
        .clone();
    claimed
        .apply_restart_effect_result(
            &claim,
            WorkloadRestartEffectResult::Succeeded {
                evidence: nimbus_workloads::WorkloadRestartEvidenceDigest::sha256(label),
            },
            None,
        )
        .expect("restart fixture command should succeed")
}

pub(crate) fn scheduled_restart_record(label: &str, not_before: u64) -> WorkloadSagaRecord {
    let record = restart_observed_record(label, WorkloadRestartPolicy::Always { max_restarts: 2 });
    let inspection_version = WorkloadInspectionVersion::from_bytes([0x55; 32]);
    let input = WorkloadRestartAdmissionInput {
        expected_revision: record.revision(),
        trigger: WorkloadRestartTrigger::Automatic { exit_code: 17 },
        inspection_version: Some(inspection_version),
        request_id: WorkloadRestartRequestId::for_automatic(record.saga_id(), inspection_version),
        not_before_unix_millis: WorkloadRestartNotBeforeUnixMillis::new(not_before),
    };
    let WorkloadRestartAdmissionUpdate::Transition(admitted) = record
        .admit_restart(input)
        .expect("restart fixture should admit")
    else {
        panic!("restart fixture admission should transition");
    };
    let request_id = admitted
        .restart_state()
        .active()
        .expect("restart fixture should be active")
        .admission()
        .request_id()
        .clone();
    let withdrawal = admitted
        .advance_restart_without_effect(&request_id)
        .expect("restart fixture should enter withdrawal");
    let withdrawn = succeed_restart_command(withdrawal, "restart-withdrawn");
    succeed_restart_command(withdrawn, "restart-quiesced")
}
