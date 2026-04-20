use super::support::*;

#[tokio::test]
async fn sqlite_async_read_cancellation_still_prevents_queued_execution() {
    let dir = tempdir().expect("temporary directory should create");
    let store = Arc::new(
        SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
            .expect("sqlite tenant store should open"),
    );
    let storage =
        SqliteTenantStorage::with_max_concurrent_reads(store, tokio::runtime::Handle::current(), 1);
    let first_gate = BlockingReadGate::new();
    let first_gate_for_task = first_gate.clone();
    let first_storage = storage.clone();
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
    let queued_storage = storage.clone();
    let second = tokio::spawn(async move {
        queued_storage
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
        .expect("queued sqlite read should resolve after cancellation")
        .expect("queued sqlite read task should join successfully")
        .expect_err("queued sqlite read should cancel");
    assert!(matches!(error, Error::Cancelled));
    assert!(
        !started.load(Ordering::SeqCst),
        "queued sqlite read should not begin executing once canceled"
    );

    first_gate.release();
    first
        .await
        .expect("first sqlite read task should join successfully")
        .expect("first sqlite read should complete");
}

#[tokio::test]
async fn sqlite_async_write_precommit_cancellation_leaves_no_state() {
    let dir = tempdir().expect("temporary directory should create");
    let store = Arc::new(
        SqliteTenantStore::open(dir.path().join("tenant.sqlite3"))
            .expect("sqlite tenant store should open"),
    );
    let storage = SqliteTenantStorage::with_max_concurrent_reads(
        store.clone(),
        tokio::runtime::Handle::current(),
        2,
    );
    let gate = BlockingReadGate::new();
    let gate_for_task = gate.clone();
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let storage = storage.clone();
        async move {
            storage
                .execute_write_cancellable(
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                    move |transaction| {
                        transaction.put_metadata("marker", b"before")?;
                        gate_for_task.block();
                        Ok("marker".to_string())
                    },
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), gate.wait_until_entered())
        .await
        .expect("sqlite write should reach the pre-commit gate");
    cancel.notify_one();
    tokio::time::sleep(Duration::from_millis(25)).await;
    gate.release();

    let outcome = timeout(Duration::from_secs(1), handle)
        .await
        .expect("sqlite async write should resolve after cancellation")
        .expect("sqlite write task should join successfully")
        .expect("sqlite write executor should return an outcome");
    assert!(matches!(outcome, TenantWriteOutcome::CancelledBeforeCommit));
    assert!(
        store
            .metadata_blob("marker")
            .expect("metadata read should succeed")
            .is_none(),
        "pre-commit cancellation should roll back sqlite metadata writes"
    );
}

#[tokio::test]
async fn sqlite_async_write_after_commit_still_reports_committed() {
    let dir = tempdir().expect("temporary directory should create");
    let faults = BlockingFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    let store = Arc::new(
        SqliteTenantStore::open_with_simulation(
            dir.path().join("tenant.sqlite3"),
            Arc::new(ManualClock::new(Timestamp(10_000))),
            faults.clone(),
        )
        .expect("sqlite tenant store should open with simulation seams"),
    );
    let storage = SqliteTenantStorage::with_max_concurrent_reads(
        store.clone(),
        tokio::runtime::Handle::current(),
        2,
    );
    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let storage = storage.clone();
        async move {
            storage
                .execute_write_cancellable(
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                    move |transaction| {
                        transaction.put_metadata("marker", b"after")?;
                        Ok("marker".to_string())
                    },
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("sqlite write should block after the durable commit point");
    cancel.notify_one();
    faults.release();

    let outcome = timeout(Duration::from_secs(1), handle)
        .await
        .expect("sqlite async write should resolve after post-commit cancellation")
        .expect("sqlite write task should join successfully")
        .expect("sqlite write executor should return an outcome");
    match outcome {
        TenantWriteOutcome::Committed(committed) => {
            assert_eq!(committed.value, "marker".to_string());
            assert!(
                committed.commit.is_none(),
                "foundation writes do not emit logical commit entries yet"
            );
        }
        TenantWriteOutcome::CancelledBeforeCommit => {
            panic!("post-commit cancellation must not downgrade a committed sqlite write")
        }
    }
    assert_eq!(
        store
            .metadata_blob("marker")
            .expect("metadata read should succeed"),
        Some(b"after".to_vec())
    );
}
