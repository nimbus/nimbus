use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use libsql::Builder;
use mysql_async::prelude::Queryable;
use nimbus_core::{
    Document, DocumentId, FieldSchema, FieldType, Filter, FilterOp, Query, SequenceNumber,
    TableName, TableSchema, TenantId,
};
use nimbus_engine::{
    ControlPlaneConfig, EnginePersistenceConfig, LocalEncryptionConfig, PersistenceDialect,
    PersistenceTopology, PoolConfig, ProviderCredentials, TenantProviderConfig,
    TenantRoutingConfig,
};
use nimbus_engine::{Engine, ProjectionToken};
use nimbus_storage::libsql::libsql_transport_connector;
use nimbus_storage::provider_test_fixtures::{
    ExternalProviderFixtureMode, external_provider_fixture_mode,
};
use nimbus_storage::{
    FaultInjector, FaultPoint, LibsqlReplicaProvider, LibsqlReplicaProviderConfig, MySqlProvider,
    MySqlProviderConfig, NoopFaultInjector, PostgresProvider, PostgresProviderConfig,
};
use serde_json::{Value, json};
use tempfile::tempdir;

use super::install_table_projection_observer;
use super::publication::{
    ProjectionPublication, ProjectionPublicationOutcome, publish_table_projection_async,
};
use super::work::install_table_projection_observer_for_testing;
use crate::identity::system_tenant_id;
use crate::keys::table_document_id;
use crate::records::{ensure_system_tenant_async, record_table_state_for_generation_async};
use crate::schema::{PROJECTION_FENCE_TABLE, SystemTable};

struct ArmedProjectionAcknowledgementLoss {
    armed: AtomicBool,
    fired: AtomicBool,
    target_tenant: Option<TenantId>,
    target_table: Option<TableName>,
    target_projection_epoch: Option<String>,
}

impl ArmedProjectionAcknowledgementLoss {
    fn process_wide() -> Self {
        Self {
            armed: AtomicBool::new(false),
            fired: AtomicBool::new(false),
            target_tenant: None,
            target_table: None,
            target_projection_epoch: None,
        }
    }

    fn for_projection_publication(
        target_tenant: TenantId,
        target_table: TableName,
        target_projection_epoch: &str,
    ) -> Self {
        Self {
            target_tenant: Some(target_tenant),
            target_table: Some(target_table),
            target_projection_epoch: Some(target_projection_epoch.to_string()),
            ..Self::process_wide()
        }
    }

    fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }

    fn fired(&self) -> bool {
        self.fired.load(Ordering::Acquire)
    }
}

impl FaultInjector for ArmedProjectionAcknowledgementLoss {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point == FaultPoint::StorageCommitAfterVisibilityBeforeReturn
            && self.armed.load(Ordering::Acquire)
            && !self.fired.swap(true, Ordering::AcqRel)
        {
            return Err(nimbus_core::Error::storage(
                nimbus_core::StorageErrorKind::Transient,
                "injected projection publication acknowledgement loss",
            ));
        }
        Ok(())
    }

    fn check_for_tenant(&self, point: FaultPoint, tenant_id: &TenantId) -> nimbus_core::Result<()> {
        if self
            .target_tenant
            .as_ref()
            .is_some_and(|target| target != tenant_id)
        {
            return Ok(());
        }
        if self.target_table.is_some() || self.target_projection_epoch.is_some() {
            return Ok(());
        }
        self.check(point)
    }

    fn check_for_durable_records(
        &self,
        point: FaultPoint,
        tenant_id: &TenantId,
        records: &[nimbus_core::TenantEventRecord],
    ) -> nimbus_core::Result<()> {
        if self.target_table.as_ref().is_some_and(|target| {
            !records
                .iter()
                .flat_map(|record| &record.writes)
                .any(|write| &write.table == target)
        }) {
            return Ok(());
        }
        if self.target_projection_epoch.as_ref().is_some_and(|target| {
            !records
                .iter()
                .flat_map(|record| &record.writes)
                .any(|write| {
                    write.current.as_ref().is_some_and(|document| {
                        document
                            .fields
                            .get("projectionEpoch")
                            .and_then(Value::as_str)
                            == Some(target.as_str())
                    })
                })
        }) {
            return Ok(());
        }
        if self
            .target_tenant
            .as_ref()
            .is_some_and(|target| target != tenant_id)
        {
            return Ok(());
        }
        self.check(point)
    }
}

fn value_schema(table: &TableName, include_note: bool) -> TableSchema {
    let mut fields = vec![FieldSchema {
        name: "value".to_string(),
        field_type: FieldType::Number,
        required: true,
    }];
    if include_note {
        fields.push(FieldSchema {
            name: "note".to_string(),
            field_type: FieldType::String,
            required: false,
        });
    }
    TableSchema {
        table: table.clone(),
        fields,
        indexes: Vec::new(),
        access_policy: None,
    }
}

async fn projected_row(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Option<Document> {
    engine
        .get_document_async(
            system_tenant_id().ok()?,
            SystemTable::Tables.table_name().ok()?,
            DocumentId::from_key(table_document_id(tenant_id, table)).ok()?,
        )
        .await
        .ok()
}

async fn fence_row(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Option<Document> {
    engine
        .get_document_async(
            system_tenant_id().ok()?,
            TableName::new(PROJECTION_FENCE_TABLE).ok()?,
            DocumentId::from_key(table_document_id(tenant_id, table)).ok()?,
        )
        .await
        .ok()
}

async fn indexed_projected_rows(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Vec<Document> {
    engine
        .query_documents_async(
            system_tenant_id().expect("system tenant id should build"),
            Query {
                table: SystemTable::Tables
                    .table_name()
                    .expect("system tables name should build"),
                filters: vec![
                    Filter {
                        field: "tenantId".to_string(),
                        op: FilterOp::Eq,
                        value: json!(tenant_id.as_str()),
                    },
                    Filter {
                        field: "name".to_string(),
                        op: FilterOp::Eq,
                        value: json!(table.as_str()),
                    },
                ],
                order: None,
                limit: None,
            },
        )
        .await
        .expect("the projected table composite index should remain readable")
}

async fn wait_for_row_count(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
    expected: u64,
) -> Document {
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(row) = projected_row(engine, tenant_id, table).await
                && row.fields.get("rowCount").and_then(Value::as_u64) == Some(expected)
            {
                return row;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    match result {
        Ok(row) => row,
        Err(error) => {
            let source_snapshot = engine
                .projection_reconciliation_snapshot_async(tenant_id)
                .await
                .map(|snapshot| (snapshot.active_tables, snapshot.projection_token));
            let diagnostics = engine
                .tenant_engine_diagnostics(tenant_id)
                .map(|snapshot| snapshot.mutation_journal);
            let visible_row = projected_row(engine, tenant_id, table).await;
            panic!(
                "runtime reconciliation should publish without another source mutation: {error:?}; \
                 source_snapshot={source_snapshot:?}; diagnostics={diagnostics:?}; \
                 visible_row={visible_row:?}"
            );
        }
    }
}

async fn wait_for_schema(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
    expected: &TableSchema,
) {
    let expected = serde_json::to_value(expected).expect("schema should serialize");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if projected_row(engine, tenant_id, table)
                .await
                .as_ref()
                .and_then(|row| row.fields.get("schema"))
                == Some(&expected)
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("schema projection should converge");
}

async fn wait_for_deleted_fence(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> Document {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if projected_row(engine, tenant_id, table).await.is_none()
                && let Some(fence) = fence_row(engine, tenant_id, table).await
                && fence.fields.get("deleted") == Some(&json!(true))
            {
                return fence;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("table deletion should leave a durable private fence")
}

async fn prepare_two_engine_projection_contract(
    engine_a: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> (DocumentId, ProjectionToken) {
    engine_a
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("source tenant should create");
    engine_a
        .set_table_schema_async(tenant_id.clone(), value_schema(table, false))
        .await
        .expect("initial source schema should commit");
    let first_document = engine_a
        .insert_document_async(
            tenant_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([("value".to_string(), json!(1))]),
        )
        .await
        .expect("first source document should commit");
    let first = wait_for_row_count(engine_a, tenant_id, table, 1).await;
    engine_a
        .flush_committed_mutation_observers_for_testing(tenant_id)
        .await
        .expect("first engine projection work should drain before lease handoff");
    let old_token = ProjectionToken {
        tenant_incarnation: first
            .fields
            .get("sourceTenantIncarnation")
            .and_then(Value::as_u64)
            .expect("projected row should carry a source tenant incarnation"),
        lease_epoch: first
            .fields
            .get("sourceLeaseEpoch")
            .and_then(Value::as_u64)
            .expect("projected row should carry a source lease epoch"),
        durable_sequence: SequenceNumber(
            first
                .fields
                .get("sourceDurableSequence")
                .and_then(Value::as_u64)
                .expect("projected row should carry a source durable sequence"),
        ),
    };
    (first_document, old_token)
}

async fn prepare_unprojected_provider_restart_scope(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
) -> ProjectionToken {
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("restart source tenant should create");
    engine
        .insert_document_async(
            tenant_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([("value".to_string(), json!(1))]),
        )
        .await
        .expect("restart source document should commit without an observer");
    engine
        .shutdown_trigger_candidates_for_testing(tenant_id)
        .expect("restart source trigger cursor should stop before the process exits");
    engine
        .flush_tenant_committer_for_testing(tenant_id)
        .await
        .expect("restart source commit should be durable before the process exits");
    let source_token = engine
        .projection_token_for_tenant_async(tenant_id)
        .await
        .expect("restart source token should read before the process exits");
    assert!(
        projected_row(engine, tenant_id, table).await.is_none(),
        "a process with no projection observer must leave the durable source scope unprojected"
    );
    source_token
}

fn stop_old_engine_lease_renewal(engine: &Engine, tenant_id: &TenantId) {
    engine
        .pause_committer_lease_renewal_for_testing(tenant_id)
        .expect("old source lease renewal should stop before forced takeover");
    engine
        .pause_committer_lease_renewal_for_testing(
            &system_tenant_id().expect("system tenant id should build"),
        )
        .expect("old system-tenant lease renewal should stop before forced takeover");
}

async fn assert_provider_restart_reconciles_scope(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
    pre_restart_token: ProjectionToken,
) {
    install_table_projection_observer(engine);
    engine
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .expect("the restarted provider runtime should load");
    let recovered_token = engine
        .projection_token_for_tenant_async(tenant_id)
        .await
        .expect("the restarted provider source token should read");
    let recovered_snapshot = engine
        .projection_reconciliation_snapshot_async(tenant_id)
        .await
        .expect("the restarted provider source snapshot should read");
    assert!(
        recovered_snapshot.active_tables.contains(table),
        "provider restart must recover the active source table before projection reconciliation"
    );
    assert!(
        recovered_token >= pre_restart_token,
        "provider restart must not regress the source projection token"
    );
    let row = wait_for_row_count(engine, tenant_id, table, 1).await;
    assert_eq!(
        row.fields.get("sourceLeaseEpoch").and_then(Value::as_u64),
        Some(recovered_token.lease_epoch)
    );
    assert_eq!(
        row.fields
            .get("sourceDurableSequence")
            .and_then(Value::as_u64),
        Some(recovered_token.durable_sequence.0)
    );
    assert_eq!(
        indexed_projected_rows(engine, tenant_id, table).await,
        vec![row],
        "restart reconciliation must atomically restore the visible row and index"
    );
    assert!(fence_row(engine, tenant_id, table).await.is_some());
}

async fn finish_two_engine_projection_contract(
    engine_b: &Arc<Engine>,
    acknowledgement_loss: &ArmedProjectionAcknowledgementLoss,
    tenant_id: &TenantId,
    table: &TableName,
    first_document: DocumentId,
    old_token: ProjectionToken,
) {
    ensure_system_tenant_async(engine_b)
        .await
        .expect("takeover engine should hold the system-tenant lease before source work resumes");
    engine_b
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .expect("takeover engine should load the source tenant");
    let second_document = engine_b
        .insert_document_async(
            tenant_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([("value".to_string(), json!(2))]),
        )
        .await
        .expect("takeover engine should commit a newer source document");
    let takeover_token = engine_b
        .projection_token_for_tenant_async(tenant_id)
        .await
        .expect("takeover source token should read");
    assert!(
        takeover_token.lease_epoch > old_token.lease_epoch,
        "takeover must advance the durable source lease epoch"
    );

    engine_b
        .shutdown_trigger_candidates_for_testing(tenant_id)
        .expect("source trigger cursor should not consume the projection fault seam");
    engine_b
        .flush_tenant_committer_for_testing(tenant_id)
        .await
        .expect("source committer work should drain before the projection fault is armed");
    let system_tenant = system_tenant_id().expect("system tenant id should build");
    engine_b
        .shutdown_trigger_candidates_for_testing(&system_tenant)
        .expect("system trigger cursor should not consume the projection fault seam");
    engine_b
        .flush_tenant_committer_for_testing(&system_tenant)
        .await
        .expect("system committer work should drain before the projection fault is armed");
    let takeover_fields = serde_json::Map::from_iter([
        ("tenantId".to_string(), json!(tenant_id.as_str())),
        ("name".to_string(), json!(table.as_str())),
        ("rowCount".to_string(), json!(2)),
        ("lastWriteAt".to_string(), json!(0)),
        ("projectionEpoch".to_string(), json!("provider-ack-loss")),
        ("projectionGeneration".to_string(), json!(1)),
        (
            "sourceLeaseEpoch".to_string(),
            json!(takeover_token.lease_epoch),
        ),
        (
            "sourceDurableSequence".to_string(),
            json!(takeover_token.durable_sequence.0),
        ),
        (
            "schema".to_string(),
            serde_json::to_value(value_schema(table, false)).expect("schema should serialize"),
        ),
    ]);
    acknowledgement_loss.arm();
    let ambiguous = match publish_table_projection_async(
        engine_b,
        ProjectionPublication {
            tenant_id: tenant_id.clone(),
            table: table.clone(),
            token: takeover_token,
            visible_fields: takeover_fields.clone(),
            delete_visible: false,
        },
    )
    .await
    {
        Err(error) => error,
        Ok(outcome) => panic!(
            "post-visibility acknowledgement loss must not report {outcome:?}; injector_fired={}",
            acknowledgement_loss.fired()
        ),
    };
    assert!(acknowledgement_loss.fired());
    assert!(
        ambiguous.to_string().contains("crash-and-replay"),
        "ambiguous projection publication must force durable recovery: {ambiguous}"
    );
    ensure_system_tenant_async(engine_b)
        .await
        .expect("ambiguous system-tenant publication should recover before replay");
    assert_eq!(
        publish_table_projection_async(
            engine_b,
            ProjectionPublication {
                tenant_id: tenant_id.clone(),
                table: table.clone(),
                token: takeover_token,
                visible_fields: takeover_fields,
                delete_visible: false,
            },
        )
        .await
        .expect("identical acknowledgement-loss replay should recover"),
        ProjectionPublicationOutcome::StaleNoOp
    );
    let newer = wait_for_row_count(engine_b, tenant_id, table, 2).await;
    assert_eq!(
        newer.fields.get("sourceLeaseEpoch").and_then(Value::as_u64),
        Some(takeover_token.lease_epoch)
    );
    assert_eq!(
        newer
            .fields
            .get("sourceDurableSequence")
            .and_then(Value::as_u64),
        Some(takeover_token.durable_sequence.0)
    );
    assert_eq!(
        indexed_projected_rows(engine_b, tenant_id, table).await,
        vec![newer.clone()],
        "the visible row and its composite index must publish atomically"
    );

    let mut stale_fields = newer.fields.clone();
    stale_fields.insert("rowCount".to_string(), json!(999));
    stale_fields.insert("schema".to_string(), json!({"revision": "stale"}));
    assert_eq!(
        publish_table_projection_async(
            engine_b,
            ProjectionPublication {
                tenant_id: tenant_id.clone(),
                table: table.clone(),
                token: old_token,
                visible_fields: stale_fields.clone(),
                delete_visible: false,
            },
        )
        .await
        .expect("stale document/schema publication should classify"),
        ProjectionPublicationOutcome::StaleNoOp
    );
    assert_eq!(
        publish_table_projection_async(
            engine_b,
            ProjectionPublication {
                tenant_id: tenant_id.clone(),
                table: table.clone(),
                token: old_token,
                visible_fields: serde_json::Map::new(),
                delete_visible: true,
            },
        )
        .await
        .expect("stale deletion should classify"),
        ProjectionPublicationOutcome::StaleNoOp
    );
    assert_eq!(
        projected_row(engine_b, tenant_id, table)
            .await
            .and_then(|row| row.fields.get("rowCount").and_then(Value::as_u64)),
        Some(2)
    );

    let projection_observer = install_table_projection_observer_for_testing(engine_b);
    let changed_schema = value_schema(table, true);
    engine_b
        .set_table_schema_async(tenant_id.clone(), changed_schema.clone())
        .await
        .expect("newer source schema should commit");
    engine_b
        .flush_committed_mutation_observers_for_testing(tenant_id)
        .await
        .expect("schema projection should drain");
    wait_for_schema(engine_b, tenant_id, table, &changed_schema).await;

    engine_b
        .delete_document_async(tenant_id.clone(), table.clone(), first_document)
        .await
        .expect("first source document should delete");
    engine_b
        .delete_document_async(tenant_id.clone(), table.clone(), second_document)
        .await
        .expect("second source document should delete");
    engine_b
        .delete_table_schema_async(tenant_id.clone(), table.clone())
        .await
        .expect("source schema should delete");
    engine_b
        .flush_committed_mutation_observers_for_testing(tenant_id)
        .await
        .expect("deletion projection should drain");
    let deletion_fence = wait_for_deleted_fence(engine_b, tenant_id, table).await;
    let deletion_token = ProjectionToken {
        tenant_incarnation: deletion_fence
            .fields
            .get("tenantIncarnation")
            .and_then(Value::as_u64)
            .expect("deletion fence should carry a tenant incarnation"),
        lease_epoch: deletion_fence
            .fields
            .get("leaseEpoch")
            .and_then(Value::as_u64)
            .expect("deletion fence should carry a lease epoch"),
        durable_sequence: SequenceNumber(
            deletion_fence
                .fields
                .get("durableSequence")
                .and_then(Value::as_u64)
                .expect("deletion fence should carry a durable sequence"),
        ),
    };
    let final_source_token = engine_b
        .projection_token_for_tenant_async(tenant_id)
        .await
        .expect("final source token should read");
    assert_eq!(deletion_token, final_source_token);
    assert!(deletion_token > takeover_token);
    assert_eq!(
        publish_table_projection_async(
            engine_b,
            ProjectionPublication {
                tenant_id: tenant_id.clone(),
                table: table.clone(),
                token: old_token,
                visible_fields: stale_fields,
                delete_visible: false,
            },
        )
        .await
        .expect("stale resurrection should classify"),
        ProjectionPublicationOutcome::StaleNoOp
    );
    assert!(projected_row(engine_b, tenant_id, table).await.is_none());
    assert!(
        indexed_projected_rows(engine_b, tenant_id, table)
            .await
            .is_empty(),
        "visible-row deletion must remove its composite index atomically"
    );

    let system_head = engine_b
        .latest_sequence_async(system_tenant.clone())
        .await
        .expect("system durable head should read");
    let journal = engine_b
        .read_durable_journal_async(system_tenant.clone(), SequenceNumber(1))
        .await
        .expect("system journal should read");
    assert_eq!(
        journal.last().map(|record| record.sequence),
        Some(system_head)
    );
    assert!(
        journal
            .windows(2)
            .all(|pair| pair[1].sequence.0 == pair[0].sequence.0 + 1),
        "system journal must remain a contiguous durable prefix"
    );

    let unrelated = TenantId::new(format!("{}-unrelated", tenant_id.as_str()))
        .expect("unrelated tenant id should build");
    engine_b
        .create_tenant_async(unrelated.clone())
        .await
        .expect("unrelated tenant should create");
    engine_b
        .insert_document_async(
            unrelated.clone(),
            table.clone(),
            serde_json::Map::from_iter([("value".to_string(), json!(7))]),
        )
        .await
        .expect("unrelated tenant should keep making progress");
    wait_for_row_count(engine_b, &unrelated, table, 1).await;

    projection_observer.cancel_next_projection_for_testing();
    projection_observer.project_tables_for_testing(
        tenant_id.clone(),
        vec![table.clone()],
        deletion_token,
    );
    engine_b
        .flush_committed_mutation_observers_for_testing(tenant_id)
        .await
        .expect("cancelled projection work should remain owned until stale replay drains");

    let diagnostics = engine_b
        .tenant_engine_diagnostics(tenant_id)
        .expect("source tenant diagnostics should load")
        .mutation_journal;
    assert_eq!(diagnostics.observer_spawned_work_token_lag_scope_count, 0);
    assert_eq!(diagnostics.observer_spawned_work_dirty_scope_count, 0);
    assert!(diagnostics.observer_spawned_work_stale_no_op_count >= 1);
    assert!(diagnostics.observer_spawned_work_delayed_retry_count >= 1);
}

async fn assert_provider_same_id_recreation_advances_projection_incarnation(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    table: &TableName,
    deleted_incarnation_token: ProjectionToken,
) {
    engine
        .delete_tenant_async(tenant_id.clone())
        .await
        .expect("provider source tenant should delete");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("provider source tenant should recreate with the same id");
    let recreated_schema = value_schema(table, false);
    engine
        .set_table_schema_async(tenant_id.clone(), recreated_schema.clone())
        .await
        .expect("recreated provider source should acquire provenance through a real mutation");
    engine
        .flush_committed_mutation_observers_for_testing(tenant_id)
        .await
        .expect("recreated provider projection should drain");
    wait_for_schema(engine, tenant_id, table, &recreated_schema).await;
    let recreated_token = engine
        .projection_token_for_tenant_async(tenant_id)
        .await
        .expect("recreated provider source token should resolve");
    assert!(
        recreated_token.tenant_incarnation > deleted_incarnation_token.tenant_incarnation,
        "provider metadata must durably advance tenant incarnation across same-id recreation"
    );
    assert_eq!(
        record_table_state_for_generation_async(
            engine,
            tenant_id,
            table,
            deleted_incarnation_token,
            "provider-deleted-incarnation",
            2,
        )
        .await
        .expect("late deleted-incarnation projection should classify"),
        ProjectionPublicationOutcome::StaleNoOp
    );
    let fence = fence_row(engine, tenant_id, table)
        .await
        .expect("recreated provider projection should retain a fence");
    assert_eq!(
        fence
            .fields
            .get("tenantIncarnation")
            .and_then(Value::as_u64),
        Some(recreated_token.tenant_incarnation)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_embedded_restart_reconciles_cancelled_scope() {
    let data = tempdir().expect("data dir should create");
    let tenant_id = TenantId::new("projection-restart-cancelled").unwrap();
    let table = TableName::new("tasks").unwrap();
    let engine_a = Arc::new(Engine::new(data.path()).unwrap());
    engine_a.create_tenant(tenant_id.clone()).unwrap();
    engine_a
        .insert_document_async(
            tenant_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([("value".to_string(), json!(1))]),
        )
        .await
        .unwrap();
    engine_a.quiesce().await;
    drop(engine_a);

    let engine_b = Arc::new(Engine::new(data.path()).unwrap());
    install_table_projection_observer(&engine_b);
    engine_b
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .expect("the restarted source runtime should load");
    let expected_token = engine_b
        .projection_token_for_tenant_async(&tenant_id)
        .await
        .expect("the restarted source token should read");
    let row = wait_for_row_count(&engine_b, &tenant_id, &table, 1).await;
    assert_eq!(
        row.fields.get("sourceLeaseEpoch").and_then(Value::as_u64),
        Some(expected_token.lease_epoch)
    );
    assert_eq!(
        row.fields
            .get("sourceDurableSequence")
            .and_then(Value::as_u64),
        Some(expected_token.durable_sequence.0)
    );
    assert!(fence_row(&engine_b, &tenant_id, &table).await.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_observer_installation_reconciles_already_loaded_runtime() {
    let data = tempdir().expect("data dir should create");
    let tenant_id = TenantId::new("projection-install-reconcile").unwrap();
    let table = TableName::new("tasks").unwrap();
    let engine = Arc::new(Engine::new(data.path()).unwrap());
    engine.create_tenant(tenant_id.clone()).unwrap();
    engine
        .insert_document_async(
            tenant_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([("value".to_string(), json!(1))]),
        )
        .await
        .unwrap();
    assert!(projected_row(&engine, &tenant_id, &table).await.is_none());

    install_table_projection_observer(&engine);
    wait_for_row_count(&engine, &tenant_id, &table, 1).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_deleted_table_tombstone_reconciles_on_reload() {
    let data = tempdir().expect("data dir should create");
    let tenant_id = TenantId::new("projection-restart-tombstone").unwrap();
    let table = TableName::new("tasks").unwrap();
    let engine_a = Arc::new(Engine::new(data.path()).unwrap());
    engine_a.create_tenant(tenant_id.clone()).unwrap();
    let document = engine_a
        .insert_document_async(
            tenant_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([("value".to_string(), json!(1))]),
        )
        .await
        .unwrap();
    ensure_system_tenant_async(&engine_a).await.unwrap();
    record_table_state_for_generation_async(
        &engine_a,
        &tenant_id,
        &table,
        ProjectionToken {
            tenant_incarnation: 1,
            lease_epoch: 0,
            durable_sequence: nimbus_core::SequenceNumber(1),
        },
        "before-restart",
        1,
    )
    .await
    .unwrap();
    engine_a
        .delete_document_async(tenant_id.clone(), table.clone(), document)
        .await
        .unwrap();
    record_table_state_for_generation_async(
        &engine_a,
        &tenant_id,
        &table,
        ProjectionToken {
            tenant_incarnation: 1,
            lease_epoch: 0,
            durable_sequence: nimbus_core::SequenceNumber(2),
        },
        "before-restart",
        2,
    )
    .await
    .unwrap();
    assert!(projected_row(&engine_a, &tenant_id, &table).await.is_none());
    assert_eq!(
        fence_row(&engine_a, &tenant_id, &table)
            .await
            .unwrap()
            .fields
            .get("deleted"),
        Some(&json!(true))
    );
    engine_a.quiesce().await;
    drop(engine_a);

    let engine_b = Arc::new(Engine::new(data.path()).unwrap());
    install_table_projection_observer(&engine_b);
    engine_b
        .ensure_tenant_exists_async(tenant_id.clone())
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let fence = fence_row(&engine_b, &tenant_id, &table).await;
            if projected_row(&engine_b, &tenant_id, &table).await.is_none()
                && fence.as_ref().and_then(|row| row.fields.get("deleted")) == Some(&json!(true))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("restart reconciliation must retain the durable deletion tombstone");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_provider_restart_reconciles_cancelled_scope() {
    let Some(connection_string) = postgres_connection_string() else {
        return;
    };
    let suffix = format!(
        "{}_{}",
        std::process::id(),
        nimbus_core::clock::system_now_millis()
    );
    let metadata_schema = format!("nimbus_projection_{}", suffix);
    let tenant_schema_prefix = format!("projection_{}__", suffix);
    let provider_config = PostgresProviderConfig {
        connection_string: connection_string.clone(),
        metadata_schema: metadata_schema.clone(),
        tenant_schema_prefix: tenant_schema_prefix.clone(),
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let control_a = tempdir().unwrap();
    let control_b = tempdir().unwrap();
    let provider = TenantProviderConfig {
        dialect: PersistenceDialect::Postgres,
        topology: PersistenceTopology::ExternalPrimary,
        routing: TenantRoutingConfig::SchemaPerTenant {
            metadata_schema: metadata_schema.clone(),
            tenant_schema_prefix: tenant_schema_prefix.clone(),
        },
        pool: PoolConfig {
            min_connections: Some(1),
            max_connections: Some(4),
        },
        credentials: ProviderCredentials::ConnectionString(connection_string),
    };
    let config_a = EnginePersistenceConfig {
        tenant_provider: provider.clone(),
        control_plane: ControlPlaneConfig::embedded_redb(control_a.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };
    let config_b = EnginePersistenceConfig {
        tenant_provider: provider,
        control_plane: ControlPlaneConfig::embedded_redb(control_b.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };
    let tenant_id = TenantId::new(format!("projection-provider-{suffix}")).unwrap();
    let restart_tenant = TenantId::new(format!("projection-provider-restart-{suffix}")).unwrap();
    let table = TableName::new("tasks").unwrap();
    let engine_before_restart = Arc::new(
        Engine::new_with_persistence_config(config_a.clone())
            .await
            .expect("pre-restart PostgreSQL engine should create"),
    );
    let pre_restart_token =
        prepare_unprojected_provider_restart_scope(&engine_before_restart, &restart_tenant, &table)
            .await;
    engine_before_restart.quiesce().await;
    drop(engine_before_restart);

    let acknowledgement_loss = Arc::new(ArmedProjectionAcknowledgementLoss::process_wide());
    let engine_a = Arc::new(Engine::new_with_persistence_config(config_a).await.unwrap());
    assert_provider_restart_reconciles_scope(&engine_a, &restart_tenant, &table, pre_restart_token)
        .await;
    let engine_b = Arc::new(
        Engine::new_with_simulation_and_persistence_config(
            config_b,
            Arc::new(nimbus_core::SystemWallClock),
            acknowledgement_loss.clone(),
        )
        .await
        .unwrap(),
    );
    let (first_document, old_token) =
        prepare_two_engine_projection_contract(&engine_a, &tenant_id, &table).await;

    stop_old_engine_lease_renewal(&engine_a, &tenant_id);
    expire_postgres_lease(&provider_config, &tenant_id).await;
    expire_postgres_lease(&provider_config, &system_tenant_id().unwrap()).await;
    finish_two_engine_projection_contract(
        &engine_b,
        &acknowledgement_loss,
        &tenant_id,
        &table,
        first_document,
        old_token,
    )
    .await;

    engine_a.quiesce().await;
    assert_provider_same_id_recreation_advances_projection_incarnation(
        &engine_b, &tenant_id, &table, old_token,
    )
    .await;
    engine_b.quiesce().await;
    PostgresProvider::connect(provider_config)
        .await
        .unwrap()
        .drop_metadata_schema_for_test()
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_libsql_two_engine_takeover_rejects_late_old_document_schema_and_delete() {
    let Some((primary_url, admin_api_url, auth_token, admin_auth_header)) =
        libsql_connection_settings()
    else {
        return;
    };
    let suffix = format!(
        "{:x}{:x}",
        std::process::id(),
        nimbus_core::clock::system_now_millis()
    );
    let metadata_namespace = format!("nimbus_projection_{}", &suffix[..suffix.len().min(16)]);
    let tenant_namespace_prefix = format!("projection_{}__", &suffix[..suffix.len().min(12)]);
    let replica_a = tempdir().expect("first libSQL replica cache should create");
    let replica_b = tempdir().expect("second libSQL replica cache should create");
    let control_a = tempdir().expect("first libSQL control dir should create");
    let control_b = tempdir().expect("second libSQL control dir should create");
    let provider_config = LibsqlReplicaProviderConfig {
        primary_url: primary_url.clone(),
        auth_token: auth_token.clone(),
        admin_api_url: admin_api_url.clone(),
        admin_auth_header: admin_auth_header.clone(),
        metadata_namespace: metadata_namespace.clone(),
        tenant_namespace_prefix: tenant_namespace_prefix.clone(),
        replica_cache_dir: replica_a.path().to_path_buf(),
        encryption_provider: None,
    };
    let provider = TenantProviderConfig {
        dialect: PersistenceDialect::Sqlite,
        topology: PersistenceTopology::ExternalPrimaryWithReplicas,
        routing: TenantRoutingConfig::NamespacePerTenant {
            metadata_namespace,
            tenant_namespace_prefix,
            replica_cache_dir: replica_a.path().to_path_buf(),
        },
        pool: PoolConfig::default(),
        credentials: ProviderCredentials::LibsqlReplica {
            primary_url,
            auth_token,
            admin_api_url,
            admin_auth_header,
        },
    };
    let config_a = EnginePersistenceConfig {
        tenant_provider: provider.clone(),
        control_plane: ControlPlaneConfig::embedded_redb(control_a.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };
    let config_b = EnginePersistenceConfig {
        tenant_provider: TenantProviderConfig {
            routing: TenantRoutingConfig::NamespacePerTenant {
                metadata_namespace: provider_config.metadata_namespace.clone(),
                tenant_namespace_prefix: provider_config.tenant_namespace_prefix.clone(),
                replica_cache_dir: replica_b.path().to_path_buf(),
            },
            ..provider
        },
        control_plane: ControlPlaneConfig::embedded_redb(control_b.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };
    let tenant_id = TenantId::new(format!("projection-libsql-{suffix}")).unwrap();
    let restart_tenant = TenantId::new(format!("projection-libsql-restart-{suffix}")).unwrap();
    let table = TableName::new("tasks").unwrap();
    let engine_before_restart = Arc::new(
        Engine::new_with_persistence_config(config_a.clone())
            .await
            .expect("pre-restart libSQL engine should create"),
    );
    let pre_restart_token =
        prepare_unprojected_provider_restart_scope(&engine_before_restart, &restart_tenant, &table)
            .await;
    engine_before_restart.quiesce().await;
    drop(engine_before_restart);

    let acknowledgement_loss = Arc::new(
        ArmedProjectionAcknowledgementLoss::for_projection_publication(
            system_tenant_id().expect("system tenant id should build"),
            TableName::new(PROJECTION_FENCE_TABLE).expect("projection fence table should build"),
            "provider-ack-loss",
        ),
    );
    let engine_a = Arc::new(
        Engine::new_with_persistence_config(config_a)
            .await
            .expect("first libSQL engine should create"),
    );
    assert_provider_restart_reconciles_scope(&engine_a, &restart_tenant, &table, pre_restart_token)
        .await;
    let engine_b = Arc::new(
        Engine::new_with_simulation_and_persistence_config_and_libsql_faults(
            config_b,
            Arc::new(nimbus_core::SystemWallClock),
            acknowledgement_loss.clone(),
            Arc::new(NoopFaultInjector),
        )
        .await
        .expect("second libSQL engine should create"),
    );
    let (first_document, old_token) =
        prepare_two_engine_projection_contract(&engine_a, &tenant_id, &table).await;

    stop_old_engine_lease_renewal(&engine_a, &tenant_id);
    expire_libsql_lease(&provider_config, &tenant_id).await;
    expire_libsql_lease(&provider_config, &system_tenant_id().unwrap()).await;
    finish_two_engine_projection_contract(
        &engine_b,
        &acknowledgement_loss,
        &tenant_id,
        &table,
        first_document,
        old_token,
    )
    .await;

    engine_a.quiesce().await;
    assert_provider_same_id_recreation_advances_projection_incarnation(
        &engine_b, &tenant_id, &table, old_token,
    )
    .await;
    engine_b.quiesce().await;
    LibsqlReplicaProvider::connect(provider_config)
        .await
        .expect("libSQL cleanup provider should connect")
        .drop_provider_namespaces_for_test()
        .await
        .expect("libSQL projection namespaces should clean up");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn projection_mysql_two_engine_takeover_rejects_late_old_document_schema_and_delete() {
    let Some(connection_string) = mysql_connection_string() else {
        return;
    };
    let suffix = format!(
        "{:x}{:x}",
        std::process::id(),
        nimbus_core::clock::system_now_millis()
    );
    let metadata_database = format!("nimbus_projection_{}", &suffix[..suffix.len().min(16)]);
    let tenant_database_prefix = format!("projection_{}__", &suffix[..suffix.len().min(12)]);
    let provider_config = MySqlProviderConfig {
        connection_string: connection_string.clone(),
        metadata_database: metadata_database.clone(),
        tenant_database_prefix: tenant_database_prefix.clone(),
        min_connections: Some(1),
        max_connections: Some(4),
    };
    let provider = TenantProviderConfig {
        dialect: PersistenceDialect::MySql,
        topology: PersistenceTopology::ExternalPrimary,
        routing: TenantRoutingConfig::DatabasePerTenant {
            metadata_database,
            tenant_database_prefix,
        },
        pool: PoolConfig {
            min_connections: Some(1),
            max_connections: Some(4),
        },
        credentials: ProviderCredentials::ConnectionString(connection_string),
    };
    let control_a = tempdir().expect("first MySQL control dir should create");
    let control_b = tempdir().expect("second MySQL control dir should create");
    let config_a = EnginePersistenceConfig {
        tenant_provider: provider.clone(),
        control_plane: ControlPlaneConfig::embedded_redb(control_a.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };
    let config_b = EnginePersistenceConfig {
        tenant_provider: provider,
        control_plane: ControlPlaneConfig::embedded_redb(control_b.path()),
        local_encryption: LocalEncryptionConfig::Disabled,
    };
    let tenant_id = TenantId::new(format!("projection-mysql-{suffix}")).unwrap();
    let restart_tenant = TenantId::new(format!("projection-mysql-restart-{suffix}")).unwrap();
    let table = TableName::new("tasks").unwrap();
    let engine_before_restart = Arc::new(
        Engine::new_with_persistence_config(config_a.clone())
            .await
            .expect("pre-restart MySQL engine should create"),
    );
    let pre_restart_token =
        prepare_unprojected_provider_restart_scope(&engine_before_restart, &restart_tenant, &table)
            .await;
    engine_before_restart.quiesce().await;
    drop(engine_before_restart);

    let acknowledgement_loss = Arc::new(ArmedProjectionAcknowledgementLoss::process_wide());
    let engine_a = Arc::new(
        Engine::new_with_persistence_config(config_a)
            .await
            .expect("first MySQL engine should create"),
    );
    assert_provider_restart_reconciles_scope(&engine_a, &restart_tenant, &table, pre_restart_token)
        .await;
    let engine_b = Arc::new(
        Engine::new_with_simulation_and_persistence_config(
            config_b,
            Arc::new(nimbus_core::SystemWallClock),
            acknowledgement_loss.clone(),
        )
        .await
        .expect("second MySQL engine should create"),
    );
    let (first_document, old_token) =
        prepare_two_engine_projection_contract(&engine_a, &tenant_id, &table).await;

    stop_old_engine_lease_renewal(&engine_a, &tenant_id);
    expire_mysql_lease(&provider_config, &tenant_id).await;
    expire_mysql_lease(&provider_config, &system_tenant_id().unwrap()).await;
    finish_two_engine_projection_contract(
        &engine_b,
        &acknowledgement_loss,
        &tenant_id,
        &table,
        first_document,
        old_token,
    )
    .await;

    engine_a.quiesce().await;
    assert_provider_same_id_recreation_advances_projection_incarnation(
        &engine_b, &tenant_id, &table, old_token,
    )
    .await;
    engine_b.quiesce().await;
    MySqlProvider::connect(provider_config)
        .await
        .expect("MySQL cleanup provider should connect")
        .drop_provider_databases_for_test()
        .await
        .expect("MySQL projection databases should clean up");
}

fn libsql_connection_settings() -> Option<(String, String, Option<String>, Option<String>)> {
    match external_provider_fixture_mode(
        "libsql",
        "libSQL projection provider",
        &["NIMBUS_LIBSQL_URL", "NIMBUS_LIBSQL_ADMIN_URL"],
    ) {
        ExternalProviderFixtureMode::UseExplicit => Some((
            std::env::var("NIMBUS_LIBSQL_URL")
                .expect("fixture policy should require the libSQL primary URL"),
            std::env::var("NIMBUS_LIBSQL_ADMIN_URL")
                .expect("fixture policy should require the libSQL admin URL"),
            std::env::var("NIMBUS_LIBSQL_AUTH_TOKEN").ok(),
            std::env::var("NIMBUS_LIBSQL_ADMIN_AUTH_HEADER").ok(),
        )),
        ExternalProviderFixtureMode::Omit => None,
    }
}

fn mysql_connection_string() -> Option<String> {
    match external_provider_fixture_mode(
        "mysql",
        "MySQL projection provider",
        &["NIMBUS_MYSQL_URL"],
    ) {
        ExternalProviderFixtureMode::UseExplicit => Some(
            std::env::var("NIMBUS_MYSQL_URL").expect("fixture policy should require the MySQL URL"),
        ),
        ExternalProviderFixtureMode::Omit => None,
    }
}

fn postgres_connection_string() -> Option<String> {
    match external_provider_fixture_mode(
        "postgres",
        "PostgreSQL projection provider",
        &["NIMBUS_TEST_POSTGRES_URL"],
    ) {
        ExternalProviderFixtureMode::UseExplicit => Some(
            std::env::var("NIMBUS_TEST_POSTGRES_URL")
                .expect("fixture policy should require the PostgreSQL URL"),
        ),
        ExternalProviderFixtureMode::Omit => None,
    }
}

async fn expire_postgres_lease(config: &PostgresProviderConfig, tenant_id: &TenantId) {
    let provider = PostgresProvider::connect(config.clone()).await.unwrap();
    let schema = provider.tenant_schema_name(tenant_id).unwrap();
    let (client, connection) =
        tokio_postgres::connect(&config.connection_string, tokio_postgres::NoTls)
            .await
            .unwrap();
    let connection = tokio::spawn(async move {
        let _ = connection.await;
    });
    let query = format!(
        "UPDATE \"{schema}\".\"committer_lease\" \
         SET expires_at = CURRENT_TIMESTAMP - INTERVAL '1 second' WHERE singleton = TRUE"
    );
    assert_eq!(client.execute(&query, &[]).await.unwrap(), 1);
    connection.abort();
}

async fn expire_libsql_lease(config: &LibsqlReplicaProviderConfig, tenant_id: &TenantId) {
    let provider = LibsqlReplicaProvider::connect(config.clone())
        .await
        .expect("libSQL inspection provider should connect");
    let namespace = provider
        .tenant_namespace(tenant_id)
        .expect("libSQL tenant namespace should build");
    let database = Builder::new_remote(
        config.primary_url.clone(),
        config.auth_token.clone().unwrap_or_default(),
    )
    .namespace(namespace)
    .connector(libsql_transport_connector().expect("libSQL transport connector should build"))
    .build()
    .await
    .expect("libSQL tenant namespace should open");
    let connection = database
        .connect()
        .expect("libSQL tenant connection should open");
    assert_eq!(
        connection
            .execute(
                "UPDATE committer_lease SET expires_at = 0 WHERE singleton = 1",
                (),
            )
            .await
            .expect("libSQL lease should expire"),
        1
    );
}

async fn expire_mysql_lease(config: &MySqlProviderConfig, tenant_id: &TenantId) {
    let provider = MySqlProvider::connect(config.clone())
        .await
        .expect("MySQL inspection provider should connect");
    let database = provider
        .tenant_database_name(tenant_id)
        .expect("MySQL tenant database should build");
    let options = mysql_async::Opts::from_url(&config.connection_string)
        .expect("MySQL connection URL should parse");
    let pool = mysql_async::Pool::new(options);
    let mut connection = pool
        .get_conn()
        .await
        .expect("MySQL inspection connection should open");
    let query = format!(
        "UPDATE `{database}`.`committer_lease` \
         SET expires_at = CURRENT_TIMESTAMP(6) - INTERVAL 1 SECOND WHERE singleton = TRUE"
    );
    connection
        .query_drop(query)
        .await
        .expect("MySQL lease should expire");
    assert_eq!(connection.affected_rows(), 1);
    drop(connection);
    pool.disconnect()
        .await
        .expect("MySQL inspection pool should disconnect");
}
