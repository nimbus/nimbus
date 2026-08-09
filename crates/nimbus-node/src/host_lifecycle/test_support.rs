use nimbus_core::WorkloadId;
use nimbus_network::{NetworkPlanDigest, NetworkPlanId};
use nimbus_workloads::{
    NodeIdentity, WorkloadDesiredDigest, WorkloadExecutionAttemptId, WorkloadExecutionProviderId,
    WorkloadExecutionReference, WorkloadNetworkReference, WorkloadOwnerEvidenceDigest,
    WorkloadProvisionAttempt, WorkloadProvisionAttemptInput, WorkloadProvisionDispatchClaim,
    WorkloadProvisionPrerequisiteEvidence, WorkloadProvisionProviderTarget,
    WorkloadProvisionSourceDigest, WorkloadProvisionStep, WorkloadProvisionSubjects,
    WorkloadProvisionSuccessEvidence, WorkloadRestartEpoch, WorkloadSagaKey, WorkloadSagaPhase,
    WorkloadSagaRevision,
};
use serde_json::json;

use super::HostLifecyclePlan;

pub(crate) fn activation_command_for_plan(
    plan: &HostLifecyclePlan,
    seed: u8,
) -> (WorkloadExecutionReference, WorkloadProvisionDispatchClaim) {
    let desired_digest = WorkloadDesiredDigest::sha256(format!("desired-{seed}"));
    let generation = plan.spec().generation();
    let node_identity = plan
        .spec()
        .assigned_node_id()
        .expect("fixture plan should retain an assigned node")
        .clone();
    let restart_epoch = WorkloadRestartEpoch::new(0);
    let execution_id = plan.execution_id();
    let execution: WorkloadExecutionReference = serde_json::from_value(json!({
        "workloadUid": plan.spec().workload_uid(),
        "nodeIdentity": node_identity,
        "executionId": execution_id,
        "restartEpoch": restart_epoch,
        "attemptId": WorkloadExecutionAttemptId::for_execution(execution_id, restart_epoch),
        "generation": generation,
        "desiredDigest": desired_digest,
    }))
    .expect("fixture execution reference should validate");
    let network: WorkloadNetworkReference = serde_json::from_value(json!({
        "planId": NetworkPlanId::generate(),
        "generation": generation,
        "digest": NetworkPlanDigest::from_bytes([seed.wrapping_add(1); 32]),
    }))
    .expect("fixture network reference should validate");
    let prerequisite = WorkloadProvisionPrerequisiteEvidence::new(
        format!("wpa_{}", format!("{:02x}", seed.wrapping_add(2)).repeat(32))
            .parse()
            .expect("fixture prerequisite attempt should validate"),
        WorkloadProvisionSuccessEvidence::ActivationPrerequisitesReady {
            network,
            execution: execution.clone(),
            evidence: WorkloadOwnerEvidenceDigest::sha256(format!("prerequisite-{seed}")),
        },
    )
    .expect("fixture prerequisite should validate");
    let key = WorkloadSagaKey::new(
        plan.spec().tenant_id().clone(),
        WorkloadId::new(format!("activation-{seed}")).expect("fixture workload id should validate"),
    );
    let attempt = WorkloadProvisionAttempt::new(WorkloadProvisionAttemptInput {
        saga_id: key.saga_id(),
        key,
        issuing_revision: WorkloadSagaRevision::new(4),
        generation,
        desired_digest,
        required_node: NodeIdentity::new(execution.node_identity().as_str())
            .expect("fixture node should validate"),
        source_digest: WorkloadProvisionSourceDigest::sha256(format!("source-{seed}")),
        execution_provider_id: WorkloadExecutionProviderId::for_registration_key(
            "node-activation-test",
        ),
        network_plan_digest: NetworkPlanDigest::from_bytes([seed.wrapping_add(3); 32]),
        selection_evidence: None,
        source_phase: WorkloadSagaPhase::NetworkAttached,
        target_phase: WorkloadSagaPhase::WorkloadActivated,
        step: WorkloadProvisionStep::ActivateWorkload,
        subjects: WorkloadProvisionSubjects::Execution(execution.clone()),
        prerequisite: Some(prerequisite),
    })
    .expect("fixture activation attempt should validate");
    let provider_target = WorkloadProvisionProviderTarget::for_attempt(&attempt)
        .expect("fixture provider target should validate")
        .expect("activation attempt should have an execution provider");
    let claim = serde_json::from_value(json!({
        "attempt": attempt,
        "claimedRevision": "5",
        "dispatchEpoch": "0",
        "providerTarget": provider_target,
        "authorization": { "kind": "initial" },
    }))
    .expect("fixture dispatch claim should validate");
    (execution, claim)
}
