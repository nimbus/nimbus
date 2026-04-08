use super::*;

#[tokio::test]
async fn paginate_documents_async_cancellable_returns_cancelled_while_blocking_work_unwinds() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    for rank in 0..32 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let probe = BlockingCancellationProbe::new();
    let handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        let probe_for_check = probe.clone();
        async move {
            service
                .paginate_documents_async_cancellable(
                    tenant_id,
                    PaginatedQuery {
                        query: query_for("tasks"),
                        page_size: 8,
                        after: None,
                    },
                    probe_for_wait.cancel_wait(),
                    probe_for_check.check(),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), probe.wait_for_first_check())
        .await
        .expect("paginated query should reach cooperative cancellation check");
    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async paginated query should resolve promptly after cancellation")
        .expect("paginated query task should join successfully")
        .expect_err("paginated query should cancel");
    assert!(matches!(error, Error::Cancelled));

    probe.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn mutation_async_cancellable_before_commit_rolls_back_document_index_and_durable_journal() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(10_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut blocker = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("blocker"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("first write should block after durable append and before apply");
    assert!(
        timeout(Duration::from_millis(100), &mut blocker)
            .await
            .is_err(),
        "first mutation should remain pending while apply is blocked"
    );
    let blocker_id = durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0))
        .first()
        .and_then(|commit| commit.writes.first())
        .map(|write| write.doc_id)
        .expect("durable blocker commit should include the inserted document id");

    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("rolled-back"))]),
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                )
                .await
        }
    });

    cancel.notify_one();
    tokio::time::sleep(Duration::from_millis(25)).await;
    faults.release();

    timeout(Duration::from_secs(1), blocker)
        .await
        .expect("first mutation should finish after apply resumes")
        .expect("blocker task should join successfully")
        .expect("first mutation should succeed");

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("queued async mutation should resolve after cancellation")
        .expect("mutation task should join successfully")
        .expect_err("queued cancellation before durable append should surface as cancelled");
    assert!(matches!(error, Error::Cancelled));
    let documents = service
        .query_documents(&tenant_id, &query_for("tasks"))
        .expect("query should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, blocker_id);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("blocker")));
    assert_eq!(
        durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1,
        "queued cancellation before durable append should not append a second commit"
    );
}

#[tokio::test]
async fn mutation_async_cancellable_after_commit_returns_committed_result() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(20_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("after-commit"))]),
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("write should block after durable append and before apply");
    cancel.notify_one();

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "post-commit cancellation should not complete before apply resumes"
    );
    faults.release();
    let document_id = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async mutation should resolve after apply resumes")
        .expect("mutation task should join successfully")
        .expect("post-commit cancellation should still return success");
    let documents = timeout(
        Duration::from_secs(1),
        service.query_documents_async(tenant_id.clone(), query_for("tasks")),
    )
    .await
    .expect("query should resolve after apply")
    .expect("query should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, document_id);
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("after-commit"))
    );
    assert_eq!(
        durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1
    );
}

#[tokio::test]
async fn mutation_async_non_cancelable_call_drops_unused_cancellation_future_after_completion() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let dropped = Arc::new(AtomicBool::new(false));

    let document_id = service
        .insert_document_async_cancellable_with_principal(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("drop-cancel-future"))]),
            PrincipalContext::anonymous(),
            DropAwarePendingCancellation {
                dropped: dropped.clone(),
            },
            || Ok(()),
        )
        .await
        .expect("mutation should succeed");

    tokio::task::yield_now().await;

    assert!(
        dropped.load(Ordering::SeqCst),
        "unused cancellation futures should be dropped once the mutation completes"
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &tasks_table(), document_id)
            .expect("inserted document should remain visible")
            .fields
            .get("title"),
        Some(&json!("drop-cancel-future"))
    );
}

#[tokio::test]
async fn mutation_journal_returns_only_after_apply_visibility() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(30_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("durable-first"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append and before apply");

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "async mutation should remain pending while apply is blocked"
    );
    let document_id = durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0))
        .first()
        .and_then(|commit| commit.writes.first())
        .map(|write| write.doc_id)
        .expect("durable commit should include the inserted document id");
    faults.release();
    let completed_id = timeout(Duration::from_secs(1), handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let documents = timeout(
        Duration::from_secs(1),
        service.query_documents_async(tenant_id.clone(), query_for("tasks")),
    )
    .await
    .expect("query should resolve after apply")
    .expect("query should succeed");
    assert_eq!(
        service
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("latest sequence should read"),
        SequenceNumber(1)
    );
    assert_eq!(
        durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1
    );
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, document_id);
    assert_eq!(completed_id, document_id);
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("durable-first"))
    );
}

#[tokio::test]
async fn sync_query_waits_for_applied_journal_visibility_and_records_wait_metrics() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(35_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("sync-wait"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );

    let (query_tx, mut query_rx) = mpsc::unbounded_channel();
    tokio::task::spawn_blocking({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        move || {
            let _ = query_tx.send(service.query_documents(&tenant_id, &query_for("tasks")));
        }
    });

    assert!(
        timeout(Duration::from_millis(100), query_rx.recv())
            .await
            .is_err(),
        "sync query should wait for the applied watermark while journaled data is not yet materialized"
    );

    faults.release();

    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let documents = timeout(Duration::from_secs(1), query_rx.recv())
        .await
        .expect("sync query should resolve after apply")
        .expect("sync query result should be sent")
        .expect("sync query should succeed");
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("sync-wait")),
        "sync query should observe the applied task after the journal worker resumes"
    );

    let stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should read after sync wait");
    assert_eq!(stats.read_wait_count, 1);
    assert!(
        stats.total_read_wait_nanos > 0,
        "sync read waits should contribute to read wait metrics"
    );
}

#[tokio::test]
async fn query_waits_for_applied_journal_visibility_and_records_wait_metrics() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(40_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("wait-for-apply"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );

    let stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should read");
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(0));
    assert_eq!(stats.apply_lag, 1);
    assert_eq!(stats.queue_depth, 0);
    assert_eq!(
        stats.queue_capacity,
        crate::tenant::DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY
    );
    assert_eq!(stats.oldest_queue_age_nanos, 0);
    assert_eq!(stats.pending_response_count, 1);
    assert!(stats.worker_running);
    assert_eq!(stats.worker_start_count, 1);
    assert_eq!(stats.worker_restart_count, 0);
    assert_eq!(stats.queue_rejection_count, 0);
    assert_eq!(stats.worker_failure_count, 0);
    assert_eq!(stats.read_wait_count, 0);

    let (query_tx, mut query_rx) = mpsc::unbounded_channel();
    tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            let result = service
                .query_documents_async(tenant_id, query_for("tasks"))
                .await;
            let _ = query_tx.send(result);
        }
    });

    assert!(
        timeout(Duration::from_millis(100), query_rx.recv())
            .await
            .is_err(),
        "query should wait for the applied watermark while journaled data is not yet materialized"
    );

    faults.release();

    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let documents = timeout(Duration::from_secs(1), query_rx.recv())
        .await
        .expect("query should resolve after apply")
        .expect("query result should be sent")
        .expect("query should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("wait-for-apply"))
    );

    let stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should read after apply");
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(1));
    assert_eq!(stats.apply_lag, 0);
    assert_eq!(stats.queue_depth, 0);
    assert_eq!(stats.oldest_queue_age_nanos, 0);
    assert_eq!(stats.pending_response_count, 0);
    assert!(!stats.worker_running);
    assert_eq!(stats.worker_start_count, 1);
    assert_eq!(stats.worker_restart_count, 0);
    assert_eq!(stats.queue_rejection_count, 0);
    assert_eq!(stats.worker_failure_count, 0);
    assert_eq!(stats.read_wait_count, 1);
    assert!(
        stats.total_read_wait_nanos > 0,
        "read wait metrics should accumulate a positive wait duration"
    );
}

#[tokio::test]
async fn mutation_admission_gate_buffers_while_journal_is_paused_without_losing_in_flight_response()
{
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .set_mutation_journal_queue_capacity_for_testing(&tenant_id, 1)
        .expect("queue capacity should be configurable for tests");
    let pause = service
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let first_insert = {
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("queued-first"))]),
                )
                .await
        })
    };

    assert!(
        tokio::task::spawn_blocking({
            let pause = pause.clone();
            move || pause.wait_until_entered(Duration::from_secs(1))
        })
        .await
        .expect("pause wait should join"),
        "journal worker should pause before draining the queued request"
    );

    let blocked_stats = service
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
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("queued-second"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), async {
        loop {
            let stats = service
                .mutation_admission_stats_for_testing(&tenant_id)
                .expect("admission stats should load while the journal is paused");
            if stats.queue_depth == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("second mutation should remain buffered at the admission gate");

    assert!(
        timeout(Duration::from_millis(150), &mut second_insert)
            .await
            .is_err(),
        "second mutation should stay pending while the journal worker is paused"
    );

    let buffered_stats = service
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

    let first_id = timeout(Duration::from_secs(1), first_insert)
        .await
        .expect("first mutation should resolve after the pause is released")
        .expect("first mutation task should join successfully")
        .expect("first mutation should succeed");
    let second_id = timeout(Duration::from_secs(1), second_insert)
        .await
        .expect("second mutation should resolve after the journal drains")
        .expect("second mutation task should join successfully")
        .expect("second mutation should succeed");

    let visible = service
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("final query should succeed after the buffered mutation drains");
    assert_eq!(visible.len(), 2);
    assert_eq!(
        visible
            .into_iter()
            .map(|document| document.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first_id, second_id])
    );

    let final_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after the queue drains");
    assert_eq!(final_stats.durable_head, SequenceNumber(2));
    assert_eq!(final_stats.applied_head, SequenceNumber(2));
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

    let final_admission_stats = service
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the gate drains");
    assert_eq!(final_admission_stats.queue_depth, 0);
    assert_eq!(final_admission_stats.shed_count, 0);
    assert_eq!(final_admission_stats.queue_rejection_count, 0);
}

#[tokio::test]
async fn mutation_journal_never_expires_admitted_work() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .set_mutation_admission_codel_for_testing(
            &tenant_id,
            Duration::from_millis(5),
            Duration::from_millis(10),
        )
        .expect("admission CoDel should be configurable for tests");
    let pause = service
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let admitted_insert = {
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("admitted"))]),
                )
                .await
        })
    };

    assert!(
        tokio::task::spawn_blocking({
            let pause = pause.clone();
            move || pause.wait_until_entered(Duration::from_secs(1))
        })
        .await
        .expect("pause wait should join"),
        "journal worker should pause after admitting the mutation to the journal queue"
    );

    tokio::time::sleep(Duration::from_millis(25)).await;
    pause.release();

    let document_id = timeout(Duration::from_secs(1), admitted_insert)
        .await
        .expect("admitted mutation should resolve after the pause is released")
        .expect("admitted mutation task should join successfully")
        .expect("admitted mutation should still succeed");

    let visible = service
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("final query should succeed after the admitted mutation drains");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, document_id);

    let admission_stats = service
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the queue drains");
    assert_eq!(admission_stats.queue_depth, 0);
    assert_eq!(admission_stats.shed_count, 0);
    assert_eq!(admission_stats.queue_rejection_count, 0);

    let journal_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after the admitted mutation commits");
    assert_eq!(journal_stats.durable_head, SequenceNumber(1));
    assert_eq!(journal_stats.applied_head, SequenceNumber(1));
    assert_eq!(journal_stats.queue_depth, 0);
}

#[tokio::test]
async fn queued_mutation_response_still_resolves_after_blocked_read_catches_up() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_500))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut first_insert)
            .await
            .is_err(),
        "first mutation should remain pending while apply is blocked"
    );

    let mut blocked_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut blocked_query)
            .await
            .is_err(),
        "query should remain pending while the first durable write is not yet applied"
    );

    let mut second_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
                )
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut second_insert)
            .await
            .is_err(),
        "queued follow-up mutation should remain pending until the blocked apply resumes"
    );

    faults.release();

    let first_id = timeout(Duration::from_secs(1), first_insert)
        .await
        .expect("first mutation should resolve after apply resumes")
        .expect("first mutation task should join successfully")
        .expect("first mutation should succeed");
    let query_results = timeout(Duration::from_secs(1), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
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
            let visible = service
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

    let visible = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_cancellable_mutation_response_still_resolves_after_blocked_read_catches_up() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_750))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("first-cancellable"))]),
                    std::future::pending::<()>(),
                    || Ok(()),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut first_insert)
            .await
            .is_err(),
        "first cancellable mutation should remain pending while apply is blocked"
    );

    let mut blocked_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut blocked_query)
            .await
            .is_err(),
        "query should remain pending while the first durable write is not yet applied"
    );

    let mut second_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("second-cancellable"),
                    )]),
                    std::future::pending::<()>(),
                    || Ok(()),
                )
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut second_insert)
            .await
            .is_err(),
        "queued follow-up cancellable mutation should remain pending until the blocked apply resumes"
    );

    faults.release();

    let first_id = timeout(Duration::from_secs(1), first_insert)
        .await
        .expect("first cancellable mutation should resolve after apply resumes")
        .expect("first cancellable mutation task should join successfully")
        .expect("first cancellable mutation should succeed");
    let query_results = timeout(Duration::from_secs(1), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
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
            let visible = service
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

    let visible = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_mutation_response_still_resolves_after_blocked_cancellable_read_catches_up() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_900))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
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

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut first_insert)
            .await
            .is_err(),
        "first mutation should remain pending while apply is blocked"
    );

    let mut blocked_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .query_documents_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    std::future::pending::<()>(),
                    || Ok(()),
                )
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut blocked_query)
            .await
            .is_err(),
        "cancellable query should remain pending while the first durable write is not yet applied"
    );

    let mut second_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
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
    assert!(
        timeout(Duration::from_millis(100), &mut second_insert)
            .await
            .is_err(),
        "queued follow-up mutation should remain pending until the blocked apply resumes"
    );

    faults.release();

    let first_id = timeout(Duration::from_secs(1), first_insert)
        .await
        .expect("first mutation should resolve after apply resumes")
        .expect("first mutation task should join successfully")
        .expect("first mutation should succeed");
    let query_results = timeout(Duration::from_secs(1), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
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
            let visible = service
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

    let visible = service
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
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(43_050))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let first_runtime = std::thread::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("ephemeral current-thread runtime should build");
            runtime.block_on(async move {
                service
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

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");

    let mut second_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
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
    assert!(
        timeout(Duration::from_millis(100), &mut second_insert)
            .await
            .is_err(),
        "queued follow-up mutation should remain pending until the blocked apply resumes"
    );

    faults.release();

    let first_id = tokio::task::spawn_blocking(move || {
        first_runtime
            .join()
            .expect("ephemeral runtime thread should join successfully")
    })
    .await
    .expect("join worker should finish")
    .expect("first mutation should succeed");
    let second_id = timeout(Duration::from_secs(3), second_insert)
        .await
        .expect("queued follow-up mutation should still resolve after the ephemeral runtime exits")
        .expect("second mutation task should join successfully")
        .expect("second mutation should succeed");

    let visible = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn get_document_async_cancellable_returns_cancelled_while_waiting_for_applied_visibility() {
    let (service, tenant_id, faults, document_id) =
        create_service_with_durable_unapplied_task(44_000, "async-get-cancel").await;
    let probe = BlockingCancellationProbe::new();

    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        async move {
            service
                .get_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    probe_for_wait.cancel_wait(),
                    || Ok(()),
                )
                .await
        }
    });

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "point read should still be waiting for applied visibility before cancellation"
    );

    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async point read should resolve promptly after cancellation")
        .expect("point read task should join successfully")
        .expect_err("point read should cancel while waiting for apply");
    assert!(matches!(error, Error::Cancelled));

    faults.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn query_documents_async_cancellable_returns_cancelled_while_waiting_for_applied_visibility()
{
    let (service, tenant_id, faults, _) =
        create_service_with_durable_unapplied_task(44_500, "async-query-cancel").await;
    let probe = BlockingCancellationProbe::new();

    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        async move {
            service
                .query_documents_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    probe_for_wait.cancel_wait(),
                    || Ok(()),
                )
                .await
        }
    });

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "query should still be waiting for applied visibility before cancellation"
    );

    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async query should resolve promptly after cancellation")
        .expect("query task should join successfully")
        .expect_err("query should cancel while waiting for apply");
    assert!(matches!(error, Error::Cancelled));

    faults.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn paginate_documents_async_cancellable_returns_cancelled_while_waiting_for_applied_visibility()
 {
    let (service, tenant_id, faults, _) =
        create_service_with_durable_unapplied_task(44_750, "async-page-cancel").await;
    let probe = BlockingCancellationProbe::new();

    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        async move {
            service
                .paginate_documents_async_cancellable(
                    tenant_id,
                    PaginatedQuery {
                        query: query_for("tasks"),
                        page_size: 1,
                        after: None,
                    },
                    probe_for_wait.cancel_wait(),
                    || Ok(()),
                )
                .await
        }
    });

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "pagination should still be waiting for applied visibility before cancellation"
    );

    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async pagination should resolve promptly after cancellation")
        .expect("pagination task should join successfully")
        .expect_err("pagination should cancel while waiting for apply");
    assert!(matches!(error, Error::Cancelled));

    faults.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn sync_get_document_waits_for_applied_journal_visibility() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(45_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("sync-get"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );
    let document_id = durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0))
        .first()
        .and_then(|commit| commit.writes.first())
        .map(|write| write.doc_id)
        .expect("durable commit should include the inserted document id");

    let (get_tx, mut get_rx) = mpsc::unbounded_channel();
    tokio::task::spawn_blocking({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        move || {
            let _ = get_tx.send(service.get_document(&tenant_id, &tasks_table(), document_id));
        }
    });

    assert!(
        timeout(Duration::from_millis(100), get_rx.recv())
            .await
            .is_err(),
        "sync point reads should wait for applied visibility instead of returning stale not-found results"
    );

    faults.release();

    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let document = timeout(Duration::from_secs(1), get_rx.recv())
        .await
        .expect("sync point read should resolve after apply")
        .expect("sync point read result should be sent")
        .expect("sync point read should succeed");
    assert_eq!(document.fields.get("title"), Some(&json!("sync-get")));
}

#[tokio::test]
async fn subscription_updates_publish_only_after_journal_apply() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(50_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let (tx, mut rx) = subscription_channel();
    let subscription = service
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "journal-sub".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
    let initial = rx
        .recv()
        .await
        .expect("initial subscription update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert_eq!(request_id.as_deref(), Some("journal-sub"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("reactive"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );

    assert!(
        timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err(),
        "subscription fan-out must stay behind the applied visibility boundary"
    );

    faults.release();

    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let update = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("subscription update should arrive after apply")
        .expect("subscription update should be sent");
    match update {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert!(request_id.is_none());
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("reactive"));
        }
        other => panic!("unexpected reactive update: {other:?}"),
    }
}

#[tokio::test]
async fn async_subscription_bootstrap_catches_up_writes_committed_before_activation() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let pause = service
        .subscription_bootstrap_pause_handle_for_testing(&tenant_id)
        .expect("bootstrap pause handle should load");
    pause.arm();

    let (tx, mut rx) = subscription_channel();
    let subscribe_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .subscribe_async(
                    tenant_id,
                    query_for("tasks"),
                    "bootstrap-gap".to_string(),
                    tx,
                )
                .await
        }
    });

    let initial = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("initial subscription result should arrive")
        .expect("subscription channel should remain open");
    match initial {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert_eq!(request_id.as_deref(), Some("bootstrap-gap"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    assert!(
        pause.wait_until_entered(Duration::from_secs(1)),
        "subscription bootstrap should pause before activation"
    );

    service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("during-bootstrap"))]),
        )
        .await
        .expect("insert during bootstrap gap should succeed");

    assert!(
        timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err(),
        "inactive bootstrap window should not publish reactive updates before activation resumes"
    );

    pause.release();

    let _subscription = timeout(Duration::from_secs(1), subscribe_task)
        .await
        .expect("subscribe task should finish after pause release")
        .expect("subscribe task should join successfully")
        .expect("subscription should register successfully");

    let catch_up = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("subscription should catch up the write committed during bootstrap")
        .expect("subscription channel should remain open");
    match catch_up {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert!(request_id.is_none());
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("during-bootstrap"));
        }
        other => panic!("unexpected bootstrap catch-up event: {other:?}"),
    }
}

#[tokio::test]
async fn async_subscription_bootstrap_cancellation_before_activation_returns_cancelled() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let pause = service
        .subscription_bootstrap_pause_handle_for_testing(&tenant_id)
        .expect("bootstrap pause handle should load");
    pause.arm();

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_notify = Arc::new(Notify::new());
    let (tx, mut rx) = subscription_channel();
    let subscribe_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let cancelled = cancelled.clone();
        let cancel_notify = cancel_notify.clone();
        async move {
            service
                .subscribe_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    "bootstrap-cancel".to_string(),
                    tx,
                    SubscriptionBootstrapCancellation::new(
                        async move { cancel_notify.notified().await },
                        move || {
                            if cancelled.load(Ordering::SeqCst) {
                                Err(Error::Cancelled)
                            } else {
                                Ok(())
                            }
                        },
                    ),
                )
                .await
        }
    });

    let initial = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("initial subscription result should arrive")
        .expect("subscription channel should remain open");
    match initial {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert_eq!(request_id.as_deref(), Some("bootstrap-cancel"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    assert!(
        pause.wait_until_entered(Duration::from_secs(1)),
        "subscription bootstrap should pause before activation"
    );

    cancelled.store(true, Ordering::SeqCst);
    cancel_notify.notify_waiters();
    pause.release();

    let error = timeout(Duration::from_secs(1), subscribe_task)
        .await
        .expect("cancelled subscribe task should finish after pause release")
        .expect("cancelled subscribe task should join successfully")
        .expect_err("subscription bootstrap should be cancelled before activation");
    assert!(matches!(error, Error::Cancelled));

    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        0,
        "cancelled bootstrap should remove the pending subscription",
    );
    match timeout(Duration::from_millis(100), rx.recv()).await {
        Err(_) | Ok(None) => {}
        Ok(Some(update)) => {
            panic!("cancelled bootstrap should not emit a catch-up update: {update:?}");
        }
    }
}

#[tokio::test]
async fn sync_subscription_bootstrap_does_not_miss_lagged_applied_commit() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(60_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("lagged-sync"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("write should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_task)
            .await
            .is_err(),
        "insert should remain pending while apply is blocked"
    );

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "sync-lagged".to_string(),
            tx,
        )
        .expect("sync subscription should register");

    let initial = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("initial sync subscription result should arrive")
        .expect("subscription channel should remain open");
    match initial {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert_eq!(request_id.as_deref(), Some("sync-lagged"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial sync subscription event: {other:?}"),
    }

    faults.release();

    timeout(Duration::from_secs(1), insert_task)
        .await
        .expect("insert should finish after apply resumes")
        .expect("insert task should join successfully")
        .expect("insert should succeed");

    let update = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("sync subscription should observe the lagged commit after apply resumes")
        .expect("subscription channel should remain open");
    match update {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert!(request_id.is_none());
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("lagged-sync"));
        }
        other => panic!("unexpected lagged sync subscription event: {other:?}"),
    }
}
