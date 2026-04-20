use super::*;

#[tokio::test]
async fn service_delete_tenant_tears_down_active_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tx, mut rx) = subscription_channel();
    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    let subscription = service
        .subscribe(&tenant_id, query, "req-delete".to_string(), tx)
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
    let _ = rx.recv().await.expect("initial update should arrive");

    service
        .delete_tenant(&tenant_id)
        .expect("tenant delete should succeed");

    let teardown = rx.recv().await.expect("teardown error should arrive");
    match teardown {
        SubscriptionUpdate::Error {
            subscription_id: actual_id,
            request_id,
            message,
        } => {
            assert_eq!(actual_id, subscription_id);
            assert!(request_id.is_none());
            assert!(message.contains("tenant deleted: demo"));
        }
        other => panic!("unexpected teardown event: {other:?}"),
    }
}

#[tokio::test]
async fn delete_tenant_async_waits_for_in_flight_operations_and_rejects_new_work() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("blocker"))]),
        )
        .expect("seed insert should succeed");
    let probe = BlockingCancellationProbe::new();

    let read_task: tokio::task::JoinHandle<neovex_core::Result<Vec<neovex_core::Document>>> =
        tokio::spawn({
            let service = service.clone();
            let tenant_id = tenant_id.clone();
            let probe = probe.clone();
            async move {
                service
                    .list_documents_async_cancellable(
                        tenant_id,
                        tasks_table(),
                        probe.clone().cancel_wait(),
                        probe.check(),
                    )
                    .await
            }
        });

    timeout(Duration::from_secs(1), probe.wait_for_first_check())
        .await
        .expect("read operation should enter its first cancellation check");

    let mut delete_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move { service.delete_tenant_async(tenant_id).await }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut delete_task)
            .await
            .is_err(),
        "tenant deletion should wait for the in-flight operation"
    );

    let mut ensure_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move { service.ensure_tenant_exists_async(tenant_id).await }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut ensure_task)
            .await
            .is_err(),
        "new work should remain blocked behind tenant deletion"
    );

    probe.release();

    timeout(Duration::from_secs(1), async {
        read_task
            .await
            .expect("read task should join")
            .expect("read task should succeed");
    })
    .await
    .expect("read task should finish after release");
    timeout(Duration::from_secs(1), async {
        delete_task
            .await
            .expect("delete task should join")
            .expect("tenant delete should succeed");
    })
    .await
    .expect("delete task should finish after the in-flight read completes");
    let error = timeout(Duration::from_secs(1), async {
        ensure_task
            .await
            .expect("ensure task should join")
            .expect_err("new work should fail after deletion begins")
    })
    .await
    .expect("ensure task should resolve after deletion completes");
    assert!(matches!(error, Error::TenantNotFound(_)));
}
