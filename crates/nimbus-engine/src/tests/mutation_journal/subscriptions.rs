use super::support::new_faulted_service;
use super::*;

#[tokio::test]
async fn subscription_updates_publish_only_after_journal_apply() {
    let (_data_dir, service, tenant_id, faults) = new_faulted_service(50_000);

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
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
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
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
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
            request_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
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
            request_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
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
            request_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
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
            request_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
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
            request_id,
            snapshot,
            ..
        } => {
            let data = snapshot.to_json_documents();
            assert!(request_id.is_none());
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("lagged-sync"));
        }
        other => panic!("unexpected lagged sync subscription event: {other:?}"),
    }
}
