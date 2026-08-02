use nimbus_core::{
    Error, FieldType, Filter, FilterOp, OrderBy, OrderDirection, PrincipalContext, Query,
    SequenceNumber,
};
use nimbus_engine::{Fault, commit_fault_labels};
use nimbus_workloads::{
    WorkloadSagaCommit, WorkloadSagaExpected, WorkloadSagaStore, WorkloadSagaStoreError,
};

use super::super::EngineWorkloadSagaStore;
use super::super::schema::{exact_table_schema, workload_saga_table, workload_saga_tenant};
use super::{document_for, engine, initial_record};

#[test]
fn exact_schema_has_nineteen_fields_four_indexes_and_system_policy() {
    let schema = exact_table_schema();
    assert_eq!(schema.fields.len(), 19);
    assert_eq!(
        schema
            .fields
            .iter()
            .find(|field| field.name == "compiledNetworkPlan")
            .map(|field| field.field_type),
        Some(FieldType::Object)
    );
    assert_eq!(
        schema
            .fields
            .iter()
            .map(|field| (field.name.as_str(), field.required))
            .collect::<Vec<_>>(),
        vec![
            ("formatVersion", true),
            ("sagaId", true),
            ("tenantId", true),
            ("workloadId", true),
            ("workloadKind", true),
            ("desiredState", true),
            ("desiredGeneration", true),
            ("desiredDigest", true),
            ("sagaRevision", true),
            ("phase", true),
            ("recoveryEligible", true),
            ("phaseDetail", true),
            ("compiledNetworkPlan", true),
            ("activationIntent", true),
            ("publicationIntent", true),
            ("admission", true),
            ("successorIntent", false),
            ("lastTransition", true),
            ("failure", false),
        ]
    );
    assert_eq!(
        schema
            .indexes
            .iter()
            .map(|index| (index.name.as_str(), index.fields.as_slice()))
            .collect::<Vec<_>>(),
        vec![
            (
                "by_tenantId_and_workloadId",
                &["tenantId".to_owned(), "workloadId".to_owned()][..]
            ),
            (
                "by_recovery",
                &["recoveryEligible".to_owned(), "sagaId".to_owned()][..]
            ),
            (
                "by_tenantId_and_phase",
                &["tenantId".to_owned(), "phase".to_owned()][..]
            ),
            (
                "by_desiredState_and_phase",
                &["desiredState".to_owned(), "phase".to_owned()][..]
            ),
        ]
    );
    let policy = schema.access_policy.expect("system policy is required");
    for rule in [policy.read, policy.create, policy.update, policy.delete] {
        assert!(rule.require_authenticated);
        assert_eq!(rule.predicates.len(), 1);
    }
}

#[tokio::test]
async fn exact_schema_prepare_and_replay_add_no_second_commit() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    store.prepare().await.expect("first prepare should succeed");
    let schema_before = engine
        .get_table_schema_async(
            workload_saga_tenant().unwrap(),
            workload_saga_table().unwrap(),
        )
        .await
        .expect("first installed schema should remain readable");
    let before = engine
        .read_durable_journal_async(
            nimbus_core::TenantId::new("_nimbus").expect("system tenant is valid"),
            SequenceNumber(0),
        )
        .await
        .expect("journal should read");
    store
        .prepare()
        .await
        .expect("second prepare should be exact");
    let schema_after = engine
        .get_table_schema_async(
            workload_saga_tenant().unwrap(),
            workload_saga_table().unwrap(),
        )
        .await
        .expect("replayed schema should remain readable");
    let after = engine
        .read_durable_journal_async(
            nimbus_core::TenantId::new("_nimbus").expect("system tenant is valid"),
            SequenceNumber(0),
        )
        .await
        .expect("journal should read");
    assert_eq!(after, before);
    assert_eq!(schema_after, schema_before);
    assert_eq!(
        schema_after
            .indexes
            .iter()
            .map(|index| &index.id)
            .collect::<Vec<_>>(),
        schema_before
            .indexes
            .iter()
            .map(|index| &index.id)
            .collect::<Vec<_>>(),
        "exact schema replay must preserve all durable index identities"
    );

    let record = initial_record("journal-replay");
    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, record.clone())
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );
    let committed = engine
        .read_durable_journal_async(
            nimbus_core::TenantId::new("_nimbus").expect("system tenant is valid"),
            SequenceNumber(0),
        )
        .await
        .expect("journal should read");
    assert_eq!(
        store
            .compare_and_swap(
                WorkloadSagaExpected::Revision(nimbus_workloads::WorkloadSagaRevision::new(88)),
                record,
            )
            .await,
        Ok(WorkloadSagaCommit::Unchanged)
    );
    let replayed = engine
        .read_durable_journal_async(
            nimbus_core::TenantId::new("_nimbus").expect("system tenant is valid"),
            SequenceNumber(0),
        )
        .await
        .expect("journal should read");
    assert_eq!(replayed, committed);
}

#[tokio::test]
async fn divergent_schema_fails_closed_without_replacement() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    store.prepare().await.expect("exact schema should prepare");
    let mut divergent = exact_table_schema();
    divergent.fields.pop();
    engine
        .set_table_schema_async(workload_saga_tenant().unwrap(), divergent.clone())
        .await
        .expect("test should install divergent schema explicitly");
    let installed = engine
        .get_table_schema_async(
            workload_saga_tenant().unwrap(),
            workload_saga_table().unwrap(),
        )
        .await
        .expect("divergent schema remains readable");

    assert_eq!(store.prepare().await, Err(WorkloadSagaStoreError::Corrupt));
    assert_eq!(
        engine
            .get_table_schema_async(
                workload_saga_tenant().unwrap(),
                workload_saga_table().unwrap()
            )
            .await
            .expect("divergent schema remains readable"),
        installed
    );
}

#[tokio::test]
async fn one_transition_atomically_publishes_document_indexes_and_one_journal_commit() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    store.prepare().await.expect("schema should prepare");
    let tenant = workload_saga_tenant().unwrap();
    let before = engine
        .read_durable_journal_async(tenant.clone(), SequenceNumber(0))
        .await
        .expect("journal should read before transition");
    let record = initial_record("atomic-transition");

    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, record.clone())
            .await,
        Ok(WorkloadSagaCommit::Applied)
    );

    let after = engine
        .read_durable_journal_async(tenant.clone(), SequenceNumber(0))
        .await
        .expect("journal should read after transition");
    assert_eq!(after.len(), before.len() + 1);
    let loaded = store
        .load(record.key())
        .await
        .expect("atomic record should load")
        .expect("atomic record should exist");
    assert_eq!(loaded, record);
    assert_eq!(
        serde_json::to_vec(loaded.active_intent().network())
            .expect("loaded compiled plan should encode"),
        serde_json::to_vec(record.active_intent().network())
            .expect("expected compiled plan should encode")
    );
    assert_all_index_projections(&engine, &record).await;
}

#[tokio::test]
async fn pre_persist_failure_leaves_no_document_index_or_journal_effect() {
    let root = tempfile::tempdir().expect("fixture root should build");
    let engine = engine(&root);
    let store = EngineWorkloadSagaStore::new(engine.clone());
    store.prepare().await.expect("schema should prepare");
    let tenant = workload_saga_tenant().unwrap();
    let before = engine
        .read_durable_journal_async(tenant.clone(), SequenceNumber(0))
        .await
        .expect("journal should read before transition");
    let record = initial_record("atomic-rollback");
    engine.commit_fault_handle_for_testing().inject(
        commit_fault_labels::PRE_PERSIST,
        Fault::Error(Error::Internal(
            "injected workload-saga pre-persist rollback".to_owned(),
        )),
    );

    assert_eq!(
        store
            .compare_and_swap(WorkloadSagaExpected::Missing, record.clone())
            .await,
        Err(WorkloadSagaStoreError::Ambiguous)
    );
    assert_eq!(store.load(record.key()).await, Ok(None));
    assert_eq!(
        engine
            .read_durable_journal_async(tenant, SequenceNumber(0))
            .await
            .expect("journal should remain readable"),
        before
    );
    assert_all_index_projections_are_empty(&engine, &record).await;
}

async fn assert_all_index_projections(
    engine: &std::sync::Arc<nimbus_engine::Engine>,
    record: &nimbus_workloads::WorkloadSagaRecord,
) {
    let expected_id = nimbus_core::DocumentId::from_key(record.saga_id().as_str())
        .expect("fixture saga id should be a document id");
    for query in index_queries(record) {
        let documents = engine
            .query_documents_async_with_principal(
                workload_saga_tenant().unwrap(),
                query,
                PrincipalContext::system(),
            )
            .await
            .expect("index projection should be queryable");
        assert_eq!(
            documents
                .iter()
                .map(|document| &document.id)
                .collect::<Vec<_>>(),
            vec![&expected_id]
        );
    }
}

async fn assert_all_index_projections_are_empty(
    engine: &std::sync::Arc<nimbus_engine::Engine>,
    record: &nimbus_workloads::WorkloadSagaRecord,
) {
    for query in index_queries(record) {
        assert!(
            engine
                .query_documents_async_with_principal(
                    workload_saga_tenant().unwrap(),
                    query,
                    PrincipalContext::system(),
                )
                .await
                .expect("empty index projection should remain queryable")
                .is_empty()
        );
    }
}

fn index_queries(record: &nimbus_workloads::WorkloadSagaRecord) -> [Query; 4] {
    let fields = document_for(record).fields;
    let value = |name: &str| {
        fields
            .get(name)
            .cloned()
            .unwrap_or_else(|| panic!("fixture field {name} should exist"))
    };
    let filter = |name: &str| Filter {
        field: name.to_owned(),
        op: FilterOp::Eq,
        value: value(name),
    };
    [
        Query {
            table: workload_saga_table().unwrap(),
            filters: vec![filter("tenantId"), filter("workloadId")],
            order: None,
            limit: None,
        },
        Query {
            table: workload_saga_table().unwrap(),
            filters: vec![filter("recoveryEligible")],
            order: Some(OrderBy {
                field: "sagaId".to_owned(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
        Query {
            table: workload_saga_table().unwrap(),
            filters: vec![filter("tenantId"), filter("phase")],
            order: None,
            limit: None,
        },
        Query {
            table: workload_saga_table().unwrap(),
            filters: vec![filter("desiredState"), filter("phase")],
            order: None,
            limit: None,
        },
    ]
}
