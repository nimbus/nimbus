use nimbus_workloads::{
    ProposedWorkloadTeardownTransition, WorkloadSagaPhase, WorkloadTeardownCommandMode,
    WorkloadTeardownDecision,
};

use super::*;
use crate::workload_saga::recovery::tests::{teardown_record, teardown_success_evidence};
use crate::workload_saga::teardown_decision::materialize_teardown_candidate;
use crate::workload_saga::teardown_test_support::{CasFault, DurableTeardownStore};

fn initial(label: &str) -> WorkloadSagaRecord {
    teardown_record(label, WorkloadSagaPhase::WithdrawalCommitted)
}

fn candidate(record: &WorkloadSagaRecord) -> WorkloadSagaRecord {
    let WorkloadTeardownDecision::PersistCandidate(
        proposed @ ProposedWorkloadTeardownTransition::Claim { .. },
    ) = record.decide_teardown().expect("withdrawal is reducible")
    else {
        panic!("withdrawal fixture must require a claim");
    };
    materialize_teardown_candidate(record, &proposed).expect("claim materializes")
}

#[tokio::test]
async fn only_direct_claim_cas_winner_receives_execute() {
    let loaded = initial("teardown-command-direct");
    let store = DurableTeardownStore::with_record(loaded.clone());
    let confirmed = WorkloadSagaCoordinator::new(store)
        .confirm_teardown_transition(&loaded, candidate(&loaded))
        .await
        .expect("direct claim confirmation succeeds");

    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::AppliedByThisCall
    );
    assert_eq!(
        confirmed
            .command()
            .expect("direct winner gets a command")
            .mode(),
        WorkloadTeardownCommandMode::Execute
    );
}

#[tokio::test]
async fn replay_and_confirmed_ambiguity_receive_inspect_only() {
    let replay_loaded = initial("teardown-command-replay");
    let replay_candidate = candidate(&replay_loaded);
    let replay_store = DurableTeardownStore::with_record(replay_candidate.clone());
    let replay = WorkloadSagaCoordinator::new(replay_store)
        .confirm_teardown_transition(&replay_loaded, replay_candidate)
        .await
        .expect("replayed claim transitions to inspection");
    assert_eq!(
        replay.command().expect("replay gets inspection").mode(),
        WorkloadTeardownCommandMode::Inspect
    );

    let ambiguous_loaded = initial("teardown-command-confirmed-ambiguity");
    let ambiguous_store = DurableTeardownStore::with_record_and_fault(
        ambiguous_loaded.clone(),
        CasFault::AmbiguousAfterApply,
    );
    let ambiguous = WorkloadSagaCoordinator::new(ambiguous_store)
        .confirm_teardown_transition(&ambiguous_loaded, candidate(&ambiguous_loaded))
        .await
        .expect("confirmed ambiguity transitions to inspection");
    assert_eq!(
        ambiguous
            .command()
            .expect("confirmed ambiguity gets inspection")
            .mode(),
        WorkloadTeardownCommandMode::Inspect
    );
}

#[tokio::test]
async fn unresolved_claim_ambiguity_emits_no_command() {
    let loaded = initial("teardown-command-unresolved");
    let store =
        DurableTeardownStore::with_record_and_fault(loaded.clone(), CasFault::AmbiguousBeforeApply);
    let confirmed = WorkloadSagaCoordinator::new(store)
        .confirm_teardown_transition(&loaded, candidate(&loaded))
        .await
        .expect("unresolved ambiguity is typed");
    assert_eq!(
        confirmed.confirmation(),
        WorkloadSagaConfirmation::UnresolvedAmbiguity
    );
    assert!(confirmed.confirmed_record().is_none());
    assert!(confirmed.command().is_none());
}

#[tokio::test]
async fn confirmed_command_binds_complete_claim_and_record_fence() {
    let loaded = initial("teardown-command-fence");
    let store = DurableTeardownStore::with_record(loaded.clone());
    let confirmed = WorkloadSagaCoordinator::new(store)
        .confirm_teardown_transition(&loaded, candidate(&loaded))
        .await
        .expect("direct claim confirmation succeeds");
    let record = confirmed.confirmed_record().expect("claim is durable");
    let command = confirmed.command().expect("direct winner gets execute");
    let claim = record
        .teardown_disposition()
        .and_then(WorkloadTeardownDisposition::claim)
        .expect("durable claim exists");
    let attempt = claim.attempt();

    assert_eq!(command.key(), record.key());
    assert_eq!(command.saga_id(), record.saga_id());
    assert_eq!(command.issuing_revision(), attempt.issuing_revision());
    assert_eq!(
        command.issuing_transition_id(),
        attempt.issuing_transition_id()
    );
    assert_eq!(command.confirmed_revision(), record.revision());
    assert_eq!(
        command.confirmed_transition_id(),
        record.last_transition().transition_id()
    );
    assert_eq!(command.generation(), attempt.generation());
    assert_eq!(command.desired_digest(), attempt.desired_digest());
    assert_eq!(command.required_node(), attempt.required_node());
    assert_eq!(command.source(), record.active_intent().source());
    assert_eq!(command.source_digest(), attempt.source_digest());
    assert_eq!(command.network_plan_digest(), attempt.network_plan_digest());
    assert_eq!(
        Some(command.execution_locator()),
        record.phase_detail().references().execution()
    );
    assert_eq!(command.selection_evidence(), attempt.selection_evidence());
    assert_eq!(command.attempt_id(), attempt.attempt_id());
    assert_eq!(command.dispatch_epoch(), claim.dispatch_epoch());
    assert_eq!(command.provider_target(), claim.provider_target());
    assert_eq!(command.step(), attempt.step());
    assert_eq!(command.subjects(), attempt.subjects());
    assert_eq!(command.mode(), WorkloadTeardownCommandMode::Execute);
    assert_eq!(command.claim(), claim);
}

#[tokio::test]
async fn confirmed_teardown_command_binds_exact_compiled_publication_membership() {
    let loaded = initial("teardown-command-network-membership");
    let store = DurableTeardownStore::with_record(loaded.clone());
    let confirmed = WorkloadSagaCoordinator::new(store)
        .confirm_teardown_transition(&loaded, candidate(&loaded))
        .await
        .expect("direct claim confirmation succeeds");
    let record = confirmed.confirmed_record().expect("claim is durable");
    let command = confirmed.command().expect("direct winner gets execute");
    let retained = command.compiled_network_plan();

    assert_eq!(
        retained,
        record.active_intent().network().compiled_plan(),
        "the command must retain the exact compiled plan authenticated by the confirmed record"
    );
    assert_eq!(
        retained.plan().digest(),
        command.network_plan_digest(),
        "retained content must authenticate the command's network-plan digest"
    );
    let WorkloadTeardownSubjects::Publication(reference) = command.subjects() else {
        panic!("withdrawal fixture must retain a publication subject");
    };
    let membership = retained
        .content()
        .listeners()
        .iter()
        .map(|listener| {
            (
                listener.endpoint_id().clone(),
                listener.listener_id().clone(),
                listener.port_lease_id().clone(),
            )
        })
        .collect::<Vec<_>>();
    let endpoints = membership
        .iter()
        .map(|(endpoint_id, _, _)| endpoint_id.clone())
        .collect::<Vec<_>>();

    assert_eq!(reference.endpoints(), endpoints);
    assert!(membership.iter().all(|(_, listener_id, lease_id)| lease_id
        == &nimbus_network::PortLeaseId::for_listener(listener_id)));
}

#[tokio::test]
async fn crossed_teardown_command_result_preserves_durable_revision() {
    let loaded = initial("teardown-command-crossed-result");
    let store = DurableTeardownStore::with_record(loaded.clone());
    let confirmed = WorkloadSagaCoordinator::new(store.clone())
        .confirm_teardown_transition(&loaded, candidate(&loaded))
        .await
        .expect("direct claim confirmation succeeds");
    let record = confirmed.confirmed_record().expect("claim is durable");
    let command = confirmed.command().expect("direct winner gets execute");
    let outcome = WorkloadTeardownProviderOutcome::Execute(
        WorkloadTeardownExecuteOutcome::Succeeded(Box::new(teardown_success_evidence(
            command.step(),
            command.subjects(),
        ))),
    );
    let result = WorkloadTeardownCommandResult::for_command(record, command, outcome)
        .expect("exact callback converts");
    let other_loaded = teardown_record(
        "teardown-command-crossed-fields",
        WorkloadSagaPhase::Withdrawn,
    );
    let other_store = DurableTeardownStore::with_record(other_loaded.clone());
    let other_confirmed = WorkloadSagaCoordinator::new(other_store)
        .confirm_teardown_transition(&other_loaded, candidate(&other_loaded))
        .await
        .expect("crossed command fixture should confirm");
    let other_record = other_confirmed
        .confirmed_record()
        .expect("crossed fixture claim is durable");
    let other_command = other_confirmed
        .command()
        .expect("crossed fixture gets execute");
    let other_result = WorkloadTeardownCommandResult::for_command(
        other_record,
        other_command,
        WorkloadTeardownProviderOutcome::Execute(WorkloadTeardownExecuteOutcome::Succeeded(
            Box::new(teardown_success_evidence(
                other_command.step(),
                other_command.subjects(),
            )),
        )),
    )
    .expect("crossed fixture result converts");
    let before = store.record();
    let mut crossed = Vec::new();

    let mut value = result.clone();
    value.command_id = other_result.command_id;
    crossed.push(("command", value));
    let mut value = result.clone();
    value.key = other_result.key.clone();
    crossed.push(("key", value));
    let mut value = result.clone();
    value.saga_id = other_result.saga_id.clone();
    crossed.push(("saga", value));
    let mut value = result.clone();
    value.confirmed_revision = value
        .confirmed_revision
        .checked_next()
        .expect("fixture revision can advance");
    crossed.push(("confirmed revision", value));
    let mut value = result.clone();
    value.confirmed_transition_id = other_result.confirmed_transition_id.clone();
    crossed.push(("confirmed transition", value));
    let mut value = result.clone();
    value.generation = WorkloadGeneration::new(value.generation.as_u64() + 1);
    crossed.push(("generation", value));
    let mut value = result.clone();
    value.desired_digest = other_result.desired_digest;
    crossed.push(("desired digest", value));
    let mut value = result.clone();
    value.required_node = other_result.required_node.clone();
    crossed.push(("required node", value));
    let mut value = result.clone();
    value.source = other_result.source.clone();
    crossed.push(("source", value));
    let mut value = result.clone();
    value.source_digest = other_result.source_digest;
    crossed.push(("source digest", value));
    let mut value = result.clone();
    value.network_plan_digest = other_result.network_plan_digest;
    crossed.push(("network plan digest", value));
    let mut value = result.clone();
    value.selection_evidence = None;
    crossed.push(("selection evidence", value));
    let mut value = result.clone();
    value.execution_locator = other_result.execution_locator.clone();
    crossed.push(("execution locator", value));
    let mut value = result.clone();
    value.attempt_id = other_result.attempt_id.clone();
    crossed.push(("attempt", value));
    let mut value = result.clone();
    value.dispatch_epoch = value
        .dispatch_epoch
        .checked_next()
        .expect("fixture dispatch epoch can advance");
    crossed.push(("dispatch epoch", value));
    let mut value = result.clone();
    value.provider_target = other_result.provider_target.clone();
    crossed.push(("provider target", value));
    let mut value = result.clone();
    value.subjects = other_result.subjects.clone();
    crossed.push(("subjects", value));
    let mut value = result.clone();
    value.step = other_result.step;
    crossed.push(("step", value));
    let mut value = result;
    value.mode = WorkloadTeardownCommandMode::Inspect;
    crossed.push(("mode", value));

    for (fence, crossed) in crossed {
        assert!(
            apply_teardown_result(record, command, crossed).is_err(),
            "crossed {fence} must fail before reduction"
        );
        assert_eq!(
            store.record(),
            before,
            "crossed {fence} must preserve durable truth"
        );
    }
}

#[tokio::test]
async fn crossed_outcome_mode_is_rejected_before_reduction() {
    let loaded = initial("teardown-command-crossed-mode");
    let store = DurableTeardownStore::with_record(loaded.clone());
    let confirmed = WorkloadSagaCoordinator::new(store)
        .confirm_teardown_transition(&loaded, candidate(&loaded))
        .await
        .expect("direct claim confirmation succeeds");
    let record = confirmed.confirmed_record().expect("claim is durable");
    let command = confirmed.command().expect("direct winner gets execute");
    assert!(
        WorkloadTeardownCommandResult::for_command(
            record,
            command,
            WorkloadTeardownProviderOutcome::Inspect(WorkloadTeardownInspectOutcome::Ambiguous),
        )
        .is_err()
    );
}
