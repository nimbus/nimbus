use nimbus_core::{Document, DocumentId, Error, Filter, FilterOp, Query, SequenceNumber};
use nimbus_engine::{Fault, commit_fault_labels};
use nimbus_storage::EmbeddedProviderKind;
use nimbus_testing::EngineFixture;

use super::*;
use crate::records::ensure_system_tenant_async;

fn token(epoch: u64, sequence: u64) -> ProjectionToken {
    token_in_incarnation(1, epoch, sequence)
}

fn token_in_incarnation(tenant_incarnation: u64, epoch: u64, sequence: u64) -> ProjectionToken {
    ProjectionToken {
        tenant_incarnation,
        lease_epoch: epoch,
        durable_sequence: SequenceNumber(sequence),
    }
}

fn visible_fields(
    tenant_id: &TenantId,
    table: &TableName,
    row_count: u64,
    token: ProjectionToken,
) -> Map<String, Value> {
    object_fields(json!({
        "tenantId": tenant_id.as_str(),
        "name": table.as_str(),
        "rowCount": row_count,
        "lastWriteAt": 1_700_000_000_000_u64,
        "projectionEpoch": "test-process",
        "projectionGeneration": 1,
        "sourceTenantIncarnation": token.tenant_incarnation,
        "sourceLeaseEpoch": token.lease_epoch,
        "sourceDurableSequence": token.durable_sequence.0,
    }))
}

fn publication(
    tenant_id: &TenantId,
    table: &TableName,
    row_count: u64,
    token: ProjectionToken,
    delete_visible: bool,
) -> ProjectionPublication {
    ProjectionPublication {
        tenant_id: tenant_id.clone(),
        table: table.clone(),
        token,
        visible_fields: visible_fields(tenant_id, table, row_count, token),
        delete_visible,
    }
}

async fn fixture(name: &str) -> (EngineFixture<Engine>, Arc<Engine>, TenantId, TableName) {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant(name, Engine::create_tenant);
    let table = TableName::new("tasks").expect("table should build");
    ensure_system_tenant_async(&engine)
        .await
        .expect("system tenant should prepare");
    (fixture, engine, tenant_id, table)
}

async fn visible_row(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Option<Document> {
    document(
        engine,
        SystemTable::Tables.table_name().unwrap(),
        tenant_id,
        table,
    )
    .await
}

async fn fence_row(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Option<Document> {
    document(
        engine,
        TableName::new(PROJECTION_FENCE_TABLE).unwrap(),
        tenant_id,
        table,
    )
    .await
}

async fn document(
    engine: &Arc<Engine>,
    system_table: TableName,
    tenant_id: &TenantId,
    table: &TableName,
) -> Option<Document> {
    engine
        .get_document_async(
            system_tenant_id().unwrap(),
            system_table,
            DocumentId::from_key(table_document_id(tenant_id, table)).unwrap(),
        )
        .await
        .ok()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_fence_rejects_late_older_document_projection() {
    let (_fixture, engine, tenant_id, table) = fixture("projection-fence-document").await;
    let newer = token(4, 20);
    let older_sequence = token(4, 19);
    let older_epoch = token(3, 10_000);

    assert_eq!(
        publish_table_projection_async(&engine, publication(&tenant_id, &table, 20, newer, false),)
            .await
            .unwrap(),
        ProjectionPublicationOutcome::Applied
    );
    for (candidate, rows) in [(older_sequence, 19), (older_epoch, 99)] {
        assert_eq!(
            publish_table_projection_async(
                &engine,
                publication(&tenant_id, &table, rows, candidate, false),
            )
            .await
            .unwrap(),
            ProjectionPublicationOutcome::StaleNoOp
        );
    }

    let row = visible_row(&engine, &tenant_id, &table).await.unwrap();
    assert_eq!(row.fields.get("rowCount").and_then(Value::as_u64), Some(20));
    assert_eq!(
        row.fields.get("sourceLeaseEpoch").and_then(Value::as_u64),
        Some(4)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recreated_tenant_incarnation_dominates_old_fence_and_late_work() {
    let (_fixture, engine, tenant_id, table) = fixture("projection-fence-incarnation").await;
    let deleted_incarnation = token_in_incarnation(1, 90, 10_000);
    let recreated_incarnation = token_in_incarnation(2, 0, 1);

    assert_eq!(
        publish_table_projection_async(
            &engine,
            publication(&tenant_id, &table, 90, deleted_incarnation, false),
        )
        .await
        .unwrap(),
        ProjectionPublicationOutcome::Applied
    );
    assert_eq!(
        publish_table_projection_async(
            &engine,
            publication(&tenant_id, &table, 1, recreated_incarnation, false),
        )
        .await
        .unwrap(),
        ProjectionPublicationOutcome::Applied,
        "a recreated tenant must replace every fence from its prior incarnation"
    );
    assert_eq!(
        publish_table_projection_async(
            &engine,
            publication(&tenant_id, &table, 99, deleted_incarnation, false),
        )
        .await
        .unwrap(),
        ProjectionPublicationOutcome::StaleNoOp,
        "late work from the deleted incarnation must stay fenced"
    );

    let row = visible_row(&engine, &tenant_id, &table).await.unwrap();
    assert_eq!(row.fields.get("rowCount").and_then(Value::as_u64), Some(1));
    assert_eq!(
        projection_token_from_fence(&fence_row(&engine, &tenant_id, &table).await.unwrap())
            .unwrap(),
        recreated_incarnation
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_fence_rejects_late_older_schema_projection() {
    let (_fixture, engine, tenant_id, table) = fixture("projection-fence-schema").await;
    let newer = token(7, 3);
    let older = token(6, 800);
    let mut current = publication(&tenant_id, &table, 0, newer, false);
    current
        .visible_fields
        .insert("schema".to_string(), json!({"revision": "new"}));
    publish_table_projection_async(&engine, current)
        .await
        .unwrap();
    let mut stale = publication(&tenant_id, &table, 0, older, false);
    stale
        .visible_fields
        .insert("schema".to_string(), json!({"revision": "old"}));

    assert_eq!(
        publish_table_projection_async(&engine, stale)
            .await
            .unwrap(),
        ProjectionPublicationOutcome::StaleNoOp
    );
    assert_eq!(
        visible_row(&engine, &tenant_id, &table)
            .await
            .unwrap()
            .fields
            .get("schema"),
        Some(&json!({"revision": "new"}))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_tombstone_prevents_deleted_row_resurrection() {
    let (_fixture, engine, tenant_id, table) = fixture("projection-fence-delete").await;
    let live = token(2, 10);
    let deleted = token(2, 11);
    publish_table_projection_async(&engine, publication(&tenant_id, &table, 4, live, false))
        .await
        .unwrap();
    publish_table_projection_async(&engine, publication(&tenant_id, &table, 0, deleted, true))
        .await
        .unwrap();

    assert_eq!(
        publish_table_projection_async(&engine, publication(&tenant_id, &table, 4, live, false),)
            .await
            .unwrap(),
        ProjectionPublicationOutcome::StaleNoOp
    );
    assert!(visible_row(&engine, &tenant_id, &table).await.is_none());
    let fence = fence_row(&engine, &tenant_id, &table).await.unwrap();
    assert_eq!(
        fence.fields.get("deleted").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        projection_token_from_fence(&fence).unwrap(),
        deleted,
        "the deletion-surviving private row is the durable winner"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_row_indexes_fence_and_commit_log_roll_back_together() {
    let (_fixture, engine, tenant_id, table) = fixture("projection-fence-rollback").await;
    let faults = engine.commit_fault_handle_for_testing();
    faults.inject(
        commit_fault_labels::PRE_PERSIST,
        Fault::Error(Error::Internal("injected projection rollback".to_string())),
    );

    let error = publish_table_projection_async(
        &engine,
        publication(&tenant_id, &table, 1, token(1, 1), false),
    )
    .await
    .expect_err("the injected pre-persist fault must fail the whole unit");
    assert!(error.to_string().contains("injected projection rollback"));
    assert!(visible_row(&engine, &tenant_id, &table).await.is_none());
    assert!(fence_row(&engine, &tenant_id, &table).await.is_none());

    let indexed = engine
        .query_documents_async(
            system_tenant_id().unwrap(),
            Query {
                table: SystemTable::Tables.table_name().unwrap(),
                filters: vec![Filter {
                    field: "tenantId".to_string(),
                    op: FilterOp::Eq,
                    value: json!(tenant_id.as_str()),
                }],
                order: None,
                limit: None,
            },
        )
        .await
        .expect("the visible-row tenant index should remain readable");
    assert!(
        indexed.is_empty(),
        "the failed unit must leave no index entry"
    );

    publish_table_projection_async(
        &engine,
        publication(&tenant_id, &table, 1, token(1, 1), false),
    )
    .await
    .expect("a later full unit should commit");
    assert_eq!(
        visible_row(&engine, &tenant_id, &table)
            .await
            .unwrap()
            .fields
            .get("rowCount")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        projection_token_from_fence(&fence_row(&engine, &tenant_id, &table).await.unwrap())
            .unwrap(),
        token(1, 1)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_retry_after_ambiguous_commit_is_idempotent() {
    let (_fixture, engine, tenant_id, table) = fixture("projection-fence-ambiguous").await;
    let source = token(9, 12);
    let faults = engine.commit_fault_handle_for_testing();
    faults.inject(
        commit_fault_labels::DURABLE_BEFORE_PUBLISH,
        Fault::Error(Error::Internal(
            "injected acknowledgement loss after durability".to_string(),
        )),
    );

    let first =
        publish_table_projection_async(&engine, publication(&tenant_id, &table, 6, source, false))
            .await;
    assert!(first.is_err(), "the first caller must not observe success");
    engine
        .ensure_tenant_exists_async(system_tenant_id().unwrap())
        .await
        .expect("the system tenant should recover its durable execution unit");

    assert_eq!(
        publish_table_projection_async(&engine, publication(&tenant_id, &table, 6, source, false),)
            .await
            .unwrap(),
        ProjectionPublicationOutcome::StaleNoOp
    );
    assert_eq!(
        projection_token_from_fence(&fence_row(&engine, &tenant_id, &table).await.unwrap())
            .unwrap(),
        source
    );
    assert_eq!(
        visible_row(&engine, &tenant_id, &table)
            .await
            .unwrap()
            .fields
            .get("rowCount")
            .and_then(Value::as_u64),
        Some(6)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_conflicts_retry_from_fresh_snapshots_with_a_fixed_bound() {
    let (_fixture, engine, tenant_id, table) = fixture("projection-fence-conflicts").await;
    let faults = engine.commit_fault_handle_for_testing();
    faults.inject_retryable_conflicts(commit_fault_labels::PRE_PERSIST, 2, None);

    assert_eq!(
        publish_table_projection_async(
            &engine,
            publication(&tenant_id, &table, 3, token(1, 3), false),
        )
        .await
        .unwrap(),
        ProjectionPublicationOutcome::Applied
    );
    assert_eq!(faults.hit_count(commit_fault_labels::PRE_PERSIST), 3);
}

#[derive(Clone, Copy)]
enum ProjectionContractBackend {
    Memory,
    Redb,
    Sqlite,
}

impl ProjectionContractBackend {
    fn name(self) -> &'static str {
        match self {
            Self::Memory => "memory",
            Self::Redb => "redb",
            Self::Sqlite => "sqlite",
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_publication_contract_matches_memory_redb_and_sqlite() {
    for backend in [
        ProjectionContractBackend::Memory,
        ProjectionContractBackend::Redb,
        ProjectionContractBackend::Sqlite,
    ] {
        let fixture = EngineFixture::new(move |path| match backend {
            ProjectionContractBackend::Memory => Engine::new_with_memory_persistence(path),
            ProjectionContractBackend::Redb => {
                Engine::new_with_embedded_provider(path, EmbeddedProviderKind::Redb)
            }
            ProjectionContractBackend::Sqlite => {
                Engine::new_with_embedded_provider(path, EmbeddedProviderKind::Sqlite)
            }
        });
        let engine = fixture.engine();
        let tenant_name = format!("projection-contract-{}", backend.name());
        let tenant_id = TenantId::new(tenant_name).expect("tenant id should build");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("tenant should create through the production async lifecycle");
        let table = TableName::new("tasks").expect("table should build");
        ensure_system_tenant_async(&engine)
            .await
            .expect("system tenant should prepare");

        let faults = engine.commit_fault_handle_for_testing();
        faults.inject(
            commit_fault_labels::PRE_PERSIST,
            Fault::Error(Error::Internal(format!(
                "injected {} projection rollback",
                backend.name()
            ))),
        );
        publish_table_projection_async(
            &engine,
            publication(&tenant_id, &table, 1, token(2, 4), false),
        )
        .await
        .expect_err("pre-persist fault should roll back the whole publication unit");
        assert!(visible_row(&engine, &tenant_id, &table).await.is_none());
        assert!(fence_row(&engine, &tenant_id, &table).await.is_none());

        assert_eq!(
            publish_table_projection_async(
                &engine,
                publication(&tenant_id, &table, 2, token(2, 5), false),
            )
            .await
            .expect("new projection should publish"),
            ProjectionPublicationOutcome::Applied
        );
        assert_eq!(
            publish_table_projection_async(
                &engine,
                publication(&tenant_id, &table, 99, token(1, 100), false),
            )
            .await
            .expect("older projection should classify"),
            ProjectionPublicationOutcome::StaleNoOp
        );
        assert_eq!(
            publish_table_projection_async(
                &engine,
                publication(&tenant_id, &table, 0, token(2, 6), true),
            )
            .await
            .expect("newer deletion should publish"),
            ProjectionPublicationOutcome::Applied
        );
        assert_eq!(
            publish_table_projection_async(
                &engine,
                publication(&tenant_id, &table, 2, token(2, 5), false),
            )
            .await
            .expect("stale resurrection should classify"),
            ProjectionPublicationOutcome::StaleNoOp
        );
        assert!(visible_row(&engine, &tenant_id, &table).await.is_none());
        assert_eq!(
            projection_token_from_fence(&fence_row(&engine, &tenant_id, &table).await.unwrap())
                .unwrap(),
            token(2, 6)
        );

        let before_recreate = engine
            .projection_token_for_tenant_async(&tenant_id)
            .await
            .expect("source token should resolve before deletion");
        engine
            .delete_tenant_async(tenant_id.clone())
            .await
            .expect("source tenant should delete through the async lifecycle");
        engine
            .create_tenant_async(tenant_id.clone())
            .await
            .expect("same-id source tenant should recreate through the async lifecycle");
        let after_recreate = engine
            .projection_token_for_tenant_async(&tenant_id)
            .await
            .expect("source token should resolve after recreation");
        assert!(
            after_recreate.tenant_incarnation > before_recreate.tenant_incarnation,
            "{} must durably advance tenant incarnation on same-id recreation",
            backend.name()
        );
        assert_eq!(
            publish_table_projection_async(
                &engine,
                publication(&tenant_id, &table, 1, after_recreate, false),
            )
            .await
            .expect("recreated source projection should classify"),
            ProjectionPublicationOutcome::Applied
        );
        assert_eq!(
            publish_table_projection_async(
                &engine,
                publication(&tenant_id, &table, 99, before_recreate, false),
            )
            .await
            .expect("late deleted-incarnation projection should classify"),
            ProjectionPublicationOutcome::StaleNoOp
        );

        let system_tenant = system_tenant_id().expect("system tenant id should build");
        let head = engine
            .latest_sequence_async(system_tenant.clone())
            .await
            .expect("system durable head should read");
        let journal = engine
            .read_durable_journal_async(system_tenant, SequenceNumber(1))
            .await
            .expect("system durable journal should read");
        assert_eq!(journal.last().map(|record| record.sequence), Some(head));
        assert!(
            journal
                .windows(2)
                .all(|pair| pair[1].sequence.0 == pair[0].sequence.0 + 1),
            "{} publication journal must remain contiguous",
            backend.name()
        );
    }
}

struct ArmedOrderedPublisherPause {
    handle: nimbus_engine::OrderedPublisherPauseHandle,
    released: bool,
}

impl ArmedOrderedPublisherPause {
    fn new(handle: nimbus_engine::OrderedPublisherPauseHandle) -> Self {
        handle.arm();
        Self {
            handle,
            released: false,
        }
    }

    async fn wait_until_entered(&self, context: &'static str) {
        let handle = self.handle.clone();
        assert!(
            tokio::task::spawn_blocking(move || {
                handle.wait_until_entered(std::time::Duration::from_secs(5))
            })
            .await
            .expect("ordered publisher pause waiter should join"),
            "{context}"
        );
    }

    fn release(mut self) {
        self.handle.release();
        self.released = true;
    }
}

impl Drop for ArmedOrderedPublisherPause {
    fn drop(&mut self) {
        if !self.released {
            self.handle.release();
        }
    }
}

async fn wait_for_committer_inbox_depth(
    engine: &Engine,
    tenant_id: &TenantId,
    minimum_depth: usize,
    context: &'static str,
) {
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if engine
                .mutation_journal_stats_for_testing(tenant_id)
                .expect("mutation journal stats should load")
                .committer_inbox_depth
                >= minimum_depth
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect(context);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ordered_publisher_serializes_schema_restore_cursor_scheduler_and_projection_jobs() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tasks = TableName::new("tasks").expect("tasks table should build");

    // Schema set/delete are two distinct actor-owned opaque jobs. Holding the
    // first after publisher dequeue must keep the later delete in the actor
    // inbox; release then proves their durable journal order.
    let schema_tenant = TenantId::new("ordered-internal-schema").expect("tenant id should build");
    engine
        .create_tenant_async(schema_tenant.clone())
        .await
        .expect("schema tenant should create through the async lifecycle");
    engine
        .shutdown_trigger_candidates_for_testing(&schema_tenant)
        .expect("schema tenant trigger worker should stop");
    let schema_pause = ArmedOrderedPublisherPause::new(
        engine
            .ordered_publisher_pause_handle_for_testing(&schema_tenant)
            .expect("schema publisher pause should load"),
    );
    let schema_set = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = schema_tenant.clone();
        let table = tasks.clone();
        async move {
            engine
                .set_table_schema_async(
                    tenant_id,
                    nimbus_core::TableSchema {
                        table,
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
        }
    });
    schema_pause
        .wait_until_entered("schema set should reach the ordered publisher")
        .await;
    let schema_delete = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = schema_tenant.clone();
        let table = tasks.clone();
        async move { engine.delete_table_schema_async(tenant_id, table).await }
    });
    wait_for_committer_inbox_depth(
        &engine,
        &schema_tenant,
        1,
        "schema delete should queue behind the held schema set",
    )
    .await;
    assert!(!schema_set.is_finished() && !schema_delete.is_finished());
    schema_pause.release();
    schema_set
        .await
        .expect("schema-set task should join")
        .expect("schema set should commit");
    schema_delete
        .await
        .expect("schema-delete task should join")
        .expect("schema delete should commit");
    let schema_event_order = engine
        .read_durable_journal_async(schema_tenant.clone(), SequenceNumber(0))
        .await
        .expect("schema journal should load")
        .into_iter()
        .flat_map(|record| record.events)
        .filter_map(|event| match event {
            nimbus_core::TenantEventKind::SchemaChange { change } => Some(match *change {
                nimbus_core::SchemaChangeEvent::SetTable { .. } => "set",
                nimbus_core::SchemaChangeEvent::DeleteTable { .. } => "delete",
            }),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(schema_event_order, ["set", "delete"]);

    // Restore/import owns the publisher first. Trigger-cursor and scheduled
    // state writes admitted afterward must remain queued behind it.
    let source = TenantId::new("ordered-internal-restore-source")
        .expect("restore source tenant id should build");
    engine
        .create_tenant_async(source.clone())
        .await
        .expect("restore source should create through the async lifecycle");
    engine
        .shutdown_trigger_candidates_for_testing(&source)
        .expect("restore source trigger worker should stop");
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
    let destination = TenantId::new("ordered-internal-restore-destination")
        .expect("restore destination tenant id should build");
    engine
        .create_tenant_async(destination.clone())
        .await
        .expect("restore destination should create through the async lifecycle");
    engine
        .shutdown_trigger_candidates_for_testing(&destination)
        .expect("restore destination trigger worker should stop");
    let restore_pause = ArmedOrderedPublisherPause::new(
        engine
            .ordered_publisher_pause_handle_for_testing(&destination)
            .expect("restore publisher pause should load"),
    );
    let restore = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = destination.clone();
        let archive = archive.clone();
        move || engine.import_point_in_time_restore_archive(&tenant_id, &archive)
    });
    restore_pause
        .wait_until_entered("restore import should reach the ordered publisher")
        .await;
    let cursor_sequence = archive.target_sequence;
    let cursor = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = destination.clone();
        move || {
            engine.set_trigger_delivery_cursor_for_testing(
                &tenant_id,
                nimbus_core::TriggerDeliveryCursor::new(cursor_sequence),
            )
        }
    });
    wait_for_committer_inbox_depth(
        &engine,
        &destination,
        1,
        "trigger cursor should queue behind restore",
    )
    .await;
    let schedule = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = destination.clone();
        let table = tasks.clone();
        async move {
            engine
                .schedule_mutation_async(
                    tenant_id,
                    nimbus_core::ScheduleRequest {
                        run_after_ms: 60_000,
                        mutation: nimbus_core::Mutation::Insert {
                            table,
                            id: None,
                            fields: serde_json::Map::from_iter([(
                                "title".to_string(),
                                json!("scheduled"),
                            )]),
                        },
                    },
                )
                .await
        }
    });
    wait_for_committer_inbox_depth(
        &engine,
        &destination,
        2,
        "scheduled state should queue behind restore and trigger cursor",
    )
    .await;
    assert!(!restore.is_finished() && !cursor.is_finished() && !schedule.is_finished());
    restore_pause.release();
    restore
        .await
        .expect("restore task should join")
        .expect("restore should commit");
    cursor
        .await
        .expect("cursor task should join")
        .expect("trigger cursor should commit");
    schedule
        .await
        .expect("scheduler task should join")
        .expect("scheduled state should commit");
    assert_eq!(
        engine
            .trigger_delivery_cursor_for_testing(&destination)
            .expect("trigger cursor should load")
            .materialized_through,
        cursor_sequence
    );
    assert_eq!(
        engine
            .list_scheduled_jobs_async(destination.clone())
            .await
            .expect("scheduled state should load")
            .len(),
        1
    );
    assert_eq!(
        engine
            .query_documents_async(
                destination.clone(),
                Query {
                    table: tasks.clone(),
                    filters: Vec::new(),
                    order: None,
                    limit: None,
                },
            )
            .await
            .expect("restored document should query")
            .len(),
        1
    );

    // Projection publication is the system tenant's execution-unit adapter.
    // Hold it at the same publisher seam and prove a later opaque schema job
    // cannot overtake its document/index/fence/journal transaction.
    ensure_system_tenant_async(&engine)
        .await
        .expect("system tenant should prepare through the async lifecycle");
    let system_tenant = system_tenant_id().expect("system tenant id should build");
    engine
        .shutdown_trigger_candidates_for_testing(&system_tenant)
        .expect("system trigger worker should stop");
    let projection_pause = ArmedOrderedPublisherPause::new(
        engine
            .ordered_publisher_pause_handle_for_testing(&system_tenant)
            .expect("system publisher pause should load"),
    );
    let projected_table = TableName::new("projected_tasks").expect("table should build");
    let projected_token = token(7, 11);
    let projection = tokio::spawn({
        let engine = engine.clone();
        let source = destination.clone();
        let table = projected_table.clone();
        async move {
            publish_table_projection_async(
                &engine,
                publication(&source, &table, 1, projected_token, false),
            )
            .await
        }
    });
    projection_pause
        .wait_until_entered("projection execution unit should reach the ordered publisher")
        .await;
    let system_schema_table =
        TableName::new("ordered_projection_followup").expect("table should build");
    let system_schema = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = system_tenant.clone();
        let table = system_schema_table.clone();
        async move {
            engine
                .set_table_schema_async(
                    tenant_id,
                    nimbus_core::TableSchema {
                        table,
                        fields: Vec::new(),
                        indexes: Vec::new(),
                        access_policy: None,
                    },
                )
                .await
        }
    });
    wait_for_committer_inbox_depth(
        &engine,
        &system_tenant,
        1,
        "system schema job should queue behind projection publication",
    )
    .await;
    assert!(!projection.is_finished() && !system_schema.is_finished());
    projection_pause.release();
    assert_eq!(
        projection
            .await
            .expect("projection task should join")
            .expect("projection should publish"),
        ProjectionPublicationOutcome::Applied
    );
    system_schema
        .await
        .expect("system schema task should join")
        .expect("system schema should commit after projection");
    assert_eq!(
        projection_token_from_fence(
            &fence_row(&engine, &destination, &projected_table)
                .await
                .expect("projection fence should exist"),
        )
        .expect("projection fence token should decode"),
        projected_token
    );
    let system_journal = engine
        .read_durable_journal_async(system_tenant, SequenceNumber(0))
        .await
        .expect("system journal should load");
    let projection_sequence = system_journal
        .iter()
        .find(|record| !record.writes.is_empty())
        .expect("projection should append a document-bearing system record")
        .sequence;
    let schema_sequence = system_journal
        .iter()
        .find(|record| {
            record.events.iter().any(|event| {
                matches!(
                    event,
                    nimbus_core::TenantEventKind::SchemaChange { change }
                        if match change.as_ref() {
                            nimbus_core::SchemaChangeEvent::SetTable { table, .. }
                            | nimbus_core::SchemaChangeEvent::DeleteTable { table, .. } =>
                                table == &system_schema_table,
                        }
                )
            })
        })
        .expect("follow-up system schema should append a journal record")
        .sequence;
    assert!(
        projection_sequence < schema_sequence,
        "projection publication must retain FIFO ownership ahead of the later opaque job"
    );

    engine.quiesce().await;
}
