pub(crate) use neovex_core::{
    AccessValue, DocumentId, Error, FieldSchema, FieldType, Filter, FilterOp, IndexDefinition,
    OrderBy, OrderDirection, Page, PaginatedQuery, PrincipalContext, Query, SequenceNumber,
    TableAccessPolicy, TableName, TableSchema, TenantId, Timestamp,
};
pub(crate) use neovex_testing::{
    BlockingFaultInjector, GeneratedTaskHistory, GeneratedTaskHistorySeedCase,
    GeneratedTaskPageExpectation, GeneratedTaskRecord, ServiceFixture, VerificationHarnessMode,
    replay_generated_task_history_async, selected_generated_task_history_seed_corpus,
    wait_for_value,
};
pub(crate) use serde_json::json;
pub(crate) use std::collections::BTreeSet;
pub(crate) use std::future::Future;
pub(crate) use std::pin::Pin;
pub(crate) use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) use std::sync::{Arc, Barrier, Condvar, Mutex};
pub(crate) use std::task::{Context, Poll};
pub(crate) use tempfile::{TempDir, tempdir};
pub(crate) use tokio::sync::{Notify, mpsc};
pub(crate) use tokio::time::{Duration, timeout};

pub(crate) use crate::service::{
    SubscriptionBootstrapCancellation, paginate_documents_for_docs_with_principal,
    query_documents_for_docs_with_principal,
};
pub(crate) use crate::tenant::DOCUMENT_CACHE_CAPACITY;
pub(crate) use crate::test_support::{
    messages_schema, messages_table, owner_matches_subject_rule, owner_write_policy,
    principal_with_subject, read_only_owner_policy,
};
pub(crate) use crate::verification::{
    ConsistencyScope, collect_durable_journal_bootstrap_mismatches,
    compare_materialized_journal_snapshots,
};
pub(crate) use crate::{
    EmbeddedReplica, Service, ServicePersistenceConfig, ShadowMaterializerConfig,
    SubscriptionUpdate,
};
pub(crate) use neovex_storage::{
    DurableJournalBootstrap, EmbeddedProviderKind, FaultPoint, ManualClock, SqliteTenantStore,
    TenantStore,
};

mod consistency;
mod embedded_providers;
mod libsql_replica_provider;
mod materialized_serving;
mod mutation_journal;
mod mysql_provider;
mod policy;
mod postgres_provider;
mod queries;
mod subscriptions;

pub(crate) fn tasks_table() -> TableName {
    TableName::new("tasks").expect("table name should be valid")
}

pub(crate) fn query_for(table: &str) -> Query {
    Query {
        table: TableName::new(table).expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}

pub(crate) fn durable_journal_commits(
    service: &Service,
    tenant_id: &TenantId,
    after: SequenceNumber,
) -> Vec<neovex_core::CommitEntry> {
    service
        .read_durable_journal(tenant_id, after)
        .expect("durable journal should read")
        .into_iter()
        .map(|record| record.as_commit_entry())
        .collect()
}

pub(crate) fn subscription_channel() -> (
    mpsc::Sender<SubscriptionUpdate>,
    mpsc::Receiver<SubscriptionUpdate>,
) {
    mpsc::channel(16)
}

pub(crate) async fn wait_for_mutation_journal_stats(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    description: &str,
    predicate: impl Fn(&crate::tenant::MutationJournalStats) -> bool,
) -> crate::tenant::MutationJournalStats {
    wait_for_value(
        description,
        Duration::from_secs(1),
        Duration::ZERO,
        || async {
            service
                .mutation_journal_stats_for_testing(tenant_id)
                .expect("mutation journal stats should load")
        },
        predicate,
    )
    .await
}

pub(crate) async fn wait_for_mutation_admission_stats(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    description: &str,
    predicate: impl Fn(&crate::tenant::MutationAdmissionStats) -> bool,
) -> crate::tenant::MutationAdmissionStats {
    wait_for_value(
        description,
        Duration::from_secs(1),
        Duration::ZERO,
        || async {
            service
                .mutation_admission_stats_for_testing(tenant_id)
                .expect("mutation admission stats should load")
        },
        predicate,
    )
    .await
}

pub(crate) async fn wait_for_active_subscription_count(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    description: &str,
    expected_count: usize,
) -> usize {
    wait_for_value(
        description,
        Duration::from_secs(1),
        Duration::ZERO,
        || async {
            service
                .active_subscription_count(tenant_id)
                .expect("subscription count should load")
        },
        |count| *count == expected_count,
    )
    .await
}

pub(crate) fn filter(field: &str, op: FilterOp, value: serde_json::Value) -> Filter {
    Filter {
        field: field.to_string(),
        op,
        value,
    }
}

pub(crate) fn materialized_snapshot_with_documents(
    documents: Vec<neovex_core::Document>,
) -> crate::MaterializedJournalSnapshot {
    crate::MaterializedJournalSnapshot {
        version: 1,
        applied_sequence: SequenceNumber(1),
        durable_head: SequenceNumber(1),
        schema: neovex_core::Schema::default(),
        documents,
        scheduled_execution_ids: Vec::new(),
    }
}

pub(crate) fn users_schema() -> TableSchema {
    TableSchema {
        table: TableName::new("users").expect("table name should be valid"),
        fields: vec![
            FieldSchema {
                name: "name".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "age".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: Vec::new(),
        access_policy: None,
    }
}

pub(crate) async fn assert_generated_task_history_matches_model_across_surfaces(
    history: &GeneratedTaskHistory,
    case: Option<GeneratedTaskHistorySeedCase>,
    test_name: &str,
) {
    let context = |invariant: &str| {
        case.map(|case| case.failure_context("neovex-engine", test_name, invariant))
            .unwrap_or_else(|| history.failure_context(invariant, None))
    };

    let model = history.model();
    let expected_query = model.query_result();
    assert!(
        expected_query.len() > history.page_size(),
        "history seed should produce at least two query pages: {}",
        context("generated-history seed should produce at least two query pages")
    );

    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = TableName::new(history.table()).expect("generated task table should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    replay_generated_task_history_async(
        history,
        {
            let service = Arc::clone(&service);
            let tenant_id = tenant_id.clone();
            let table = table.clone();
            move |_slot, record| {
                let service = Arc::clone(&service);
                let tenant_id = tenant_id.clone();
                let table = table.clone();
                let fields = record.fields();
                async move {
                    service
                        .insert_document_async(tenant_id, table, fields)
                        .await
                }
            }
        },
        {
            let service = Arc::clone(&service);
            let tenant_id = tenant_id.clone();
            let table = table.clone();
            move |_slot, document_id, record| {
                let service = Arc::clone(&service);
                let tenant_id = tenant_id.clone();
                let table = table.clone();
                let fields = record.fields();
                async move {
                    service
                        .update_document_async(tenant_id, table, document_id, fields)
                        .await
                        .map(|_| ())
                }
            }
        },
        {
            let service = Arc::clone(&service);
            let tenant_id = tenant_id.clone();
            let table = table.clone();
            move |_slot, document_id| {
                let service = Arc::clone(&service);
                let tenant_id = tenant_id.clone();
                let table = table.clone();
                async move {
                    service
                        .delete_document_async(tenant_id, table, document_id)
                        .await
                }
            }
        },
    )
    .await
    .expect("generated history replay should succeed");

    let live_documents = normalize_generated_task_documents(
        service
            .list_documents(&tenant_id, &table)
            .expect("live list should succeed"),
    );
    assert_eq!(
        live_documents,
        model.final_documents(),
        "{}",
        context("live final state should match the generated-history oracle")
    );

    let ordered_query = history.ordered_query();
    let live_query = normalize_generated_task_documents(
        service
            .query_documents_async(tenant_id.clone(), ordered_query.clone())
            .await
            .expect("live query should succeed"),
    );
    assert_eq!(
        live_query,
        expected_query,
        "{}",
        context("live query should match the generated-history oracle")
    );

    let live_first_page = service
        .paginate_documents_async(tenant_id.clone(), history.paginated_query(None))
        .await
        .expect("live first page should succeed");
    assert_generated_task_page_matches(
        &live_first_page,
        &model.first_page(),
        &context("live first page should match the generated-history oracle"),
    );
    let live_second_page = service
        .paginate_documents_async(
            tenant_id.clone(),
            history.paginated_query(live_first_page.next_cursor.clone()),
        )
        .await
        .expect("live second page should succeed");
    assert_generated_task_page_matches(
        &live_second_page,
        &model.second_page(),
        &context("live second page should match the generated-history oracle"),
    );

    let shadow = service
        .build_shadow_materializer_async(
            tenant_id.clone(),
            ShadowMaterializerConfig {
                compaction_threshold_records: 2,
            },
        )
        .await
        .expect("shadow materializer should build");
    let snapshot = shadow.current_snapshot();
    let shadow_query = normalize_generated_task_documents(
        query_documents_for_docs_with_principal(
            snapshot.documents.clone(),
            &snapshot.schema,
            &ordered_query,
            &PrincipalContext::anonymous(),
        )
        .expect("shadow query should succeed"),
    );
    assert_eq!(
        shadow_query,
        expected_query,
        "{}",
        context("shadow query should match the generated-history oracle")
    );
    let shadow_first_page = paginate_documents_for_docs_with_principal(
        snapshot.documents.clone(),
        &snapshot.schema,
        &history.paginated_query(None),
        &PrincipalContext::anonymous(),
    )
    .expect("shadow first page should succeed");
    assert_generated_task_page_matches(
        &shadow_first_page,
        &model.first_page(),
        &context("shadow first page should match the generated-history oracle"),
    );
    let shadow_second_page = paginate_documents_for_docs_with_principal(
        snapshot.documents.clone(),
        &snapshot.schema,
        &history.paginated_query(shadow_first_page.next_cursor.clone()),
        &PrincipalContext::anonymous(),
    )
    .expect("shadow second page should succeed");
    assert_generated_task_page_matches(
        &shadow_second_page,
        &model.second_page(),
        &context("shadow second page should match the generated-history oracle"),
    );

    let replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("embedded replica should bootstrap");
    let replica_query = normalize_generated_task_documents(
        replica
            .query_documents(&ordered_query)
            .expect("replica query should succeed"),
    );
    assert_eq!(
        replica_query,
        expected_query,
        "{}",
        context("replica query should match the generated-history oracle")
    );
    let replica_first_page = replica
        .paginate_documents(&history.paginated_query(None))
        .expect("replica first page should succeed");
    assert_generated_task_page_matches(
        &replica_first_page,
        &model.first_page(),
        &context("replica first page should match the generated-history oracle"),
    );
    let replica_second_page = replica
        .paginate_documents(&history.paginated_query(replica_first_page.next_cursor.clone()))
        .expect("replica second page should succeed");
    assert_generated_task_page_matches(
        &replica_second_page,
        &model.second_page(),
        &context("replica second page should match the generated-history oracle"),
    );
}

pub(crate) fn document_bodies(documents: &[neovex_core::Document]) -> Vec<&str> {
    documents
        .iter()
        .map(|document| {
            document
                .get_field("body")
                .and_then(serde_json::Value::as_str)
                .expect("body should be present and a string")
        })
        .collect()
}

pub(crate) fn subscription_bodies(data: &[serde_json::Value]) -> Vec<&str> {
    data.iter()
        .map(|value| {
            value["body"]
                .as_str()
                .expect("subscription body should be present and a string")
        })
        .collect()
}

pub(crate) fn normalize_generated_task_documents(
    documents: Vec<neovex_core::Document>,
) -> Vec<GeneratedTaskRecord> {
    let mut records = documents
        .into_iter()
        .map(|document| GeneratedTaskRecord::from_json(&document.to_json()))
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.rank.cmp(&right.rank))
            .then_with(|| left.status.cmp(&right.status))
    });
    records
}

pub(crate) fn normalize_generated_task_values(
    values: Vec<serde_json::Value>,
) -> Vec<GeneratedTaskRecord> {
    let mut records = values
        .iter()
        .map(GeneratedTaskRecord::from_json)
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.title
            .cmp(&right.title)
            .then_with(|| left.rank.cmp(&right.rank))
            .then_with(|| left.status.cmp(&right.status))
    });
    records
}

pub(crate) fn assert_generated_task_page_matches(
    page: &Page,
    expected: &GeneratedTaskPageExpectation,
    context: &str,
) {
    assert_eq!(
        normalize_generated_task_values(page.data.clone()),
        expected.data,
        "{context}: page data should match the generated-history oracle",
    );
    assert_eq!(
        page.has_more, expected.has_more,
        "{context}: has_more should match the generated-history oracle",
    );
    assert_eq!(
        page.next_cursor.is_some(),
        expected.has_more,
        "{context}: next_cursor presence should track has_more",
    );
}

pub(crate) struct BlockingCancellationProbe {
    entered: Notify,
    cancel: Notify,
    released: Notify,
    cancelled: AtomicBool,
    first_check: AtomicBool,
    release_gate: (Mutex<bool>, Condvar),
}

pub(crate) struct DropAwarePendingCancellation {
    dropped: Arc<AtomicBool>,
}

impl Future for DropAwarePendingCancellation {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        Poll::Pending
    }
}

impl Drop for DropAwarePendingCancellation {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}

impl BlockingCancellationProbe {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Notify::new(),
            cancel: Notify::new(),
            released: Notify::new(),
            cancelled: AtomicBool::new(false),
            first_check: AtomicBool::new(true),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    pub(crate) async fn wait_for_first_check(&self) {
        self.entered.notified().await;
    }

    pub(crate) fn trigger_cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.cancel.notify_one();
    }

    pub(crate) fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking cancellation probe should acquire release lock");
        *released = true;
        cvar.notify_all();
    }

    pub(crate) async fn wait_until_released_from_first_check(&self) {
        self.released.notified().await;
    }

    pub(crate) async fn cancel_wait(self: Arc<Self>) {
        self.cancel.notified().await;
    }

    pub(crate) fn check(self: Arc<Self>) -> impl Fn() -> neovex_core::Result<()> + Send + 'static {
        move || {
            if self.first_check.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                let (lock, cvar) = &self.release_gate;
                let mut released = lock
                    .lock()
                    .expect("blocking cancellation probe should acquire release lock");
                while !*released {
                    released = cvar
                        .wait(released)
                        .expect("blocking cancellation probe should wait for release");
                }
                self.released.notify_one();
            }

            if self.cancelled.load(Ordering::SeqCst) {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        }
    }
}

pub(crate) async fn create_service_with_durable_unapplied_task(
    timestamp_ms: u64,
    title: &str,
) -> (
    TempDir,
    Arc<Service>,
    TenantId,
    Arc<BlockingFaultInjector>,
    DocumentId,
) {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(timestamp_ms))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let title = title.to_string();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!(title))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    drop(insert_handle);
    let document_id = durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0))
        .first()
        .and_then(|commit| commit.writes.first())
        .map(|write| write.doc_id)
        .expect("durable commit should include the inserted document id");

    (data_dir, service, tenant_id, faults, document_id)
}

#[tokio::test]
async fn service_create_duplicate_tenant_returns_already_exists() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let error = service
        .create_tenant(tenant_id)
        .expect_err("duplicate tenant should fail");
    assert!(matches!(error, Error::AlreadyExists(_)));
}

#[tokio::test]
async fn service_delete_nonexistent_tenant_returns_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");

    let error = service
        .delete_tenant(&tenant_id)
        .expect_err("missing tenant should fail");
    assert!(matches!(error, Error::TenantNotFound(_)));
}

#[tokio::test]
async fn service_missing_document_operations_return_not_found() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let missing_id = neovex_core::DocumentId::new();

    let get_error = service
        .get_document(&tenant_id, &tasks_table(), missing_id)
        .expect_err("missing get should fail");
    assert!(matches!(get_error, Error::DocumentNotFound(_)));

    let update_error = service
        .update_document(
            &tenant_id,
            tasks_table(),
            missing_id,
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect_err("missing update should fail");
    assert!(matches!(update_error, Error::DocumentNotFound(_)));

    let delete_error = service
        .delete_document(&tenant_id, tasks_table(), missing_id)
        .expect_err("missing delete should fail");
    assert!(matches!(delete_error, Error::DocumentNotFound(_)));
}

#[tokio::test]
async fn service_tenant_data_is_isolated_across_tenants() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let alpha_tenant = fixture.create_tenant("alpha", Service::create_tenant);
    let beta_tenant = fixture.create_tenant("beta", Service::create_tenant);

    service
        .insert_document(
            &alpha_tenant,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Alpha"))]),
        )
        .expect("insert should succeed");
    service
        .insert_document(
            &beta_tenant,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Beta"))]),
        )
        .expect("insert should succeed");

    let alpha_docs = service
        .list_documents(&alpha_tenant, &tasks_table())
        .expect("list should succeed");
    let beta_docs = service
        .list_documents(&beta_tenant, &tasks_table())
        .expect("list should succeed");

    assert_eq!(alpha_docs.len(), 1);
    assert_eq!(beta_docs.len(), 1);
    assert_eq!(alpha_docs[0].fields.get("title"), Some(&json!("Alpha")));
    assert_eq!(beta_docs[0].fields.get("title"), Some(&json!("Beta")));
}

#[tokio::test]
async fn service_lazy_loads_tenant_from_disk() {
    let data_dir = tempdir().expect("tempdir should create");
    let service = Service::new(data_dir.path()).expect("service should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Persisted"))]),
        )
        .expect("insert should succeed");

    drop(service);

    let reloaded = Service::new(data_dir.path()).expect("service should reopen");
    let documents = reloaded
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Persisted")));
}

#[tokio::test]
async fn service_unsubscribe_stops_notifications() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let (tx, mut rx) = subscription_channel();
    let subscription = service
        .subscribe(&tenant_id, query_for("tasks"), "req-unsub".to_string(), tx)
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
    let _ = rx.recv().await.expect("initial update should arrive");

    service
        .unsubscribe(&tenant_id, subscription_id)
        .expect("unsubscribe should succeed");
    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
        )
        .expect("insert should succeed");

    let result = timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(
        !matches!(result, Ok(Some(_))),
        "unsubscribe should stop notifications"
    );
}

#[tokio::test]
async fn service_validates_insert_against_schema() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .set_table_schema(&tenant_id, users_schema())
        .expect("schema should save");

    let missing_name = service
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("age".to_string(), json!(30))]),
        )
        .expect_err("insert should fail");
    assert!(matches!(missing_name, Error::SchemaValidation(_)));

    let wrong_type = service
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("name".to_string(), json!(123))]),
        )
        .expect_err("insert should fail");
    assert!(matches!(wrong_type, Error::SchemaValidation(_)));

    service
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("age".to_string(), json!(30)),
            ]),
        )
        .expect("insert should succeed");
}

#[tokio::test]
async fn service_validates_update_against_full_document() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .set_table_schema(&tenant_id, users_schema())
        .expect("schema should save");
    let document_id = service
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("age".to_string(), json!(30)),
            ]),
        )
        .expect("insert should succeed");

    let wrong_type = service
        .update_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            document_id,
            serde_json::Map::from_iter([("age".to_string(), json!("not a number"))]),
        )
        .expect_err("update should fail");
    assert!(matches!(wrong_type, Error::SchemaValidation(_)));

    service
        .update_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            document_id,
            serde_json::Map::from_iter([("age".to_string(), json!(31))]),
        )
        .expect("update should succeed");
}

#[tokio::test]
async fn no_schema_allows_anything() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .insert_document(
            &tenant_id,
            TableName::new("events").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("payload".to_string(), json!({ "kind": "anything" })),
                ("count".to_string(), json!(7)),
            ]),
        )
        .expect("insert should succeed");
}
