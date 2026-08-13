use nimbus_workloads::{
    ProposedWorkloadTeardownTransition, WorkloadSagaPhase, WorkloadTeardownDecision,
};

use super::*;
use crate::workload_saga::recovery::tests::{
    cleanup_pending_record, no_reference_teardown_record, restart_settlement_pending_record,
    teardown_record,
};
use crate::workload_saga::{WorkloadSagaAction, WorkloadSagaDecision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActionKind {
    Provision,
    Teardown,
    PromoteSuccessor,
    Quiescent,
}

fn action_kind(action: &WorkloadSagaAction) -> ActionKind {
    match action {
        WorkloadSagaAction::Provision(_) => ActionKind::Provision,
        WorkloadSagaAction::Teardown(_) => ActionKind::Teardown,
        WorkloadSagaAction::PromoteSuccessor { .. } => ActionKind::PromoteSuccessor,
        WorkloadSagaAction::Quiescent => ActionKind::Quiescent,
    }
}

#[test]
fn teardown_recovery_delegates_to_workloads_reducer() {
    let record = teardown_record("teardown-decision-delegation", WorkloadSagaPhase::Withdrawn);
    let portable = record
        .decide_teardown()
        .expect("portable teardown reducer should decide the next step");
    let projected = WorkloadSagaDecision::for_record(&record)
        .expect("compute should project the portable teardown decision");

    assert_eq!(projected.action(), &WorkloadSagaAction::Teardown(portable));
}

#[test]
fn raw_teardown_actions_are_absent_from_recovery_surface() {
    let record = teardown_record(
        "teardown-decision-exhaustive-surface",
        WorkloadSagaPhase::WithdrawalCommitted,
    );
    let projected = WorkloadSagaDecision::for_record(&record)
        .expect("teardown recovery should produce one portable action");

    assert_eq!(action_kind(projected.action()), ActionKind::Teardown);
}

#[test]
fn teardown_cleanup_waits_and_restart_settlement_records() {
    let cleanup = cleanup_pending_record("teardown-decision-cleanup-wait");
    let cleanup_decision = WorkloadSagaDecision::for_record(&cleanup)
        .expect("cleanup state should remain a typed portable decision");
    assert!(matches!(
        cleanup_decision.action(),
        WorkloadSagaAction::Teardown(WorkloadTeardownDecision::CleanupPending { .. })
    ));

    let settlement = restart_settlement_pending_record("teardown-decision-restart-wait");
    let settlement_decision = WorkloadSagaDecision::for_record(&settlement)
        .expect("restart settlement should remain a typed portable decision");
    assert!(matches!(
        settlement_decision.action(),
        WorkloadSagaAction::Teardown(WorkloadTeardownDecision::PersistCandidate(
            ProposedWorkloadTeardownTransition::RecordTerminal
        ))
    ));
}

#[test]
fn materialized_candidates_equal_workloads_owned_reducer_results() {
    for record in [
        teardown_record(
            "teardown-decision-effectful-candidate",
            WorkloadSagaPhase::WithdrawalCommitted,
        ),
        no_reference_teardown_record(
            "teardown-decision-resource-free-candidate",
            WorkloadSagaPhase::WithdrawalCommitted,
        ),
        no_reference_teardown_record(
            "teardown-decision-terminal-candidate",
            WorkloadSagaPhase::NetworkReleased,
        ),
    ] {
        let WorkloadTeardownDecision::PersistCandidate(proposed) = record
            .decide_teardown()
            .expect("fixture should produce a workloads-owned candidate")
        else {
            panic!("fixture should produce a durable candidate");
        };
        let expected = match &proposed {
            ProposedWorkloadTeardownTransition::Claim {
                attempt,
                provider_target,
            } => record
                .claim_teardown((**attempt).clone(), provider_target.clone())
                .expect("portable claim reducer should succeed"),
            ProposedWorkloadTeardownTransition::ResourceFree { step, .. } => record
                .record_resource_free_teardown_step(*step)
                .expect("portable resource-free reducer should succeed"),
            ProposedWorkloadTeardownTransition::RecordTerminal => record
                .record_terminal_teardown()
                .expect("portable terminal reducer should succeed"),
        };

        assert_eq!(
            materialize_teardown_candidate(&record, &proposed)
                .expect("compute should materialize the portable candidate"),
            expected
        );
    }
}
