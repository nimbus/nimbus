use super::publisher_test_seams::{
    ArmedOneShotDirectFaultInjector, BlockingAmbiguousApplyFaultInjector,
    BlockingDefinitiveAppendFaultInjector, DurableAppendThenRecoveryFaultInjector,
    RetryExhaustionThenHealthyAppendFaultInjector, RetryableThenBlockingAppendFaultInjector,
};
use super::support::{
    assert_future_stays_pending, expect_blocking_wait_reaches_state, expect_catch_up_future_within,
    expect_future_within,
};
use super::*;
use nimbus_storage::NoopFaultInjector;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_preserves_sequence_order_across_transient_retry() {
    let data_dir = tempdir().expect("transient publisher tempdir should build");
    let faults = RetryableThenBlockingAppendFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_000))),
            faults.clone(),
        )
        .expect("transient publisher engine should create"),
    );
    let tenant_id = TenantId::new("publisher-retry-order").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("transient publisher tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("first publisher batch should commit");

    let second = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(2))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("retry of batch N should block before persistence", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_retry_blocked(timeout)
    })
    .await;

    let third = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(3))]),
                )
                .await
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "batch N+1 should wait in the publisher queue behind retrying batch N",
        |stats| stats.publisher_queue_depth == 1,
    )
    .await;
    let persisted_while_retrying = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("durable prefix should read while retry is blocked");
    assert_eq!(
        persisted_while_retrying
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1)],
        "batch N+1 must not persist around the retrying batch N"
    );
    assert_eq!(
        engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("retry diagnostics should load")
            .publisher_transient_error_count,
        1
    );

    faults.release_retry();
    expect_catch_up_future_within(second, "retrying publisher batch should complete")
        .await
        .expect("second insert task should join")
        .expect("second insert should succeed after retry");
    expect_catch_up_future_within(third, "following publisher batch should complete")
        .await
        .expect("third insert task should join")
        .expect("third insert should succeed");
    assert_eq!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("final durable prefix should read")
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2), SequenceNumber(3)]
    );
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("final retry diagnostics should load");
    assert_eq!(stats.publisher_transient_error_count, 1);
    assert_eq!(stats.publisher_fatal_error_count, 0);
    assert_eq!(stats.publisher_ambiguous_error_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn assignment_failure_mid_batch_discards_staged_suffix_and_keeps_tenant_live() {
    let data_dir = tempdir().expect("assignment failure tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_500))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_500)),
        )
        .expect("assignment failure engine should create"),
    );
    let tenant_id = TenantId::new("assignment-failure-recovery").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    let faults = engine.commit_fault_handle_for_testing();
    faults.inject_error_on_nth_hit(
        crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
        1,
        Error::InvalidInput("injected mid-batch assignment failure".to_string()),
    );

    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("assignment drain pause should load");
    pause.arm();
    let mut writes = Vec::new();
    for index in 0..3 {
        writes.push(tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                    )
                    .await
            }
        }));
        if index == 0 {
            expect_blocking_wait_reaches_state("assignment batch should pause before drain", {
                let pause = pause.clone();
                move |timeout| pause.wait_until_entered(timeout)
            })
            .await;
        }
    }
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "assignment batch followers should collect behind the pause",
        |stats| stats.queue_depth == 2,
    )
    .await;
    pause.release();
    let mut typed_failures = 0usize;
    let mut successful_followers = 0usize;
    for write in writes {
        match expect_catch_up_future_within(write, "assignment caller should resolve")
            .await
            .expect("assignment task should join")
        {
            Err(Error::InvalidInput(message))
                if message == "injected mid-batch assignment failure" =>
            {
                typed_failures += 1;
            }
            Ok(_) => successful_followers += 1,
            Err(other) => panic!("assignment caller received the wrong error: {other}"),
        }
    }
    let assignment_hits =
        faults.hit_count(crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE);
    assert!(
        typed_failures >= 2,
        "the staged request and injected-failure request must both receive the typed error; typed={typed_failures}, successful={successful_followers}, hits={assignment_hits}"
    );
    let journal_before_follow_up = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("journal should remain readable");
    assert_eq!(journal_before_follow_up.len(), successful_followers);
    assert_eq!(
        journal_before_follow_up
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        (1..=u64::try_from(successful_followers).unwrap())
            .map(SequenceNumber)
            .collect::<Vec<_>>(),
        "a mid-batch assignment error must not leave a phantom sequence"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(99))]),
        )
        .await
        .expect("the next batch should publish normally after assignment recovery");
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("runtime should remain loaded"),
        runtime_before,
        "recoverable assignment failure must not evict the tenant"
    );
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("recovered journal should read");
    assert_eq!(journal.len(), successful_followers + 1);
    assert_eq!(
        journal
            .last()
            .expect("follow-up record should exist")
            .sequence,
        SequenceNumber(u64::try_from(successful_followers + 1).unwrap())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_assignment_failure_discards_staged_suffix_and_keeps_tenant_live() {
    let data_dir = tempdir().expect("serial assignment failure tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_575))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_575)),
        )
        .expect("serial assignment failure engine should create"),
    );
    let tenant_id = TenantId::new("serial-assignment-failure").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial kill-switch arm");
    engine
        .commit_fault_handle_for_testing()
        .inject_error_on_nth_hit(
            crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
            1,
            Error::InvalidInput("injected serial assignment failure".to_string()),
        );

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("the serial assignment fault should fail its caller");
    assert!(
        matches!(error, Error::InvalidInput(ref message) if message == "injected serial assignment failure")
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("journal should remain readable")
            .is_empty(),
        "assignment failure must not create a durable record"
    );
    let (assigned_through, pending) = engine
        .write_log_assignment_for_testing(&tenant_id)
        .expect("serial write-log assignment state should load");
    assert_eq!(assigned_through, SequenceNumber(0));
    assert!(
        pending.is_empty(),
        "serial assignment recovery must remove the phantom staged suffix"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("the next serial batch should assign and commit without panicking");
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("runtime should remain loaded"),
        runtime_before,
        "recoverable serial assignment failure must not evict the tenant"
    );
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("serial journal diagnostics should load");
    assert_eq!(
        stats.publisher_mode,
        crate::tenant::CommitterPipelineMode::Serial
    );
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(1));
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("serial journal should read after recovery");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_lost_ack_evicts_and_replays_without_retryable_error() {
    let data_dir = tempdir().expect("serial recovery fallback tempdir should build");
    let faults = Arc::new(DurableAppendThenRecoveryFaultInjector::default());
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_580))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_580)),
        )
        .expect("serial recovery fallback engine should create"),
    );
    let tenant_id = TenantId::new("serial-recovery-fallback").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("serial runtime identity should load");
    engine.fail_serial_recovery_reads_for_testing(tenant_id.clone(), false, true);
    faults.arm();

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("the lost append acknowledgement must force crash-and-replay");
    assert_eq!(
        error.retryability(),
        nimbus_core::Retryability::Terminal,
        "a write that may have landed must never receive a safe-retry marker"
    );
    assert!(
        matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")),
        "the terminal error must identify the replay policy: {error}"
    );

    let replayed = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("the replacement runtime should replay the landed record");
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].fields.get("index"), Some(&json!(1)));
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("replacement runtime identity should load"),
        runtime_before,
        "an uncertain serial append must evict its runtime"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("the next serial assignment should heal without a write-log assertion");
    let after_heal = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("healed diagnostics should load");
    assert_eq!(after_heal.durable_head, SequenceNumber(2));
    assert_eq!(after_heal.applied_head, SequenceNumber(2));
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("healed durable journal should read");
    assert_eq!(
        journal
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2)]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quiesce_racing_serial_crash_replay_completes_without_panic() {
    let data_dir = tempdir().expect("serial quiesce race tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_id_source(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_581))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_581)),
        )
        .expect("serial quiesce race engine should create"),
    );
    let tenant_id = TenantId::new("serial-quiesce-race").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");

    let writer = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("serial append should block before ambiguous apply", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let residual = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "direct commit should queue before the quiesce race",
        |stats| stats.committer_inbox_depth == 1,
    )
    .await;

    let delete = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || engine.delete_tenant(&tenant_id)
    });
    tokio::pin!(delete);
    assert_future_stays_pending(
        &mut delete,
        "synchronous deletion should wait for the in-flight writer and residual submitter",
    )
    .await;

    let quiesce = tokio::spawn({
        let engine = engine.clone();
        async move { engine.quiesce().await }
    });
    expect_future_within(
        async {
            while !engine.background_shutdown_started() {
                tokio::task::yield_now().await;
            }
        },
        "quiesce should close the background spawn gate",
    )
    .await;
    faults.release_failure();

    let error = expect_catch_up_future_within(
        writer,
        "serial crash-and-replay should resolve during quiesce",
    )
    .await
    .expect("writer task should join without a background-task panic")
    .expect_err("ambiguous serial write should fail for replay");
    assert!(matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")));
    let residual_error = expect_catch_up_future_within(
        residual,
        "residual direct commit should receive a typed error during quiesce",
    )
    .await
    .expect("residual direct task should join without a panic")
    .expect_err("the old runtime must reject residual direct work");
    assert_eq!(
        residual_error.storage_kind(),
        Some(nimbus_core::StorageErrorKind::Unavailable)
    );
    expect_catch_up_future_within(
        &mut delete,
        "synchronous deletion should complete after the residual submitter is failed",
    )
    .await
    .expect("synchronous delete task should join without panic")
    .expect("synchronous delete should remove the tenant");
    expect_catch_up_future_within(quiesce, "quiesce should await inline eviction completion")
        .await
        .expect("quiesce task should join without a spawn rejection panic");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_crash_replay_rejects_residual_direct_commit_without_running_it() {
    let data_dir = tempdir().expect("serial residual direct tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_581))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_582)),
        )
        .expect("serial residual direct engine should create"),
    );
    let tenant_id = TenantId::new("serial-residual-direct").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");

    let crashing = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("serial batch should block after its durable append", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let residual = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "direct commit should queue behind the crashing serial batch",
        |stats| stats.committer_inbox_depth == 1,
    )
    .await;
    faults.release_failure();

    let crash_error = expect_catch_up_future_within(crashing, "crashing batch should resolve")
        .await
        .expect("crashing batch task should join")
        .expect_err("ambiguous serial write should fail for replay");
    assert!(
        matches!(crash_error, Error::Internal(ref message) if message.contains("crash-and-replay"))
    );
    let residual_error = expect_catch_up_future_within(
        residual,
        "residual direct commit should fail before the old runtime executes it",
    )
    .await
    .expect("direct commit task should join without panicking")
    .expect_err("residual direct commit should receive the typed eviction error");
    assert_eq!(
        residual_error.storage_kind(),
        Some(nimbus_core::StorageErrorKind::Unavailable)
    );
    assert!(
        residual_error
            .to_string()
            .contains("restarting after durable recovery")
    );

    let documents = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("replacement runtime should replay only the durable batch");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("index"), Some(&json!(1)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_lost_ack_with_unreadable_progress_evicts_and_replays_once() {
    let data_dir = tempdir().expect("direct lost-ack tempdir should build");
    let faults =
        ArmedOneShotDirectFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_582))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_582)),
        )
        .expect("direct lost-ack engine should create"),
    );
    let tenant_id = TenantId::new("direct-lost-ack").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("direct runtime identity should load");
    engine.fail_direct_recovery_read_for_testing(tenant_id.clone());
    faults.arm();

    let error = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(1))]),
            )
        }
    })
    .await
    .expect("direct lost-ack task should join")
    .expect_err("an unknowable landed direct write must require replay");
    assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);
    assert!(matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")));

    let replayed = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("replacement runtime should replay the landed direct write");
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].fields.get("index"), Some(&json!(1)));
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("replacement runtime identity should load"),
        runtime_before
    );
    let replayed_journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("replayed direct journal should read");
    assert_eq!(replayed_journal.len(), 1);
    assert_eq!(replayed_journal[0].sequence, SequenceNumber(1));

    tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
        }
    })
    .await
    .expect("replacement direct task should join")
    .expect("the replacement runtime should accept the next direct commit");
    let healed_journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("healed direct journal should read");
    assert_eq!(
        healed_journal
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2)]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_unchanged_head_failure_remains_retryable_and_batch_scoped() {
    let data_dir = tempdir().expect("direct definitive tempdir should build");
    let faults = ArmedOneShotDirectFaultInjector::new(FaultPoint::StorageCommitBeforeVisibility);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_583))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_583)),
        )
        .expect("direct definitive engine should create"),
    );
    let tenant_id = TenantId::new("direct-definitive").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("direct runtime identity should load");
    faults.arm();

    let error = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(1))]),
            )
        }
    })
    .await
    .expect("direct definitive task should join")
    .expect_err("the pre-visibility direct fault should fail only its batch");
    assert_eq!(
        error.retryability(),
        nimbus_core::Retryability::RetryableAfterBackoff
    );
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("definitive failure must keep the runtime loaded"),
        runtime_before
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("definitive direct journal should read")
            .is_empty()
    );
    let (_, pending) = engine
        .write_log_assignment_for_testing(&tenant_id)
        .expect("definitive direct assignment state should load");
    assert!(pending.is_empty());

    tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
        }
    })
    .await
    .expect("follow-up direct task should join")
    .expect("the next direct commit should reuse the discarded suffix safely");
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("follow-up direct journal should read");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_err_err_recovery_evicts_when_append_did_not_land() {
    let data_dir = tempdir().expect("serial unlanded ambiguity tempdir should build");
    let faults = Arc::new(nimbus_storage::ScriptedFaultInjector::new([
        nimbus_storage::FaultOccurrence {
            point: FaultPoint::JournalAppendBeforeDurableFlush,
            visit: 1,
        },
    ]));
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_585))),
            faults,
            Arc::new(nimbus_core::SeededIdSource::new(46_585)),
        )
        .expect("serial unlanded ambiguity engine should create"),
    );
    let tenant_id = TenantId::new("serial-unlanded-ambiguity").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("serial runtime identity should load");
    engine.fail_serial_recovery_reads_for_testing(tenant_id.clone(), true, true);

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("an unknowable serial append must force crash-and-replay");
    assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);
    assert!(matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")));

    let replayed = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("the replacement runtime should load the durable truth");
    assert!(
        replayed.is_empty(),
        "the replacement runtime must not invent the unlanded record"
    );
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("replacement runtime identity should load"),
        runtime_before,
        "Err/Err recovery must evict even when replay later proves no record landed"
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("reloaded journal should remain readable")
            .is_empty()
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("the replacement runtime should reuse sequence one safely");
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("replacement journal should read");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn assignment_worker_panic_discards_staged_suffix_and_keeps_tenant_live() {
    let data_dir = tempdir().expect("assignment panic tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_625))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_625)),
        )
        .expect("assignment panic engine should create"),
    );
    let tenant_id = TenantId::new("assignment-panic-recovery").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    engine
        .commit_fault_handle_for_testing()
        .inject_panic_on_nth_hit(
            crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
            1,
        );

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("the injected assignment panic should fail its caller");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("assignment batch panicked")),
        "the assignment join error should reach the caller"
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("journal should remain readable")
            .is_empty(),
        "the staged record from a panicked assignment worker must be discarded"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("a follow-up batch should publish after panic recovery");
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("runtime should remain loaded"),
        runtime_before,
        "assignment panics are recoverable and must not evict the tenant"
    );
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("journal should read after recovery");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_assignment_worker_panic_discards_staged_suffix_and_keeps_tenant_live() {
    let data_dir = tempdir().expect("serial assignment panic tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_650))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_650)),
        )
        .expect("serial assignment panic engine should create"),
    );
    let tenant_id = TenantId::new("serial-assignment-panic").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial kill-switch arm");
    engine
        .commit_fault_handle_for_testing()
        .inject_panic_on_nth_hit(
            crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
            1,
        );

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("the injected serial assignment panic should fail its caller");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("serial committer queued batch panicked")),
        "the serial join error should reach the caller"
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("serial journal should remain readable")
            .is_empty(),
        "the staged record from a panicked serial worker must not become durable"
    );
    let (assigned_through, pending) = engine
        .write_log_assignment_for_testing(&tenant_id)
        .expect("serial write-log assignment state should load");
    assert_eq!(assigned_through, SequenceNumber(0));
    assert!(
        pending.is_empty(),
        "serial panic must discard staged suffix"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("a follow-up serial batch should commit after panic recovery");
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("runtime should remain loaded"),
        runtime_before
    );
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("serial journal should read after recovery");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn definitive_publisher_append_error_is_batch_scoped_and_tenant_stays_loaded() {
    let data_dir = tempdir().expect("definitive publisher tempdir should build");
    let faults = Arc::new(nimbus_storage::ScriptedFaultInjector::new([
        nimbus_storage::FaultOccurrence {
            point: FaultPoint::JournalAppendBeforeDurableFlush,
            visit: 1,
        },
    ]));
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_750))),
            faults,
            Arc::new(nimbus_core::SeededIdSource::new(46_750)),
        )
        .expect("definitive publisher engine should create"),
    );
    let tenant_id = TenantId::new("definitive-publisher-failure").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("pre-durability append fault should fail only its batch");
    assert!(
        error
            .to_string()
            .contains("journal_append_before_durable_flush")
    );
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("tenant should still be loaded"),
        runtime_before
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("next batch should publish on the same runtime");
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("publisher diagnostics should load");
    assert_eq!(stats.publisher_fatal_error_count, 1);
    assert_eq!(stats.publisher_ambiguous_error_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn definitive_recovery_drains_batches_behind_response_fences() {
    let data_dir = tempdir().expect("fenced recovery tempdir should build");
    let faults = BlockingDefinitiveAppendFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_775))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_775)),
        )
        .expect("fenced recovery engine should create"),
    );
    let tenant_id = TenantId::new("definitive-fenced-recovery").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let first = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("the first append should block before failing", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let fence = engine
        .enqueue_publisher_response_fence_for_testing(&tenant_id)
        .await
        .expect("response fence should enqueue behind the failed batch");
    let second = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(2))]),
                )
                .await
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "the response fence and following assigned batch should both queue",
        |stats| stats.publisher_queue_depth == 2,
    )
    .await;

    faults.release_failure();
    for failed in [first, second] {
        let error = expect_catch_up_future_within(
            failed,
            "every batch in the failed assigned suffix should resolve",
        )
        .await
        .expect("failed insert task should join")
        .expect_err("the definitive suffix should fail");
        assert!(
            matches!(error, Error::InvalidInput(ref message) if message == "injected definitive publisher failure"),
            "fence-stranded batches must retain the original typed error: {error}"
        );
    }
    assert!(matches!(
        fence
            .await
            .expect("response fence should complete")
            .expect("independent deferred response should retain its result"),
        crate::tenant::QueuedMutationResult::Scheduled(false)
    ));

    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("recovered publisher stats should load");
    let (assigned_through, pending) = engine
        .write_log_assignment_for_testing(&tenant_id)
        .expect("recovered write-log state should load");
    assert_eq!(stats.durable_head, SequenceNumber(0));
    assert_eq!(assigned_through, stats.durable_head);
    assert!(
        pending.is_empty(),
        "recovery must remove every phantom staged record"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(3))]),
        )
        .await
        .expect("a follow-up insert should succeed on the recovered runtime");
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("recovered journal should read");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn definitive_recovery_retries_fence_conflict_whose_sequence_was_discarded() {
    let data_dir = tempdir().expect("discarded conflict tempdir should build");
    let faults = BlockingDefinitiveAppendFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_787))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_787)),
        )
        .expect("discarded conflict engine should create"),
    );
    let tenant_id = TenantId::new("discarded-conflict-retry").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let failing = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("value".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("assigned sequence 1 should block before failing", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let fence = engine
        .enqueue_publisher_conflict_response_fence_for_testing(&tenant_id, SequenceNumber(1))
        .await
        .expect("conflict response fence should enqueue");
    let retrying = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            let response = fence.await.map_err(|_| {
                Error::Internal("publisher dropped the conflict response fence".to_string())
            })?;
            match response {
                Err(error)
                    if error.retryability() == nimbus_core::Retryability::Retryable
                        && error.conflicting_sequence().is_none() =>
                {
                    engine
                        .insert_document_async(
                            tenant_id,
                            tasks_table(),
                            serde_json::Map::from_iter([("value".to_string(), json!(2))]),
                        )
                        .await
                }
                Err(error) => Err(error),
                Ok(_) => Err(Error::Internal(
                    "discarded conflict fence unexpectedly succeeded".to_string(),
                )),
            }
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "conflict response fence should queue behind the failing assignment",
        |stats| stats.publisher_queue_depth == 1,
    )
    .await;

    faults.release_failure();
    let failing_error = expect_catch_up_future_within(
        failing,
        "the definitive batch should fail without durable advance",
    )
    .await
    .expect("failing update task should join")
    .expect_err("the injected definitive append should fail");
    assert!(
        matches!(failing_error, Error::InvalidInput(ref message) if message == "injected definitive publisher failure")
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        expect_catch_up_future_within(retrying, "discarded conflict waiter should retry"),
    )
    .await
    .expect("discarded conflict target must not hang the caller")
    .expect("retrying update task should join")
    .expect("retrying update should re-prepare and commit");

    let documents = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("updated document should query");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("value"), Some(&json!(2)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_discard_rewrites_same_batch_conflict_before_retry() {
    let data_dir = tempdir().expect("serial deferred conflict tempdir should build");
    let faults = BlockingDefinitiveAppendFaultInjector::new_on_visit(2);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_790))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_790)),
        )
        .expect("serial deferred conflict engine should create"),
    );
    let tenant_id = TenantId::new("serial-deferred-conflict").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");
    let document_id = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("value".to_string(), json!("seed"))]),
        )
        .await
        .expect("seed write should consume the healthy first append");

    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();
    let first = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("first".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("first serial update should reach the paused drainer", {
        let pause = pause.clone();
        move |timeout| pause.wait_until_entered(timeout)
    })
    .await;

    // Force the second prepared write through the ordinary CallerWait branch
    // so its dependency resolves against M1's staged B+1 image. Production
    // callers without an inline plan use this exact branch.
    engine.strip_next_inline_reprepare_for_testing(&tenant_id);
    let mut second = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("second".to_string(), json!(2))]),
                )
                .await
        }
    });
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "second same-document prepare should join M1's serial batch",
        |stats| stats.queue_depth == 1,
    )
    .await;
    pause.release();
    expect_blocking_wait_reaches_state(
        "the same-document batch should block before append failure",
        {
            let faults = faults.clone();
            move |timeout| faults.wait_until_blocked(timeout)
        },
    )
    .await;
    assert_future_stays_pending(
        &mut second,
        "deferred CallerWait must not complete before the append outcome is known",
    )
    .await;

    faults.release_failure();
    let first_error = expect_catch_up_future_within(first, "M1 should receive the append failure")
        .await
        .expect("first update task should join")
        .expect_err("M1 should fail with the definitive append error");
    assert!(
        matches!(first_error, Error::InvalidInput(ref message) if message == "injected definitive publisher failure")
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        expect_catch_up_future_within(second, "rewritten M2 conflict should retry"),
    )
    .await
    .expect("discarded B+1 must not strand M2")
    .expect("second update task should join")
    .expect("M2 should retry from a fresh snapshot and commit");

    let document = engine
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("retried document should remain readable");
    assert_eq!(document.fields.get("first"), None);
    assert_eq!(document.fields.get("second"), Some(&json!(2)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_discard_rewrites_same_batch_conflict_before_retry() {
    let data_dir = tempdir().expect("publisher deferred conflict tempdir should build");
    let faults = BlockingDefinitiveAppendFaultInjector::new_on_visit(2);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_795))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_795)),
        )
        .expect("publisher deferred conflict engine should create"),
    );
    let tenant_id = TenantId::new("publisher-deferred-conflict").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, true)
        .expect("test should request the publisher arm");
    let document_id = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("value".to_string(), json!("seed"))]),
        )
        .await
        .expect("seed write should consume the healthy first append");

    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();
    let first = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("first".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("first publisher update should reach the paused drainer", {
        let pause = pause.clone();
        move |timeout| pause.wait_until_entered(timeout)
    })
    .await;

    // Force the second prepared write through the ordinary CallerWait branch
    // so its dependency resolves against M1's staged B+1 image. Production
    // callers without an inline plan use this exact branch.
    engine.strip_next_inline_reprepare_for_testing(&tenant_id);
    let mut second = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("second".to_string(), json!(2))]),
                )
                .await
        }
    });
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "second same-document prepare should join M1's publisher batch",
        |stats| stats.queue_depth == 1,
    )
    .await;
    pause.release();
    expect_blocking_wait_reaches_state(
        "the same-document publisher batch should block before append failure",
        {
            let faults = faults.clone();
            move |timeout| faults.wait_until_blocked(timeout)
        },
    )
    .await;
    assert_future_stays_pending(
        &mut second,
        "attached CallerWait must not complete before the append outcome is known",
    )
    .await;

    faults.release_failure();
    let first_error = expect_catch_up_future_within(first, "M1 should receive the append failure")
        .await
        .expect("first update task should join")
        .expect_err("M1 should fail with the definitive append error");
    assert!(
        matches!(first_error, Error::InvalidInput(ref message) if message == "injected definitive publisher failure")
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        expect_catch_up_future_within(second, "rewritten M2 conflict should retry"),
    )
    .await
    .expect("discarded B+1 must not strand M2")
    .expect("second update task should join")
    .expect("M2 should retry from a fresh snapshot and commit");

    let document = engine
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("retried document should remain readable");
    assert_eq!(document.fields.get("first"), None);
    assert_eq!(document.fields.get("second"), Some(&json!(2)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_retry_exhaustion_without_durable_advance_is_batch_scoped() {
    let data_dir = tempdir().expect("retry exhaustion tempdir should build");
    let faults = RetryExhaustionThenHealthyAppendFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_800))),
            faults,
            Arc::new(nimbus_core::SeededIdSource::new(46_800)),
        )
        .expect("retry exhaustion engine should create"),
    );
    let tenant_id = TenantId::new("retry-exhaustion-batch-scope").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("retry exhaustion should fail the batch");
    assert_eq!(
        error.retryability(),
        nimbus_core::Retryability::RetryableAfterBackoff
    );
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("tenant should remain loaded"),
        runtime_before
    );
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("publisher should continue after bounded retry exhaustion");
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("publisher diagnostics should load");
    assert_eq!(stats.publisher_transient_error_count, 4);
    assert_eq!(stats.publisher_ambiguous_error_count, 0);
}
