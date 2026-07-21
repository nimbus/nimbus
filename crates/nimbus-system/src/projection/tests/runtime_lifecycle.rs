use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn newer_source_token_wins_even_when_local_generation_is_lower() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-generation", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    let first = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("first source row should commit");
    let second = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("second source row should commit");
    record_table_state_for_generation_async(
        &engine,
        &tenant_id,
        &tasks,
        ProjectionToken {
            tenant_incarnation: 1,
            lease_epoch: 0,
            durable_sequence: nimbus_core::SequenceNumber(2),
        },
        "projection-test-epoch",
        2,
    )
    .await
    .expect("new-generation projection should record two rows");

    engine
        .delete_document_async(tenant_id.clone(), tasks.clone(), second)
        .await
        .expect("source row should delete");
    record_table_state_for_generation_async(
        &engine,
        &tenant_id,
        &tasks,
        ProjectionToken {
            tenant_incarnation: 1,
            lease_epoch: 0,
            durable_sequence: nimbus_core::SequenceNumber(3),
        },
        "projection-test-epoch",
        1,
    )
    .await
    .expect("the newer source token should publish despite its lower local generation");
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1),
        "durable source order, not the process-local generation, owns publication legality"
    );
    engine
        .delete_document_async(tenant_id, tasks, first)
        .await
        .expect("remaining source row should delete");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn reloaded_runtime_skips_parked_old_generation_projection() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-generation-race", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    let held_projection = tenant_work(&engine, &projection_work, &tenant_id)
        .projection_lock
        .clone()
        .lock_owned()
        .await;

    for index in 0..2 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("old-generation source row should commit");
    }
    observer.project_tables(
        tenant_id.clone(),
        vec![tasks.clone()],
        ProjectionToken::default(),
    );
    projection_work.wait_until_registered(&tenant_id).await;

    engine
        .delete_tenant_async(tenant_id.clone())
        .await
        .expect("old runtime should evict while its projection is parked");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should reload with a fresh runtime generation");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(10))]),
        )
        .await
        .expect("new-generation source row should commit");
    observer.project_tables(
        tenant_id.clone(),
        vec![tasks.clone()],
        ProjectionToken::default(),
    );
    tokio::time::timeout(Duration::from_secs(5), async {
        while projection_work.stats(&tenant_id).depth != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both runtime generations should register before the lock is released");

    drop(held_projection);
    tokio::time::timeout(
        Duration::from_secs(5),
        projection_work.wait_for_idle(&tenant_id),
    )
    .await
    .expect("new-generation projection should drain");
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1),
        "the parked old generation must not overwrite the reloaded runtime's count"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn applied_wait_eviction_error_requeues_owned_scope_and_releases_lock() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-eviction-wait", Engine::create_tenant);
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    let tenant_work = tenant_work(&engine, &projection_work, &tenant_id);
    engine
        .park_applied_sequence_waiters_for_testing(&tenant_id, nimbus_core::SequenceNumber(1))
        .expect("test should expose a durable-but-unapplied target");

    observer.project_tables(
        tenant_id.clone(),
        vec![TableName::new("tasks").expect("table name should build")],
        ProjectionToken::default(),
    );
    projection_work.wait_until_registered(&tenant_id).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if tenant_work.projection_lock.try_lock().is_err() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("projection task should acquire its tenant lock");

    engine
        .fail_applied_sequence_waiters_for_testing(&tenant_id)
        .expect("test eviction should wake applied waiters");
    let retained = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stats = projection_work.stats(&tenant_id);
            if stats.depth == 0 && stats.dirty_projection_scope_count == 1 {
                break stats;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("eviction error must restore the owned scope before releasing work");
    assert_eq!(retained.depth, 0);
    assert_eq!(retained.dirty_projection_scope_count, 1);
    assert_eq!(retained.delayed_retry_count, 1);
    assert!(
        tenant_work.projection_lock.try_lock().is_ok(),
        "the tenant projection lock must be released after the typed wait error"
    );
}
