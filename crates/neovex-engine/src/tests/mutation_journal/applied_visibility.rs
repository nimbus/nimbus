use super::support::new_faulted_service;
use super::*;

#[tokio::test]
async fn mutation_journal_returns_only_after_apply_visibility() {
    let (_data_dir, service, tenant_id, faults) = new_faulted_service(30_000);

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
        .map(|write| write.doc_id.clone())
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
async fn get_document_async_cancellable_returns_cancelled_while_waiting_for_applied_visibility() {
    let (_data_dir, service, tenant_id, faults, document_id) =
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
    wait_for_mutation_journal_stats(
        &service,
        &tenant_id,
        "point read cancellation cleanup should drain applied visibility after releasing the durable fault",
        |stats| stats.applied_head == stats.durable_head && stats.apply_lag == 0,
    )
    .await;
}

#[tokio::test]
async fn query_documents_async_cancellable_returns_cancelled_while_waiting_for_applied_visibility()
{
    let (_data_dir, service, tenant_id, faults, _) =
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
    wait_for_mutation_journal_stats(
        &service,
        &tenant_id,
        "query cancellation cleanup should drain applied visibility after releasing the durable fault",
        |stats| stats.applied_head == stats.durable_head && stats.apply_lag == 0,
    )
    .await;
}

#[tokio::test]
async fn paginate_documents_async_cancellable_returns_cancelled_while_waiting_for_applied_visibility()
 {
    let (_data_dir, service, tenant_id, faults, _) =
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
    wait_for_mutation_journal_stats(
        &service,
        &tenant_id,
        "pagination cancellation cleanup should drain applied visibility after releasing the durable fault",
        |stats| stats.applied_head == stats.durable_head && stats.apply_lag == 0,
    )
    .await;
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
        .map(|write| write.doc_id.clone())
        .expect("durable commit should include the inserted document id");

    let (get_tx, mut get_rx) = mpsc::unbounded_channel();
    tokio::task::spawn_blocking({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        move || {
            let _ =
                get_tx.send(service.get_document(&tenant_id, &tasks_table(), document_id.clone()));
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
