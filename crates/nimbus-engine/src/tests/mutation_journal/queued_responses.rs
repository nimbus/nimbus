use super::support::{
    assert_future_stays_pending, expect_blocking_wait_reaches_state, expect_catch_up_future_within,
    expect_future_within, new_faulted_engine,
};
use super::*;

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

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn assignment_failure_keeps_the_cancelled_requests_own_outcome() {
    let data_dir = tempdir().expect("assignment deferred tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_650))),
            Arc::new(nimbus_storage::NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_650)),
        )
        .expect("assignment deferred engine should create"),
    );
    let tenant_id = TenantId::new("assignment-deferred-outcome").expect("tenant id should build");
    crate::tenant::configure_committer_arm_for_testing(
        tenant_id.clone(),
        crate::tenant::CommitterArm::Serial,
    );
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    // The cancelled request is admitted first so the assignment loop is already
    // holding its deferred response when the second request fails assignment.
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let mut cancelled_write = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async_with(
                    tenant_id,
                    tasks_table(),
                    None,
                    serde_json::Map::from_iter([("title".to_string(), json!("cancelled"))]),
                    crate::AsyncMutationContext::anonymous(
                        async move {
                            cancel_for_wait.notified().await;
                        },
                        || Ok(()),
                    ),
                )
                .await
        }
    });

    expect_blocking_wait_reaches_state(
        "journal worker should pause before draining the cancelled request",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;
    cancel.notify_one();
    assert_future_stays_pending(
        &mut cancelled_write,
        "the cancelled request should stay queued behind the paused committer",
    )
    .await;

    engine
        .commit_fault_handle_for_testing()
        .inject_error_on_nth_hit(
            crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
            1,
            Error::InvalidInput("injected assignment failure after a deferral".to_string()),
        );
    let failing_write = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("fails-assignment"))]),
                )
                .await
        }
    });
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "the failing request should collect behind the pause",
        |stats| stats.queue_depth >= 1,
    )
    .await;
    pause.release();

    let cancelled_error = expect_catch_up_future_within(
        cancelled_write,
        "the cancelled caller should resolve after the failed assignment recovers",
    )
    .await
    .expect("cancelled task should join successfully")
    .expect_err("a cancelled request must not report success");
    assert!(
        matches!(cancelled_error, Error::Cancelled),
        "a request the assignment loop already resolved must keep its own outcome across a later assignment failure, got: {cancelled_error}"
    );

    let assignment_error = expect_catch_up_future_within(
        failing_write,
        "the failing caller should resolve after the failed assignment recovers",
    )
    .await
    .expect("failing task should join successfully")
    .expect_err("the injected assignment fault should fail its own caller");
    assert!(
        matches!(
            assignment_error,
            Error::InvalidInput(ref message)
                if message == "injected assignment failure after a deferral"
        ),
        "the request that hit the fault must receive the typed assignment error, got: {assignment_error}"
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("journal should remain readable")
            .is_empty(),
        "neither request should leave a durable record"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ordered_assignment_panic_keeps_preassigned_outcomes_and_fails_active_request() {
    let data_dir = tempdir().expect("assignment panic tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_660))),
            Arc::new(nimbus_storage::NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_660)),
        )
        .expect("assignment panic engine should create"),
    );
    let tenant_id = TenantId::new("assignment-panic-outcome").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let mut cancelled_write = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async_with(
                    tenant_id,
                    tasks_table(),
                    None,
                    serde_json::Map::from_iter([("title".to_string(), json!("cancelled"))]),
                    crate::AsyncMutationContext::anonymous(
                        async move {
                            cancel_for_wait.notified().await;
                        },
                        || Ok(()),
                    ),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state(
        "journal worker should pause before draining the cancelled request",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;
    cancel.notify_one();
    assert_future_stays_pending(
        &mut cancelled_write,
        "the cancelled request should stay queued behind the paused committer",
    )
    .await;

    engine
        .commit_fault_handle_for_testing()
        .inject_panic_on_nth_hit(
            crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
            1,
        );
    let active_write = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("panics-assignment"))]),
                )
                .await
        }
    });
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "the active request should collect behind the pause",
        |stats| stats.queue_depth >= 1,
    )
    .await;
    pause.release();

    let cancelled_error = expect_catch_up_future_within(
        cancelled_write,
        "the cancelled caller should resolve after panic recovery",
    )
    .await
    .expect("cancelled task should join successfully")
    .expect_err("a cancelled request must not report success");
    assert!(
        matches!(cancelled_error, Error::Cancelled),
        "pre-assignment cancellation must survive an assignment-task panic: {cancelled_error}"
    );

    let active_error = expect_catch_up_future_within(
        active_write,
        "the active caller should receive the typed assignment-task failure",
    )
    .await
    .expect("active task should join successfully")
    .expect_err("the assignment panic must fail the active request");
    assert!(
        matches!(active_error, Error::Internal(ref message) if message.contains("committer assignment batch panicked")),
        "active work must receive the typed join failure rather than a dropped response: {active_error}"
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("journal should remain readable")
            .is_empty(),
        "panic recovery must discard the staged suffix"
    );
    let (assigned_head, pending) = engine
        .write_log_assignment_for_testing(&tenant_id)
        .expect("recovered assignment should read");
    assert_eq!(assigned_head, SequenceNumber(0));
    assert!(pending.is_empty());
}
