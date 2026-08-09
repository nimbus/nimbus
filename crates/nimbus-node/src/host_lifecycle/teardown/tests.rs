use nimbus_core::{TenantId, WorkloadId};
use nimbus_network::{NetworkPlanDigest, NetworkPlanId, NetworkProviderId};
use nimbus_workloads::{
    NodeIdentity, TenantWorkloadUid, WorkloadDesiredDigest, WorkloadExecutableContentDigest,
    WorkloadExecutionAttemptId, WorkloadExecutionId, WorkloadExecutionProviderId,
    WorkloadExecutionReference, WorkloadGeneration, WorkloadNetworkReference,
    WorkloadOwnerEvidenceDigest, WorkloadProvisionAttempt, WorkloadProvisionAttemptInput,
    WorkloadProvisionDispatchClaim, WorkloadProvisionPrerequisiteEvidence,
    WorkloadProvisionProviderTarget, WorkloadProvisionSourceEvidence,
    WorkloadProvisionSourceGeneration, WorkloadProvisionSourceIdentity,
    WorkloadProvisionSourceResourceVersion, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadRestartEpoch, WorkloadSagaKey, WorkloadSagaPhase,
    WorkloadSagaRevision, WorkloadSagaTransitionId, WorkloadTeardownAttempt,
    WorkloadTeardownAttemptInput, WorkloadTeardownClaim, WorkloadTeardownCommandId,
    WorkloadTeardownCommandMode, WorkloadTeardownProviderTarget, WorkloadTeardownStep,
    WorkloadTeardownSubjects,
};
use serde_json::json;

use super::super::{
    HostTeardownExecuteClaim, HostTeardownInspectClaim, HostTeardownProviderClaimInput,
};

#[derive(Clone)]
pub(crate) struct Fixture {
    pub(crate) claim: WorkloadTeardownClaim,
    pub(crate) source: WorkloadProvisionSourceEvidence,
    pub(crate) execution: WorkloadExecutionReference,
    pub(crate) activation_claim: WorkloadProvisionDispatchClaim,
    pub(crate) confirmed_revision: WorkloadSagaRevision,
    pub(crate) confirmed_transition_id: WorkloadSagaTransitionId,
}

pub(crate) fn fixture(step: WorkloadTeardownStep) -> Fixture {
    fixture_with_source_tag(step, "primary")
}

pub(crate) fn fixture_with_source_tag(step: WorkloadTeardownStep, source_tag: &str) -> Fixture {
    let generation = WorkloadGeneration::new(7);
    let desired_digest = WorkloadDesiredDigest::sha256("host-teardown-desired");
    let node = NodeIdentity::new("node-host-teardown").expect("node should validate");
    let workload_uid: TenantWorkloadUid = format!("twu_{}", "21".repeat(32))
        .try_into()
        .expect("workload uid should validate");
    let execution_id = WorkloadExecutionId::for_execution(&workload_uid, &node, generation);
    let restart_epoch = WorkloadRestartEpoch::new(0);
    let execution: WorkloadExecutionReference = serde_json::from_value(json!({
        "workloadUid": workload_uid,
        "nodeIdentity": node,
        "executionId": execution_id,
        "restartEpoch": restart_epoch,
        "attemptId": WorkloadExecutionAttemptId::for_execution(&execution_id, restart_epoch),
        "generation": generation,
        "desiredDigest": desired_digest,
    }))
    .expect("execution reference should validate");
    let execution_provider_id =
        WorkloadExecutionProviderId::for_registration_key("host-teardown-provider");
    let source = WorkloadProvisionSourceEvidence::standalone_sandbox(
        WorkloadProvisionSourceIdentity::standalone_sandbox(
            format!("sandbox-host-teardown-{source_tag}"),
            "default",
        )
        .expect("source identity should validate"),
        WorkloadProvisionSourceGeneration::new(3),
        WorkloadProvisionSourceResourceVersion::new(format!("rv-host-teardown-{source_tag}"))
            .expect("resource version should validate"),
        WorkloadExecutableContentDigest::sha256(format!("host-teardown-executable-{source_tag}")),
        NetworkProviderId::for_registration_key("host-teardown-attachment"),
        execution_provider_id.clone(),
    )
    .expect("source evidence should validate");
    let network_plan_digest = NetworkPlanDigest::from_bytes([0x31; 32]);
    let network: WorkloadNetworkReference = serde_json::from_value(json!({
        "planId": NetworkPlanId::generate(),
        "generation": generation,
        "digest": network_plan_digest,
    }))
    .expect("network reference should validate");
    let prerequisite = WorkloadProvisionPrerequisiteEvidence::new(
        format!("wpa_{}", "31".repeat(32))
            .parse()
            .expect("prerequisite attempt should validate"),
        WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network,
            execution: execution.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256("host-teardown-prerequisite"),
        },
    )
    .expect("prerequisite evidence should validate");
    let activation_key = WorkloadSagaKey::new(
        TenantId::new("tenant-host-teardown").expect("tenant should validate"),
        WorkloadId::new("host-teardown-activation").expect("workload should validate"),
    );
    let activation_attempt = WorkloadProvisionAttempt::new(WorkloadProvisionAttemptInput {
        saga_id: activation_key.saga_id(),
        key: activation_key,
        issuing_revision: WorkloadSagaRevision::new(4),
        generation,
        desired_digest,
        required_node: execution.node_identity().clone(),
        source_digest: source.source_digest(),
        execution_provider_id: execution_provider_id.clone(),
        network_plan_digest,
        selection_evidence: None,
        source_phase: WorkloadSagaPhase::NetworkAttached,
        target_phase: WorkloadSagaPhase::WorkloadActivated,
        step: WorkloadProvisionStep::ActivateWorkload,
        subjects: WorkloadProvisionSubjects::Execution(execution.clone()),
        prerequisite: Some(prerequisite),
    })
    .expect("activation attempt should validate");
    let activation_target = WorkloadProvisionProviderTarget::for_attempt(&activation_attempt)
        .expect("activation target should validate")
        .expect("activation should select an execution provider");
    let activation_claim: WorkloadProvisionDispatchClaim = serde_json::from_value(json!({
        "attempt": activation_attempt,
        "claimedRevision": "5",
        "dispatchEpoch": "0",
        "providerTarget": activation_target,
        "authorization": { "kind": "initial" },
    }))
    .expect("activation claim should validate");
    let key = WorkloadSagaKey::new(
        TenantId::new("tenant-host-teardown").expect("tenant should validate"),
        WorkloadId::new("host-teardown").expect("workload should validate"),
    );
    let issuing_transition_id: WorkloadSagaTransitionId = format!("wst_{}", "11".repeat(32))
        .parse()
        .expect("transition should validate");
    let (source_phase, target_phase) = step.phases();
    let attempt = WorkloadTeardownAttempt::new(WorkloadTeardownAttemptInput {
        saga_id: key.saga_id(),
        key,
        issuing_revision: WorkloadSagaRevision::new(20),
        issuing_transition_id,
        generation,
        desired_digest,
        required_node: execution.node_identity().clone(),
        source_digest: source.source_digest(),
        execution_provider_id: execution_provider_id.clone(),
        network_plan_digest,
        selection_evidence: None,
        cause: nimbus_workloads::WorkloadTeardownCause::Successor {
            generation: WorkloadGeneration::new(8),
            desired_digest: WorkloadDesiredDigest::sha256("host-teardown-successor"),
        },
        successor_fence: None,
        source_phase,
        target_phase,
        step,
        subjects: WorkloadTeardownSubjects::Execution(execution.clone()),
    })
    .expect("teardown attempt should validate");
    let provider_target = WorkloadTeardownProviderTarget::for_attempt(&attempt)
        .expect("provider target should validate")
        .expect("execution teardown should select a provider");
    let claim: WorkloadTeardownClaim = serde_json::from_value(json!({
        "attempt": attempt,
        "claimedRevision": "21",
        "dispatchEpoch": "0",
        "providerTarget": provider_target,
        "authorization": { "kind": "initial" },
    }))
    .expect("teardown claim should validate");
    Fixture {
        claim,
        source,
        execution,
        activation_claim,
        confirmed_revision: WorkloadSagaRevision::new(21),
        confirmed_transition_id: format!("wst_{}", "12".repeat(32))
            .parse()
            .expect("confirmed transition should validate"),
    }
}

pub(crate) fn input(
    fixture: &Fixture,
    mode: WorkloadTeardownCommandMode,
) -> HostTeardownProviderClaimInput {
    HostTeardownProviderClaimInput {
        command_id: WorkloadTeardownCommandId::for_confirmed_dispatch(
            &fixture.claim,
            fixture.confirmed_revision,
            &fixture.confirmed_transition_id,
            mode,
        )
        .expect("command id should derive"),
        confirmed_revision: fixture.confirmed_revision,
        confirmed_transition_id: fixture.confirmed_transition_id.clone(),
        source: fixture.source.clone(),
        execution: fixture.execution.clone(),
        provider_target: fixture.claim.provider_target().clone(),
        claim: fixture.claim.clone(),
    }
}

pub(crate) fn inspection_fixture(fixture: &Fixture, transition_tag: &str) -> Fixture {
    let mut inspection = fixture.clone();
    inspection.confirmed_revision = inspection
        .confirmed_revision
        .checked_next()
        .expect("inspection revision should advance");
    inspection.confirmed_transition_id = format!("wst_{}", transition_tag.repeat(32))
        .parse()
        .expect("inspection transition should validate");
    inspection
}

pub(crate) fn retry_fixture_after_not_completed(
    fixture: &Fixture,
    inspection: &Fixture,
    retry_evidence: WorkloadOwnerEvidenceDigest,
    transition_tag: &str,
) -> Fixture {
    let inspection_command_id = WorkloadTeardownCommandId::for_confirmed_dispatch(
        &fixture.claim,
        inspection.confirmed_revision,
        &inspection.confirmed_transition_id,
        WorkloadTeardownCommandMode::Inspect,
    )
    .expect("inspection command should derive");
    let claimed_revision = inspection
        .confirmed_revision
        .checked_next()
        .expect("retry revision should advance");
    let dispatch_epoch = fixture
        .claim
        .dispatch_epoch()
        .checked_next()
        .expect("retry dispatch epoch should advance");
    let mut encoded = serde_json::to_value(&fixture.claim).expect("claim should serialize");
    encoded["claimedRevision"] = json!(claimed_revision.to_string());
    encoded["dispatchEpoch"] = json!(dispatch_epoch.to_string());
    encoded["authorization"] = json!({
        "kind": "retry_after_not_completed",
        "evidence": {
            "attemptId": fixture.claim.attempt().attempt_id(),
            "dispatchEpoch": fixture.claim.dispatch_epoch(),
            "inspectedRevision": inspection.confirmed_revision,
            "inspectedTransitionId": &inspection.confirmed_transition_id,
            "inspectionCommandId": inspection_command_id,
            "providerTarget": fixture.claim.provider_target(),
            "step": fixture.claim.attempt().step(),
            "evidence": retry_evidence,
        }
    });
    let claim = serde_json::from_value(encoded).expect("retry claim should validate");
    let mut retry = fixture.clone();
    retry.claim = claim;
    retry.confirmed_revision = claimed_revision;
    retry.confirmed_transition_id = format!("wst_{}", transition_tag.repeat(32))
        .parse()
        .expect("retry transition should validate");
    retry
}

#[test]
fn host_teardown_claim_binds_complete_confirmed_command_fence() {
    let fixture = fixture(WorkloadTeardownStep::StopExecution);
    let claim =
        HostTeardownExecuteClaim::new(input(&fixture, WorkloadTeardownCommandMode::Execute))
            .expect("complete exact command fence should validate");
    assert_eq!(claim.execution(), &fixture.execution);
    assert_eq!(
        claim.command_id(),
        input(&fixture, WorkloadTeardownCommandMode::Execute).command_id
    );

    let mut crossed = input(&fixture, WorkloadTeardownCommandMode::Execute);
    crossed.confirmed_transition_id = format!("wst_{}", "13".repeat(32))
        .parse()
        .expect("crossed transition should validate");
    assert!(HostTeardownExecuteClaim::new(crossed).is_err());
}

#[test]
fn drain_claim_requires_exact_withdrawn_to_drained_transition() {
    let drain = fixture(WorkloadTeardownStep::DrainExecution);
    let claim = HostTeardownExecuteClaim::new(input(&drain, WorkloadTeardownCommandMode::Execute))
        .expect("exact drain claim should validate");
    claim
        .require_step(WorkloadTeardownStep::DrainExecution)
        .expect("drain claim should authorize only the exact drain transition");
    assert_eq!(
        claim.step().phases(),
        (WorkloadSagaPhase::Withdrawn, WorkloadSagaPhase::Drained)
    );

    let stop = fixture(WorkloadTeardownStep::StopExecution);
    let stop_claim =
        HostTeardownExecuteClaim::new(input(&stop, WorkloadTeardownCommandMode::Execute))
            .expect("exact stop claim should validate");
    assert!(
        stop_claim
            .require_step(WorkloadTeardownStep::DrainExecution)
            .is_err()
    );
}

#[test]
fn inspect_claim_requires_inspection_revision_and_command_mode() {
    let mut fixture = fixture(WorkloadTeardownStep::StopExecution);
    fixture.confirmed_revision = WorkloadSagaRevision::new(22);
    let claim =
        HostTeardownInspectClaim::new(input(&fixture, WorkloadTeardownCommandMode::Inspect))
            .expect("exact inspection fence should validate");
    assert_eq!(claim.execution(), &fixture.execution);

    let execute_mode = input(&fixture, WorkloadTeardownCommandMode::Execute);
    assert!(HostTeardownInspectClaim::new(execute_mode).is_err());
}
