//! GR4 store-retry coverage for trigger execution: a transient store failure
//! at the pre-save fault point must retry the save in place rather than
//! re-enqueue the key, so the handler never runs twice for one attempt.

use super::*;

#[tokio::test]
async fn trigger_execution_retries_after_a_transient_store_failure_before_save() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let clock = Arc::new(ManualClock::new(Timestamp(80_000)));
    let faults = CountedFaultInjector::fail_nth_call(FaultPoint::TriggerExecutionBeforeSave, 1);
    let engine = Arc::new(
        Engine::new_with_simulation(data_dir.path(), clock.clone(), faults.clone())
            .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let document_id = DocumentId::from_key("store-retry").expect("document id should build");
    let executor = Arc::new(RecordingTriggerExecutor::default());
    engine
        .install_trigger_invocation_executor(executor.clone())
        .expect("trigger executor should install");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![trigger_registration(
                "firebase:storeRetryWritten",
                FirestoreCloudEventType::Written,
                ["tasks", "{taskId}"],
            )],
        )
        .expect("trigger registrations should persist in runtime");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("store-retry"))]),
        )
        .expect("insert should succeed despite a transient trigger-execution store failure");

    // The fault fires on `check_fault(TriggerExecutionBeforeSave)`, which is
    // reached only *after* `execute_invocation` has already run and the
    // Completed outcome has been computed in memory. The worker retries the
    // save itself in place (bounded, real-time backoff) instead of
    // re-enqueueing the key, so no clock advance is needed here: re-running
    // the handler on this retry would be the GR4 double-execution bug this
    // test guards against.
    wait_for_value(
        "trigger execution should retry past a transient pre-save store failure and complete",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(records[0].state, TriggerInvocationState::Completed { .. })
        },
    )
    .await;

    let records = engine
        .list_trigger_invocations_for_testing(&tenant_id)
        .expect("trigger invocations should load");
    assert_eq!(
        records.len(),
        1,
        "exactly one terminal trigger invocation record should exist after the store retry"
    );
    assert!(matches!(
        records[0].state,
        TriggerInvocationState::Completed { .. }
    ));
    assert_eq!(
        faults.failure_count(),
        1,
        "the store fault should have injected exactly one failure"
    );
    assert!(
        faults.visit_count() >= 2,
        "the pre-save store check must have been retried after the injected failure"
    );
    assert_eq!(
        executor.calls(),
        vec!["firebase:storeRetryWritten".to_string()],
        "a post-execution save retry must not re-invoke the handler (GR4 double-execution guard)"
    );
}

#[tokio::test]
async fn trigger_execution_retries_after_a_transient_store_failure_before_terminal_save() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let clock = Arc::new(ManualClock::new(Timestamp(82_000)));
    let faults = CountedFaultInjector::fail_nth_call(FaultPoint::TriggerExecutionBeforeSave, 1);
    let engine = Arc::new(
        Engine::new_with_simulation(data_dir.path(), clock.clone(), faults.clone())
            .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let document_id =
        DocumentId::from_key("terminal-store-retry").expect("document id should build");
    let executor = Arc::new(RecordingTriggerExecutor::with_terminal_failure_for(
        "firebase:terminalStoreRetryWritten",
    ));
    engine
        .install_trigger_invocation_executor(executor.clone())
        .expect("trigger executor should install");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![trigger_registration(
                "firebase:terminalStoreRetryWritten",
                FirestoreCloudEventType::Written,
                ["tasks", "{taskId}"],
            )],
        )
        .expect("trigger registrations should persist in runtime");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("terminal-store-retry"))]),
        )
        .expect("insert should succeed despite a transient trigger-execution store failure");

    // Same fault point as the Completed case above, but the handler itself
    // reports a terminal business failure: `record.fail_terminal` runs
    // before the fault fires, so the retried save must persist
    // TerminalFailure without re-invoking the (already-run) handler.
    wait_for_value(
        "trigger execution should retry past a transient pre-save store failure and persist the terminal outcome",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::TerminalFailure { attempt: 1, .. }
                )
        },
    )
    .await;

    let records = engine
        .list_trigger_invocations_for_testing(&tenant_id)
        .expect("trigger invocations should load");
    assert_eq!(
        records.len(),
        1,
        "exactly one terminal trigger invocation record should exist after the store retry"
    );
    assert!(matches!(
        records[0].state,
        TriggerInvocationState::TerminalFailure { attempt: 1, .. }
    ));
    assert_eq!(
        faults.failure_count(),
        1,
        "the store fault should have injected exactly one failure"
    );
    assert!(
        faults.visit_count() >= 2,
        "the pre-save store check must have been retried after the injected failure"
    );
    assert_eq!(
        executor.calls(),
        vec!["firebase:terminalStoreRetryWritten".to_string()],
        "a post-execution save retry must not re-invoke the handler (GR4 double-execution guard)"
    );
}
