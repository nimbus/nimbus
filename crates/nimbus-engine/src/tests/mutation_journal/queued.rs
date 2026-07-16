use super::support::{
    assert_future_stays_pending, expect_blocking_wait_reaches_state, expect_catch_up_future_within,
    expect_future_within, new_faulted_engine,
};
use super::*;

#[tokio::test]
async fn async_schema_write_advances_runtime_journal_before_next_queued_document_write() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .set_table_schema_async(
            tenant_id.clone(),
            TableSchema {
                table: tasks_table(),
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    field_type: FieldType::String,
                    required: true,
                }],
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .await
        .expect("async schema write should succeed");

    let after_schema = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after schema write");
    assert!(after_schema.durable_head.0 >= 1);
    assert_eq!(after_schema.applied_head, after_schema.durable_head);
    assert_eq!(after_schema.apply_lag, 0);
    let schema_head = after_schema.durable_head;

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("after-schema"))]),
        )
        .await
        .expect("queued document write should follow the schema commit sequence");

    let after_insert = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "queued insert should advance after schema commit",
        |stats| stats.durable_head.0 > schema_head.0 && stats.applied_head == stats.durable_head,
    )
    .await;
    assert_eq!(
        durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1
    );
    assert_eq!(after_insert.queue_depth, 0);
    assert_eq!(after_insert.worker_failure_count, 0);
}

/// Liveness smoke for the real `insert_document_async` path under concurrency:
/// several tasks issue rapid mutations in true parallelism and every one must
/// drain within a bound. This exercises the mutation journal end-to-end and
/// catches gross liveness regressions.
///
/// It is deliberately NOT presented as a reliable reproducer of the specific
/// lost-wakeup race it was born from — that window is nanosecond-scale (the
/// benchmark that found it needed concurrency up to 256 to hit it ~1 run in 4),
/// so at this scale it will not, on its own, catch a call-site regression that
/// hoists `has_pending()` out of the closure. The precise, deterministic guard
/// for the race is
/// `tenant::mutation::journal::tests::release_worker_clears_running_before_evaluating_the_gate`;
/// a full revert of the closure signature is caught by compilation. Counts are
/// modest because `cargo test` builds unoptimized (durable commits are ~10-50x
/// slower than the release build the benchmark uses).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_mutations_do_not_strand_the_journal_worker() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    const TASKS: usize = 4;
    const MUTATIONS_PER_TASK: usize = 200;

    let workload = {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            let mut handles = Vec::with_capacity(TASKS);
            for task in 0..TASKS {
                let engine = engine.clone();
                let tenant_id = tenant_id.clone();
                handles.push(tokio::spawn(async move {
                    for index in 0..MUTATIONS_PER_TASK {
                        engine
                            .insert_document_async(
                                tenant_id.clone(),
                                tasks_table(),
                                serde_json::Map::from_iter([(
                                    "title".to_string(),
                                    json!(format!("t{task}-{index}")),
                                )]),
                            )
                            .await
                            .expect("concurrent insert should not fail");
                    }
                }));
            }
            for handle in handles {
                handle.await.expect("mutation task should not panic");
            }
        }
    };

    tokio::time::timeout(std::time::Duration::from_secs(45), workload)
        .await
        .expect(
            "every concurrent mutation must drain — a hang here means the journal-worker \
             lost-wakeup deadlock has regressed",
        );

    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    assert_eq!(stats.queue_depth, 0, "all mutations drained");
    assert_eq!(stats.worker_failure_count, 0, "no worker failures");
}

async fn run_paused_insert_burst(engine: &Arc<Engine>, tenant_id: &TenantId, count: usize) {
    assert!(count > 0, "a burst must contain at least one mutation");
    engine
        .set_mutation_admission_codel_for_testing(
            tenant_id,
            Duration::from_secs(60),
            Duration::from_secs(60),
        )
        .expect("the burst should not be shed by CoDel");
    let pause = engine
        .mutation_journal_pause_handle_for_testing(tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let mut inserts = Vec::with_capacity(count);
    inserts.push(tokio::spawn({
        let engine = Arc::clone(engine);
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("burst-0"))]),
                )
                .await
        }
    }));
    expect_blocking_wait_reaches_state(
        "journal worker should pause with the first burst mutation admitted",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;

    for index in 1..count {
        inserts.push(tokio::spawn({
            let engine = Arc::clone(engine);
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        serde_json::Map::from_iter([(
                            "title".to_string(),
                            json!(format!("burst-{index}")),
                        )]),
                    )
                    .await
            }
        }));
    }
    if count > 1 {
        wait_for_mutation_admission_stats(
            engine,
            tenant_id,
            "the rest of the burst should be queued behind the paused drainer",
            |stats| stats.queue_depth == count - 1,
        )
        .await;
    }
    pause.release();

    for insert in inserts {
        expect_catch_up_future_within(insert, "every mutation in the paused burst should commit")
            .await
            .expect("burst task should join")
            .expect("burst mutation should succeed");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adaptive_batch_grows_under_backlog_and_shrinks_when_idle() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("adaptive-batch", Engine::create_tenant);
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics before burst should load")
        .commit_phases;

    run_paused_insert_burst(&engine, &tenant_id, 96).await;
    let after_burst = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics after burst should load")
        .commit_phases;
    let burst_size_sum = after_burst
        .journal_batch_size_sum
        .saturating_sub(before.journal_batch_size_sum);
    let burst_count = after_burst
        .journal_batch_count
        .saturating_sub(before.journal_batch_count);
    assert_eq!(
        burst_count, 1,
        "the paused backlog should drain as one batch"
    );
    assert_eq!(burst_size_sum, 96);
    assert!(
        burst_size_sum / burst_count > 32,
        "backlog should raise the effective batch above the base cap"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("idle"))]),
        )
        .await
        .expect("idle mutation should succeed");
    let after_idle = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics after idle mutation should load")
        .commit_phases;
    assert_eq!(
        after_idle
            .journal_batch_count
            .saturating_sub(after_burst.journal_batch_count),
        1
    );
    assert_eq!(
        after_idle
            .journal_batch_size_sum
            .saturating_sub(after_burst.journal_batch_size_sum),
        1,
        "an idle arrival should retain base behavior rather than waiting for a max batch"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adaptive_batch_never_splits_the_durable_round_trip() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults =
        CountedFaultInjector::fail_nth_call(FaultPoint::JournalDurableAppendBeforeApply, u64::MAX);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(43_500))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(43_500)),
        )
        .expect("memory engine should create"),
    );
    let tenant_id = TenantId::new("adaptive-round-trip").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    let visits_before = faults.visit_count();
    let metrics_before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics before burst should load")
        .commit_phases;

    run_paused_insert_burst(&engine, &tenant_id, 65).await;

    let metrics_after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics after burst should load")
        .commit_phases;
    let drained_batches = metrics_after
        .journal_batch_count
        .saturating_sub(metrics_before.journal_batch_count);
    let append_boundaries = faults.visit_count().saturating_sub(visits_before);
    assert_eq!(drained_batches, 1);
    assert_eq!(
        append_boundaries, drained_batches,
        "each adaptive drain must cross the post-append fault boundary exactly once; the record slice is passed to one append_durable_records_batch call"
    );
}

#[tokio::test]
async fn mutation_admission_gate_buffers_while_journal_is_paused_without_losing_in_flight_response()
{
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .set_mutation_journal_queue_capacity_for_testing(&tenant_id, 1)
        .expect("queue capacity should be configurable for tests");
    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let first_insert = {
        let engine = Arc::clone(&engine);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("queued-first"))]),
                )
                .await
        })
    };

    expect_blocking_wait_reaches_state(
        "journal worker should pause before draining the queued request",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;

    let blocked_stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load while the queue is paused");
    assert_eq!(blocked_stats.queue_depth, 1);
    assert_eq!(blocked_stats.queue_capacity, 1);
    assert!(blocked_stats.oldest_queue_age_nanos > 0);
    assert_eq!(blocked_stats.pending_response_count, 1);
    assert!(blocked_stats.worker_running);
    assert_eq!(blocked_stats.worker_start_count, 1);
    assert_eq!(blocked_stats.worker_restart_count, 0);
    assert_eq!(blocked_stats.queue_rejection_count, 0);
    assert_eq!(blocked_stats.worker_failure_count, 0);

    let mut second_insert = tokio::spawn({
        let engine = Arc::clone(&engine);
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("queued-second"))]),
                )
                .await
        }
    });

    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "second mutation should remain buffered at the admission gate",
        |stats| stats.queue_depth == 1,
    )
    .await;

    assert_future_stays_pending(
        &mut second_insert,
        "second mutation should stay pending while the journal worker is paused",
    )
    .await;

    let buffered_stats = engine
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the second mutation is buffered");
    assert_eq!(buffered_stats.queue_depth, 1);
    assert_eq!(
        buffered_stats.queue_capacity,
        crate::tenant::DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY
    );
    assert!(buffered_stats.oldest_queue_age_nanos > 0);
    assert_eq!(buffered_stats.shed_count, 0);
    assert_eq!(buffered_stats.queue_rejection_count, 0);

    pause.release();

    let first_id = expect_future_within(
        first_insert,
        "first mutation should resolve after the pause is released",
    )
    .await
    .expect("first mutation task should join successfully")
    .expect("first mutation should succeed");
    let second_id = expect_future_within(
        second_insert,
        "second mutation should resolve after the journal drains",
    )
    .await
    .expect("second mutation task should join successfully")
    .expect("second mutation should succeed");

    let visible = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("final query should succeed after the buffered mutation drains");
    assert_eq!(visible.len(), 2);
    assert_eq!(
        visible
            .into_iter()
            .map(|document| document.id.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first_id, second_id])
    );

    let final_stats = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "mutation journal worker to go idle after the buffered queue drains",
        |stats| !stats.worker_running,
    )
    .await;
    assert!(final_stats.durable_head.0 >= 2);
    assert_eq!(final_stats.applied_head, final_stats.durable_head);
    assert_eq!(final_stats.apply_lag, 0);
    assert_eq!(final_stats.queue_depth, 0);
    assert_eq!(final_stats.queue_capacity, 1);
    assert_eq!(final_stats.oldest_queue_age_nanos, 0);
    assert_eq!(final_stats.pending_response_count, 0);
    assert!(!final_stats.worker_running);
    assert_eq!(final_stats.worker_start_count, 1);
    assert_eq!(final_stats.worker_restart_count, 0);
    assert_eq!(final_stats.queue_rejection_count, 0);
    assert_eq!(final_stats.worker_failure_count, 0);
    assert_eq!(
        durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        2
    );

    let final_admission_stats = engine
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the gate drains");
    assert_eq!(final_admission_stats.queue_depth, 0);
    assert_eq!(final_admission_stats.shed_count, 0);
    assert_eq!(final_admission_stats.queue_rejection_count, 0);
}

#[tokio::test]
async fn mutation_journal_never_expires_admitted_work() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .set_mutation_admission_codel_for_testing(
            &tenant_id,
            Duration::from_millis(5),
            Duration::from_millis(10),
        )
        .expect("admission CoDel should be configurable for tests");
    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let mut admitted_insert = {
        let engine = Arc::clone(&engine);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("admitted"))]),
                )
                .await
        })
    };

    expect_blocking_wait_reaches_state(
        "journal worker should pause after admitting the mutation to the journal queue",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;

    assert_future_stays_pending(
        &mut admitted_insert,
        "admitted mutation should remain pending while the journal worker pause is armed",
    )
    .await;
    pause.release();

    let document_id = expect_future_within(
        admitted_insert,
        "admitted mutation should resolve after the pause is released",
    )
    .await
    .expect("admitted mutation task should join successfully")
    .expect("admitted mutation should still succeed");

    let visible = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("final query should succeed after the admitted mutation drains");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, document_id);

    let admission_stats = engine
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the queue drains");
    assert_eq!(admission_stats.queue_depth, 0);
    assert_eq!(admission_stats.shed_count, 0);
    assert_eq!(admission_stats.queue_rejection_count, 0);

    let journal_stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after the admitted mutation commits");
    assert!(journal_stats.durable_head.0 >= 1);
    assert_eq!(journal_stats.applied_head, journal_stats.durable_head);
    assert_eq!(journal_stats.apply_lag, 0);
    assert_eq!(journal_stats.queue_depth, 0);
    assert_eq!(
        durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1
    );
}

#[tokio::test]
async fn queued_mutation_response_still_resolves_after_blocked_read_catches_up() {
    let (_data_dir, engine, tenant_id, faults) = new_faulted_engine(42_500);

    let mut first_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
                )
                .await
        }
    });

    expect_future_within(
        faults.wait_until_entered(),
        "journal worker should block after durable append",
    )
    .await;
    assert_future_stays_pending(
        &mut first_insert,
        "first mutation should remain pending while apply is blocked",
    )
    .await;

    let mut blocked_query = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    assert_future_stays_pending(
        &mut blocked_query,
        "query should remain pending while the first durable write is not yet applied",
    )
    .await;

    let mut second_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut second_insert,
        "queued follow-up mutation should remain pending until the blocked apply resumes",
    )
    .await;

    faults.release();

    let first_id = expect_future_within(
        first_insert,
        "first mutation should resolve after apply resumes",
    )
    .await
    .expect("first mutation task should join successfully")
    .expect("first mutation should succeed");
    let query_results = expect_future_within(
        blocked_query,
        "blocked query should resolve after apply resumes",
    )
    .await
    .expect("blocked query task should join successfully")
    .expect("blocked query should succeed");
    assert!(
        query_results
            .iter()
            .any(|document| document.fields.get("title") == Some(&json!("first"))),
        "blocked query should observe the first applied write"
    );

    let second_id = match timeout(Duration::from_secs(3), second_insert).await {
        Ok(result) => result
            .expect("second mutation task should join successfully")
            .expect("second mutation should succeed"),
        Err(error) => {
            let visible = engine
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("live query should still succeed");
            let visible_titles = visible
                .iter()
                .map(|document| {
                    document.fields["title"]
                        .as_str()
                        .expect("title should be present and a string")
                })
                .collect::<Vec<_>>();
            panic!(
                "queued follow-up mutation should resolve after the blocked read catches up: {error:?}; visible documents: {:?}; first_id={first_id}",
                visible_titles
            );
        }
    };

    let visible = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_cancellable_mutation_response_still_resolves_after_blocked_read_catches_up() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_750))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async_with(
                    tenant_id,
                    tasks_table(),
                    None,
                    serde_json::Map::from_iter([("title".to_string(), json!("first-cancellable"))]),
                    crate::AsyncMutationContext::anonymous(std::future::pending::<()>(), || Ok(())),
                )
                .await
        }
    });

    expect_future_within(
        faults.wait_until_entered(),
        "journal worker should block after durable append",
    )
    .await;
    assert_future_stays_pending(
        &mut first_insert,
        "first cancellable mutation should remain pending while apply is blocked",
    )
    .await;

    let mut blocked_query = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    assert_future_stays_pending(
        &mut blocked_query,
        "query should remain pending while the first durable write is not yet applied",
    )
    .await;

    let mut second_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async_with(
                    tenant_id,
                    tasks_table(),
                    None,
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("second-cancellable"),
                    )]),
                    crate::AsyncMutationContext::anonymous(std::future::pending::<()>(), || Ok(())),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut second_insert,
        "queued follow-up cancellable mutation should remain pending until the blocked apply resumes",
    )
    .await;

    faults.release();

    let first_id = expect_future_within(
        first_insert,
        "first cancellable mutation should resolve after apply resumes",
    )
    .await
    .expect("first cancellable mutation task should join successfully")
    .expect("first cancellable mutation should succeed");
    let query_results = expect_future_within(
        blocked_query,
        "blocked query should resolve after apply resumes",
    )
    .await
    .expect("blocked query task should join successfully")
    .expect("blocked query should succeed");
    assert!(
        query_results
            .iter()
            .any(|document| document.fields.get("title") == Some(&json!("first-cancellable"))),
        "blocked query should observe the first applied cancellable write"
    );

    let second_id = match timeout(Duration::from_secs(3), second_insert).await {
        Ok(result) => result
            .expect("second cancellable mutation task should join successfully")
            .expect("second cancellable mutation should succeed"),
        Err(error) => {
            let visible = engine
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("live query should still succeed");
            let visible_titles = visible
                .iter()
                .map(|document| {
                    document.fields["title"]
                        .as_str()
                        .expect("title should be present and a string")
                })
                .collect::<Vec<_>>();
            panic!(
                "queued follow-up cancellable mutation should resolve after the blocked read catches up: {error:?}; visible documents: {:?}; first_id={first_id}",
                visible_titles
            );
        }
    };

    let visible = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_mutation_response_still_resolves_after_blocked_cancellable_read_catches_up() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_900))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("first-query-cancellable"),
                    )]),
                )
                .await
        }
    });

    expect_future_within(
        faults.wait_until_entered(),
        "journal worker should block after durable append",
    )
    .await;
    assert_future_stays_pending(
        &mut first_insert,
        "first mutation should remain pending while apply is blocked",
    )
    .await;

    let mut blocked_query = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .query_documents_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    std::future::pending::<()>(),
                    || Ok(()),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut blocked_query,
        "cancellable query should remain pending while the first durable write is not yet applied",
    )
    .await;

    let mut second_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("second-query-cancellable"),
                    )]),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut second_insert,
        "queued follow-up mutation should remain pending until the blocked apply resumes",
    )
    .await;

    faults.release();

    let first_id = expect_future_within(
        first_insert,
        "first mutation should resolve after apply resumes",
    )
    .await
    .expect("first mutation task should join successfully")
    .expect("first mutation should succeed");
    let query_results = expect_future_within(
        blocked_query,
        "blocked query should resolve after apply resumes",
    )
    .await
    .expect("blocked query task should join successfully")
    .expect("blocked query should succeed");
    assert!(
        query_results.iter().any(
            |document| document.fields.get("title") == Some(&json!("first-query-cancellable"))
        ),
        "blocked query should observe the first applied write"
    );

    let second_id = match timeout(Duration::from_secs(3), second_insert).await {
        Ok(result) => result
            .expect("second mutation task should join successfully")
            .expect("second mutation should succeed"),
        Err(error) => {
            let visible = engine
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("live query should still succeed");
            let visible_titles = visible
                .iter()
                .map(|document| {
                    document.fields["title"]
                        .as_str()
                        .expect("title should be present and a string")
                })
                .collect::<Vec<_>>();
            panic!(
                "queued follow-up mutation should resolve after the blocked cancellable read catches up: {error:?}; visible documents: {:?}; first_id={first_id}",
                visible_titles
            );
        }
    };

    let visible = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_mutation_response_resolves_when_worker_starts_on_ephemeral_current_thread_runtime()
{
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(43_050))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let first_runtime = std::thread::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("ephemeral current-thread runtime should build");
            runtime.block_on(async move {
                engine
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        serde_json::Map::from_iter([(
                            "title".to_string(),
                            json!("first-ephemeral-runtime"),
                        )]),
                    )
                    .await
            })
        }
    });

    expect_future_within(
        faults.wait_until_entered(),
        "journal worker should block after durable append",
    )
    .await;

    let mut second_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("second-after-ephemeral-runtime"),
                    )]),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut second_insert,
        "queued follow-up mutation should remain pending until the blocked apply resumes",
    )
    .await;

    faults.release();

    let first_id = tokio::task::spawn_blocking(move || {
        first_runtime
            .join()
            .expect("ephemeral runtime thread should join successfully")
    })
    .await
    .expect("join worker should finish")
    .expect("first mutation should succeed");
    let second_id = expect_catch_up_future_within(
        second_insert,
        "queued follow-up mutation should still resolve after the ephemeral runtime exits",
    )
    .await
    .expect("second mutation task should join successfully")
    .expect("second mutation should succeed");

    let visible = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}
