use super::*;

#[tokio::test]
async fn queued_canceled_async_read_never_begins_real_storage_execution() {
    let store = Arc::new(TenantStore::create_in_memory().expect("store should open"));
    let read_storage =
        RedbTenantStorage::with_max_concurrent_reads(store, tokio::runtime::Handle::current(), 1);
    let first_gate = BlockingReadGate::new();
    let first_gate_for_task = first_gate.clone();
    let first_storage = read_storage.clone();
    let first = tokio::spawn(async move {
        first_storage
            .execute(move |_store| {
                first_gate_for_task.block();
                Ok(())
            })
            .await
    });

    timeout(Duration::from_secs(1), first_gate.wait_until_entered())
        .await
        .expect("first read should acquire the only permit");

    let started = Arc::new(AtomicBool::new(false));
    let cancel = Arc::new(Notify::new());
    let started_for_task = started.clone();
    let cancel_for_wait = cancel.clone();
    let queued_read_storage = read_storage.clone();
    let second = tokio::spawn(async move {
        queued_read_storage
            .execute_cancellable(
                async move {
                    cancel_for_wait.notified().await;
                },
                || Ok(()),
                move |_store, _check_cancel| {
                    started_for_task.store(true, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
    });

    cancel.notify_one();
    let error = timeout(Duration::from_secs(1), second)
        .await
        .expect("queued read should resolve after cancellation")
        .expect("queued read task should join successfully")
        .expect_err("queued read should cancel");
    assert!(matches!(error, Error::Cancelled));
    assert!(
        !started.load(Ordering::SeqCst),
        "queued read should not begin executing once canceled"
    );

    first_gate.release();
    first
        .await
        .expect("first read task should join successfully")
        .expect("first read should complete");
}

#[tokio::test]
async fn same_tenant_async_reads_can_progress_concurrently() {
    let store = Arc::new(TenantStore::create_in_memory().expect("store should open"));
    let read_storage =
        RedbTenantStorage::with_max_concurrent_reads(store, tokio::runtime::Handle::current(), 2);
    let first_gate = BlockingReadGate::new();
    let first_gate_for_task = first_gate.clone();
    let first_storage = read_storage.clone();
    let first = tokio::spawn(async move {
        first_storage
            .execute(move |_store| {
                first_gate_for_task.block();
                Ok(1usize)
            })
            .await
    });

    timeout(Duration::from_secs(1), first_gate.wait_until_entered())
        .await
        .expect("first read should start");

    let second_started = Arc::new(AtomicBool::new(false));
    let second_started_for_task = second_started.clone();
    let second_storage = read_storage.clone();
    let second = tokio::spawn(async move {
        second_storage
            .execute(move |_store| {
                second_started_for_task.store(true, Ordering::SeqCst);
                Ok(2usize)
            })
            .await
    });

    let second_result = timeout(Duration::from_secs(1), second)
        .await
        .expect("second read should not wait behind the blocked first read")
        .expect("second read task should join successfully")
        .expect("second read should complete");
    assert_eq!(second_result, 2);
    assert!(
        second_started.load(Ordering::SeqCst),
        "second read should begin while the first read is still blocked"
    );

    first_gate.release();
    let first_result = first
        .await
        .expect("first read task should join successfully")
        .expect("first read should complete");
    assert_eq!(first_result, 1);
}

#[tokio::test]
async fn canceled_async_write_before_commit_leaves_no_durable_state() {
    let store = Arc::new(TenantStore::create_in_memory().expect("store should open"));
    let storage = RedbTenantStorage::with_max_concurrent_reads(
        store.clone(),
        tokio::runtime::Handle::current(),
        2,
    );
    let document = Document::new(
        TableName::new("tasks").expect("table should build"),
        serde_json::Map::from_iter([("rank".to_string(), json!(7))]),
    );
    let indexes = vec![IndexDefinition {
        name: "by_rank".to_string(),
        fields: vec!["rank".to_string()],
    }];
    let gate = BlockingReadGate::new();
    let gate_for_task = gate.clone();
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let storage = storage.clone();
        let document = document.clone();
        async move {
            storage
                .execute_write_cancellable(
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                    move |transaction| {
                        transaction.insert_document_with_indexes(&document, &indexes)?;
                        gate_for_task.block();
                        Ok(document.id)
                    },
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), gate.wait_until_entered())
        .await
        .expect("write should reach the pre-commit gate");
    cancel.notify_one();
    tokio::time::sleep(Duration::from_millis(25)).await;
    gate.release();

    let outcome = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async write should resolve after cancellation")
        .expect("write task should join successfully")
        .expect("write executor should return an outcome");
    assert!(matches!(outcome, TenantWriteOutcome::CancelledBeforeCommit));
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("get should succeed")
            .is_none(),
        "document should not become visible after pre-commit cancellation"
    );
    assert!(
        store
            .index_scan_eq(&document.table, "by_rank", &json!(7))
            .expect("index scan should succeed")
            .is_empty(),
        "index entries should roll back with the canceled write"
    );
    assert!(
        store
            .read_commit_log_from(SequenceNumber(1))
            .expect("commit log should read")
            .is_empty(),
        "commit log should stay empty after pre-commit cancellation"
    );
}

#[tokio::test]
async fn canceled_async_write_after_commit_still_reports_committed() {
    let clock = Arc::new(ManualClock::new(Timestamp(10_000)));
    let faults = BlockingFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    let store = Arc::new(
        TenantStore::create_in_memory_with_simulation(clock, faults.clone())
            .expect("store should open with simulation seams"),
    );
    let storage = RedbTenantStorage::with_max_concurrent_reads(
        store.clone(),
        tokio::runtime::Handle::current(),
        2,
    );
    let document = Document::new(
        TableName::new("tasks").expect("table should build"),
        serde_json::Map::from_iter([("rank".to_string(), json!(11))]),
    );
    let indexes = vec![IndexDefinition {
        name: "by_rank".to_string(),
        fields: vec!["rank".to_string()],
    }];
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let storage = storage.clone();
        let document = document.clone();
        async move {
            storage
                .execute_write_cancellable(
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                    move |transaction| {
                        transaction.insert_document_with_indexes(&document, &indexes)?;
                        Ok(document.id)
                    },
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("write should block after the durable commit point");
    cancel.notify_one();
    faults.release();

    let outcome = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async write should resolve after post-commit cancellation")
        .expect("write task should join successfully")
        .expect("write executor should return an outcome");
    match outcome {
        TenantWriteOutcome::Committed(committed) => {
            assert_eq!(committed.value, document.id);
            assert_eq!(
                committed
                    .commit
                    .expect("committed write should append a commit entry")
                    .sequence,
                SequenceNumber(1)
            );
        }
        TenantWriteOutcome::CancelledBeforeCommit => {
            panic!("post-commit cancellation must not downgrade a committed write")
        }
    }
    assert!(
        store
            .get(&document.table, &document.id)
            .expect("get should succeed")
            .is_some(),
        "document should stay visible after post-commit cancellation"
    );
    assert_eq!(
        store
            .index_scan_eq(&document.table, "by_rank", &json!(11))
            .expect("index scan should succeed")
            .len(),
        1
    );
    assert_eq!(
        store
            .read_commit_log_from(SequenceNumber(1))
            .expect("commit log should read")
            .len(),
        1
    );
}
