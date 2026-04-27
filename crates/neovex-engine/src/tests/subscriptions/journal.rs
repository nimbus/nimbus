use super::*;

#[tokio::test]
async fn journal_batch_delete_updates_preserve_deleted_documents_from_durable_journal() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let first_document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Task A")),
                ("status".to_string(), json!("active")),
            ]),
        )
        .expect("first fixture insert should succeed");
    let second_document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Task B")),
                ("status".to_string(), json!("active")),
            ]),
        )
        .expect("second fixture insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe(
            &tenant_id,
            Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
            "batch-delete".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let initial = rx
        .recv()
        .await
        .expect("initial subscription update should arrive");
    match initial {
        SubscriptionUpdate::Result { snapshot, .. } => {
            assert_eq!(snapshot.to_json_documents().len(), 2)
        }
        other => panic!("unexpected initial subscription update: {other:?}"),
    }

    let pause = service
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let first_delete = {
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            service
                .delete_document_async(tenant_id, tasks_table(), first_document_id)
                .await
        })
    };

    let pause_wait = pause.clone();
    assert!(
        tokio::task::spawn_blocking(move || pause_wait.wait_until_entered(Duration::from_secs(1)))
            .await
            .expect("pause wait should join"),
        "journal worker should pause before applying the queued delete batch"
    );

    let second_delete = {
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            service
                .delete_document_async(tenant_id, tasks_table(), second_document_id)
                .await
        })
    };

    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
    pause.release();

    timeout(Duration::from_secs(1), async {
        first_delete
            .await
            .expect("first delete task should join")
            .expect("first delete should succeed");
        second_delete
            .await
            .expect("second delete task should join")
            .expect("second delete should succeed");
    })
    .await
    .expect("queued deletes should complete once the journal worker is released");

    let update = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("coalesced delete subscription update should arrive")
        .expect("subscription channel should remain open");
    match update {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert!(
                snapshot.commit.is_none(),
                "multi-commit coalesced deliveries should omit per-commit metadata"
            );
            assert!(
                data.is_empty(),
                "the active query should be empty after both deletes"
            );
            let titles = snapshot
                .deleted_documents
                .into_iter()
                .map(|document| {
                    document
                        .fields
                        .get("title")
                        .and_then(|value| value.as_str())
                        .expect("deleted document should retain its title")
                        .to_string()
                })
                .collect::<BTreeSet<_>>();
            assert_eq!(
                titles,
                BTreeSet::from(["Task A".to_string(), "Task B".to_string()])
            );
        }
        other => panic!("unexpected coalesced delete update: {other:?}"),
    }
}

#[tokio::test]
async fn service_does_not_fail_committed_mutation_when_subscription_re_evaluation_errors() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .insert_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("rank".to_string(), json!("1"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: Some(neovex_core::OrderBy {
            field: "rank".to_string(),
            direction: neovex_core::OrderDirection::Asc,
        }),
        limit: None,
    };

    let _subscription = service
        .subscribe(&tenant_id, query, "req-3".to_string(), tx)
        .expect("subscribe should succeed");
    let _ = rx
        .recv()
        .await
        .expect("initial subscription result should arrive");

    let result = service.insert_document(
        &tenant_id,
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
    );
    assert!(result.is_ok(), "committed mutation should still succeed");

    let event = rx
        .recv()
        .await
        .expect("subscription error event should arrive");
    match event {
        SubscriptionUpdate::Error { message, .. } => {
            assert!(message.contains("ordering cannot mix string and number values"));
        }
        other => panic!("unexpected subscription event: {other:?}"),
    }

    service
        .update_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            result.expect("insert should return document id"),
            serde_json::Map::from_iter([("rank".to_string(), json!("2"))]),
        )
        .expect("repair update should succeed");

    let recovered = rx
        .recv()
        .await
        .expect("recovered subscription result should arrive");
    match recovered {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(data.len(), 2);
        }
        other => panic!("unexpected recovered subscription event: {other:?}"),
    }
}
