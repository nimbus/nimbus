use super::publisher_test_seams::{
    ArmedOneShotDirectFaultInjector, BlockingAmbiguousApplyFaultInjector,
    NestedWriteDuringEvictionObserver,
};
use super::support::{
    assert_future_stays_pending, expect_blocking_wait_reaches_state, expect_catch_up_future_within,
};
use super::*;
use std::time::Instant;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn observer_nested_sync_write_rejected_during_direct_eviction_does_not_self_deadlock() {
    let data_dir = tempdir().expect("nested direct eviction tempdir should build");
    let faults =
        ArmedOneShotDirectFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_582))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_583)),
        )
        .expect("nested direct eviction engine should create"),
    );
    let tenant_id = TenantId::new("direct-eviction-observer-nested")
        .expect("nested direct eviction tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let (entered, entered_receiver) = std::sync::mpsc::sync_channel(1);
    let (release, release_receiver) = std::sync::mpsc::sync_channel(1);
    let (result, result_receiver) = std::sync::mpsc::sync_channel(1);
    let observer = Arc::new(NestedWriteDuringEvictionObserver {
        engine: Arc::downgrade(&engine),
        entered,
        release: Mutex::new(release_receiver),
        result,
    });
    engine.install_committed_mutation_observer("nested-direct-eviction-test", observer.clone());

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(0))]),
        )
        .await
        .expect("seed write should reach the observer dispatcher");
    tokio::task::spawn_blocking(move || entered_receiver.recv_timeout(Duration::from_secs(5)))
        .await
        .expect("seed observer wait should join")
        .expect("seed observer callback should block");

    engine.fail_direct_recovery_read_for_testing(tenant_id.clone());
    faults.arm();
    let writer = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(1))]),
            )
        }
    });
    expect_blocking_wait_reaches_state("direct writer should begin durable-recovery eviction", {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move |timeout| {
            let started = Instant::now();
            while started.elapsed() < timeout {
                if engine.ensure_tenant_exists(&tenant_id).is_err_and(|error| {
                    error.storage_kind() == Some(nimbus_core::StorageErrorKind::Unavailable)
                }) {
                    return true;
                }
                std::thread::yield_now();
            }
            false
        }
    })
    .await;

    release
        .send(())
        .expect("nested observer should remain blocked until released");
    let nested_error =
        tokio::task::spawn_blocking(move || result_receiver.recv_timeout(Duration::from_secs(5)))
            .await
            .expect("nested write result wait should join")
            .expect("nested observer write should return promptly")
            .expect_err("nested write must be rejected by eviction admission");
    assert_eq!(
        nested_error.storage_kind(),
        Some(nimbus_core::StorageErrorKind::Unavailable)
    );
    assert_eq!(
        nested_error.retryability(),
        nimbus_core::Retryability::RetryableAfterBackoff
    );

    let writer_error = expect_catch_up_future_within(writer, "direct eviction should finish")
        .await
        .expect("direct writer task should join")
        .expect_err("ambiguous direct writer should require crash-and-replay");
    assert!(
        matches!(writer_error, Error::Internal(ref message) if message.contains("crash-and-replay"))
    );
    let documents = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("dispatcher drain should let eviction complete and reload the tenant");
    assert_eq!(documents.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ambiguous_publisher_eviction_drains_then_reopens_a_distinct_runtime() {
    let data_dir = tempdir().expect("guarded eviction tempdir should build");
    let faults = Arc::new(nimbus_storage::ScriptedFaultInjector::new([
        nimbus_storage::FaultOccurrence {
            point: FaultPoint::JournalDurableAppendBeforeApply,
            visit: 1,
        },
    ]));
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_900))),
            faults,
        )
        .expect("guarded eviction engine should create"),
    );
    let tenant_id = TenantId::new("guarded-ambiguous-eviction").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    let eviction_blocker = engine
        .tenant_operation_guard_for_testing(&tenant_id)
        .expect("test operation should hold eviction before load-gate acquisition");

    timeout(
        Duration::from_secs(5),
        engine.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("recover"))]),
        ),
    )
    .await
    .expect("ambiguous writer should resolve within the eviction timeout")
    .expect_err("post-durable fault should require guarded eviction");
    let mut reload = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    let mut reload_writer = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("after-recovery"))]),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut reload,
        "reload should wait for the failed runtime to finish eviction",
    )
    .await;
    assert_future_stays_pending(
        &mut reload_writer,
        "writer reload should wait for the failed runtime to finish eviction",
    )
    .await;
    drop(eviction_blocker);
    let documents = expect_catch_up_future_within(
        reload,
        "tenant reload should finish after the old operation guard drops",
    )
    .await
    .expect("tenant reload task should join")
    .expect("reopen should wait for the old store handle to drain and recover");
    assert!(
        documents.iter().any(|document| {
            document
                .fields
                .get("title")
                .is_some_and(|title| title == "recover")
        }),
        "the reload must retain the durably committed document"
    );
    expect_catch_up_future_within(
        reload_writer,
        "writer should transparently reopen after eviction",
    )
    .await
    .expect("writer reload task should join")
    .expect("writer should succeed after the failed runtime finishes eviction");
    assert_eq!(
        engine
            .query_documents_async(tenant_id.clone(), query_for("tasks"))
            .await
            .expect("reopened tenant should remain queryable")
            .len(),
        2
    );
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("reopened runtime identity should load"),
        runtime_before
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn applied_sequence_waiter_returns_retryable_error_when_runtime_is_evicted() {
    let data_dir = tempdir().expect("applied-wait eviction tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_925))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_925)),
        )
        .expect("applied-wait eviction engine should create"),
    );
    let tenant_id = TenantId::new("applied-wait-eviction").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

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
    expect_blocking_wait_reaches_state("durable append should pause before ambiguous apply", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let mut waiter = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .count_table_documents_async(tenant_id, tasks_table())
                .await
        }
    });
    assert_future_stays_pending(
        &mut waiter,
        "table-count waiter should park behind the durable-but-unapplied sequence",
    )
    .await;

    faults.release_failure();
    expect_catch_up_future_within(writer, "ambiguous writer should enter eviction")
        .await
        .expect("writer task should join")
        .expect_err("ambiguous writer should fail for crash-and-replay");
    let wait_error = tokio::time::timeout(
        Duration::from_secs(5),
        expect_catch_up_future_within(waiter, "applied waiter should wake on eviction"),
    )
    .await
    .expect("eviction must wake the applied waiter within the bound")
    .expect("waiter task should join")
    .expect_err("the dead runtime cannot satisfy its applied target");
    assert_eq!(
        wait_error.storage_kind(),
        Some(nimbus_core::StorageErrorKind::Unavailable)
    );
    assert_eq!(
        wait_error.retryability(),
        nimbus_core::Retryability::RetryableAfterBackoff
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ambiguous_eviction_and_explicit_delete_complete_without_lock_inversion() {
    let data_dir = tempdir().expect("delete race tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_925))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_925)),
        )
        .expect("delete race engine should create"),
    );
    let tenant_id = TenantId::new("ambiguous-delete-race").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let writer = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("delete-race"))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("publisher should block after durable append", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let mut delete = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move { engine.delete_tenant_async(tenant_id).await }
    });
    assert_future_stays_pending(
        &mut delete,
        "explicit deletion should wait for the publisher-owned operation guard",
    )
    .await;
    faults.release_failure();

    let writer_error = expect_catch_up_future_within(
        writer,
        "ambiguous writer should resolve while deletion owns the load gate",
    )
    .await
    .expect("writer task should join")
    .expect_err("ambiguous writer should fail for replay");
    assert!(
        matches!(writer_error, Error::Internal(ref message) if message.contains("crash-and-replay"))
    );
    expect_catch_up_future_within(
        delete,
        "explicit deletion should complete after guard drain",
    )
    .await
    .expect("delete task should join")
    .expect("explicit deletion should succeed");
    assert!(matches!(
        engine
            .query_documents_async(tenant_id, query_for("tasks"))
            .await,
        Err(Error::TenantNotFound(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ambiguous_eviction_fails_and_drains_stranded_mutation_queues_before_reload() {
    let data_dir = tempdir().expect("stranded eviction tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_950))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_950)),
        )
        .expect("stranded eviction engine should create"),
    );
    let tenant_id = TenantId::new("ambiguous-stranded-queues").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let spawn_insert = |index: usize| {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
        })
    };
    let durable = spawn_insert(1);
    expect_blocking_wait_reaches_state("first publisher batch should block after append", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause should load");
    pause.arm();
    let journal_stranded = spawn_insert(2);
    expect_blocking_wait_reaches_state("second request should strand in the journal queue", {
        let pause = pause.clone();
        move |timeout| pause.wait_until_entered(timeout)
    })
    .await;
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "second request should remain in the paused journal queue",
        |stats| stats.queue_depth == 1,
    )
    .await;

    let admission_stranded = spawn_insert(3);
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "third request should remain in admission behind the paused actor",
        |stats| stats.queue_depth == 1,
    )
    .await;
    faults.release_failure();

    for failed in [durable, journal_stranded, admission_stranded] {
        let error = expect_catch_up_future_within(
            failed,
            "eviction should resolve every accepted mutation request",
        )
        .await
        .expect("evicted mutation task should join")
        .expect_err("evicted mutation should receive the typed replay error");
        assert!(
            matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")),
            "stranded callers should retain the eviction error: {error}"
        );
    }
    pause.release();

    let documents = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("the tenant should reload and recover its durable prefix");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("index"), Some(&json!(1)));
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("reloaded journal stats should load");
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_torn_tail_recovery_replays_exactly_one_contiguous_prefix() {
    let data_dir = tempdir().expect("torn-tail publisher tempdir should build");
    let faults = Arc::new(nimbus_storage::ScriptedFaultInjector::new([
        nimbus_storage::FaultOccurrence {
            point: FaultPoint::JournalDurableAppendBeforeApply,
            visit: 1,
        },
    ]));
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(45_000))),
            faults,
            Arc::new(nimbus_core::SeededIdSource::new(45_000)),
        )
        .expect("torn-tail publisher engine should create"),
    );
    let tenant_id = TenantId::new("publisher-torn-tail").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("torn-tail publisher tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let error = timeout(
        Duration::from_secs(5),
        engine.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("replay-me"))]),
        ),
    )
    .await
    .expect("torn-tail writer should resolve within the eviction timeout")
    .expect_err("post-append fault must force crash-and-replay");
    assert!(
        error.to_string().contains("crash-and-replay"),
        "ambiguous publisher failure should identify replay recovery: {error}"
    );

    // Failure completion deliberately precedes load-gate acquisition. The
    // access waits for eviction completion, then opens a fresh runtime and
    // applies the durable tail.
    let documents = timeout(
        Duration::from_secs(5),
        engine.query_documents_async(tenant_id.clone(), query_for("tasks")),
    )
    .await
    .expect("torn-tail reload should resolve within the eviction timeout")
    .expect("next access should recover the durable torn tail");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("replay-me")));
    let journal = engine
        .read_durable_journal_async(tenant_id.clone(), SequenceNumber(0))
        .await
        .expect("recovered durable prefix should read");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("recovered journal stats should load");
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(1));
    assert_eq!(stats.apply_lag, 0);
    assert_eq!(stats.publisher_ambiguous_error_count, 1);
}
