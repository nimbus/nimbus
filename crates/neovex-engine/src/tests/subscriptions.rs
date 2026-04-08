use super::*;

#[tokio::test]
async fn service_insert_drives_subscription_updates() {
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
        .subscribe(&tenant_id, query, "req-1".to_string(), tx)
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert_eq!(request_id.as_deref(), Some("req-1"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    service
        .insert_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        )
        .expect("insert should succeed");

    let update = rx.recv().await.expect("reactive update should arrive");
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
            assert_eq!(data[0]["title"], json!("Hello"));
        }
        other => panic!("unexpected reactive update: {other:?}"),
    }
}

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
        SubscriptionUpdate::Result { data, .. } => assert_eq!(data.len(), 2),
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
        SubscriptionUpdate::Result {
            commit,
            deleted_documents,
            data,
            ..
        } => {
            assert!(
                commit.is_none(),
                "multi-commit coalesced deliveries should omit per-commit metadata"
            );
            assert!(
                data.is_empty(),
                "the active query should be empty after both deletes"
            );
            let titles = deleted_documents
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
async fn service_update_and_delete_drive_subscription_updates() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    let subscription = service
        .subscribe(&tenant_id, query, "req-2".to_string(), tx)
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Before"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    service
        .update_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    let updated = rx.recv().await.expect("update should arrive");
    match updated {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert!(request_id.is_none());
            assert_eq!(data[0]["title"], json!("After"));
        }
        other => panic!("unexpected update subscription event: {other:?}"),
    }

    service
        .delete_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            document_id,
        )
        .expect("delete should succeed");

    let deleted = rx.recv().await.expect("delete should arrive");
    match deleted {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert_eq!(data, Vec::<serde_json::Value>::new());
        }
        other => panic!("unexpected delete subscription event: {other:?}"),
    }
}

#[tokio::test]
async fn repeated_get_document_calls_record_document_cache_hits() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Cached"))]),
        )
        .expect("insert should succeed");

    let first = service
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("first get should succeed");
    let second = service
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("second get should succeed");

    assert_eq!(first.fields.get("title"), Some(&json!("Cached")));
    assert_eq!(second.fields.get("title"), Some(&json!("Cached")));

    let stats = service
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
}

#[tokio::test]
async fn document_cache_evicts_least_recently_used_entries_when_capacity_is_exceeded() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_ids = (0..=DOCUMENT_CACHE_CAPACITY)
        .map(|index| {
            service
                .insert_document(
                    &tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!(format!("Task {index}")),
                    )]),
                )
                .expect("insert should succeed")
        })
        .collect::<Vec<_>>();

    for document_id in &document_ids {
        service
            .get_document(&tenant_id, &tasks_table(), *document_id)
            .expect("get should succeed");
    }

    let stats = service
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, DOCUMENT_CACHE_CAPACITY + 1);
    assert_eq!(stats.entries, DOCUMENT_CACHE_CAPACITY);
    assert_eq!(stats.evictions, 1);

    service
        .get_document(&tenant_id, &tasks_table(), document_ids[0])
        .expect("evicted document should still load from storage");
    service
        .get_document(
            &tenant_id,
            &tasks_table(),
            *document_ids
                .last()
                .expect("cache population should include a last document"),
        )
        .expect("most recent document should stay cached");

    let stats = service
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, DOCUMENT_CACHE_CAPACITY + 2);
    assert_eq!(stats.entries, DOCUMENT_CACHE_CAPACITY);
    assert_eq!(stats.evictions, 2);
}

#[tokio::test]
async fn query_cache_entries_are_invalidated_before_the_next_read_after_mutation() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("insert should succeed");

    let documents = timeout(
        Duration::from_secs(1),
        service.query_documents_async(tenant_id.clone(), query_for("tasks")),
    )
    .await
    .expect("query should resolve after apply")
    .expect("query should succeed");
    assert_eq!(documents.len(), 1);

    let cached = service
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("cached get should succeed");
    assert_eq!(cached.fields.get("title"), Some(&json!("Before")));

    let stats = service
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    let refreshed = service
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("post-update get should succeed");
    assert_eq!(refreshed.fields.get("title"), Some(&json!("After")));

    let stats = service
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);

    let cached_again = service
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("second post-update get should succeed");
    assert_eq!(cached_again.fields.get("title"), Some(&json!("After")));

    let stats = service
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 1);
}

#[tokio::test]
async fn subscription_re_evaluation_after_mutation_sees_fresh_cached_data() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe(&tenant_id, query_for("tasks"), "cache-sub".to_string(), tx)
        .expect("subscribe should succeed");

    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Before"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let cached = service
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("cached get should succeed");
    assert_eq!(cached.fields.get("title"), Some(&json!("Before")));

    let stats = service
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    let update = rx.recv().await.expect("subscription update should arrive");
    match update {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("After"));
        }
        other => panic!("unexpected subscription update: {other:?}"),
    }

    let refreshed = service
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("refreshed get should succeed");
    assert_eq!(refreshed.fields.get("title"), Some(&json!("After")));

    let stats = service
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 0);
}

#[tokio::test]
async fn slow_subscription_channels_are_dropped_instead_of_growing_unbounded() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("insert should succeed");

    let (tx, mut rx) = mpsc::channel::<SubscriptionUpdate>(1);
    let _subscription = service
        .subscribe(&tenant_id, query_for("tasks"), "slow-sub".to_string(), tx)
        .expect("subscribe should succeed");

    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        1
    );

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    timeout(Duration::from_secs(1), async {
        loop {
            if service
                .active_subscription_count(&tenant_id)
                .expect("subscription count should load")
                == 0
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("slow subscription should still be dropped after async delivery attempts");

    let initial = rx
        .recv()
        .await
        .expect("initial update should still be buffered");
    match initial {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Before"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
            | Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
    ));
}

#[tokio::test]
async fn service_only_notifies_subscriptions_for_affected_tables() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tasks_tx, mut tasks_rx) = subscription_channel();
    let (users_tx, mut users_rx) = subscription_channel();
    let tasks_query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };
    let users_query = Query {
        table: TableName::new("users").expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    };

    let _tasks_subscription = service
        .subscribe(&tenant_id, tasks_query, "tasks-1".to_string(), tasks_tx)
        .expect("tasks subscribe should succeed");
    let _users_subscription = service
        .subscribe(&tenant_id, users_query, "users-1".to_string(), users_tx)
        .expect("users subscribe should succeed");

    let _ = tasks_rx
        .recv()
        .await
        .expect("tasks initial update should arrive");
    let _ = users_rx
        .recv()
        .await
        .expect("users initial update should arrive");

    service
        .insert_document(
            &tenant_id,
            TableName::new("tasks").expect("table name should be valid"),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        )
        .expect("insert should succeed");

    let tasks_update = tasks_rx.recv().await.expect("tasks update should arrive");
    match tasks_update {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Hello"));
        }
        other => panic!("unexpected tasks subscription event: {other:?}"),
    }

    let users_update = timeout(Duration::from_millis(100), users_rx.recv()).await;
    assert!(
        users_update.is_err(),
        "users subscription should not be invalidated"
    );
}

#[tokio::test]
async fn service_insert_only_notifies_filtered_subscriptions_for_matching_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (active_tx, mut active_rx) = subscription_channel();
    let (done_tx, mut done_rx) = subscription_channel();
    let active_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("active"))],
        order: None,
        limit: None,
    };
    let done_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("done"))],
        order: None,
        limit: None,
    };

    let _active_subscription = service
        .subscribe(&tenant_id, active_query, "active-1".to_string(), active_tx)
        .expect("active subscribe should succeed");
    let _done_subscription = service
        .subscribe(&tenant_id, done_query, "done-1".to_string(), done_tx)
        .expect("done subscribe should succeed");

    let _ = active_rx
        .recv()
        .await
        .expect("active initial update should arrive");
    let _ = done_rx
        .recv()
        .await
        .expect("done initial update should arrive");

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Ship it")),
                ("status".to_string(), json!("active")),
            ]),
        )
        .expect("insert should succeed");

    let active_update = active_rx.recv().await.expect("active update should arrive");
    match active_update {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Ship it"));
        }
        other => panic!("unexpected active subscription event: {other:?}"),
    }

    let done_update = timeout(Duration::from_millis(100), done_rx.recv()).await;
    assert!(
        done_update.is_err(),
        "non-matching filtered subscription should not be invalidated"
    );
}

#[tokio::test]
async fn service_delete_only_notifies_filtered_subscriptions_for_matching_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let active_document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Keep moving")),
                ("status".to_string(), json!("active")),
            ]),
        )
        .expect("active seed insert should succeed");
    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Archive")),
                ("status".to_string(), json!("done")),
            ]),
        )
        .expect("done seed insert should succeed");

    let (active_tx, mut active_rx) = subscription_channel();
    let (done_tx, mut done_rx) = subscription_channel();
    let active_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("active"))],
        order: None,
        limit: None,
    };
    let done_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("done"))],
        order: None,
        limit: None,
    };

    let _active_subscription = service
        .subscribe(
            &tenant_id,
            active_query,
            "active-delete".to_string(),
            active_tx,
        )
        .expect("active subscribe should succeed");
    let _done_subscription = service
        .subscribe(&tenant_id, done_query, "done-delete".to_string(), done_tx)
        .expect("done subscribe should succeed");

    let _ = active_rx
        .recv()
        .await
        .expect("active initial update should arrive");
    let _ = done_rx
        .recv()
        .await
        .expect("done initial update should arrive");

    service
        .delete_document(&tenant_id, tasks_table(), active_document_id)
        .expect("delete should succeed");

    let active_update = active_rx
        .recv()
        .await
        .expect("active delete update should arrive");
    match active_update {
        SubscriptionUpdate::Result {
            data,
            deleted_documents,
            ..
        } => {
            assert!(data.is_empty());
            assert_eq!(deleted_documents.len(), 1);
            assert_eq!(
                deleted_documents[0].fields.get("status"),
                Some(&json!("active"))
            );
        }
        other => panic!("unexpected active delete subscription event: {other:?}"),
    }

    let done_update = timeout(Duration::from_millis(100), done_rx.recv()).await;
    assert!(
        done_update.is_err(),
        "deleting a non-matching document should not invalidate the other filter"
    );
}

#[tokio::test]
async fn service_updates_remain_conservative_for_filtered_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let active_document_id = service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Before")),
                ("status".to_string(), json!("active")),
            ]),
        )
        .expect("seed insert should succeed");

    let (active_tx, mut active_rx) = subscription_channel();
    let (done_tx, mut done_rx) = subscription_channel();
    let active_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("active"))],
        order: None,
        limit: None,
    };
    let done_query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("done"))],
        order: None,
        limit: None,
    };

    let _active_subscription = service
        .subscribe(
            &tenant_id,
            active_query,
            "active-update".to_string(),
            active_tx,
        )
        .expect("active subscribe should succeed");
    let _done_subscription = service
        .subscribe(&tenant_id, done_query, "done-update".to_string(), done_tx)
        .expect("done subscribe should succeed");

    let _ = active_rx
        .recv()
        .await
        .expect("active initial update should arrive");
    let _ = done_rx
        .recv()
        .await
        .expect("done initial update should arrive");

    service
        .update_document(
            &tenant_id,
            tasks_table(),
            active_document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    let active_update = active_rx.recv().await.expect("active update should arrive");
    match active_update {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("After"));
        }
        other => panic!("unexpected active update subscription event: {other:?}"),
    }

    let done_update = done_rx
        .recv()
        .await
        .expect("done update should still arrive");
    match done_update {
        SubscriptionUpdate::Result { data, .. } => {
            assert!(data.is_empty());
        }
        other => panic!("unexpected done update subscription event: {other:?}"),
    }
}

#[tokio::test]
async fn service_limited_subscriptions_skip_out_of_window_ordered_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    for rank in [1, 2, 3] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("title".to_string(), json!(format!("Task {rank}"))),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("seed insert should succeed");
    }

    let (tx, mut rx) = subscription_channel();
    let query = Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: Some(2),
    };

    let _subscription = service
        .subscribe(&tenant_id, query, "ranked-limit".to_string(), tx)
        .expect("subscribe should succeed");

    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 2);
            assert_eq!(data[0]["rank"], json!(1));
            assert_eq!(data[1]["rank"], json!(2));
        }
        other => panic!("unexpected initial subscription update: {other:?}"),
    }

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Task 99")),
                ("rank".to_string(), json!(99)),
            ]),
        )
        .expect("outside-window insert should succeed");
    assert!(
        timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err(),
        "writes beyond the visible ordered window should not invalidate the subscription"
    );

    let document_id = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Eq, json!(2))],
                order: None,
                limit: Some(1),
            },
        )
        .expect("rank lookup should succeed")
        .first()
        .map(|document| document.id)
        .expect("rank-2 document should exist");
    service
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("rank".to_string(), json!(5))]),
        )
        .expect("window-shifting update should succeed");

    let shifted = rx.recv().await.expect("window shift update should arrive");
    match shifted {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 2);
            assert_eq!(data[0]["rank"], json!(1));
            assert_eq!(data[1]["rank"], json!(3));
        }
        other => panic!("unexpected shifted subscription update: {other:?}"),
    }

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Task 4")),
                ("rank".to_string(), json!(4)),
            ]),
        )
        .expect("second outside-window insert should succeed");
    assert!(
        timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err(),
        "dependency tracking should refresh after reevaluation and keep skipping later out-of-window writes"
    );

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!("Task 0")),
                ("rank".to_string(), json!(0)),
            ]),
        )
        .expect("inside-window insert should succeed");

    let refreshed = rx.recv().await.expect("inside-window update should arrive");
    match refreshed {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 2);
            assert_eq!(data[0]["rank"], json!(0));
            assert_eq!(data[1]["rank"], json!(1));
        }
        other => panic!("unexpected refreshed subscription update: {other:?}"),
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
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 2);
        }
        other => panic!("unexpected recovered subscription event: {other:?}"),
    }
}

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

    let delete_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move { service.delete_tenant_async(tenant_id).await }
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(
        !delete_task.is_finished(),
        "tenant deletion should wait for the in-flight operation"
    );

    let ensure_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move { service.ensure_tenant_exists_async(tenant_id).await }
    });
    tokio::time::sleep(Duration::from_millis(25)).await;
    assert!(
        !ensure_task.is_finished(),
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
