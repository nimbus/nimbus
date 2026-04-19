use super::support::new_faulted_service;
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
    timeout(
        Duration::from_secs(1),
        probe.wait_until_released_from_first_check(),
    )
    .await
    .expect("blocking cancellation check should unwind after release");
}

#[tokio::test]
async fn mutation_async_cancellable_before_commit_rolls_back_document_index_and_durable_journal() {
    let (_data_dir, service, tenant_id, faults) = new_faulted_service(10_000);

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
    let mut handle = tokio::spawn({
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
    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "queued canceled mutation should remain blocked behind the earlier durable append until apply resumes"
    );
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
    let (_data_dir, service, tenant_id, faults) = new_faulted_service(20_000);

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
