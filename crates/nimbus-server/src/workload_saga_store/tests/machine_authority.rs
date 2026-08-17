use nimbus_compute::machine_stop_authority::{
    MachineWorkloadAuthorityStore, MachineWorkloadSagaAuthorityState,
};
use nimbus_workloads::{
    DesiredWorkloadState, WorkloadActivationIntent, WorkloadGeneration, WorkloadNetworkIntent,
    WorkloadPublicationIntent, WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaIntent,
    WorkloadSagaIntentUpdate, WorkloadSagaRecord, WorkloadSagaStore,
};

use super::super::EngineWorkloadSagaStore;
use super::{
    compiled_network_plan, engine, initial_record, provision_source_for_execution_provider,
};

fn provider(key: &str) -> nimbus_workloads::WorkloadExecutionProviderId {
    nimbus_workloads::WorkloadExecutionProviderId::for_registration_key(key)
}

async fn persist(
    store: &EngineWorkloadSagaStore,
    expected: WorkloadSagaExpected,
    record: &WorkloadSagaRecord,
) {
    assert_eq!(
        store.compare_and_swap(expected, record.clone()).await,
        Ok(WorkloadSagaCommit::Applied)
    );
}

fn successor_for_provider(
    current: &WorkloadSagaRecord,
    execution_provider_id: nimbus_workloads::WorkloadExecutionProviderId,
) -> WorkloadSagaRecord {
    let generation = current
        .active_intent()
        .generation()
        .checked_next()
        .expect("fixture generation should advance");
    let executable = current.active_intent().executable().clone();
    let source = provision_source_for_execution_provider(
        &executable,
        current.key().workload_id().as_str(),
        generation.as_u64(),
        current
            .active_intent()
            .source()
            .attachment_provider_id()
            .clone(),
        execution_provider_id,
    );
    let successor = WorkloadSagaIntent::new_with_restart_policy(
        current.active_intent().kind(),
        DesiredWorkloadState::Stopped,
        generation,
        executable,
        source,
        current.active_intent().restart_policy(),
        WorkloadNetworkIntent::new(compiled_network_plan(
            current.key().tenant_id(),
            current.key().workload_id().as_str(),
            generation.as_u64(),
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        )),
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        current.active_intent().admission().clone(),
    )
    .expect("fixture successor should validate");
    let WorkloadSagaIntentUpdate::Transition(successor) = current
        .apply_intent(successor)
        .expect("higher stopped intent should begin retirement")
    else {
        panic!("higher stopped intent should create one transition");
    };
    *successor
}

fn terminal_record_for_provider(
    label: &str,
    execution_provider_id: nimbus_workloads::WorkloadExecutionProviderId,
) -> WorkloadSagaRecord {
    let running = initial_record(label);
    let executable = running.active_intent().executable().clone();
    let source = provision_source_for_execution_provider(
        &executable,
        label,
        1,
        running
            .active_intent()
            .source()
            .attachment_provider_id()
            .clone(),
        execution_provider_id,
    );
    let stopped = WorkloadSagaIntent::new_with_restart_policy(
        running.active_intent().kind(),
        DesiredWorkloadState::Stopped,
        WorkloadGeneration::new(1),
        executable,
        source,
        running.active_intent().restart_policy(),
        WorkloadNetworkIntent::new(compiled_network_plan(
            running.key().tenant_id(),
            label,
            1,
            WorkloadActivationIntent::PrepareOnly,
            WorkloadPublicationIntent::Withheld,
        )),
        WorkloadActivationIntent::PrepareOnly,
        WorkloadPublicationIntent::Withheld,
        running.active_intent().admission().clone(),
    )
    .expect("fixture stopped intent should validate");
    WorkloadSagaRecord::new(running.key().clone(), stopped)
        .expect("stopped record should be terminal")
}

#[tokio::test]
async fn machine_authority_lists_active_and_successor_intents_for_their_exact_providers() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let store = EngineWorkloadSagaStore::new(engine(&root));
    let active_provider = provider("fixture-execution");
    let successor_provider = provider("forwarded-machine");
    let initial = initial_record("machine-provider-transition");
    let successor = successor_for_provider(&initial, successor_provider.clone());
    persist(&store, WorkloadSagaExpected::Missing, &initial).await;
    persist(
        &store,
        WorkloadSagaExpected::Revision(initial.revision()),
        &successor,
    )
    .await;

    let active = store
        .list_machine_workload_authority_from_engine(&active_provider)
        .await
        .expect("active provider authority should enumerate");
    let retiring = store
        .list_machine_workload_authority_from_engine(&successor_provider)
        .await
        .expect("successor provider authority should enumerate");
    let unrelated = store
        .list_machine_workload_authority_from_engine(&provider("unrelated"))
        .await
        .expect("unrelated provider scan should remain complete");

    assert_eq!(active.len(), 1);
    assert_eq!(retiring.len(), 1);
    assert_eq!(active[0].key(), successor.key());
    assert_eq!(retiring[0].key(), successor.key());
    assert_eq!(active[0].generation().as_u64(), 1);
    assert_eq!(retiring[0].generation().as_u64(), 2);
    assert_eq!(
        active[0].state(),
        MachineWorkloadSagaAuthorityState::ActiveDesired
    );
    assert_eq!(
        retiring[0].state(),
        MachineWorkloadSagaAuthorityState::Retiring
    );
    assert!(unrelated.is_empty());
}

#[tokio::test]
async fn machine_authority_scan_is_complete_bounded_indexed_and_durable_after_reopen() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let writer_engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(std::sync::Arc::clone(&writer_engine));
    let execution_provider = provider("fixture-execution");
    for index in 0..129 {
        let record = initial_record(&format!("machine-page-{index:03}"));
        persist(&store, WorkloadSagaExpected::Missing, &record).await;
    }
    let terminal = terminal_record_for_provider("machine-terminal", execution_provider.clone());
    persist(&store, WorkloadSagaExpected::Missing, &terminal).await;

    let before = writer_engine
        .query_planning_stats_for_testing(
            &super::super::schema::workload_saga_tenant().expect("system tenant should validate"),
        )
        .expect("query stats should load");
    let first = store
        .list_machine_workload_authority_from_engine(&execution_provider)
        .await
        .expect("complete authority should enumerate");
    let after = writer_engine
        .query_planning_stats_for_testing(
            &super::super::schema::workload_saga_tenant().expect("system tenant should validate"),
        )
        .expect("query stats should load");

    assert_eq!(first.len(), 130);
    assert_eq!(
        first
            .iter()
            .filter(|authority| authority.state() == MachineWorkloadSagaAuthorityState::Terminal)
            .count(),
        1
    );
    assert_eq!(after.query_full_scan_count, before.query_full_scan_count);
    assert_eq!(
        after.query_single_field_index_count,
        before.query_single_field_index_count
    );
    assert_eq!(
        after.query_composite_index_count - before.query_composite_index_count,
        3,
        "two recovery partitions and one lookahead continuation must use the composite index"
    );

    drop(store);
    drop(writer_engine);
    let reopened = EngineWorkloadSagaStore::new(engine(&root));
    assert_eq!(
        reopened
            .list_machine_workload_authority_from_engine(&execution_provider)
            .await
            .expect("reopened authority should enumerate"),
        first
    );
}
