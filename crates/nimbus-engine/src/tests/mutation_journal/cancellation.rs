use super::support::{expect_future_within, new_faulted_engine};
use super::*;

#[tokio::test]
async fn paginate_documents_async_cancellable_returns_cancelled_while_blocking_work_unwinds() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    for rank in 0..32 {
        engine
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let probe = BlockingCancellationProbe::new();
    let handle = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        let probe_for_check = probe.clone();
        async move {
            engine
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
    let (_data_dir, engine, tenant_id, faults) = new_faulted_engine(10_000);

    let blocker = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
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
    let blocker_id = durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0))
        .first()
        .and_then(|commit| commit.writes.first())
        .map(|write| write.doc_id.clone())
        .expect("durable blocker commit should include the inserted document id");

    // Park the committer before its next drain. The blocker above stops inside
    // the *publisher's* durable append, not inside the committer, so the
    // committer stays free to drain and assign the request below the moment it
    // is enqueued -- which happens before the cancel future has had a chance to
    // run. Sequence assignment is the point of no return for a queued mutation,
    // so without this pause the test races the committer for the only window in
    // which cancellation can still take effect, and loses it under load.
    let drain_pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("mutation journal pause handle should load");
    drain_pause.arm();

    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async_with(
                    tenant_id,
                    tasks_table(),
                    None,
                    serde_json::Map::from_iter([("title".to_string(), json!("rolled-back"))]),
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

    assert!(
        tokio::task::spawn_blocking({
            let drain_pause = drain_pause.clone();
            move || drain_pause.wait_until_entered(Duration::from_secs(5))
        })
        .await
        .expect("drain pause waiter should join"),
        "the committer should park before draining the queued mutation"
    );

    cancel.notify_one();
    expect_future_within(
        engine.wait_for_queued_mutation_cancellation_observed_for_testing(&tenant_id),
        "queued mutation should record cancellation before the committer resumes",
    )
    .await
    .expect("cancellation observation wait should succeed");
    // Only now is the request both enqueued and observably cancelled, so the
    // drain that follows must resolve it without assigning a sequence.
    drain_pause.release();
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
    let documents = engine
        .query_documents(&tenant_id, &query_for("tasks"))
        .expect("query should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, blocker_id);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("blocker")));
    assert_eq!(
        durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1,
        "queued cancellation before durable append should not append a second commit"
    );
}

#[tokio::test]
async fn mutation_async_cancellable_after_commit_returns_committed_result() {
    let (_data_dir, engine, tenant_id, faults) = new_faulted_engine(20_000);

    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let mut handle = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async_with(
                    tenant_id,
                    tasks_table(),
                    None,
                    serde_json::Map::from_iter([("title".to_string(), json!("after-commit"))]),
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
        engine.query_documents_async(tenant_id.clone(), query_for("tasks")),
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
        durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1
    );
}

#[tokio::test]
async fn mutation_async_non_cancelable_call_drops_unused_cancellation_future_after_completion() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let dropped = Arc::new(AtomicBool::new(false));

    let document_id = engine
        .insert_document_async_with(
            tenant_id.clone(),
            tasks_table(),
            None,
            serde_json::Map::from_iter([("title".to_string(), json!("drop-cancel-future"))]),
            crate::AsyncMutationContext::with_principal(
                PrincipalContext::anonymous(),
                DropAwarePendingCancellation {
                    dropped: dropped.clone(),
                },
                || Ok(()),
            ),
        )
        .await
        .expect("mutation should succeed");

    tokio::task::yield_now().await;

    assert!(
        dropped.load(Ordering::SeqCst),
        "unused cancellation futures should be dropped once the mutation completes"
    );
    assert_eq!(
        engine
            .get_document(&tenant_id, &tasks_table(), document_id.clone())
            .expect("inserted document should remain visible")
            .fields
            .get("title"),
        Some(&json!("drop-cancel-future"))
    );
}
