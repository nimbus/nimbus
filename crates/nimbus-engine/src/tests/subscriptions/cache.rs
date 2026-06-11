use super::*;

#[tokio::test]
async fn repeated_get_document_calls_record_document_cache_hits() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let document_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Cached"))]),
        )
        .expect("insert should succeed");

    let first = engine
        .get_document(&tenant_id, &tasks_table(), document_id.clone())
        .expect("first get should succeed");
    let second = engine
        .get_document(&tenant_id, &tasks_table(), document_id.clone())
        .expect("second get should succeed");

    assert_eq!(first.fields.get("title"), Some(&json!("Cached")));
    assert_eq!(second.fields.get("title"), Some(&json!("Cached")));

    let stats = engine
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);
}

#[tokio::test]
async fn document_cache_evicts_least_recently_used_entries_when_capacity_is_exceeded() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let document_ids = (0..=DOCUMENT_CACHE_CAPACITY)
        .map(|index| {
            engine
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
        engine
            .get_document(&tenant_id, &tasks_table(), document_id.clone())
            .expect("get should succeed");
    }

    let stats = engine
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, DOCUMENT_CACHE_CAPACITY + 1);
    assert_eq!(stats.entries, DOCUMENT_CACHE_CAPACITY);
    assert_eq!(stats.evictions, 1);

    engine
        .get_document(&tenant_id, &tasks_table(), document_ids[0].clone())
        .expect("evicted document should still load from storage");
    engine
        .get_document(
            &tenant_id,
            &tasks_table(),
            document_ids
                .last()
                .expect("cache population should include a last document")
                .clone(),
        )
        .expect("most recent document should stay cached");

    let stats = engine
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, DOCUMENT_CACHE_CAPACITY + 2);
    assert_eq!(stats.entries, DOCUMENT_CACHE_CAPACITY);
    assert_eq!(stats.evictions, 2);
}

#[tokio::test]
async fn query_cache_entries_are_invalidated_before_the_next_read_after_mutation() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let document_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("insert should succeed");

    let documents = timeout(
        Duration::from_secs(1),
        engine.query_documents_async(tenant_id.clone(), query_for("tasks")),
    )
    .await
    .expect("query should resolve after apply")
    .expect("query should succeed");
    assert_eq!(documents.len(), 1);

    let cached = engine
        .get_document(&tenant_id, &tasks_table(), document_id.clone())
        .expect("cached get should succeed");
    assert_eq!(cached.fields.get("title"), Some(&json!("Before")));

    let stats = engine
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);

    engine
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    let refreshed = engine
        .get_document(&tenant_id, &tasks_table(), document_id.clone())
        .expect("post-update get should succeed");
    assert_eq!(refreshed.fields.get("title"), Some(&json!("After")));

    let stats = engine
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 1);

    let cached_again = engine
        .get_document(&tenant_id, &tasks_table(), document_id.clone())
        .expect("second post-update get should succeed");
    assert_eq!(cached_again.fields.get("title"), Some(&json!("After")));

    let stats = engine
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 1);
}

#[tokio::test]
async fn direct_mutations_invalidate_document_cache_before_applied_head_is_visible() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let document_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("insert should succeed");

    let cached = engine
        .get_document(&tenant_id, &tasks_table(), document_id.clone())
        .expect("cached get should succeed");
    assert_eq!(cached.fields.get("title"), Some(&json!("Before")));

    let pause = engine
        .document_cache_invalidation_pause_handle_for_testing(&tenant_id)
        .expect("document cache invalidation pause handle should load");
    pause.arm();

    let update_task = {
        let engine = Arc::clone(&engine);
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        tokio::task::spawn_blocking(move || {
            engine.update_document(
                &tenant_id,
                tasks_table(),
                document_id,
                serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
            )
        })
    };
    let mut update_task = update_task;

    let pause_wait = pause.clone();
    let reached_pause =
        tokio::task::spawn_blocking(move || pause_wait.wait_until_entered(Duration::from_secs(1)))
            .await
            .expect("pause wait should join");
    assert!(
        reached_pause,
        "direct mutation should reach document cache invalidation before publishing applied head"
    );

    let pending_window =
        ci_or_local_duration(Duration::from_millis(100), Duration::from_millis(250));
    assert!(
        timeout(pending_window, &mut update_task).await.is_err(),
        "direct update should remain paused at cache invalidation"
    );

    let read_task = {
        let engine = Arc::clone(&engine);
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        tokio::spawn(async move {
            engine
                .get_document_async(tenant_id, tasks_table(), document_id)
                .await
        })
    };
    let mut read_task = read_task;

    match timeout(pending_window, &mut read_task).await {
        Err(_) => {}
        Ok(joined) => {
            let document = joined
                .expect("premature read task should join")
                .expect("premature read should succeed");
            panic!(
                "read completed while cache invalidation was paused with title {:?}",
                document.fields.get("title")
            );
        }
    }

    pause.release();

    let updated_id = timeout(Duration::from_secs(1), update_task)
        .await
        .expect("direct update should complete after invalidation is released")
        .expect("direct update task should join")
        .expect("direct update should succeed");
    assert_eq!(updated_id, document_id);

    let refreshed = timeout(Duration::from_secs(1), read_task)
        .await
        .expect("pending read should complete after applied head advances")
        .expect("pending read task should join")
        .expect("pending read should succeed");
    assert_eq!(refreshed.fields.get("title"), Some(&json!("After")));

    let stats = engine
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.misses, 2);
}

#[tokio::test]
async fn subscription_re_evaluation_after_mutation_sees_fresh_cached_data() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let document_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Before"))]),
        )
        .expect("insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let _subscription = engine
        .subscribe(&tenant_id, query_for("tasks"), "cache-sub".to_string(), tx)
        .expect("subscribe should succeed");

    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Before"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let cached = engine
        .get_document(&tenant_id, &tasks_table(), document_id.clone())
        .expect("cached get should succeed");
    assert_eq!(cached.fields.get("title"), Some(&json!("Before")));

    let stats = engine
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 1);
    assert_eq!(stats.misses, 0);

    engine
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect("update should succeed");

    let update = rx.recv().await.expect("subscription update should arrive");
    match update {
        SubscriptionUpdate::Result { snapshot, .. } => {
            let data = snapshot.to_json_documents();
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("After"));
        }
        other => panic!("unexpected subscription update: {other:?}"),
    }

    let refreshed = engine
        .get_document(&tenant_id, &tasks_table(), document_id.clone())
        .expect("refreshed get should succeed");
    assert_eq!(refreshed.fields.get("title"), Some(&json!("After")));

    let stats = engine
        .document_cache_stats_for_testing(&tenant_id)
        .expect("cache stats should load");
    assert_eq!(stats.hits, 2);
    assert_eq!(stats.misses, 0);
}
