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
