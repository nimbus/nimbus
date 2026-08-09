use nimbus_sandbox::ProviderCommandOperation;
use nimbus_workloads::{WorkloadRestartEvidenceDigest, WorkloadRestartStep};

use super::*;

#[test]
fn every_restart_step_maps_to_one_exact_provider_journal_operation() {
    let cases = [
        (
            WorkloadRestartStep::WithdrawPublication,
            ProviderCommandOperation::WithdrawPublication,
        ),
        (
            WorkloadRestartStep::QuiesceExecution,
            ProviderCommandOperation::ResetWorkloadForRestart,
        ),
        (
            WorkloadRestartStep::PrepareExecution,
            ProviderCommandOperation::PrepareRestartAttempt,
        ),
        (
            WorkloadRestartStep::AttachNetwork,
            ProviderCommandOperation::AttachRetainedNetwork,
        ),
        (
            WorkloadRestartStep::InspectActivationPrerequisites,
            ProviderCommandOperation::InspectRestartActivationPrerequisites,
        ),
        (
            WorkloadRestartStep::ActivateExecution,
            ProviderCommandOperation::ActivateRestartedWorkload,
        ),
        (
            WorkloadRestartStep::InspectReadiness,
            ProviderCommandOperation::InspectRestartReadiness,
        ),
        (
            WorkloadRestartStep::Publish,
            ProviderCommandOperation::PublishRestartIngress,
        ),
        (
            WorkloadRestartStep::ObservePublication,
            ProviderCommandOperation::ObserveRestartPublication,
        ),
    ];

    for (step, expected) in cases {
        assert_eq!(operation(step), expected, "{step:?}");
    }
}

#[test]
fn only_process_bound_restart_effects_reconcile_live_absence() {
    for step in [
        WorkloadRestartStep::AttachNetwork,
        WorkloadRestartStep::ActivateExecution,
        WorkloadRestartStep::Publish,
        WorkloadRestartStep::ObservePublication,
    ] {
        assert!(requires_live_reconciliation(step), "{step:?}");
    }
    for step in [
        WorkloadRestartStep::WithdrawPublication,
        WorkloadRestartStep::QuiesceExecution,
        WorkloadRestartStep::PrepareExecution,
        WorkloadRestartStep::InspectActivationPrerequisites,
        WorkloadRestartStep::InspectReadiness,
    ] {
        assert!(!requires_live_reconciliation(step), "{step:?}");
    }
}

#[test]
fn execute_absence_requires_guest_inspection_before_terminal_absence() {
    let absence = MachineApiWorkloadRestartObservation::AuthenticatedAbsent {
        evidence: evidence_digest(b"execute observed no effect"),
    };

    assert_eq!(
        durable_observation_kind(MachineApiWorkloadRestartCommandMode::Execute, &absence),
        ProviderCommandObservationKind::Ambiguous,
        "execute-time absence is not authenticated inspection evidence"
    );
    assert_eq!(
        durable_observation_kind(MachineApiWorkloadRestartCommandMode::Inspect, &absence),
        ProviderCommandObservationKind::Absent,
        "exact inspection may persist authenticated absence"
    );
}

#[test]
fn restart_evidence_digest_is_domain_separated_and_deterministic() {
    let first = evidence_digest(b"provider evidence");
    let replay = evidence_digest(b"provider evidence");
    let other = evidence_digest(b"other provider evidence");

    assert_eq!(first, replay);
    assert_ne!(first, other);
    assert_ne!(
        first,
        WorkloadRestartEvidenceDigest::sha256(b"provider evidence")
    );
}
