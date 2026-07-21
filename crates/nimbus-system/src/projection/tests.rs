use std::time::Duration;

use nimbus_testing::EngineFixture;
use serde_json::json;

use super::*;

fn test_observer(
    engine: &Arc<Engine>,
    capacity: usize,
    high_watermark: usize,
) -> (Arc<TableProjectionObserver>, Arc<ProjectionWork>) {
    let projection_work = Arc::new(ProjectionWork::new(engine, capacity, high_watermark));
    (
        Arc::new(TableProjectionObserver {
            projection_work: projection_work.clone(),
        }),
        projection_work,
    )
}

fn tenant_work(
    engine: &Engine,
    projection_work: &Arc<ProjectionWork>,
    tenant_id: &TenantId,
) -> Arc<TenantProjectionWork> {
    projection_work.tenant_work(
        tenant_id,
        engine
            .committed_mutation_observer_runtime_identity(tenant_id)
            .expect("tenant runtime identity should load"),
    )
}

async fn projected_table_row_count(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Option<u64> {
    let rows = match engine
        .list_documents_async(
            crate::system_tenant_id().expect("system tenant id should build"),
            crate::schema::SystemTable::Tables
                .table_name()
                .expect("system tables name should build"),
        )
        .await
    {
        Ok(rows) => rows,
        // The system tenant is seeded by the first projection, so its
        // absence means nothing has been projected yet.
        Err(nimbus_core::Error::TenantNotFound(_)) => return None,
        Err(error) => panic!("projected table records should list: {error}"),
    };
    rows.into_iter()
        .find(|row| {
            row.fields.get("tenantId") == Some(&json!(tenant_id.as_str()))
                && row.fields.get("name") == Some(&json!(table.as_str()))
        })
        .and_then(|row| {
            row.fields
                .get("rowCount")
                .and_then(serde_json::Value::as_u64)
        })
}

async fn projected_table_source_token(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Option<ProjectionToken> {
    let rows = match engine
        .list_documents_async(
            crate::system_tenant_id().expect("system tenant id should build"),
            crate::schema::SystemTable::Tables
                .table_name()
                .expect("system tables name should build"),
        )
        .await
    {
        Ok(rows) => rows,
        Err(nimbus_core::Error::TenantNotFound(_)) => return None,
        Err(error) => panic!("projected table records should list: {error}"),
    };
    rows.into_iter()
        .find(|row| {
            row.fields.get("tenantId") == Some(&json!(tenant_id.as_str()))
                && row.fields.get("name") == Some(&json!(table.as_str()))
        })
        .map(|row| ProjectionToken {
            tenant_incarnation: row
                .fields
                .get("sourceTenantIncarnation")
                .and_then(serde_json::Value::as_u64)
                .expect("projected row should carry a numeric source tenant incarnation"),
            lease_epoch: row
                .fields
                .get("sourceLeaseEpoch")
                .and_then(serde_json::Value::as_u64)
                .expect("projected row should carry a numeric source lease epoch"),
            durable_sequence: nimbus_core::SequenceNumber(
                row.fields
                    .get("sourceDurableSequence")
                    .and_then(serde_json::Value::as_u64)
                    .expect("projected row should carry a numeric source durable sequence"),
            ),
        })
}

#[test]
fn projection_dirty_scope_retains_maximum_source_token() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-token-max", Engine::create_tenant);
    let projection_work = Arc::new(ProjectionWork::new(&engine, 2, 1));
    let tenant_work = tenant_work(&engine, &projection_work, &tenant_id);
    let tasks = TableName::new("tasks").expect("table name should build");
    let earlier_epoch = ProjectionToken {
        tenant_incarnation: 1,
        lease_epoch: 7,
        durable_sequence: nimbus_core::SequenceNumber(500),
    };
    let later_epoch = ProjectionToken {
        tenant_incarnation: 1,
        lease_epoch: 8,
        durable_sequence: nimbus_core::SequenceNumber(1),
    };
    tenant_work.mark_scopes_dirty(
        &[(tasks.clone(), later_epoch)],
        &projection_work.dirty_tenants,
    );
    tenant_work.mark_scopes_dirty(
        &[(tasks.clone(), earlier_epoch)],
        &projection_work.dirty_tenants,
    );

    let dirty = tenant_work
        .dirty_tables
        .lock()
        .expect("projection dirty-table lock should not be poisoned");
    assert_eq!(dirty.len(), 1);
    assert_eq!(dirty.get(&tasks), Some(&later_epoch));
    assert_eq!(projection_work.dirty_tenants.load(Ordering::Acquire), 1);
    drop(dirty);

    let later_sequence = ProjectionToken {
        tenant_incarnation: 1,
        lease_epoch: 8,
        durable_sequence: nimbus_core::SequenceNumber(2),
    };
    tenant_work.mark_scopes_dirty(
        &[(tasks.clone(), later_sequence)],
        &projection_work.dirty_tenants,
    );
    tenant_work.mark_scopes_dirty(
        &[(
            tasks.clone(),
            ProjectionToken {
                tenant_incarnation: 1,
                lease_epoch: 8,
                durable_sequence: nimbus_core::SequenceNumber(0),
            },
        )],
        &projection_work.dirty_tenants,
    );
    assert_eq!(
        tenant_work
            .dirty_tables
            .lock()
            .expect("projection dirty-table lock should not be poisoned")
            .get(&tasks),
        Some(&later_sequence)
    );
}

#[test]
fn projection_callback_without_entered_tokio_runtime_uses_engine_executor() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id =
        fixture.create_tenant("projection-engine-owned-executor", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    engine
        .insert_document(
            &tenant_id,
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .expect("source row should commit before observer installation");
    let (observer, projection_work) = test_observer(&engine, 16, 12);

    // This synchronous callback intentionally runs without entering any Tokio
    // runtime. The observer must use the engine-owned executor rather than
    // discarding a durable projection event.
    observer.project_tables(
        tenant_id.clone(),
        vec![tasks.clone()],
        ProjectionToken::default(),
    );

    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("verification runtime should build")
        .block_on(async {
            tokio::time::timeout(
                Duration::from_secs(5),
                projection_work.wait_for_idle(&tenant_id),
            )
            .await
            .expect("engine-owned observer work should drain");
            assert_eq!(
                projected_table_row_count(&engine, &tenant_id, &tasks).await,
                Some(1),
                "a synchronous observer callback must still land its projection"
            );
        });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_restore_refreshes_schema_and_enqueues_restored_tables() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    engine.install_committed_mutation_observer("restore-projection-test", observer.clone());
    engine.install_table_schema_change_observer("restore-projection-test", observer);

    let tasks = TableName::new("tasks").expect("table name should build");
    let source = fixture.create_tenant("projection-restore-source", Engine::create_tenant);
    engine
        .set_table_schema_async(
            source.clone(),
            nimbus_core::TableSchema {
                table: tasks.clone(),
                fields: vec![nimbus_core::FieldSchema {
                    name: "title".to_string(),
                    field_type: nimbus_core::FieldType::String,
                    required: true,
                }],
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .await
        .expect("restore source schema should commit");
    engine
        .insert_document_async(
            source.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("restored"))]),
        )
        .await
        .expect("restore source document should commit");
    let archive = engine
        .export_latest_point_in_time_restore_archive(&source)
        .expect("restore source archive should export");
    let destination =
        fixture.create_tenant("projection-restore-destination", Engine::create_tenant);
    engine
        .import_point_in_time_restore_archive(&destination, &archive)
        .expect("restore archive should import");
    engine
        .flush_committed_mutation_observers_for_testing(&destination)
        .await
        .expect("restored table projection should flush");

    assert!(
        engine
            .get_table_schema_async(destination.clone(), tasks.clone())
            .await
            .is_ok(),
        "the loaded destination runtime must adopt the restored schema snapshot"
    );
    assert_eq!(
        projected_table_row_count(&engine, &destination, &tasks).await,
        Some(1),
        "restore completion must enqueue every restored table scope"
    );
    assert_eq!(
        projection_work
            .stats(&destination)
            .dirty_projection_scope_count,
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_ordinary_failure_requeues_owned_scope() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-ordinary-retry", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("source row should commit");
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    projection_work.fail_next_projections(1);

    observer.project_tables(
        tenant_id.clone(),
        vec![tasks.clone()],
        ProjectionToken::default(),
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        projection_work.wait_for_idle(&tenant_id),
    )
    .await
    .expect("ordinary projection retry should drain");

    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1)
    );
    let stats = projection_work.stats(&tenant_id);
    assert_eq!(stats.delayed_retry_count, 1);
    assert_eq!(stats.catch_up_projection_count, 1);
    assert_eq!(stats.dirty_projection_scope_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_lease_contention_uses_delayed_bounded_retry() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-lease-retry", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("source row should commit");
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    projection_work.contend_next_projections(2);
    engine.install_committed_mutation_observer("projection-lease-retry-test", observer.clone());

    observer.project_tables(
        tenant_id.clone(),
        vec![tasks.clone()],
        ProjectionToken::default(),
    );
    let first_retry = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stats = projection_work.stats(&tenant_id);
            if stats.delayed_retry_count == 2 {
                break stats;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both injected lease contentions should reach delayed retry");
    assert!(first_retry.dirty_projection_scope_count != 0 || first_retry.depth != 0);
    assert!(first_retry.catch_up_projection_count <= 1);
    assert!(first_retry.current_retry_backoff_millis >= 50);
    let diagnostics = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("projection retry diagnostics should load")
        .mutation_journal;
    assert_eq!(
        diagnostics.observer_spawned_work_dirty_scope_count,
        first_retry.dirty_projection_scope_count
    );
    assert_eq!(diagnostics.observer_spawned_work_delayed_retry_count, 2);
    assert!(diagnostics.observer_spawned_work_consecutive_failure_count >= 2);
    assert!(diagnostics.observer_spawned_work_current_retry_backoff_millis >= 100);

    tokio::time::timeout(
        Duration::from_secs(5),
        projection_work.wait_for_idle(&tenant_id),
    )
    .await
    .expect("lease contention should retain and eventually land the scope");
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1)
    );
    let recovered = projection_work.stats(&tenant_id);
    assert_eq!(recovered.delayed_retry_count, 2);
    assert_eq!(recovered.consecutive_failure_count, 0);
    assert_eq!(recovered.dirty_projection_scope_count, 0);
}

#[test]
fn projection_runtime_reconciliation_retry_diagnostics_clear_current_backoff() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-reconcile-stats", Engine::create_tenant);
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    engine.install_committed_mutation_observer("projection-reconcile-stats-test", observer);
    let identity = engine
        .committed_mutation_observer_runtime_identity(&tenant_id)
        .expect("tenant runtime identity should load");

    assert!(projection_work.record_reconciliation_retry(
        &tenant_id,
        &identity,
        Duration::from_millis(200),
    ));
    let retrying = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("reconciliation diagnostics should load")
        .mutation_journal;
    assert_eq!(retrying.observer_spawned_work_reconciliation_retry_count, 1);
    assert_eq!(
        retrying.observer_spawned_work_current_reconciliation_backoff_millis,
        200
    );

    projection_work.finish_reconciliation(&tenant_id, &identity);
    let recovered = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("recovered diagnostics should load")
        .mutation_journal;
    assert_eq!(
        recovered.observer_spawned_work_reconciliation_retry_count,
        1
    );
    assert_eq!(
        recovered.observer_spawned_work_current_reconciliation_backoff_millis,
        0
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_task_cancellation_requeues_owned_scope() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-cancel-retry", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("source row should commit");
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    projection_work.cancel_next_projection();

    observer.project_tables(
        tenant_id.clone(),
        vec![tasks.clone()],
        ProjectionToken::default(),
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        projection_work.wait_for_idle(&tenant_id),
    )
    .await
    .expect("cancelled projection retry should drain");

    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1)
    );
    let stats = projection_work.stats(&tenant_id);
    assert_eq!(stats.delayed_retry_count, 1);
    assert_eq!(stats.catch_up_projection_count, 1);
    assert_eq!(stats.dirty_projection_scope_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_observer_cancellation_preserves_newer_token() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-cancel-token", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("source row should commit");
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    engine.install_committed_mutation_observer(
        "projection-cancel-token-diagnostics",
        observer.clone(),
    );
    let initial = ProjectionToken {
        tenant_incarnation: 1,
        lease_epoch: 7,
        durable_sequence: nimbus_core::SequenceNumber(10),
    };
    observer.project_tables(tenant_id.clone(), vec![tasks.clone()], initial);
    tokio::time::timeout(
        Duration::from_secs(5),
        projection_work.wait_for_idle(&tenant_id),
    )
    .await
    .expect("initial projection should drain");

    let newer = ProjectionToken {
        tenant_incarnation: 1,
        lease_epoch: 7,
        durable_sequence: nimbus_core::SequenceNumber(12),
    };
    let older = ProjectionToken {
        tenant_incarnation: 1,
        lease_epoch: 7,
        durable_sequence: nimbus_core::SequenceNumber(11),
    };
    projection_work.cancel_next_projection();
    observer.project_tables(tenant_id.clone(), vec![tasks.clone()], newer);
    observer.project_tables(tenant_id.clone(), vec![tasks.clone()], older);
    assert_eq!(
        projection_work.stats(&tenant_id).token_lag_scope_count,
        1,
        "one table scope should report token lag until the cancelled newer token lands"
    );

    tokio::time::timeout(
        Duration::from_secs(5),
        projection_work.wait_for_idle(&tenant_id),
    )
    .await
    .expect("cancelled newer projection should remain owned until it lands");
    assert_eq!(
        projected_table_source_token(&engine, &tenant_id, &tasks).await,
        Some(newer),
        "an older accepted observer must not replace the cancelled newer token"
    );
    let recovered = projection_work.stats(&tenant_id);
    assert_eq!(recovered.token_lag_scope_count, 0);
    assert_eq!(recovered.delayed_retry_count, 1);

    observer.project_tables(tenant_id.clone(), vec![tasks.clone()], older);
    tokio::time::timeout(
        Duration::from_secs(5),
        projection_work.wait_for_idle(&tenant_id),
    )
    .await
    .expect("stale replay should drain as an idempotent no-op");
    let stale = projection_work.stats(&tenant_id);
    assert_eq!(stale.stale_no_op_count, 1);
    assert_eq!(stale.token_lag_scope_count, 0);
    let diagnostics = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("projection diagnostics should load")
        .mutation_journal;
    assert_eq!(diagnostics.observer_spawned_work_stale_no_op_count, 1);
    assert_eq!(diagnostics.observer_spawned_work_token_lag_scope_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_diagnostics_evict_completed_token_frontiers() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-frontier-eviction", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    let notes = TableName::new("notes").expect("table name should build");
    for table in [&tasks, &notes] {
        engine
            .insert_document_async(
                tenant_id.clone(),
                table.clone(),
                serde_json::Map::from_iter([("value".to_string(), json!(table.as_str()))]),
            )
            .await
            .expect("source row should commit");
    }

    let (observer, projection_work) = test_observer(&engine, 16, 12);
    let token = engine
        .projection_token_for_tenant_async(&tenant_id)
        .await
        .expect("projection token should resolve");
    observer.project_tables(tenant_id.clone(), vec![tasks.clone(), notes.clone()], token);
    tokio::time::timeout(
        Duration::from_secs(5),
        projection_work.wait_for_idle(&tenant_id),
    )
    .await
    .expect("table projections should drain");

    assert_eq!(projection_work.stats(&tenant_id).token_lag_scope_count, 0);
    let work = tenant_work(&engine, &projection_work, &tenant_id);
    assert!(
        work.token_frontiers
            .lock()
            .expect("projection token-frontier lock should not be poisoned")
            .is_empty(),
        "completed diagnostic frontiers must not retain historical table names"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn committed_observer_flush_waits_for_spawned_projection_tail() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-flush-tail", Engine::create_tenant);
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    let held_projection = tenant_work(&engine, &projection_work, &tenant_id)
        .projection_lock
        .clone()
        .lock_owned()
        .await;
    engine.install_committed_mutation_observer("projection-flush-tail-test", observer);

    engine
        .insert_document_async(
            tenant_id.clone(),
            TableName::new("tasks").expect("table name should build"),
            serde_json::Map::from_iter([("title".to_string(), json!("seed"))]),
        )
        .await
        .expect("seed insert should commit");
    projection_work.wait_until_registered(&tenant_id).await;

    let mut flush = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .flush_committed_mutation_observers_for_testing(&tenant_id)
                .await
        }
    });
    projection_work.wait_until_flush_waits(&tenant_id).await;
    assert!(
        !flush.is_finished(),
        "the observer channel fence must not overtake registered projection work"
    );

    drop(held_projection);
    tokio::time::timeout(Duration::from_secs(5), &mut flush)
        .await
        .expect("projection tail should drain")
        .expect("flush task should join")
        .expect("observer flush should succeed");
    assert_eq!(projection_work.stats(&tenant_id).depth, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_cap_drops_are_tenant_scoped_and_reset_on_runtime_reload() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = fixture.create_tenant("projection-work-cap-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("projection-work-cap-b", Engine::create_tenant);
    let (observer, projection_work) = test_observer(&engine, 2, 1);
    let held_projection = tenant_work(&engine, &projection_work, &tenant_a)
        .projection_lock
        .clone()
        .lock_owned()
        .await;
    engine.install_committed_mutation_observer("projection-work-cap-test", observer);

    for index in 0..4 {
        engine
            .insert_document_async(
                tenant_a.clone(),
                TableName::new("tasks").expect("table name should build"),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("projection saturation must not block durable mutation responses");
    }

    let stats = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stats = engine
                .tenant_engine_diagnostics(&tenant_a)
                .expect("projection diagnostics should load")
                .mutation_journal;
            if stats.observer_spawned_work_dropped_event_count == 2 {
                break stats;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("projection cap policy should engage");
    assert_eq!(stats.observer_spawned_work_depth, 2);
    assert_eq!(stats.observer_spawned_work_capacity, 2);
    assert_eq!(stats.observer_spawned_work_high_watermark, 1);
    assert_eq!(stats.observer_spawned_work_high_water_warning_count, 1);
    assert_eq!(stats.observer_spawned_work_cap_breach_count, 2);
    assert_eq!(stats.observer_spawned_work_dropped_event_count, 2);
    assert!(!stats.observer_spawned_work_poisoned);

    let tasks = TableName::new("tasks").expect("table name should build");
    engine
        .insert_document_async(
            tenant_b.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(10))]),
        )
        .await
        .expect("tenant B mutation should remain healthy");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_b)
        .await
        .expect("tenant B projection should drain independently");
    assert_eq!(
        projected_table_row_count(&engine, &tenant_b, &tasks).await,
        Some(1),
        "tenant B must continue projecting while tenant A is saturated"
    );
    let tenant_b_stats = engine
        .tenant_engine_diagnostics(&tenant_b)
        .expect("tenant B diagnostics should load")
        .mutation_journal;
    assert_eq!(tenant_b_stats.observer_spawned_work_depth, 0);
    assert_eq!(tenant_b_stats.observer_spawned_work_cap_breach_count, 0);
    assert_eq!(tenant_b_stats.observer_spawned_work_dropped_event_count, 0);
    assert!(!tenant_b_stats.observer_spawned_work_poisoned);

    drop(held_projection);
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_a)
        .await
        .expect("accepted projection work should drain after the cap breach");
    assert_eq!(projection_work.stats(&tenant_a).depth, 0);

    engine
        .delete_tenant_async(tenant_a.clone())
        .await
        .expect("saturated tenant should delete");
    engine
        .create_tenant_async(tenant_a.clone())
        .await
        .expect("tenant should recreate with a fresh runtime");
    engine
        .insert_document_async(
            tenant_a.clone(),
            tasks,
            serde_json::Map::from_iter([("index".to_string(), json!(20))]),
        )
        .await
        .expect("fresh tenant runtime should accept projection work");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_a)
        .await
        .expect("fresh runtime projection should drain");
    let reloaded = projection_work.stats(&tenant_a);
    assert_eq!(reloaded.depth, 0);
    assert_eq!(reloaded.cap_breach_count, 0);
    assert_eq!(reloaded.dropped_event_count, 0);
    assert!(!reloaded.poisoned);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_cap_breach_resumes_projecting_after_in_flight_work_drains() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-cap-recovery", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    let (observer, projection_work) = test_observer(&engine, 2, 1);
    let held_projection = tenant_work(&engine, &projection_work, &tenant_id)
        .projection_lock
        .clone()
        .lock_owned()
        .await;
    engine.install_committed_mutation_observer("projection-cap-recovery-test", observer);

    for index in 0..4 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("projection saturation must not block durable mutation responses");
    }
    let breached = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stats = projection_work.stats(&tenant_id);
            if stats.dropped_event_count == 2 {
                break stats;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the per-tenant cap should drop both events past its capacity");
    assert_eq!(breached.depth, 2);
    assert_eq!(breached.cap_breach_count, 2);
    assert!(
        !breached.poisoned,
        "a per-tenant cap breach is backpressure and must not become permanent state"
    );

    drop(held_projection);
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("accepted projection work should drain after the cap breach");
    assert_eq!(projection_work.stats(&tenant_id).depth, 0);

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(4))]),
        )
        .await
        .expect("post-drain mutation should commit");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("post-drain projection should drain");
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(5),
        "a drained cap breach must resume projecting without replacing the tenant runtime"
    );

    let resumed = projection_work.stats(&tenant_id);
    assert_eq!(resumed.depth, 0);
    assert_eq!(
        resumed.cap_breach_count, 2,
        "recovery must not erase the breaches that already happened"
    );
    assert_eq!(
        resumed.dropped_event_count, 2,
        "the dropped events must stay observable after the tenant recovers"
    );
    assert!(!resumed.poisoned);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropped_projection_events_catch_up_once_per_table_after_capacity_returns() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-catch-up", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    let filler = TableName::new("filler").expect("table name should build");
    let (observer, projection_work) = test_observer(&engine, 2, 1);
    engine.install_committed_mutation_observer("projection-catch-up-test", observer);

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(0))]),
        )
        .await
        .expect("seed row should commit");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("seed projection should drain");
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1),
        "the seed projection must land before the cap is saturated"
    );

    // Saturate the cap with work that never projects `tasks`, so no
    // in-flight task can incorporate what the drops below skip.
    let saturating = (0..2)
        .map(|_| {
            projection_work
                .register(
                    &tenant_id,
                    engine
                        .committed_mutation_observer_runtime_identity(&tenant_id)
                        .expect("tenant runtime identity should load"),
                    &[(filler.clone(), ProjectionToken::default())],
                )
                .expect("saturating work should register up to the cap")
        })
        .collect::<Vec<_>>();

    for index in 1..6 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("projection saturation must not block durable mutation responses");
    }
    let dropped = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stats = projection_work.stats(&tenant_id);
            if stats.dropped_event_count == 5 {
                break stats;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("every commit past the cap should drop");
    assert_eq!(
        dropped.dirty_projection_scope_count, 1,
        "five drops on one table must coalesce into a single catch-up scope"
    );
    assert_eq!(dropped.catch_up_projection_count, 0);
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1),
        "the dropped commits must not be projected while the cap is breached"
    );

    drop(saturating);
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("the catch-up projection should drain");

    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(6),
        "a dropped projection event must be caught up without a further mutation"
    );
    let recovered = projection_work.stats(&tenant_id);
    assert_eq!(recovered.depth, 0);
    assert_eq!(
        recovered.dirty_projection_scope_count, 0,
        "a completed catch-up must clear its dirty marker"
    );
    assert_eq!(
        recovered.catch_up_projection_count, 1,
        "coalesced drops must cost exactly one catch-up projection"
    );
    assert_eq!(
        recovered.dropped_event_count, 5,
        "catching up must not erase the drops that already happened"
    );
    assert!(!recovered.poisoned);
}

/// Builds a tenant whose `tasks` projection is owed a catch-up: a seeded
/// row is projected, then the work cap is saturated so five further
/// commits drop and coalesce into one dirty scope. Returns the guards
/// holding the cap; dropping them releases capacity and starts the drain.
async fn saturate_until_catch_up_is_owed(
    engine: &Arc<Engine>,
    projection_work: &Arc<ProjectionWork>,
    tenant_id: &TenantId,
    tasks: &TableName,
) -> Vec<ProjectionWorkGuard> {
    let filler = TableName::new("filler").expect("table name should build");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(0))]),
        )
        .await
        .expect("seed row should commit");
    engine
        .flush_committed_mutation_observers_for_testing(tenant_id)
        .await
        .expect("seed projection should drain");
    assert_eq!(
        projected_table_row_count(engine, tenant_id, tasks).await,
        Some(1)
    );

    let saturating = (0..2)
        .map(|_| {
            projection_work
                .register(
                    tenant_id,
                    engine
                        .committed_mutation_observer_runtime_identity(tenant_id)
                        .expect("tenant runtime identity should load"),
                    &[(filler.clone(), ProjectionToken::default())],
                )
                .expect("saturating work should register up to the cap")
        })
        .collect::<Vec<_>>();
    for index in 1..6 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("projection saturation must not block durable mutation responses");
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if projection_work.stats(tenant_id).dropped_event_count == 5 {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("every commit past the cap should drop");
    assert_eq!(
        projection_work
            .stats(tenant_id)
            .dirty_projection_scope_count,
        1
    );
    saturating
}

/// A catch-up that fails must keep its dirty marker so a later drain
/// retries it. Clearing the marker on the claim alone would let the tenant
/// report idle while the dropped commits never reached `_nimbus`, and with
/// no further mutation on that table nothing would ever mark it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_failed_catch_up_keeps_its_marker_and_a_later_drain_lands_it() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-catch-up-retry", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    let (observer, projection_work) = test_observer(&engine, 2, 1);
    engine.install_committed_mutation_observer("projection-catch-up-retry-test", observer);

    let saturating =
        saturate_until_catch_up_is_owed(&engine, &projection_work, &tenant_id, &tasks).await;

    // Fail the first catch-up attempt only. Recovery must come from the
    // retry, not from any further mutation on `tasks`.
    projection_work.fail_next_projections(1);
    drop(saturating);
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("the retried catch-up projection should drain");

    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(6),
        "a catch-up that failed once must still land without a further mutation"
    );
    let recovered = projection_work.stats(&tenant_id);
    assert_eq!(recovered.depth, 0);
    assert_eq!(
        recovered.dirty_projection_scope_count, 0,
        "the marker must clear once the catch-up actually succeeds"
    );
    assert_eq!(
        recovered.catch_up_projection_count, 2,
        "the failed attempt must be retried exactly once more"
    );
    assert_eq!(recovered.delayed_retry_count, 1);
    assert_eq!(recovered.consecutive_failure_count, 0);
    // At least the five commits the cap dropped. A drain reservation that
    // loses the race for a marker and finds the cap full counts a drop of
    // its own, so the exact total is not pinned here.
    assert!(
        recovered.dropped_event_count >= 5,
        "catching up must not erase the drops that already happened, saw {}",
        recovered.dropped_event_count
    );
    assert!(!recovered.poisoned);
}

/// A persistent failure must retain its scope and slow down exponentially;
/// it may never trade liveness for a finite abandonment counter.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_permanent_failure_retains_scope_without_hot_loop() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-catch-up-bounded", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    let (observer, projection_work) = test_observer(&engine, 2, 1);
    engine.install_committed_mutation_observer("projection-catch-up-bounded-test", observer);

    let saturating =
        saturate_until_catch_up_is_owed(&engine, &projection_work, &tenant_id, &tasks).await;

    projection_work.fail_next_projections(100);
    drop(saturating);

    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if projection_work.stats(&tenant_id).consecutive_failure_count >= 3 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("projection retries should reach the bounded backoff state");

    let retained = projection_work.stats(&tenant_id);
    assert!(
        retained.dirty_projection_scope_count != 0 || retained.depth != 0,
        "failed work must remain represented by a marker or its in-flight replacement"
    );
    assert!(
        retained.catch_up_projection_count <= 5,
        "exponential backoff must bound retry attempts, saw {}",
        retained.catch_up_projection_count
    );
    assert!(retained.consecutive_failure_count >= 3);
    assert!(retained.current_retry_backoff_millis >= 200);
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1),
        "the retained scope must not claim a projection that never landed"
    );

    projection_work.fail_next_projections(0);
    projection_work.schedule_catch_up_drain();
    tokio::time::timeout(
        Duration::from_secs(6),
        engine.flush_committed_mutation_observers_for_testing(&tenant_id),
    )
    .await
    .expect("the retained scope should land after the fault clears")
    .expect("projection flush should succeed");
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(6)
    );
}

/// A claimed catch-up must hold the tenant busy for the whole interval
/// between losing its dirty marker and landing in `_nimbus`. The drain
/// reserves its in-flight slot before it claims, so there is no point at
/// which the tenant reports neither a dirty scope nor in-flight work while
/// a catch-up is still owed.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_flush_never_observes_no_marker_and_no_work() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-catch-up-fence", Engine::create_tenant);
    let tasks = TableName::new("tasks").expect("table name should build");
    let filler = TableName::new("filler").expect("table name should build");
    let (observer, projection_work) = test_observer(&engine, 2, 1);
    engine.install_committed_mutation_observer("projection-catch-up-fence-test", observer);

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(0))]),
        )
        .await
        .expect("seed row should commit");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("seed projection should drain");
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1),
        "the seed projection must land before the cap is saturated"
    );

    // Hold the projection lock so the catch-up cannot finish once spawned,
    // which is what makes the busy window observable.
    let work = tenant_work(&engine, &projection_work, &tenant_id);
    let held_projection = work.projection_lock.clone().lock_owned().await;

    let saturating = (0..2)
        .map(|_| {
            projection_work
                .register(
                    &tenant_id,
                    engine
                        .committed_mutation_observer_runtime_identity(&tenant_id)
                        .expect("tenant runtime identity should load"),
                    &[(filler.clone(), ProjectionToken::default())],
                )
                .expect("saturating work should register up to the cap")
        })
        .collect::<Vec<_>>();

    for index in 1..4 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks.clone(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("projection saturation must not block durable mutation responses");
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        while projection_work.stats(&tenant_id).dropped_event_count < 3 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("every commit past the cap should drop");

    drop(saturating);
    let claimed = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let stats = projection_work.stats(&tenant_id);
            if stats.catch_up_projection_count == 1 {
                break stats;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("returned capacity should claim the dirty scope");
    assert_eq!(
        claimed.dirty_projection_scope_count, 0,
        "claiming a catch-up clears the marker it stands in for"
    );
    assert_eq!(
        claimed.depth, 1,
        "the claim must be covered by an in-flight reservation the flush seam can see"
    );

    // The seed flush above already tripped this seam, so re-arm it before
    // asserting on the flush that matters.
    work.flush_waiting.store(false, Ordering::Release);
    let mut flush = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .flush_committed_mutation_observers_for_testing(&tenant_id)
                .await
        }
    });
    projection_work.wait_until_flush_waits(&tenant_id).await;
    assert!(
        !flush.is_finished(),
        "a flush must not return while a claimed catch-up has not landed"
    );
    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(1),
        "the catch-up must not have projected while it is still blocked"
    );

    drop(held_projection);
    tokio::time::timeout(Duration::from_secs(5), &mut flush)
        .await
        .expect("the catch-up should land and release the flush")
        .expect("flush task should join")
        .expect("flush should succeed");

    assert_eq!(
        projected_table_row_count(&engine, &tenant_id, &tasks).await,
        Some(4),
        "the flush must only return once the catch-up reached _nimbus"
    );
    let recovered = projection_work.stats(&tenant_id);
    assert_eq!(recovered.depth, 0);
    assert_eq!(recovered.dirty_projection_scope_count, 0);
    assert_eq!(recovered.catch_up_projection_count, 1);
    assert!(!recovered.poisoned);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn aggregate_cap_drop_catches_up_the_victim_tenant_after_capacity_returns() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let hog = fixture.create_tenant("projection-aggregate-catch-up-hog", Engine::create_tenant);
    let victim = fixture.create_tenant(
        "projection-aggregate-catch-up-victim",
        Engine::create_tenant,
    );
    let tasks = TableName::new("tasks").expect("table name should build");
    let filler = TableName::new("filler").expect("table name should build");
    let projection_work = Arc::new(ProjectionWork::new_with_aggregate(&engine, 4, 3, 2, 1));
    let observer = Arc::new(TableProjectionObserver {
        projection_work: projection_work.clone(),
    });

    engine
        .insert_document_async(
            victim.clone(),
            tasks.clone(),
            serde_json::Map::from_iter([("index".to_string(), json!(0))]),
        )
        .await
        .expect("victim source row should commit");

    let saturating = (0..2)
        .map(|_| {
            projection_work
                .register(
                    &hog,
                    engine
                        .committed_mutation_observer_runtime_identity(&hog)
                        .expect("hog runtime identity should load"),
                    &[(filler.clone(), ProjectionToken::default())],
                )
                .expect("the hog should fill the aggregate cap")
        })
        .collect::<Vec<_>>();

    observer.project_tables(
        victim.clone(),
        vec![tasks.clone()],
        ProjectionToken::default(),
    );
    let dropped = projection_work.stats(&victim);
    assert_eq!(
        dropped.dropped_event_count, 1,
        "the aggregate cap must reject the victim while the hog holds it"
    );
    assert_eq!(dropped.dirty_projection_scope_count, 1);
    assert_eq!(
        projected_table_row_count(&engine, &victim, &tasks).await,
        None,
        "the aggregate-cap victim must not be projected while the cap is breached"
    );

    drop(saturating);
    tokio::time::timeout(
        Duration::from_secs(5),
        projection_work.wait_for_idle(&victim),
    )
    .await
    .expect("the victim catch-up should drain");

    assert_eq!(
        projected_table_row_count(&engine, &victim, &tasks).await,
        Some(1),
        "an aggregate-cap drop must be caught up from another tenant's drain"
    );
    let recovered = projection_work.stats(&victim);
    assert_eq!(recovered.dirty_projection_scope_count, 0);
    assert_eq!(recovered.catch_up_projection_count, 1);
    assert_eq!(recovered.dropped_event_count, 1);
    assert!(!recovered.poisoned);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn tenant_scoped_diagnostics_ignore_other_tenant_projection_work() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = fixture.create_tenant("projection-flush-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("projection-flush-b", Engine::create_tenant);
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    engine.install_committed_mutation_observer("projection-tenant-flush-test", observer);
    let tenant_b_work = projection_work
        .register(
            &tenant_b,
            engine
                .committed_mutation_observer_runtime_identity(&tenant_b)
                .expect("tenant B runtime identity should load"),
            &[(
                TableName::new("tasks").expect("table name should build"),
                ProjectionToken::default(),
            )],
        )
        .expect("tenant B background work should register");

    tokio::time::timeout(
        Duration::from_secs(1),
        engine.flush_committed_mutation_observers_for_testing(&tenant_a),
    )
    .await
    .expect("tenant A flush must not wait for tenant B")
    .expect("tenant A flush should succeed");
    assert_eq!(projection_work.stats(&tenant_a).depth, 0);
    assert_eq!(projection_work.stats(&tenant_b).depth, 1);
    drop(tenant_b_work);
    assert_eq!(projection_work.stats(&tenant_b).depth, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_work_sweeps_evicted_tenant_runtime_generations() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let (observer, projection_work) = test_observer(&engine, 16, 12);
    engine.install_committed_mutation_observer("projection-churn-test", observer);

    for index in 0..8 {
        let tenant_id =
            TenantId::new(format!("projection-churn-{index}")).expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("ephemeral tenant should create");
        engine
            .insert_document_async(
                tenant_id.clone(),
                TableName::new("tasks").expect("table name should build"),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("ephemeral tenant mutation should commit");
        engine
            .flush_committed_mutation_observers_for_testing(&tenant_id)
            .await
            .expect("ephemeral tenant projection should drain");
        engine
            .delete_tenant_async(tenant_id)
            .await
            .expect("ephemeral tenant should delete");
    }

    assert_eq!(
        projection_work.tenant_count(),
        0,
        "dead runtime generations must not accumulate in projection state"
    );
}

#[test]
fn projection_register_hot_path_does_not_scan_before_amortized_sweep() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("projection-hot-path", Engine::create_tenant);
    let projection_work = Arc::new(ProjectionWork::new(&engine, 64, 48));
    let tasks = TableName::new("tasks").expect("table name should build");

    for _ in 0..64 {
        let guard = projection_work
            .register(
                &tenant_id,
                engine
                    .committed_mutation_observer_runtime_identity(&tenant_id)
                    .expect("tenant runtime identity should load"),
                &[(tasks.clone(), ProjectionToken::default())],
            )
            .expect("hot-path projection should register below its cap");
        drop(guard);
    }

    assert_eq!(
        projection_work.sweep_count(),
        0,
        "ordinary projection registration must not scan the tenant map"
    );
}

#[tokio::test]
async fn projection_guard_release_does_not_scan_registry_on_caller_task() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id =
        fixture.create_tenant("projection-release-o1-scheduling", Engine::create_tenant);
    let projection_work = Arc::new(ProjectionWork::new(&engine, 16, 12));
    let tasks = TableName::new("tasks").expect("table name should build");
    let tenant_work = tenant_work(&engine, &projection_work, &tenant_id);
    tenant_work.mark_scopes_dirty(
        &[(tasks, ProjectionToken::default())],
        &projection_work.dirty_tenants,
    );
    let guard = projection_work
        .register(
            &tenant_id,
            engine
                .committed_mutation_observer_runtime_identity(&tenant_id)
                .expect("tenant runtime identity should load"),
            &[],
        )
        .expect("projection guard should reserve below the cap");

    engine.quiesce().await;
    drop(guard);

    assert_eq!(
        projection_work.drain_scan_count(),
        0,
        "guard release may only schedule coalesced catch-up work; it must not scan every tenant on the caller task"
    );
}

#[test]
fn stale_drain_candidate_cannot_underflow_dirty_tenant_count_after_runtime_replacement() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id =
        fixture.create_tenant("projection-stale-drain-candidate", Engine::create_tenant);
    let projection_work = Arc::new(ProjectionWork::new(&engine, 16, 12));
    let tasks = TableName::new("tasks").expect("table name should build");
    let stale_work = tenant_work(&engine, &projection_work, &tenant_id);
    stale_work.mark_scopes_dirty(
        &[(tasks, ProjectionToken::default())],
        &projection_work.dirty_tenants,
    );
    let (candidate_tenant, stale_candidate) = projection_work
        .dirty_projection_candidates()
        .into_iter()
        .next()
        .expect("dirty runtime should be snapshotted");

    engine
        .delete_tenant(&tenant_id)
        .expect("old runtime should delete");
    engine
        .create_tenant(tenant_id.clone())
        .expect("replacement runtime should create");
    projection_work
        .tenants
        .lock()
        .expect("projection registry lock should not be poisoned")
        .registrations_since_sweep = PROJECTION_TENANT_SWEEP_INTERVAL - 1;

    projection_work.drain_dirty_candidate(&engine, candidate_tenant, stale_candidate);

    assert_eq!(
        projection_work.dirty_tenants.load(Ordering::Acquire),
        0,
        "sweeping a stale dirty generation and reserving its replacement must decrement exactly once"
    );
    let replacement = projection_work
        .existing_tenant_work(&tenant_id)
        .expect("replacement projection state should register");
    assert!(
        !Arc::ptr_eq(&stale_work, &replacement),
        "the test must exercise a replaced runtime generation"
    );
}

#[test]
fn projection_aggregate_cap_drops_then_resumes_victim_after_drain() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = fixture.create_tenant("projection-aggregate-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("projection-aggregate-b", Engine::create_tenant);
    let tenant_c = fixture.create_tenant("projection-aggregate-c", Engine::create_tenant);
    let tenant_d = fixture.create_tenant("projection-aggregate-d", Engine::create_tenant);
    let projection_work = Arc::new(ProjectionWork::new_with_aggregate(&engine, 4, 3, 2, 1));
    let tasks = TableName::new("tasks").expect("table name should build");

    let register = |tenant_id: &TenantId| {
        projection_work.register(
            tenant_id,
            engine
                .committed_mutation_observer_runtime_identity(tenant_id)
                .expect("tenant runtime identity should load"),
            &[(tasks.clone(), ProjectionToken::default())],
        )
    };
    let guard_a = register(&tenant_a).expect("tenant A should fit below both caps");
    let guard_b = register(&tenant_b).expect("tenant B should fill the aggregate cap");
    assert!(
        register(&tenant_c).is_none(),
        "the aggregate cap must reject the offending third registration"
    );
    let rejected = projection_work.stats(&tenant_c);
    assert_eq!(rejected.depth, 0);
    assert_eq!(rejected.cap_breach_count, 1);
    assert_eq!(rejected.dropped_event_count, 1);
    assert!(
        !rejected.poisoned,
        "an aggregate-cap race must not permanently poison the tenant that lost it"
    );
    assert!(!projection_work.stats(&tenant_a).poisoned);
    assert!(!projection_work.stats(&tenant_b).poisoned);
    assert_eq!(
        projection_work
            .aggregate_cap_breach_count
            .load(Ordering::Relaxed),
        1
    );

    drop(guard_a);
    drop(guard_b);
    let resumed = register(&tenant_c)
        .expect("the aggregate-cap victim must resume after the hog work drains");
    let resumed_stats = projection_work.stats(&tenant_c);
    assert_eq!(resumed_stats.depth, 1);
    assert_eq!(resumed_stats.cap_breach_count, 1);
    assert_eq!(resumed_stats.dropped_event_count, 1);
    assert!(!resumed_stats.poisoned);
    drop(resumed);

    let quiet_guard =
        register(&tenant_d).expect("a quiet process must admit a tenant below its per-tenant cap");
    assert_eq!(projection_work.stats(&tenant_d).depth, 1);
    assert!(!projection_work.stats(&tenant_d).poisoned);
    drop(quiet_guard);
}

#[path = "tests/runtime_lifecycle.rs"]
mod runtime_lifecycle;
