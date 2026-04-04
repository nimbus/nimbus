use neovex_core::{
    AccessOperator, AccessPredicate, AccessRule, AccessValue, DocumentId, Error, FieldSchema,
    FieldType, Filter, FilterOp, IndexDefinition, OrderBy, OrderDirection, Page, PaginatedQuery,
    PrincipalClaimSource, PrincipalContext, Query, SequenceNumber, TableAccessPolicy, TableName,
    TableSchema, TenantId, Timestamp,
};
use neovex_test_support::{
    GeneratedTaskHistory, GeneratedTaskHistorySeedCase, GeneratedTaskPageExpectation,
    GeneratedTaskRecord, ServiceFixture, VerificationHarnessMode,
    replay_generated_task_history_async, selected_generated_task_history_seed_corpus,
};
use proptest::prelude::*;
use serde_json::json;
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier, Condvar, Mutex};
use std::task::{Context, Poll};
use tempfile::tempdir;
use tokio::sync::{Notify, mpsc};
use tokio::time::{Duration, timeout};

use crate::evaluator::{
    evaluate_paginated, evaluate_paginated_cancellable, evaluate_query, evaluate_query_cancellable,
};
use crate::service::{
    SubscriptionBootstrapCancellation, paginate_documents_for_docs_with_principal,
    query_documents_for_docs_with_principal,
};
use crate::tenant::DOCUMENT_CACHE_CAPACITY;
use crate::verification::{
    ConsistencyScope, collect_durable_journal_bootstrap_mismatches,
    compare_materialized_journal_snapshots,
};
use crate::{EmbeddedReplica, Service, ShadowMaterializerConfig, SubscriptionUpdate};
use neovex_storage::{
    DurableJournalBootstrap, FaultInjector, FaultPoint, ManualClock, TenantStore,
};

fn tasks_table() -> TableName {
    TableName::new("tasks").expect("table name should be valid")
}

fn query_for(table: &str) -> Query {
    Query {
        table: TableName::new(table).expect("table name should be valid"),
        filters: Vec::new(),
        order: None,
        limit: None,
    }
}

fn durable_journal_commits(
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

fn subscription_channel() -> (
    mpsc::Sender<SubscriptionUpdate>,
    mpsc::Receiver<SubscriptionUpdate>,
) {
    mpsc::channel(16)
}

async fn wait_for_mutation_journal_stats(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    description: &str,
    predicate: impl Fn(&crate::tenant::MutationJournalStats) -> bool,
) -> crate::tenant::MutationJournalStats {
    let started_at = tokio::time::Instant::now();
    loop {
        let stats = service
            .mutation_journal_stats_for_testing(tenant_id)
            .expect("mutation journal stats should load");
        if predicate(&stats) {
            return stats;
        }
        assert!(
            started_at.elapsed() < Duration::from_secs(1),
            "timed out waiting for {description}; last mutation journal stats: {stats:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

fn filter(field: &str, op: FilterOp, value: serde_json::Value) -> Filter {
    Filter {
        field: field.to_string(),
        op,
        value,
    }
}

fn rank_document(rank: i64) -> neovex_core::Document {
    neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
    )
}

fn materialized_snapshot_with_documents(
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

fn users_schema() -> TableSchema {
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

fn messages_table(name: &str) -> TableName {
    TableName::new(name).expect("table name should be valid")
}

fn principal_with_subject(subject: &str) -> PrincipalContext {
    PrincipalContext {
        authenticated: true,
        claims: serde_json::Map::from_iter([("subject".to_string(), json!(subject))]),
        verified_claims: serde_json::Map::new(),
    }
}

fn owner_matches_subject_rule(left: AccessValue) -> AccessRule {
    AccessRule {
        require_authenticated: true,
        predicates: vec![AccessPredicate {
            left,
            op: AccessOperator::Eq,
            right: AccessValue::PrincipalClaim {
                principal: PrincipalClaimSource::Identity,
                claim: "subject".to_string(),
            },
        }],
    }
}

fn read_only_owner_policy() -> TableAccessPolicy {
    TableAccessPolicy {
        read: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "owner".to_string(),
        }),
        ..TableAccessPolicy::default()
    }
}

fn owner_write_policy() -> TableAccessPolicy {
    TableAccessPolicy {
        create: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "owner".to_string(),
        }),
        update: owner_matches_subject_rule(AccessValue::ExistingDocumentField {
            field: "owner".to_string(),
        }),
        delete: owner_matches_subject_rule(AccessValue::ExistingDocumentField {
            field: "owner".to_string(),
        }),
        ..TableAccessPolicy::default()
    }
}

fn owner_read_write_policy() -> TableAccessPolicy {
    TableAccessPolicy {
        read: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "owner".to_string(),
        }),
        create: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "owner".to_string(),
        }),
        update: owner_matches_subject_rule(AccessValue::ExistingDocumentField {
            field: "owner".to_string(),
        }),
        delete: owner_matches_subject_rule(AccessValue::ExistingDocumentField {
            field: "owner".to_string(),
        }),
    }
}

async fn assert_generated_task_history_matches_model_across_surfaces(
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

fn messages_schema(
    table: &str,
    indexes: Vec<IndexDefinition>,
    access_policy: Option<TableAccessPolicy>,
) -> TableSchema {
    TableSchema {
        table: messages_table(table),
        fields: vec![
            FieldSchema {
                name: "owner".to_string(),
                field_type: FieldType::String,
                required: true,
            },
            FieldSchema {
                name: "body".to_string(),
                field_type: FieldType::String,
                required: true,
            },
        ],
        indexes,
        access_policy,
    }
}

fn document_bodies(documents: &[neovex_core::Document]) -> Vec<&str> {
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

fn subscription_bodies(data: &[serde_json::Value]) -> Vec<&str> {
    data.iter()
        .map(|value| {
            value["body"]
                .as_str()
                .expect("subscription body should be present and a string")
        })
        .collect()
}

fn normalize_generated_task_documents(
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

fn normalize_generated_task_values(values: Vec<serde_json::Value>) -> Vec<GeneratedTaskRecord> {
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

fn assert_generated_task_page_matches(
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

struct BlockingCancellationProbe {
    entered: Notify,
    cancel: Notify,
    cancelled: AtomicBool,
    first_check: AtomicBool,
    release_gate: (Mutex<bool>, Condvar),
}

struct BlockingFaultInjector {
    point: FaultPoint,
    entered: Notify,
    release_gate: (Mutex<bool>, Condvar),
}

struct DropAwarePendingCancellation {
    dropped: Arc<AtomicBool>,
}

impl BlockingFaultInjector {
    fn new(point: FaultPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            entered: Notify::new(),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }

    fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        *released = true;
        cvar.notify_all();
    }
}

impl FaultInjector for BlockingFaultInjector {
    fn check(&self, point: FaultPoint) -> neovex_core::Result<()> {
        if point != self.point {
            return Ok(());
        }
        self.entered.notify_one();
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking fault injector should acquire release lock");
        while !*released {
            released = cvar
                .wait(released)
                .expect("blocking fault injector should wait for release");
        }
        Ok(())
    }
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
    fn new() -> Arc<Self> {
        Arc::new(Self {
            entered: Notify::new(),
            cancel: Notify::new(),
            cancelled: AtomicBool::new(false),
            first_check: AtomicBool::new(true),
            release_gate: (Mutex::new(false), Condvar::new()),
        })
    }

    async fn wait_for_first_check(&self) {
        self.entered.notified().await;
    }

    fn trigger_cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.cancel.notify_one();
    }

    fn release(&self) {
        let (lock, cvar) = &self.release_gate;
        let mut released = lock
            .lock()
            .expect("blocking cancellation probe should acquire release lock");
        *released = true;
        cvar.notify_all();
    }

    async fn cancel_wait(self: Arc<Self>) {
        self.cancel.notified().await;
    }

    fn check(self: Arc<Self>) -> impl Fn() -> neovex_core::Result<()> + Send + 'static {
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
            }

            if self.cancelled.load(Ordering::SeqCst) {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        }
    }
}

async fn create_service_with_durable_unapplied_task(
    timestamp_ms: u64,
    title: &str,
) -> (
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

    (service, tenant_id, faults, document_id)
}

#[test]
fn evaluator_returns_ordered_and_limited_results() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let a = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!("B"))]),
    );
    let b = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("title".to_string(), json!("A"))]),
    );
    store.insert(&a).expect("insert should succeed");
    store.insert(&b).expect("insert should succeed");

    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: Some(neovex_core::OrderBy {
            field: "title".to_string(),
            direction: neovex_core::OrderDirection::Asc,
        }),
        limit: Some(1),
    };

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("A")));
}

#[test]
fn evaluator_applies_equality_filters() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let todo = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("status".to_string(), json!("todo"))]),
    );
    let done = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("status".to_string(), json!("done"))]),
    );
    store.insert(&todo).expect("insert should succeed");
    store.insert(&done).expect("insert should succeed");

    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: vec![neovex_core::Filter {
            field: "status".to_string(),
            op: neovex_core::FilterOp::Eq,
            value: json!("todo"),
        }],
        order: None,
        limit: None,
    };

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("status"), Some(&json!("todo")));
}

#[test]
fn evaluator_rejects_mixed_order_value_types() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let alpha = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("rank".to_string(), json!("1"))]),
    );
    let beta = neovex_core::Document::new(
        TableName::new("tasks").expect("table name should be valid"),
        serde_json::Map::from_iter([("rank".to_string(), json!(2))]),
    );
    store.insert(&alpha).expect("insert should succeed");
    store.insert(&beta).expect("insert should succeed");

    let query = Query {
        table: TableName::new("tasks").expect("table name should be valid"),
        filters: Vec::new(),
        order: Some(neovex_core::OrderBy {
            field: "rank".to_string(),
            direction: neovex_core::OrderDirection::Asc,
        }),
        limit: None,
    };

    let error = evaluate_query(&store, &query).expect_err("query should fail");
    assert!(
        error
            .to_string()
            .contains("ordering cannot mix string and number values"),
        "unexpected error: {error}"
    );
}

#[test]
fn evaluator_supports_neq_filter() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let todo = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("status".to_string(), json!("todo"))]),
    );
    let done = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("status".to_string(), json!("done"))]),
    );
    store.insert(&todo).expect("insert should succeed");
    store.insert(&done).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![filter("status", FilterOp::Neq, json!("todo"))];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("status"), Some(&json!("done")));
}

#[test]
fn evaluator_supports_range_filters_on_numbers() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for rank in [1, 2, 3] {
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let mut gt_query = query_for("tasks");
    gt_query.filters = vec![filter("rank", FilterOp::Gt, json!(1))];
    assert_eq!(
        evaluate_query(&store, &gt_query)
            .expect("gt query should evaluate")
            .len(),
        2
    );

    let mut gte_query = query_for("tasks");
    gte_query.filters = vec![filter("rank", FilterOp::Gte, json!(2))];
    assert_eq!(
        evaluate_query(&store, &gte_query)
            .expect("gte query should evaluate")
            .len(),
        2
    );

    let mut lt_query = query_for("tasks");
    lt_query.filters = vec![filter("rank", FilterOp::Lt, json!(3))];
    assert_eq!(
        evaluate_query(&store, &lt_query)
            .expect("lt query should evaluate")
            .len(),
        2
    );

    let mut lte_query = query_for("tasks");
    lte_query.filters = vec![filter("rank", FilterOp::Lte, json!(2))];
    assert_eq!(
        evaluate_query(&store, &lte_query)
            .expect("lte query should evaluate")
            .len(),
        2
    );
}

#[test]
fn evaluator_range_filter_on_unsupported_field_type_still_errors_after_pushdown_defers() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let document = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([(
            "rank".to_string(),
            json!({
                "nested": 1
            }),
        )]),
    );
    store.insert(&document).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![filter("rank", FilterOp::Gt, json!(0))];

    let error = evaluate_query(&store, &query).expect_err("query should fail");
    assert!(
        error
            .to_string()
            .contains("comparisons only support string and number fields"),
        "unexpected error: {error}"
    );
}

#[test]
fn evaluator_query_cancellable_stops_mid_scan() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for rank in 0..32 {
        store
            .insert(&rank_document(rank))
            .expect("insert should succeed");
    }

    let mut checks = 0usize;
    let error = evaluate_query_cancellable(&store, &query_for("tasks"), &mut || {
        checks += 1;
        if checks > 8 {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    })
    .expect_err("query should cancel");

    assert!(matches!(error, Error::Cancelled));
}

#[test]
fn evaluator_paginated_cancellable_stops_mid_scan() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for rank in 0..32 {
        store
            .insert(&rank_document(rank))
            .expect("insert should succeed");
    }

    let query = PaginatedQuery {
        query: Query {
            table: tasks_table(),
            filters: Vec::new(),
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
        page_size: 5,
        after: None,
    };

    let mut checks = 0usize;
    let error = evaluate_paginated_cancellable(&store, &query, &mut || {
        checks += 1;
        if checks > 8 {
            Err(Error::Cancelled)
        } else {
            Ok(())
        }
    })
    .expect_err("pagination should cancel");

    assert!(matches!(error, Error::Cancelled));
}

#[test]
fn evaluator_supports_range_filters_on_strings() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let mut query = query_for("tasks");
    query.filters = vec![filter("title", FilterOp::Gt, json!("alpha"))];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 2);
    assert!(
        documents
            .iter()
            .all(|doc| doc.fields["title"] != json!("alpha"))
    );
}

#[test]
fn evaluator_filter_on_missing_field_excludes_document() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let titled = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("Hello"))]),
    );
    let untitled = neovex_core::Document::new(tasks_table(), serde_json::Map::new());
    store.insert(&titled).expect("insert should succeed");
    store.insert(&untitled).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![filter("title", FilterOp::Eq, json!("Hello"))];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Hello")));
}

#[test]
fn evaluator_applies_multiple_filters_as_and() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let alpha = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([
            ("status".to_string(), json!("todo")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    let beta = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([
            ("status".to_string(), json!("todo")),
            ("rank".to_string(), json!(2)),
        ]),
    );
    let gamma = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([
            ("status".to_string(), json!("done")),
            ("rank".to_string(), json!(1)),
        ]),
    );
    store.insert(&alpha).expect("insert should succeed");
    store.insert(&beta).expect("insert should succeed");
    store.insert(&gamma).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![
        filter("status", FilterOp::Eq, json!("todo")),
        filter("rank", FilterOp::Eq, json!(1)),
    ];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("status"), Some(&json!("todo")));
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(1)));
}

#[test]
fn evaluator_orders_descending() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let mut query = query_for("tasks");
    query.order = Some(OrderBy {
        field: "title".to_string(),
        direction: OrderDirection::Desc,
    });

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents[0].fields.get("title"), Some(&json!("charlie")));
    assert_eq!(documents[2].fields.get("title"), Some(&json!("alpha")));
}

#[test]
fn evaluator_without_order_sorts_by_document_id() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let first = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("second inserted"))]),
    );
    let second = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("first inserted"))]),
    );
    store.insert(&second).expect("insert should succeed");
    store.insert(&first).expect("insert should succeed");

    let documents = evaluate_query(&store, &query_for("tasks")).expect("query should evaluate");
    let ids = documents
        .iter()
        .map(|document| document.id)
        .collect::<Vec<_>>();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    assert_eq!(ids, sorted_ids);
}

#[test]
fn evaluator_honors_limit_zero_and_none() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo"] {
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let mut zero_limit_query = query_for("tasks");
    zero_limit_query.limit = Some(0);
    assert!(
        evaluate_query(&store, &zero_limit_query)
            .expect("query should evaluate")
            .is_empty()
    );

    let documents = evaluate_query(&store, &query_for("tasks")).expect("query should evaluate");
    assert_eq!(documents.len(), 2);
}

#[test]
fn evaluator_compares_integers_and_floats() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let low = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("rank".to_string(), json!(1))]),
    );
    let high = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("rank".to_string(), json!(2.5))]),
    );
    store.insert(&low).expect("insert should succeed");
    store.insert(&high).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.filters = vec![filter("rank", FilterOp::Gt, json!(1.5))];

    let documents = evaluate_query(&store, &query).expect("query should evaluate");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(2.5)));
}

#[test]
fn evaluator_rejects_ordering_on_boolean_fields() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let document = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("active".to_string(), json!(true))]),
    );
    store.insert(&document).expect("insert should succeed");

    let mut query = query_for("tasks");
    query.order = Some(OrderBy {
        field: "active".to_string(),
        direction: OrderDirection::Asc,
    });

    let error = evaluate_query(&store, &query).expect_err("query should fail");
    assert!(
        error
            .to_string()
            .contains("ordering only supports string and number fields"),
        "unexpected error: {error}"
    );
}

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

#[tokio::test]
async fn query_uses_index_for_equality_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for index in 0..100 {
        let status = if index < 10 { "active" } else { "inactive" };
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("status".to_string(), json!(status))]),
            )
            .expect("insert should succeed");
    }

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 10);
}

#[tokio::test]
async fn subscription_initial_evaluation_uses_indexed_query_path() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for status in ["active", "inactive", "active"] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("status".to_string(), json!(status))]),
            )
            .expect("insert should succeed");
    }

    let (tx, mut rx) = subscription_channel();
    let subscription = service
        .subscribe(
            &tenant_id,
            Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
            "sub-index-1".to_string(),
            tx,
        )
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
            assert_eq!(request_id.as_deref(), Some("sub-index-1"));
            assert_eq!(data.len(), 2);
            assert!(
                data.iter()
                    .all(|document| document["status"] == json!("active"))
            );
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }
}

#[test]
fn subscription_initial_evaluation_uses_materialized_serving_path_for_full_scan_shape() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Ada"))]),
        )
        .expect("seed insert should succeed");

    let query = query_for("tasks");
    let (tx, mut rx) = subscription_channel();
    let subscription = service
        .subscribe(&tenant_id, query, "sub-fullscan-sync".to_string(), tx)
        .expect("subscribe should succeed");

    let initial = rx.blocking_recv().expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(subscription_id, subscription.id());
            assert_eq!(request_id.as_deref(), Some("sub-fullscan-sync"));
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Ada"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let surface_stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(surface_stats.loaded_table_count, 1);
    assert_eq!(surface_stats.table_load_count, 1);
    assert_eq!(surface_stats.evaluation_count, 1);

    let planning_stats = service
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(planning_stats.query_full_scan_count, 1);
}

#[tokio::test]
async fn subscription_async_initial_evaluation_uses_materialized_serving_path_for_full_scan_shape()
{
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Ada"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let subscription = service
        .subscribe_async(
            tenant_id.clone(),
            query_for("tasks"),
            "sub-fullscan-async".to_string(),
            tx,
        )
        .await
        .expect("async subscribe should succeed");

    let initial = rx.recv().await.expect("initial update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(subscription_id, subscription.id());
            assert_eq!(request_id.as_deref(), Some("sub-fullscan-async"));
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("Ada"));
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let surface_stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(surface_stats.loaded_table_count, 1);
    assert_eq!(surface_stats.table_load_count, 1);
    assert_eq!(surface_stats.evaluation_count, 1);

    let planning_stats = service
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(planning_stats.query_full_scan_count, 1);
}

#[tokio::test]
async fn setting_schema_backfills_indexes_for_existing_documents() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    for status in ["active", "inactive", "active"] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("status".to_string(), json!(status))]),
            )
            .expect("insert should succeed");
    }

    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("active"))],
                order: None,
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 2);
}

#[tokio::test]
async fn query_uses_index_for_range_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..100 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Gte, json!(90))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 10);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(90)));
    assert_eq!(documents[9].fields.get("rank"), Some(&json!(99)));
}

#[tokio::test]
async fn query_uses_index_for_eq_filter_and_still_applies_remaining_filters() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for (status, rank) in [("active", 1), ("active", 2), ("inactive", 2)] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("insert should succeed");
    }

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![
                    filter("status", FilterOp::Eq, json!("active")),
                    filter("rank", FilterOp::Gte, json!(2)),
                ],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("status"), Some(&json!("active")));
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(2)));
}

#[tokio::test]
async fn subscription_re_evaluation_uses_indexed_query_path() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "status".to_string(),
            field_type: FieldType::String,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status".to_string(),
            fields: vec!["status".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("status".to_string(), json!("active"))]),
        )
        .expect("seed insert should succeed");

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
            "sub-index-2".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let _ = rx.recv().await.expect("initial update should arrive");

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("status".to_string(), json!("active"))]),
        )
        .expect("active insert should succeed");
    let active_update = rx.recv().await.expect("active update should arrive");
    match active_update {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 2);
            assert!(
                data.iter()
                    .all(|document| document["status"] == json!("active"))
            );
        }
        other => panic!("unexpected active update: {other:?}"),
    }

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("status".to_string(), json!("inactive"))]),
        )
        .expect("inactive insert should succeed");
    let inactive_update = timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(
        inactive_update.is_err(),
        "non-matching indexed insert should not invalidate the subscription"
    );
}

#[tokio::test]
async fn subscription_re_evaluation_uses_materialized_serving_path_for_full_scan_shape() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Ada"))]),
        )
        .expect("seed insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "sub-fullscan-update".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let _ = rx.recv().await.expect("initial update should arrive");

    let initial_surface_stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(initial_surface_stats.table_load_count, 1);
    assert_eq!(initial_surface_stats.evaluation_count, 1);

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Beta"))]),
        )
        .expect("follow-up insert should succeed");

    let update = rx.recv().await.expect("subscription update should arrive");
    match update {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(data.len(), 2);
        }
        other => panic!("unexpected subscription event: {other:?}"),
    }

    let surface_stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(surface_stats.table_load_count, 1);
    assert_eq!(surface_stats.evaluation_count, 2);

    let planning_stats = service
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(planning_stats.query_full_scan_count, 2);
}

#[tokio::test]
async fn query_uses_index_for_bounded_range_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..50 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![
                    filter("rank", FilterOp::Gte, json!(20)),
                    filter("rank", FilterOp::Lt, json!(25)),
                ],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed");
    assert_eq!(documents.len(), 5);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(20)));
    assert_eq!(documents[4].fields.get("rank"), Some(&json!(24)));
}

#[tokio::test]
async fn query_uses_three_field_composite_range_index_through_planner() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "team".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![IndexDefinition {
            name: "by_team_status_rank".to_string(),
            fields: vec!["team".to_string(), "status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for (team, status, rank) in [
        ("alpha", "open", 1),
        ("alpha", "open", 2),
        ("alpha", "open", 3),
        ("alpha", "done", 2),
        ("beta", "open", 2),
    ] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("team".to_string(), json!(team)),
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("insert should succeed");
    }

    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![
                    filter("team", FilterOp::Eq, json!("alpha")),
                    filter("status", FilterOp::Eq, json!("open")),
                    filter("rank", FilterOp::Gte, json!(2)),
                    filter("rank", FilterOp::Lt, json!(4)),
                ],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("three-field composite query should succeed");

    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].fields.get("rank"), Some(&json!(2)));
    assert_eq!(documents[1].fields.get("rank"), Some(&json!(3)));

    let stats = service
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(stats.query_composite_index_count, 1);
    assert_eq!(stats.query_single_field_index_count, 0);
    assert_eq!(stats.query_full_scan_count, 0);
}

#[tokio::test]
async fn query_documents_cancellable_stops_during_index_scan() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..64 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let mut checks = 0usize;
    let error = service
        .query_documents_cancellable(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Gte, json!(0))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            &mut || {
                checks += 1;
                if checks > 8 {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            },
        )
        .expect_err("query should cancel");

    assert!(matches!(error, Error::Cancelled));
}

#[tokio::test]
async fn query_documents_async_cancellable_returns_cancelled_while_blocking_work_unwinds() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    for rank in 0..32 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let probe = BlockingCancellationProbe::new();
    let handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        let probe_for_check = probe.clone();
        async move {
            service
                .query_documents_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    probe_for_wait.cancel_wait(),
                    probe_for_check.check(),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), probe.wait_for_first_check())
        .await
        .expect("query should reach cooperative cancellation check");
    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async query should resolve promptly after cancellation")
        .expect("query task should join successfully")
        .expect_err("query should cancel");
    assert!(matches!(error, Error::Cancelled));

    probe.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn query_documents_async_cancellable_returns_cancelled_during_index_scan() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    service
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: tasks_table(),
                fields: vec![FieldSchema {
                    name: "rank".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                }],
                indexes: vec![IndexDefinition {
                    name: "by_rank".to_string(),
                    fields: vec!["rank".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("schema should save");

    for rank in 0..32 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let probe = BlockingCancellationProbe::new();
    let handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        let probe_for_check = probe.clone();
        async move {
            service
                .query_documents_async_cancellable(
                    tenant_id,
                    Query {
                        table: tasks_table(),
                        filters: vec![filter("rank", FilterOp::Gte, json!(0))],
                        order: Some(OrderBy {
                            field: "rank".to_string(),
                            direction: OrderDirection::Asc,
                        }),
                        limit: None,
                    },
                    probe_for_wait.cancel_wait(),
                    probe_for_check.check(),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), probe.wait_for_first_check())
        .await
        .expect("indexed query should reach cooperative cancellation check");
    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("indexed async query should resolve promptly after cancellation")
        .expect("indexed query task should join successfully")
        .expect_err("indexed query should cancel");
    assert!(matches!(error, Error::Cancelled));

    probe.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn paginated_query_uses_index_for_range_filter() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for rank in 0..10 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let first_page = service
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("rank", FilterOp::Gte, json!(5))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: None,
            },
        )
        .expect("first page should succeed");
    assert_eq!(first_page.data.len(), 2);
    assert_eq!(first_page.data[0]["rank"], json!(5));
    assert_eq!(first_page.data[1]["rank"], json!(6));
    assert!(first_page.has_more);

    let second_page = service
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("rank", FilterOp::Gte, json!(5))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: first_page.next_cursor.clone(),
            },
        )
        .expect("second page should succeed");
    assert_eq!(second_page.data.len(), 2);
    assert_eq!(second_page.data[0]["rank"], json!(7));
    assert_eq!(second_page.data[1]["rank"], json!(8));
}

#[tokio::test]
async fn paginated_query_uses_composite_index_for_exact_prefix_and_cursor_progress() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
        ],
        indexes: vec![neovex_core::IndexDefinition {
            name: "by_status_rank".to_string(),
            fields: vec!["status".to_string(), "rank".to_string()],
        }],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for (status, rank) in [
        ("open", 1),
        ("open", 2),
        ("open", 3),
        ("open", 4),
        ("done", 0),
        ("done", 5),
    ] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .expect("insert should succeed");
    }

    let first_page = service
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: None,
            },
        )
        .expect("first page should succeed");
    assert_eq!(first_page.data.len(), 2);
    assert_eq!(first_page.data[0]["status"], json!("open"));
    assert_eq!(first_page.data[0]["rank"], json!(1));
    assert_eq!(first_page.data[1]["status"], json!("open"));
    assert_eq!(first_page.data[1]["rank"], json!(2));
    assert!(first_page.has_more);

    let second_page = service
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: first_page.next_cursor.clone(),
            },
        )
        .expect("second page should succeed");
    assert_eq!(second_page.data.len(), 2);
    assert_eq!(second_page.data[0]["status"], json!("open"));
    assert_eq!(second_page.data[0]["rank"], json!(3));
    assert_eq!(second_page.data[1]["status"], json!("open"));
    assert_eq!(second_page.data[1]["rank"], json!(4));
    assert!(!second_page.has_more);
    assert!(second_page.next_cursor.is_none());
}

#[test]
fn query_planning_stats_distinguish_composite_single_field_and_fallback_paths() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![
            FieldSchema {
                name: "status".to_string(),
                field_type: FieldType::String,
                required: false,
            },
            FieldSchema {
                name: "rank".to_string(),
                field_type: FieldType::Number,
                required: false,
            },
            FieldSchema {
                name: "title".to_string(),
                field_type: FieldType::String,
                required: false,
            },
        ],
        indexes: vec![
            IndexDefinition {
                name: "by_status_rank".to_string(),
                fields: vec!["status".to_string(), "rank".to_string()],
            },
            IndexDefinition {
                name: "by_rank".to_string(),
                fields: vec!["rank".to_string()],
            },
        ],
        access_policy: None,
    };
    service
        .set_table_schema(&tenant_id, schema)
        .expect("schema should save");

    for (status, rank, title) in [
        ("open", 1, "a"),
        ("open", 2, "b"),
        ("open", 3, "c"),
        ("done", 4, "d"),
    ] {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!(status)),
                    ("rank".to_string(), json!(rank)),
                    ("title".to_string(), json!(title)),
                ]),
            )
            .expect("insert should succeed");
    }

    let composite = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("composite query should succeed");
    assert_eq!(composite.len(), 3);

    let single_field = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("rank", FilterOp::Gte, json!(2))],
                order: Some(OrderBy {
                    field: "rank".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("single-field query should succeed");
    assert_eq!(single_field.len(), 3);

    let fallback = service
        .query_documents(
            &tenant_id,
            &Query {
                table: tasks_table(),
                filters: vec![filter("title", FilterOp::Eq, json!("b"))],
                order: None,
                limit: None,
            },
        )
        .expect("fallback query should succeed");
    assert_eq!(fallback.len(), 1);

    let page = service
        .paginate_documents(
            &tenant_id,
            &PaginatedQuery {
                query: Query {
                    table: tasks_table(),
                    filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                    order: Some(OrderBy {
                        field: "rank".to_string(),
                        direction: OrderDirection::Asc,
                    }),
                    limit: None,
                },
                page_size: 2,
                after: None,
            },
        )
        .expect("paginated composite query should succeed");
    assert_eq!(page.data.len(), 2);

    let stats = service
        .query_planning_stats_for_testing(&tenant_id)
        .expect("query planning stats should load");
    assert_eq!(stats.query_composite_index_count, 1);
    assert_eq!(stats.query_single_field_index_count, 1);
    assert_eq!(stats.query_full_scan_count, 1);
    assert_eq!(stats.paginated_composite_index_count, 1);
    assert_eq!(stats.paginated_single_field_index_count, 0);
    assert_eq!(stats.paginated_full_scan_count, 0);
}

#[test]
fn full_scan_queries_warm_materialized_surface_and_warm_table_gets_reuse_it() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_reads");

    let keep_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("first insert should succeed");
    let warm_only_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("skip")),
                ("body".to_string(), json!("Hidden")),
            ]),
        )
        .expect("second insert should succeed");
    let _ = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("third insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let first = service
        .query_documents(&tenant_id, &query)
        .expect("first full-scan query should succeed");
    assert_eq!(document_bodies(&first), vec!["Ada", "Beta"]);
    assert_eq!(first[0].id, keep_id);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.evaluation_count, 1);
    assert_eq!(stats.paginated_count, 0);
    assert_eq!(stats.get_hit_count, 0);

    let second = service
        .query_documents(&tenant_id, &query)
        .expect("second full-scan query should succeed");
    assert_eq!(document_bodies(&second), vec!["Ada", "Beta"]);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.evaluation_count, 2);

    let warm_only = service
        .get_document(&tenant_id, &table, warm_only_id)
        .expect("warm-table get should succeed from the materialized surface");
    assert_eq!(warm_only.get_field("body"), Some(&json!("Hidden")));

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.get_hit_count, 1);
}

#[test]
fn pinned_materialized_serving_snapshots_remain_stable_after_later_applies() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_serving_handle_stability");

    let _ = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada"]);

    let before_insert = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;
    let pinned = service
        .materialized_serving_snapshot_for_testing(&tenant_id, before_insert)
        .expect("serving snapshot should load")
        .expect("warmed table should expose a serving snapshot");
    assert_eq!(pinned.covered_sequence(), before_insert);
    let pinned_documents = pinned
        .table_documents(&table)
        .expect("pinned snapshot should include the warmed table");
    assert_eq!(document_bodies(&pinned_documents), vec!["Ada"]);

    let _ = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");

    let after_insert = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;
    let current = service
        .materialized_serving_snapshot_for_testing(&tenant_id, after_insert)
        .expect("current serving snapshot should load")
        .expect("published serving snapshot should advance after apply");
    assert_eq!(current.covered_sequence(), after_insert);
    let current_documents = current
        .table_documents(&table)
        .expect("current snapshot should include the warmed table");
    let mut current_bodies = document_bodies(&current_documents)
        .into_iter()
        .collect::<Vec<_>>();
    current_bodies.sort_unstable();
    assert_eq!(current_bodies, vec!["Ada", "Beta"]);

    assert_eq!(pinned.covered_sequence(), before_insert);
    let pinned_documents = pinned
        .table_documents(&table)
        .expect("pinned snapshot should still include the warmed table");
    let pinned_bodies = document_bodies(&pinned_documents)
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        pinned_bodies,
        vec!["Ada"],
        "a pinned serving snapshot should continue to reflect the exact frontier it captured"
    );
}

#[test]
fn materialized_surface_reacquires_retained_covering_version_for_older_required_sequence() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_serving_handle_retention");

    service
        .set_materialized_read_surface_version_capacity_for_testing(&tenant_id, 3)
        .expect("materialized surface version capacity should be configurable for tests");

    let _ = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada"]);

    let first_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");

    let second_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    let retained = service
        .materialized_serving_snapshot_for_testing(&tenant_id, first_sequence)
        .expect("retained serving snapshot should load")
        .expect("historical retained version should remain available");
    assert_eq!(retained.covered_sequence(), first_sequence);
    let retained_documents = retained
        .table_documents(&table)
        .expect("retained snapshot should include the warmed table");
    assert_eq!(document_bodies(&retained_documents), vec!["Ada"]);

    let current = service
        .materialized_serving_snapshot_for_testing(&tenant_id, second_sequence)
        .expect("current serving snapshot should load")
        .expect("current version should remain available");
    assert_eq!(current.covered_sequence(), second_sequence);
    let current_documents = current
        .table_documents(&table)
        .expect("current snapshot should include the warmed table");
    let mut current_bodies = document_bodies(&current_documents)
        .into_iter()
        .collect::<Vec<_>>();
    current_bodies.sort_unstable();
    assert_eq!(current_bodies, vec!["Ada", "Beta"]);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.retained_version_count, 1);
    assert_eq!(stats.earliest_retained_sequence, Some(first_sequence));
    assert_eq!(stats.latest_retained_sequence, Some(first_sequence));
    assert_eq!(stats.latest_covered_sequence, Some(second_sequence));
}

#[test]
fn pinned_materialized_serving_snapshot_is_exact_across_multiple_loaded_tables() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let alpha = messages_table("messages_snapshot_alpha");
    let beta = messages_table("messages_snapshot_beta");

    service
        .set_materialized_read_surface_version_capacity_for_testing(&tenant_id, 4)
        .expect("materialized surface version capacity should be configurable for tests");

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("alpha seed insert should succeed");
    service
        .insert_document(
            &tenant_id,
            beta.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Gamma")),
            ]),
        )
        .expect("beta seed insert should succeed");

    let query_for = |table: TableName| Query {
        table,
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    service
        .query_documents(&tenant_id, &query_for(alpha.clone()))
        .expect("alpha warm query should succeed");
    service
        .query_documents(&tenant_id, &query_for(beta.clone()))
        .expect("beta warm query should succeed");

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("alpha update insert should succeed");
    let alpha_update_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    service
        .insert_document(
            &tenant_id,
            beta.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Delta")),
            ]),
        )
        .expect("beta update insert should succeed");
    let latest_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    let exact_snapshot = service
        .materialized_serving_snapshot_for_testing(&tenant_id, alpha_update_sequence)
        .expect("exact serving snapshot should load")
        .expect("snapshot at the alpha update frontier should be retained");
    assert_eq!(exact_snapshot.covered_sequence(), alpha_update_sequence);
    let alpha_documents = exact_snapshot
        .table_documents(&alpha)
        .expect("exact snapshot should include warmed alpha");
    let mut alpha_bodies = document_bodies(&alpha_documents)
        .into_iter()
        .collect::<Vec<_>>();
    alpha_bodies.sort_unstable();
    assert_eq!(alpha_bodies, vec!["Ada", "Beta"]);
    let beta_documents = exact_snapshot
        .table_documents(&beta)
        .expect("exact snapshot should include warmed beta");
    let beta_bodies = document_bodies(&beta_documents)
        .into_iter()
        .collect::<Vec<_>>();
    assert_eq!(
        beta_bodies,
        vec!["Gamma"],
        "the snapshot pinned at the earlier frontier should not include the later beta write"
    );

    let latest_snapshot = service
        .materialized_serving_snapshot_for_testing(&tenant_id, latest_sequence)
        .expect("latest serving snapshot should load")
        .expect("latest snapshot should remain available");
    assert_eq!(latest_snapshot.covered_sequence(), latest_sequence);
    let latest_beta_documents = latest_snapshot
        .table_documents(&beta)
        .expect("latest snapshot should include warmed beta");
    let mut latest_beta_bodies = document_bodies(&latest_beta_documents)
        .into_iter()
        .collect::<Vec<_>>();
    latest_beta_bodies.sort_unstable();
    assert_eq!(latest_beta_bodies, vec!["Delta", "Gamma"]);
}

#[tokio::test]
async fn serving_snapshot_waiter_wakes_when_new_frontier_is_published() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_snapshot_waiter");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");

    let first_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;
    let required_sequence = SequenceNumber(first_sequence.0.saturating_add(1));

    let waiter = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .wait_for_materialized_serving_snapshot_for_testing(
                    tenant_id,
                    required_sequence,
                    std::future::pending::<()>(),
                )
                .await
        }
    });

    timeout(Duration::from_millis(200), async {
        loop {
            let stats = service
                .serving_snapshot_manager_stats_for_testing(&tenant_id)
                .expect("serving snapshot manager stats should load");
            if stats.waiter_count == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("waiter should register");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("second insert should succeed");

    let snapshot = timeout(Duration::from_millis(200), waiter)
        .await
        .expect("snapshot waiter should wake")
        .expect("snapshot waiter task should join")
        .expect("snapshot waiter should succeed");
    assert_eq!(snapshot.covered_sequence(), required_sequence);
    let documents = snapshot
        .table_documents(&table)
        .expect("woken snapshot should include the target table");
    let mut bodies = document_bodies(&documents).into_iter().collect::<Vec<_>>();
    bodies.sort_unstable();
    assert_eq!(bodies, vec!["Ada", "Beta"]);

    let stats = service
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot manager stats should load");
    assert_eq!(stats.waiter_count, 0);
    assert_eq!(stats.latest_retained_sequence, Some(required_sequence));
}

#[test]
fn pinned_serving_snapshot_extends_retention_until_release() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_snapshot_pin_retention");

    service
        .set_materialized_read_surface_version_capacity_for_testing(&tenant_id, 2)
        .expect("materialized surface version capacity should be configurable for tests");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");

    let first_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;
    let pinned = service
        .materialized_serving_snapshot_for_testing(&tenant_id, first_sequence)
        .expect("first serving snapshot should load")
        .expect("first serving snapshot should exist");

    for body in ["Beta", "Gamma"] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("keep")),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("follow-up insert should succeed");
    }
    let third_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    let pinned_stats = service
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot manager stats should load");
    assert_eq!(pinned_stats.retained_snapshot_count, 3);
    assert_eq!(
        pinned_stats.earliest_retained_sequence,
        Some(first_sequence)
    );
    assert_eq!(pinned_stats.latest_retained_sequence, Some(third_sequence));
    assert_eq!(pinned_stats.pinned_snapshot_count, 1);

    drop(pinned);

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Delta")),
            ]),
        )
        .expect("final insert should succeed");
    let fourth_sequence = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load")
        .applied_head;

    let released_stats = service
        .serving_snapshot_manager_stats_for_testing(&tenant_id)
        .expect("serving snapshot manager stats should load");
    assert_eq!(released_stats.retained_snapshot_count, 2);
    assert_eq!(
        released_stats.earliest_retained_sequence,
        Some(third_sequence)
    );
    assert_eq!(
        released_stats.latest_retained_sequence,
        Some(fourth_sequence)
    );
    assert_eq!(released_stats.pinned_snapshot_count, 0);
    assert!(
        released_stats.pruned_snapshot_count >= 2,
        "older snapshots should prune once the pin is released"
    );
}

#[test]
fn warmed_materialized_tables_track_global_applied_coverage_without_reloading() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_coverage");

    let _document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada"]);

    let journal_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    let publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("materialized publication should load")
        .expect("warmed table should publish");
    assert_eq!(publication.covered_sequence, journal_stats.applied_head);
    assert_eq!(publication.document_count, 1);

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Elsewhere"))]),
        )
        .expect("unrelated insert should succeed");

    let journal_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    let publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("materialized publication should load")
        .expect("warmed table should stay published");
    assert_eq!(publication.covered_sequence, journal_stats.applied_head);
    assert_eq!(publication.document_count, 1);

    let refreshed = service
        .query_documents(&tenant_id, &query)
        .expect("refreshed query should reuse the warmed publication");
    assert_eq!(document_bodies(&refreshed), vec!["Ada"]);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.evaluation_count, 2);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(journal_stats.applied_head)
    );
}

#[test]
fn warmed_tables_do_not_block_each_other_from_reusing_serving_snapshots() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let alpha = messages_table("messages_materialized_alpha_reuse");
    let beta = messages_table("messages_materialized_beta_reuse");

    for (table, body) in [(alpha.clone(), "Alpha"), (beta.clone(), "Beta")] {
        service
            .insert_document(
                &tenant_id,
                table,
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("keep")),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("seed insert should succeed");
    }

    let query_for = |table: TableName| Query {
        table,
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    assert_eq!(
        document_bodies(
            &service
                .query_documents(&tenant_id, &query_for(alpha.clone()))
                .expect("alpha warm query should succeed"),
        ),
        vec!["Alpha"]
    );
    assert_eq!(
        document_bodies(
            &service
                .query_documents(&tenant_id, &query_for(beta.clone()))
                .expect("beta warm query should succeed"),
        ),
        vec!["Beta"]
    );

    service
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Elsewhere"))]),
        )
        .expect("unrelated insert should succeed");

    let beta_again = service
        .query_documents(&tenant_id, &query_for(beta.clone()))
        .expect("beta query should reuse the warmed serving snapshot");
    assert_eq!(document_bodies(&beta_again), vec!["Beta"]);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 2);
    assert_eq!(stats.table_load_count, 2);
    assert_eq!(stats.evaluation_count, 3);
    assert_eq!(stats.retained_version_count, 0);
    assert_eq!(stats.retained_estimated_bytes, 0);

    let journal_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    let beta_publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &beta)
        .expect("beta publication stats should load")
        .expect("beta table should stay published");
    assert_eq!(
        beta_publication.covered_sequence,
        journal_stats.applied_head
    );
}

#[test]
fn materialized_surface_handles_concurrent_reads_and_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_concurrent");
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("seed")),
            ]),
        )
        .expect("seed insert should succeed");
    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["seed"]);

    let barrier = Arc::new(Barrier::new(2));
    let reader_service = service.clone();
    let reader_tenant = tenant_id.clone();
    let reader_query = query.clone();
    let reader_barrier = barrier.clone();
    let reader = std::thread::spawn(move || {
        reader_barrier.wait();
        for _ in 0..64 {
            let documents = reader_service
                .query_documents(&reader_tenant, &reader_query)
                .expect("concurrent materialized query should succeed");
            let bodies = document_bodies(&documents);
            let mut sorted = bodies.clone();
            sorted.sort_unstable();
            assert_eq!(bodies, sorted);
            let unique = bodies.iter().copied().collect::<BTreeSet<_>>();
            assert_eq!(unique.len(), bodies.len());
        }
    });

    let writer_service = service.clone();
    let writer_tenant = tenant_id.clone();
    let writer_table = table.clone();
    let writer_barrier = barrier;
    let writer = std::thread::spawn(move || {
        writer_barrier.wait();
        for index in 0..32 {
            writer_service
                .insert_document(
                    &writer_tenant,
                    writer_table.clone(),
                    serde_json::Map::from_iter([
                        ("owner".to_string(), json!("user-123")),
                        ("body".to_string(), json!(format!("msg-{index:02}"))),
                    ]),
                )
                .expect("concurrent insert should succeed");
        }
    });

    reader.join().expect("reader thread should finish");
    writer.join().expect("writer thread should finish");

    let documents = service
        .query_documents(&tenant_id, &query)
        .expect("final query should succeed");
    let bodies = document_bodies(&documents);
    let mut sorted = bodies.clone();
    sorted.sort_unstable();
    assert_eq!(bodies, sorted);
    assert_eq!(bodies.len(), 33);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.table_load_count, 1);
    assert!(stats.evaluation_count >= 66);
}

#[test]
fn materialized_surface_evicts_least_recently_used_tables_under_byte_budget() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let alpha = messages_table("messages_materialized_alpha");
    let beta = messages_table("messages_materialized_beta");

    service
        .set_materialized_read_surface_limits_for_testing(&tenant_id, 8, 1)
        .expect("materialized surface limits should be configurable for tests");

    for table in [alpha.clone(), beta.clone()] {
        service
            .insert_document(
                &tenant_id,
                table,
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("keep")),
                    ("body".to_string(), json!("payload that exceeds one byte")),
                ]),
            )
            .expect("seed insert should succeed");
    }

    let query_for_table = |table: TableName| Query {
        table,
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let alpha_docs = service
        .query_documents(&tenant_id, &query_for_table(alpha.clone()))
        .expect("alpha warm query should succeed");
    assert_eq!(alpha_docs.len(), 1);

    let beta_docs = service
        .query_documents(&tenant_id, &query_for_table(beta.clone()))
        .expect("beta warm query should succeed");
    assert_eq!(beta_docs.len(), 1);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 2);
    assert_eq!(stats.eviction_count, 1);
    assert_eq!(stats.resident_document_count, 1);
    assert_eq!(stats.byte_capacity, 1);
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
            .expect("alpha publication should load")
            .is_none(),
        "older table should be evicted under the byte budget"
    );
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &beta)
            .expect("beta publication should load")
            .is_some(),
        "newest table should remain resident"
    );
}

#[tokio::test]
async fn paused_first_load_catches_up_before_publication() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_bypass");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let publish_pause = service
        .materialized_read_publish_pause_handle_for_testing(&tenant_id)
        .expect("publish pause handle should load");
    publish_pause.arm();

    let first_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let query = query.clone();
        async move { service.query_documents_async(tenant_id, query).await }
    });

    assert!(
        tokio::task::spawn_blocking({
            let publish_pause = publish_pause.clone();
            move || publish_pause.wait_until_entered(Duration::from_secs(1))
        })
        .await
        .expect("pause waiter should join"),
        "first warmer should pause before publication"
    );

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.in_flight_load_count, 1);
    assert_eq!(stats.bypass_count, 0);

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("concurrent insert should succeed");

    publish_pause.release();

    let first_query = first_query
        .await
        .expect("first query task should join")
        .expect("first query should succeed");
    assert_eq!(document_bodies(&first_query), vec!["Ada", "Beta"]);

    let publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("publication should load")
        .expect("first query should publish its snapshot");
    let after_insert_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    assert_eq!(
        publication.covered_sequence,
        after_insert_stats.applied_head
    );
    assert_eq!(publication.document_count, 2);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.bypass_count, 0);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.in_flight_load_count, 0);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(after_insert_stats.applied_head)
    );
}

#[tokio::test]
async fn concurrent_first_load_only_publishes_caught_up_newest_materialized_table() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_concurrent_publish");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("seed insert should succeed");

    let query = Query {
        table: table.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let publish_pause = service
        .materialized_read_publish_pause_handle_for_testing(&tenant_id)
        .expect("publish pause handle should load");
    publish_pause.arm();

    let first_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let query = query.clone();
        async move { service.query_documents_async(tenant_id, query).await }
    });

    assert!(
        tokio::task::spawn_blocking({
            let publish_pause = publish_pause.clone();
            move || publish_pause.wait_until_entered(Duration::from_secs(1))
        })
        .await
        .expect("pause waiter should join"),
        "first loader should pause before publication"
    );
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &table)
            .expect("materialized publication should load")
            .is_none(),
        "no partially caught-up table should be visible before publication"
    );

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("concurrent insert should succeed");
    let after_insert_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");

    let second_query = tokio::task::spawn_blocking({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let query = query.clone();
        move || service.query_documents(&tenant_id, &query)
    });

    tokio::time::sleep(Duration::from_millis(25)).await;
    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.in_flight_load_count, 1);
    assert_eq!(stats.table_load_count, 0);
    assert_eq!(stats.bypass_count, 0);
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &table)
            .expect("materialized publication should load")
            .is_none(),
        "waiting readers should not publish a second in-flight table"
    );

    publish_pause.release();

    let first_query = first_query
        .await
        .expect("first query task should join")
        .expect("first query should succeed");
    assert_eq!(document_bodies(&first_query), vec!["Ada", "Beta"]);
    let second_query = second_query
        .await
        .expect("second query task should join")
        .expect("second query should succeed");
    assert_eq!(document_bodies(&second_query), vec!["Ada", "Beta"]);

    let publication_after_release = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &table)
        .expect("materialized publication should load")
        .expect("warmed table should remain published");
    assert_eq!(
        publication_after_release.covered_sequence,
        after_insert_stats.applied_head
    );
    assert_eq!(publication_after_release.document_count, 2);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.bypass_count, 0);
    assert_eq!(stats.in_flight_load_count, 0);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(after_insert_stats.applied_head)
    );
}

#[tokio::test]
async fn async_paginated_full_scans_reuse_and_refresh_materialized_surface_after_async_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_paginated");

    for body in ["Beta", "Delta", "Gamma"] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("status".to_string(), json!("keep")),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("seed insert should succeed");
    }

    let query = PaginatedQuery {
        query: Query {
            table: table.clone(),
            filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
            order: Some(OrderBy {
                field: "body".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
        page_size: 2,
        after: None,
    };

    let first_page = service
        .paginate_documents_async(tenant_id.clone(), query.clone())
        .await
        .expect("first paginated full-scan query should succeed");
    assert_eq!(subscription_bodies(&first_page.data), vec!["Beta", "Delta"]);
    assert!(first_page.has_more);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.paginated_count, 1);

    service
        .insert_document_async(
            tenant_id.clone(),
            table.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Able")),
            ]),
        )
        .await
        .expect("async insert after warmup should succeed");

    let refreshed_page = service
        .paginate_documents_async(tenant_id.clone(), query)
        .await
        .expect("refreshed paginated full-scan query should succeed");
    assert_eq!(
        subscription_bodies(&refreshed_page.data),
        vec!["Able", "Beta"]
    );
    assert!(refreshed_page.has_more);

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 1);
    assert_eq!(stats.paginated_count, 2);
}

#[tokio::test]
async fn materialized_surface_rewarms_evicted_tables_and_publishes_fresh_frontiers_after_writes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let alpha = messages_table("messages_materialized_rewarm_alpha");
    let beta = messages_table("messages_materialized_rewarm_beta");

    service
        .set_materialized_read_surface_limits_for_testing(&tenant_id, 1, usize::MAX)
        .expect("materialized surface limits should be configurable for tests");

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("alpha seed insert should succeed");
    service
        .insert_document(
            &tenant_id,
            beta.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Gamma")),
            ]),
        )
        .expect("beta seed insert should succeed");

    let alpha_query = Query {
        table: alpha.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let beta_query = Query {
        table: beta.clone(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let warmed_alpha = service
        .query_documents(&tenant_id, &alpha_query)
        .expect("warming alpha should succeed");
    assert_eq!(document_bodies(&warmed_alpha), vec!["Ada"]);

    let alpha_publication = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
        .expect("alpha publication should load")
        .expect("alpha should publish after the first warm load");

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Beta")),
            ]),
        )
        .expect("resident alpha insert should succeed");
    let after_resident_insert = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after resident insert");
    let alpha_after_resident_insert = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
        .expect("alpha publication should load")
        .expect("resident alpha table should stay published");
    assert_eq!(
        alpha_after_resident_insert.generation, alpha_publication.generation,
        "resident apply should advance coverage in place instead of republishing the table"
    );
    assert_eq!(
        alpha_after_resident_insert.covered_sequence,
        after_resident_insert.applied_head
    );
    assert_eq!(alpha_after_resident_insert.document_count, 2);

    let warmed_beta = service
        .query_documents(&tenant_id, &beta_query)
        .expect("warming beta should succeed");
    assert_eq!(document_bodies(&warmed_beta), vec!["Gamma"]);
    assert!(
        service
            .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
            .expect("alpha publication should load")
            .is_none(),
        "warming beta under a one-table budget should evict alpha"
    );

    service
        .insert_document(
            &tenant_id,
            alpha.clone(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("keep")),
                ("body".to_string(), json!("Delta")),
            ]),
        )
        .expect("evicted alpha insert should succeed");
    let after_rewarm_insert = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after evicted insert");

    let rewarmed_alpha = service
        .query_documents(&tenant_id, &alpha_query)
        .expect("rewarming alpha should succeed");
    assert_eq!(
        document_bodies(&rewarmed_alpha),
        vec!["Ada", "Beta", "Delta"]
    );

    let republished_alpha = service
        .materialized_table_publication_stats_for_testing(&tenant_id, &alpha)
        .expect("alpha publication should load")
        .expect("rewarmed alpha should publish again");
    assert!(
        republished_alpha.generation > alpha_publication.generation,
        "rewarming an evicted table should publish a newer generation"
    );
    assert_eq!(republished_alpha.document_count, 3);
    assert_eq!(
        republished_alpha.covered_sequence, after_rewarm_insert.applied_head,
        "rewarmed tables should publish the exact frontier they cover"
    );

    let stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(stats.loaded_table_count, 1);
    assert_eq!(stats.table_load_count, 3);
    assert_eq!(stats.eviction_count, 2);
    assert_eq!(
        stats.latest_covered_sequence,
        Some(after_rewarm_insert.applied_head)
    );
}

#[tokio::test]
async fn paginate_documents_async_cancellable_returns_cancelled_while_blocking_work_unwinds() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    for rank in 0..32 {
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .expect("insert should succeed");
    }

    let probe = BlockingCancellationProbe::new();
    let handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        let probe_for_check = probe.clone();
        async move {
            service
                .paginate_documents_async_cancellable(
                    tenant_id,
                    PaginatedQuery {
                        query: query_for("tasks"),
                        page_size: 8,
                        after: None,
                    },
                    probe_for_wait.cancel_wait(),
                    probe_for_check.check(),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), probe.wait_for_first_check())
        .await
        .expect("paginated query should reach cooperative cancellation check");
    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async paginated query should resolve promptly after cancellation")
        .expect("paginated query task should join successfully")
        .expect_err("paginated query should cancel");
    assert!(matches!(error, Error::Cancelled));

    probe.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn mutation_async_cancellable_before_commit_rolls_back_document_index_and_durable_journal() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(10_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut blocker = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("blocker"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("first write should block after durable append and before apply");
    assert!(
        timeout(Duration::from_millis(100), &mut blocker)
            .await
            .is_err(),
        "first mutation should remain pending while apply is blocked"
    );
    let blocker_id = durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0))
        .first()
        .and_then(|commit| commit.writes.first())
        .map(|write| write.doc_id)
        .expect("durable blocker commit should include the inserted document id");

    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("rolled-back"))]),
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                )
                .await
        }
    });

    cancel.notify_one();
    tokio::time::sleep(Duration::from_millis(25)).await;
    faults.release();

    timeout(Duration::from_secs(1), blocker)
        .await
        .expect("first mutation should finish after apply resumes")
        .expect("blocker task should join successfully")
        .expect("first mutation should succeed");

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("queued async mutation should resolve after cancellation")
        .expect("mutation task should join successfully")
        .expect_err("queued cancellation before durable append should surface as cancelled");
    assert!(matches!(error, Error::Cancelled));
    let documents = service
        .query_documents(&tenant_id, &query_for("tasks"))
        .expect("query should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, blocker_id);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("blocker")));
    assert_eq!(
        durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1,
        "queued cancellation before durable append should not append a second commit"
    );
}

#[tokio::test]
async fn mutation_async_cancellable_after_commit_returns_committed_result() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(20_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let cancel = Arc::new(Notify::new());
    let cancel_for_wait = cancel.clone();
    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("after-commit"))]),
                    async move {
                        cancel_for_wait.notified().await;
                    },
                    || Ok(()),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("write should block after durable append and before apply");
    cancel.notify_one();

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "post-commit cancellation should not complete before apply resumes"
    );
    faults.release();
    let document_id = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async mutation should resolve after apply resumes")
        .expect("mutation task should join successfully")
        .expect("post-commit cancellation should still return success");
    let documents = timeout(
        Duration::from_secs(1),
        service.query_documents_async(tenant_id.clone(), query_for("tasks")),
    )
    .await
    .expect("query should resolve after apply")
    .expect("query should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, document_id);
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("after-commit"))
    );
    assert_eq!(
        durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1
    );
}

#[tokio::test]
async fn mutation_async_non_cancelable_call_drops_unused_cancellation_future_after_completion() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let dropped = Arc::new(AtomicBool::new(false));

    let document_id = service
        .insert_document_async_cancellable_with_principal(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("drop-cancel-future"))]),
            PrincipalContext::anonymous(),
            DropAwarePendingCancellation {
                dropped: dropped.clone(),
            },
            || Ok(()),
        )
        .await
        .expect("mutation should succeed");

    tokio::task::yield_now().await;

    assert!(
        dropped.load(Ordering::SeqCst),
        "unused cancellation futures should be dropped once the mutation completes"
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &tasks_table(), document_id)
            .expect("inserted document should remain visible")
            .fields
            .get("title"),
        Some(&json!("drop-cancel-future"))
    );
}

#[tokio::test]
async fn mutation_journal_returns_only_after_apply_visibility() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(30_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("durable-first"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append and before apply");

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "async mutation should remain pending while apply is blocked"
    );
    let document_id = durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0))
        .first()
        .and_then(|commit| commit.writes.first())
        .map(|write| write.doc_id)
        .expect("durable commit should include the inserted document id");
    faults.release();
    let completed_id = timeout(Duration::from_secs(1), handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let documents = timeout(
        Duration::from_secs(1),
        service.query_documents_async(tenant_id.clone(), query_for("tasks")),
    )
    .await
    .expect("query should resolve after apply")
    .expect("query should succeed");
    assert_eq!(
        service
            .latest_sequence_async(tenant_id.clone())
            .await
            .expect("latest sequence should read"),
        SequenceNumber(1)
    );
    assert_eq!(
        durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1
    );
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, document_id);
    assert_eq!(completed_id, document_id);
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("durable-first"))
    );
}

#[tokio::test]
async fn sync_query_waits_for_applied_journal_visibility_and_records_wait_metrics() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(35_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("sync-wait"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );

    let (query_tx, mut query_rx) = mpsc::unbounded_channel();
    tokio::task::spawn_blocking({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        move || {
            let _ = query_tx.send(service.query_documents(&tenant_id, &query_for("tasks")));
        }
    });

    assert!(
        timeout(Duration::from_millis(100), query_rx.recv())
            .await
            .is_err(),
        "sync query should wait for the applied watermark while journaled data is not yet materialized"
    );

    faults.release();

    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let documents = timeout(Duration::from_secs(1), query_rx.recv())
        .await
        .expect("sync query should resolve after apply")
        .expect("sync query result should be sent")
        .expect("sync query should succeed");
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("sync-wait")),
        "sync query should observe the applied task after the journal worker resumes"
    );

    let stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should read after sync wait");
    assert_eq!(stats.read_wait_count, 1);
    assert!(
        stats.total_read_wait_nanos > 0,
        "sync read waits should contribute to read wait metrics"
    );
}

#[tokio::test]
async fn query_waits_for_applied_journal_visibility_and_records_wait_metrics() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(40_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("wait-for-apply"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );

    let stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should read");
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(0));
    assert_eq!(stats.apply_lag, 1);
    assert_eq!(stats.queue_depth, 0);
    assert_eq!(
        stats.queue_capacity,
        crate::tenant::DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY
    );
    assert_eq!(stats.oldest_queue_age_nanos, 0);
    assert_eq!(stats.pending_response_count, 1);
    assert!(stats.worker_running);
    assert_eq!(stats.worker_start_count, 1);
    assert_eq!(stats.worker_restart_count, 0);
    assert_eq!(stats.queue_rejection_count, 0);
    assert_eq!(stats.worker_failure_count, 0);
    assert_eq!(stats.read_wait_count, 0);

    let (query_tx, mut query_rx) = mpsc::unbounded_channel();
    tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            let result = service
                .query_documents_async(tenant_id, query_for("tasks"))
                .await;
            let _ = query_tx.send(result);
        }
    });

    assert!(
        timeout(Duration::from_millis(100), query_rx.recv())
            .await
            .is_err(),
        "query should wait for the applied watermark while journaled data is not yet materialized"
    );

    faults.release();

    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let documents = timeout(Duration::from_secs(1), query_rx.recv())
        .await
        .expect("query should resolve after apply")
        .expect("query result should be sent")
        .expect("query should succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].fields.get("title"),
        Some(&json!("wait-for-apply"))
    );

    let stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should read after apply");
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(1));
    assert_eq!(stats.apply_lag, 0);
    assert_eq!(stats.queue_depth, 0);
    assert_eq!(stats.oldest_queue_age_nanos, 0);
    assert_eq!(stats.pending_response_count, 0);
    assert!(!stats.worker_running);
    assert_eq!(stats.worker_start_count, 1);
    assert_eq!(stats.worker_restart_count, 0);
    assert_eq!(stats.queue_rejection_count, 0);
    assert_eq!(stats.worker_failure_count, 0);
    assert_eq!(stats.read_wait_count, 1);
    assert!(
        stats.total_read_wait_nanos > 0,
        "read wait metrics should accumulate a positive wait duration"
    );
}

#[tokio::test]
async fn mutation_admission_gate_buffers_while_journal_is_paused_without_losing_in_flight_response()
{
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .set_mutation_journal_queue_capacity_for_testing(&tenant_id, 1)
        .expect("queue capacity should be configurable for tests");
    let pause = service
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let first_insert = {
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("queued-first"))]),
                )
                .await
        })
    };

    assert!(
        tokio::task::spawn_blocking({
            let pause = pause.clone();
            move || pause.wait_until_entered(Duration::from_secs(1))
        })
        .await
        .expect("pause wait should join"),
        "journal worker should pause before draining the queued request"
    );

    let blocked_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load while the queue is paused");
    assert_eq!(blocked_stats.queue_depth, 1);
    assert_eq!(blocked_stats.queue_capacity, 1);
    assert!(blocked_stats.oldest_queue_age_nanos > 0);
    assert_eq!(blocked_stats.pending_response_count, 1);
    assert!(blocked_stats.worker_running);
    assert_eq!(blocked_stats.worker_start_count, 1);
    assert_eq!(blocked_stats.worker_restart_count, 0);
    assert_eq!(blocked_stats.queue_rejection_count, 0);
    assert_eq!(blocked_stats.worker_failure_count, 0);

    let mut second_insert = tokio::spawn({
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("queued-second"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), async {
        loop {
            let stats = service
                .mutation_admission_stats_for_testing(&tenant_id)
                .expect("admission stats should load while the journal is paused");
            if stats.queue_depth == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("second mutation should remain buffered at the admission gate");

    assert!(
        timeout(Duration::from_millis(150), &mut second_insert)
            .await
            .is_err(),
        "second mutation should stay pending while the journal worker is paused"
    );

    let buffered_stats = service
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the second mutation is buffered");
    assert_eq!(buffered_stats.queue_depth, 1);
    assert_eq!(
        buffered_stats.queue_capacity,
        crate::tenant::DEFAULT_MUTATION_ADMISSION_QUEUE_CAPACITY
    );
    assert!(buffered_stats.oldest_queue_age_nanos > 0);
    assert_eq!(buffered_stats.shed_count, 0);
    assert_eq!(buffered_stats.queue_rejection_count, 0);

    pause.release();

    let first_id = timeout(Duration::from_secs(1), first_insert)
        .await
        .expect("first mutation should resolve after the pause is released")
        .expect("first mutation task should join successfully")
        .expect("first mutation should succeed");
    let second_id = timeout(Duration::from_secs(1), second_insert)
        .await
        .expect("second mutation should resolve after the journal drains")
        .expect("second mutation task should join successfully")
        .expect("second mutation should succeed");

    let visible = service
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("final query should succeed after the buffered mutation drains");
    assert_eq!(visible.len(), 2);
    assert_eq!(
        visible
            .into_iter()
            .map(|document| document.id)
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first_id, second_id])
    );

    let final_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after the queue drains");
    assert_eq!(final_stats.durable_head, SequenceNumber(2));
    assert_eq!(final_stats.applied_head, SequenceNumber(2));
    assert_eq!(final_stats.apply_lag, 0);
    assert_eq!(final_stats.queue_depth, 0);
    assert_eq!(final_stats.queue_capacity, 1);
    assert_eq!(final_stats.oldest_queue_age_nanos, 0);
    assert_eq!(final_stats.pending_response_count, 0);
    assert!(!final_stats.worker_running);
    assert_eq!(final_stats.worker_start_count, 1);
    assert_eq!(final_stats.worker_restart_count, 0);
    assert_eq!(final_stats.queue_rejection_count, 0);
    assert_eq!(final_stats.worker_failure_count, 0);

    let final_admission_stats = service
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the gate drains");
    assert_eq!(final_admission_stats.queue_depth, 0);
    assert_eq!(final_admission_stats.shed_count, 0);
    assert_eq!(final_admission_stats.queue_rejection_count, 0);
}

#[tokio::test]
async fn mutation_journal_never_expires_admitted_work() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    service
        .set_mutation_admission_codel_for_testing(
            &tenant_id,
            Duration::from_millis(5),
            Duration::from_millis(10),
        )
        .expect("admission CoDel should be configurable for tests");
    let pause = service
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let admitted_insert = {
        let service = Arc::clone(&service);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("admitted"))]),
                )
                .await
        })
    };

    assert!(
        tokio::task::spawn_blocking({
            let pause = pause.clone();
            move || pause.wait_until_entered(Duration::from_secs(1))
        })
        .await
        .expect("pause wait should join"),
        "journal worker should pause after admitting the mutation to the journal queue"
    );

    tokio::time::sleep(Duration::from_millis(25)).await;
    pause.release();

    let document_id = timeout(Duration::from_secs(1), admitted_insert)
        .await
        .expect("admitted mutation should resolve after the pause is released")
        .expect("admitted mutation task should join successfully")
        .expect("admitted mutation should still succeed");

    let visible = service
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("final query should succeed after the admitted mutation drains");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, document_id);

    let admission_stats = service
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the queue drains");
    assert_eq!(admission_stats.queue_depth, 0);
    assert_eq!(admission_stats.shed_count, 0);
    assert_eq!(admission_stats.queue_rejection_count, 0);

    let journal_stats = service
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after the admitted mutation commits");
    assert_eq!(journal_stats.durable_head, SequenceNumber(1));
    assert_eq!(journal_stats.applied_head, SequenceNumber(1));
    assert_eq!(journal_stats.queue_depth, 0);
}

#[tokio::test]
async fn queued_mutation_response_still_resolves_after_blocked_read_catches_up() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_500))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut first_insert)
            .await
            .is_err(),
        "first mutation should remain pending while apply is blocked"
    );

    let mut blocked_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut blocked_query)
            .await
            .is_err(),
        "query should remain pending while the first durable write is not yet applied"
    );

    let mut second_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
                )
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut second_insert)
            .await
            .is_err(),
        "queued follow-up mutation should remain pending until the blocked apply resumes"
    );

    faults.release();

    let first_id = timeout(Duration::from_secs(1), first_insert)
        .await
        .expect("first mutation should resolve after apply resumes")
        .expect("first mutation task should join successfully")
        .expect("first mutation should succeed");
    let query_results = timeout(Duration::from_secs(1), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
        .expect("blocked query task should join successfully")
        .expect("blocked query should succeed");
    assert!(
        query_results
            .iter()
            .any(|document| document.fields.get("title") == Some(&json!("first"))),
        "blocked query should observe the first applied write"
    );

    let second_id = match timeout(Duration::from_secs(3), second_insert).await {
        Ok(result) => result
            .expect("second mutation task should join successfully")
            .expect("second mutation should succeed"),
        Err(error) => {
            let visible = service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("live query should still succeed");
            let visible_titles = visible
                .iter()
                .map(|document| {
                    document.fields["title"]
                        .as_str()
                        .expect("title should be present and a string")
                })
                .collect::<Vec<_>>();
            panic!(
                "queued follow-up mutation should resolve after the blocked read catches up: {error:?}; visible documents: {:?}; first_id={first_id}",
                visible_titles
            );
        }
    };

    let visible = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_cancellable_mutation_response_still_resolves_after_blocked_read_catches_up() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_750))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("first-cancellable"))]),
                    std::future::pending::<()>(),
                    || Ok(()),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut first_insert)
            .await
            .is_err(),
        "first cancellable mutation should remain pending while apply is blocked"
    );

    let mut blocked_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut blocked_query)
            .await
            .is_err(),
        "query should remain pending while the first durable write is not yet applied"
    );

    let mut second_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("second-cancellable"),
                    )]),
                    std::future::pending::<()>(),
                    || Ok(()),
                )
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut second_insert)
            .await
            .is_err(),
        "queued follow-up cancellable mutation should remain pending until the blocked apply resumes"
    );

    faults.release();

    let first_id = timeout(Duration::from_secs(1), first_insert)
        .await
        .expect("first cancellable mutation should resolve after apply resumes")
        .expect("first cancellable mutation task should join successfully")
        .expect("first cancellable mutation should succeed");
    let query_results = timeout(Duration::from_secs(1), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
        .expect("blocked query task should join successfully")
        .expect("blocked query should succeed");
    assert!(
        query_results
            .iter()
            .any(|document| document.fields.get("title") == Some(&json!("first-cancellable"))),
        "blocked query should observe the first applied cancellable write"
    );

    let second_id = match timeout(Duration::from_secs(3), second_insert).await {
        Ok(result) => result
            .expect("second cancellable mutation task should join successfully")
            .expect("second cancellable mutation should succeed"),
        Err(error) => {
            let visible = service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("live query should still succeed");
            let visible_titles = visible
                .iter()
                .map(|document| {
                    document.fields["title"]
                        .as_str()
                        .expect("title should be present and a string")
                })
                .collect::<Vec<_>>();
            panic!(
                "queued follow-up cancellable mutation should resolve after the blocked read catches up: {error:?}; visible documents: {:?}; first_id={first_id}",
                visible_titles
            );
        }
    };

    let visible = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_mutation_response_still_resolves_after_blocked_cancellable_read_catches_up() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_900))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("first-query-cancellable"),
                    )]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut first_insert)
            .await
            .is_err(),
        "first mutation should remain pending while apply is blocked"
    );

    let mut blocked_query = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .query_documents_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    std::future::pending::<()>(),
                    || Ok(()),
                )
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut blocked_query)
            .await
            .is_err(),
        "cancellable query should remain pending while the first durable write is not yet applied"
    );

    let mut second_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("second-query-cancellable"),
                    )]),
                )
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut second_insert)
            .await
            .is_err(),
        "queued follow-up mutation should remain pending until the blocked apply resumes"
    );

    faults.release();

    let first_id = timeout(Duration::from_secs(1), first_insert)
        .await
        .expect("first mutation should resolve after apply resumes")
        .expect("first mutation task should join successfully")
        .expect("first mutation should succeed");
    let query_results = timeout(Duration::from_secs(1), blocked_query)
        .await
        .expect("blocked query should resolve after apply resumes")
        .expect("blocked query task should join successfully")
        .expect("blocked query should succeed");
    assert!(
        query_results.iter().any(
            |document| document.fields.get("title") == Some(&json!("first-query-cancellable"))
        ),
        "blocked query should observe the first applied write"
    );

    let second_id = match timeout(Duration::from_secs(3), second_insert).await {
        Ok(result) => result
            .expect("second mutation task should join successfully")
            .expect("second mutation should succeed"),
        Err(error) => {
            let visible = service
                .query_documents_async(tenant_id.clone(), query_for("tasks"))
                .await
                .expect("live query should still succeed");
            let visible_titles = visible
                .iter()
                .map(|document| {
                    document.fields["title"]
                        .as_str()
                        .expect("title should be present and a string")
                })
                .collect::<Vec<_>>();
            panic!(
                "queued follow-up mutation should resolve after the blocked cancellable read catches up: {error:?}; visible documents: {:?}; first_id={first_id}",
                visible_titles
            );
        }
    };

    let visible = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_mutation_response_resolves_when_worker_starts_on_ephemeral_current_thread_runtime()
{
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(43_050))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let first_runtime = std::thread::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("ephemeral current-thread runtime should build");
            runtime.block_on(async move {
                service
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        serde_json::Map::from_iter([(
                            "title".to_string(),
                            json!("first-ephemeral-runtime"),
                        )]),
                    )
                    .await
            })
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");

    let mut second_insert = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("second-after-ephemeral-runtime"),
                    )]),
                )
                .await
        }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut second_insert)
            .await
            .is_err(),
        "queued follow-up mutation should remain pending until the blocked apply resumes"
    );

    faults.release();

    let first_id = tokio::task::spawn_blocking(move || {
        first_runtime
            .join()
            .expect("ephemeral runtime thread should join successfully")
    })
    .await
    .expect("join worker should finish")
    .expect("first mutation should succeed");
    let second_id = timeout(Duration::from_secs(3), second_insert)
        .await
        .expect("queued follow-up mutation should still resolve after the ephemeral runtime exits")
        .expect("second mutation task should join successfully")
        .expect("second mutation should succeed");

    let visible = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn get_document_async_cancellable_returns_cancelled_while_waiting_for_applied_visibility() {
    let (service, tenant_id, faults, document_id) =
        create_service_with_durable_unapplied_task(44_000, "async-get-cancel").await;
    let probe = BlockingCancellationProbe::new();

    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        async move {
            service
                .get_document_async_cancellable(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    probe_for_wait.cancel_wait(),
                    || Ok(()),
                )
                .await
        }
    });

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "point read should still be waiting for applied visibility before cancellation"
    );

    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async point read should resolve promptly after cancellation")
        .expect("point read task should join successfully")
        .expect_err("point read should cancel while waiting for apply");
    assert!(matches!(error, Error::Cancelled));

    faults.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn query_documents_async_cancellable_returns_cancelled_while_waiting_for_applied_visibility()
{
    let (service, tenant_id, faults, _) =
        create_service_with_durable_unapplied_task(44_500, "async-query-cancel").await;
    let probe = BlockingCancellationProbe::new();

    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        async move {
            service
                .query_documents_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    probe_for_wait.cancel_wait(),
                    || Ok(()),
                )
                .await
        }
    });

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "query should still be waiting for applied visibility before cancellation"
    );

    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async query should resolve promptly after cancellation")
        .expect("query task should join successfully")
        .expect_err("query should cancel while waiting for apply");
    assert!(matches!(error, Error::Cancelled));

    faults.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn paginate_documents_async_cancellable_returns_cancelled_while_waiting_for_applied_visibility()
 {
    let (service, tenant_id, faults, _) =
        create_service_with_durable_unapplied_task(44_750, "async-page-cancel").await;
    let probe = BlockingCancellationProbe::new();

    let mut handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let probe_for_wait = probe.clone();
        async move {
            service
                .paginate_documents_async_cancellable(
                    tenant_id,
                    PaginatedQuery {
                        query: query_for("tasks"),
                        page_size: 1,
                        after: None,
                    },
                    probe_for_wait.cancel_wait(),
                    || Ok(()),
                )
                .await
        }
    });

    assert!(
        timeout(Duration::from_millis(100), &mut handle)
            .await
            .is_err(),
        "pagination should still be waiting for applied visibility before cancellation"
    );

    probe.trigger_cancel();

    let error = timeout(Duration::from_secs(1), handle)
        .await
        .expect("async pagination should resolve promptly after cancellation")
        .expect("pagination task should join successfully")
        .expect_err("pagination should cancel while waiting for apply");
    assert!(matches!(error, Error::Cancelled));

    faults.release();
    tokio::time::sleep(Duration::from_millis(25)).await;
}

#[tokio::test]
async fn sync_get_document_waits_for_applied_journal_visibility() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(45_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("sync-get"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );
    let document_id = durable_journal_commits(service.as_ref(), &tenant_id, SequenceNumber(0))
        .first()
        .and_then(|commit| commit.writes.first())
        .map(|write| write.doc_id)
        .expect("durable commit should include the inserted document id");

    let (get_tx, mut get_rx) = mpsc::unbounded_channel();
    tokio::task::spawn_blocking({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        move || {
            let _ = get_tx.send(service.get_document(&tenant_id, &tasks_table(), document_id));
        }
    });

    assert!(
        timeout(Duration::from_millis(100), get_rx.recv())
            .await
            .is_err(),
        "sync point reads should wait for applied visibility instead of returning stale not-found results"
    );

    faults.release();

    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let document = timeout(Duration::from_secs(1), get_rx.recv())
        .await
        .expect("sync point read should resolve after apply")
        .expect("sync point read result should be sent")
        .expect("sync point read should succeed");
    assert_eq!(document.fields.get("title"), Some(&json!("sync-get")));
}

#[tokio::test]
async fn subscription_updates_publish_only_after_journal_apply() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(50_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let (tx, mut rx) = subscription_channel();
    let subscription = service
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "journal-sub".to_string(),
            tx,
        )
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
    let initial = rx
        .recv()
        .await
        .expect("initial subscription update should arrive");
    match initial {
        SubscriptionUpdate::Result {
            subscription_id: actual_id,
            request_id,
            data,
            ..
        } => {
            assert_eq!(actual_id, subscription_id);
            assert_eq!(request_id.as_deref(), Some("journal-sub"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("reactive"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );

    assert!(
        timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err(),
        "subscription fan-out must stay behind the applied visibility boundary"
    );

    faults.release();

    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    let update = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("subscription update should arrive after apply")
        .expect("subscription update should be sent");
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
            assert_eq!(data[0]["title"], json!("reactive"));
        }
        other => panic!("unexpected reactive update: {other:?}"),
    }
}

#[tokio::test]
async fn async_subscription_bootstrap_catches_up_writes_committed_before_activation() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let pause = service
        .subscription_bootstrap_pause_handle_for_testing(&tenant_id)
        .expect("bootstrap pause handle should load");
    pause.arm();

    let (tx, mut rx) = subscription_channel();
    let subscribe_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .subscribe_async(
                    tenant_id,
                    query_for("tasks"),
                    "bootstrap-gap".to_string(),
                    tx,
                )
                .await
        }
    });

    let initial = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("initial subscription result should arrive")
        .expect("subscription channel should remain open");
    match initial {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert_eq!(request_id.as_deref(), Some("bootstrap-gap"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    assert!(
        pause.wait_until_entered(Duration::from_secs(1)),
        "subscription bootstrap should pause before activation"
    );

    service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("during-bootstrap"))]),
        )
        .await
        .expect("insert during bootstrap gap should succeed");

    assert!(
        timeout(Duration::from_millis(100), rx.recv())
            .await
            .is_err(),
        "inactive bootstrap window should not publish reactive updates before activation resumes"
    );

    pause.release();

    let _subscription = timeout(Duration::from_secs(1), subscribe_task)
        .await
        .expect("subscribe task should finish after pause release")
        .expect("subscribe task should join successfully")
        .expect("subscription should register successfully");

    let catch_up = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("subscription should catch up the write committed during bootstrap")
        .expect("subscription channel should remain open");
    match catch_up {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert!(request_id.is_none());
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("during-bootstrap"));
        }
        other => panic!("unexpected bootstrap catch-up event: {other:?}"),
    }
}

#[tokio::test]
async fn async_subscription_bootstrap_cancellation_before_activation_returns_cancelled() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);

    let pause = service
        .subscription_bootstrap_pause_handle_for_testing(&tenant_id)
        .expect("bootstrap pause handle should load");
    pause.arm();

    let cancelled = Arc::new(AtomicBool::new(false));
    let cancel_notify = Arc::new(Notify::new());
    let (tx, mut rx) = subscription_channel();
    let subscribe_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let cancelled = cancelled.clone();
        let cancel_notify = cancel_notify.clone();
        async move {
            service
                .subscribe_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    "bootstrap-cancel".to_string(),
                    tx,
                    SubscriptionBootstrapCancellation::new(
                        async move { cancel_notify.notified().await },
                        move || {
                            if cancelled.load(Ordering::SeqCst) {
                                Err(Error::Cancelled)
                            } else {
                                Ok(())
                            }
                        },
                    ),
                )
                .await
        }
    });

    let initial = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("initial subscription result should arrive")
        .expect("subscription channel should remain open");
    match initial {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert_eq!(request_id.as_deref(), Some("bootstrap-cancel"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    assert!(
        pause.wait_until_entered(Duration::from_secs(1)),
        "subscription bootstrap should pause before activation"
    );

    cancelled.store(true, Ordering::SeqCst);
    cancel_notify.notify_waiters();
    pause.release();

    let error = timeout(Duration::from_secs(1), subscribe_task)
        .await
        .expect("cancelled subscribe task should finish after pause release")
        .expect("cancelled subscribe task should join successfully")
        .expect_err("subscription bootstrap should be cancelled before activation");
    assert!(matches!(error, Error::Cancelled));

    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        0,
        "cancelled bootstrap should remove the pending subscription",
    );
    match timeout(Duration::from_millis(100), rx.recv()).await {
        Err(_) | Ok(None) => {}
        Ok(Some(update)) => {
            panic!("cancelled bootstrap should not emit a catch-up update: {update:?}");
        }
    }
}

#[tokio::test]
async fn sync_subscription_bootstrap_does_not_miss_lagged_applied_commit() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(60_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_task = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("lagged-sync"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("write should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_task)
            .await
            .is_err(),
        "insert should remain pending while apply is blocked"
    );

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "sync-lagged".to_string(),
            tx,
        )
        .expect("sync subscription should register");

    let initial = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("initial sync subscription result should arrive")
        .expect("subscription channel should remain open");
    match initial {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert_eq!(request_id.as_deref(), Some("sync-lagged"));
            assert!(data.is_empty());
        }
        other => panic!("unexpected initial sync subscription event: {other:?}"),
    }

    faults.release();

    timeout(Duration::from_secs(1), insert_task)
        .await
        .expect("insert should finish after apply resumes")
        .expect("insert task should join successfully")
        .expect("insert should succeed");

    let update = timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("sync subscription should observe the lagged commit after apply resumes")
        .expect("subscription channel should remain open");
    match update {
        SubscriptionUpdate::Result {
            request_id, data, ..
        } => {
            assert!(request_id.is_none());
            assert_eq!(data.len(), 1);
            assert_eq!(data[0]["title"], json!("lagged-sync"));
        }
        other => panic!("unexpected lagged sync subscription event: {other:?}"),
    }
}

#[tokio::test]
async fn service_reload_recovers_durable_journal_before_serving_async_reads() {
    let data_dir = tempdir().expect("service tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let service = Service::new(data_dir.path()).expect("service should create");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    drop(service);

    let store = TenantStore::open(data_dir.path().join("demo.redb")).expect("store should open");
    let document = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("recovered"))]),
    );
    store
        .append_durable_records_batch(&[neovex_core::DurableMutationRecord::new(
            SequenceNumber(1),
            Timestamp(60_000),
            vec![neovex_core::WriteOp {
                table: document.table.clone(),
                op_type: neovex_core::WriteOpType::Insert,
                doc_id: document.id,
                previous: None,
                current: Some(document.clone()),
            }],
            None,
        )
        .expect("durable record should build")])
        .expect("durable journal append should succeed");
    drop(store);

    let reopened = Arc::new(Service::new(data_dir.path()).expect("service should reopen"));
    let documents = reopened
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("async read should recover and succeed");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].id, document.id);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("recovered")));
    assert_eq!(
        reopened
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("journal stats should read after recovery"),
        crate::tenant::MutationJournalStats {
            durable_head: SequenceNumber(1),
            applied_head: SequenceNumber(1),
            apply_lag: 0,
            queue_depth: 0,
            queue_capacity: crate::tenant::DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY,
            oldest_queue_age_nanos: 0,
            pending_response_count: 0,
            worker_running: false,
            worker_start_count: 0,
            worker_restart_count: 0,
            queue_rejection_count: 0,
            worker_failure_count: 0,
            read_wait_count: 0,
            total_read_wait_nanos: 0,
        }
    );

    let second_document_id = reopened
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("after-reopen"))]),
        )
        .await
        .expect("follow-up async insert should succeed after recovery");
    let after_reopen_documents = reopened
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("async reads should continue to succeed after follow-up writes");
    assert_eq!(after_reopen_documents.len(), 2);
    assert!(
        after_reopen_documents
            .iter()
            .any(|candidate| candidate.id == document.id),
        "recovered durable writes should remain visible after follow-up traffic"
    );
    assert!(
        after_reopen_documents
            .iter()
            .any(|candidate| candidate.id == second_document_id),
        "follow-up async writes should succeed after the reopen path"
    );

    let recovered_stats = wait_for_mutation_journal_stats(
        &reopened,
        &tenant_id,
        "mutation journal worker to go idle after the follow-up async write",
        |stats| !stats.worker_running,
    )
    .await;
    assert_eq!(recovered_stats.durable_head, SequenceNumber(2));
    assert_eq!(recovered_stats.applied_head, SequenceNumber(2));
    assert_eq!(recovered_stats.apply_lag, 0);
    assert_eq!(recovered_stats.queue_depth, 0);
    assert_eq!(
        recovered_stats.queue_capacity,
        crate::tenant::DEFAULT_MUTATION_JOURNAL_QUEUE_CAPACITY
    );
    assert_eq!(recovered_stats.oldest_queue_age_nanos, 0);
    assert_eq!(recovered_stats.pending_response_count, 0);
    assert!(!recovered_stats.worker_running);
    assert_eq!(recovered_stats.worker_start_count, 1);
    assert_eq!(recovered_stats.worker_restart_count, 0);
    assert_eq!(recovered_stats.queue_rejection_count, 0);
    assert_eq!(recovered_stats.worker_failure_count, 0);
}

#[tokio::test]
async fn durable_journal_reads_return_strictly_ordered_authoritative_records() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let document_id = service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("journal"))]),
        )
        .await
        .expect("insert should succeed");
    service
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("journal-updated"))]),
        )
        .await
        .expect("update should succeed");

    let records = service
        .read_durable_journal_async(tenant_id.clone(), SequenceNumber(0))
        .await
        .expect("durable journal should read");
    assert_eq!(
        records
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2)]
    );
    assert_eq!(
        records[0].writes[0].op_type,
        neovex_core::WriteOpType::Insert
    );
    assert_eq!(
        records[1].writes[0].op_type,
        neovex_core::WriteOpType::Update
    );
    assert_eq!(
        records[1].writes[0]
            .current
            .as_ref()
            .and_then(|document| document.fields.get("title")),
        Some(&json!("journal-updated"))
    );

    let filtered = service
        .read_durable_journal_async(tenant_id, SequenceNumber(1))
        .await
        .expect("filtered durable journal should read");
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].sequence, SequenceNumber(2));
}

#[tokio::test]
async fn durable_journal_stream_resumes_from_sequence_cursor_with_duplicate_tolerant_pages() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let first_document_id = service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
        )
        .await
        .expect("first insert should succeed");
    service
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            first_document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("first-updated"))]),
        )
        .await
        .expect("update should succeed");
    service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
        )
        .await
        .expect("second insert should succeed");

    let first_page = service
        .stream_durable_journal_async(tenant_id.clone(), SequenceNumber(0), 1)
        .await
        .expect("first journal page should read");
    assert_eq!(first_page.cursor_floor, SequenceNumber(0));
    assert_eq!(first_page.latest_sequence, SequenceNumber(3));
    assert!(first_page.has_more);
    assert_eq!(first_page.next_cursor, SequenceNumber(1));
    assert_eq!(first_page.records.len(), 1);
    assert_eq!(first_page.records[0].sequence, SequenceNumber(1));

    let replayed_first_page = service
        .stream_durable_journal_async(tenant_id.clone(), SequenceNumber(0), 1)
        .await
        .expect("replayed first journal page should read");
    assert_eq!(replayed_first_page.records, first_page.records);
    assert_eq!(replayed_first_page.next_cursor, first_page.next_cursor);

    let second_page = service
        .stream_durable_journal_async(tenant_id.clone(), first_page.next_cursor, 1)
        .await
        .expect("second journal page should read");
    assert!(second_page.has_more);
    assert_eq!(second_page.next_cursor, SequenceNumber(2));
    assert_eq!(second_page.records.len(), 1);
    assert_eq!(second_page.records[0].sequence, SequenceNumber(2));

    let third_page = service
        .stream_durable_journal_async(tenant_id.clone(), second_page.next_cursor, 1)
        .await
        .expect("third journal page should read");
    assert!(!third_page.has_more);
    assert_eq!(third_page.next_cursor, SequenceNumber(3));
    assert_eq!(third_page.records.len(), 1);
    assert_eq!(third_page.records[0].sequence, SequenceNumber(3));

    let empty_page = service
        .stream_durable_journal_async(tenant_id, third_page.next_cursor, 1)
        .await
        .expect("empty journal page should read");
    assert!(!empty_page.has_more);
    assert_eq!(empty_page.next_cursor, SequenceNumber(3));
    assert!(empty_page.records.is_empty());
}

#[tokio::test]
async fn durable_journal_bootstrap_metadata_reconstructs_same_state_as_live_reads() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(80_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut insert_handle = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("bootstrap"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut insert_handle)
            .await
            .is_err(),
        "mutation should remain pending while apply is blocked"
    );

    let bootstrap = service
        .export_durable_journal_bootstrap_async(tenant_id.clone())
        .await
        .expect("bootstrap metadata should read");
    assert_eq!(bootstrap.resume_after, SequenceNumber(0));
    assert_eq!(bootstrap.bootstrap_cut, SequenceNumber(1));
    assert_eq!(bootstrap.cursor_floor, SequenceNumber(0));
    assert_eq!(bootstrap.snapshot.applied_sequence, SequenceNumber(0));
    assert_eq!(bootstrap.snapshot.durable_head, SequenceNumber(1));
    assert!(bootstrap.snapshot.documents.is_empty());

    let page = service
        .stream_durable_journal_async(tenant_id.clone(), bootstrap.resume_after, 10)
        .await
        .expect("journal tail should read");
    assert_eq!(page.records.len(), 1);
    assert_eq!(page.records[0].sequence, SequenceNumber(1));

    faults.release();
    timeout(Duration::from_secs(1), insert_handle)
        .await
        .expect("mutation should finish after apply resumes")
        .expect("mutation task should join")
        .expect("mutation should succeed");

    let rebuilt = TenantStore::create_in_memory().expect("rebuild store should open");
    rebuilt
        .rebuild_materialized_journal_from_snapshot(
            &bootstrap.snapshot,
            &page.records,
            Some(bootstrap.bootstrap_cut),
        )
        .expect("snapshot plus stream tail should rebuild");

    faults.release();

    let live_documents = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("live read should succeed after apply");
    let rebuilt_documents = rebuilt
        .scan_table(&tasks_table())
        .expect("rebuilt store should scan");
    assert_eq!(rebuilt_documents, live_documents);
}

#[tokio::test]
async fn embedded_replica_bootstrap_matches_live_query_and_pagination_results() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for (title, rank) in [("alpha", 1), ("beta", 2), ("gamma", 3)] {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("title".to_string(), json!(title)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");
    let live_query = service
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("live query should succeed");
    let replica_query = replica
        .query_documents(&query_for("tasks"))
        .expect("replica query should succeed");
    assert_eq!(replica_query, live_query);

    let paginated = PaginatedQuery {
        query: Query {
            table: tasks_table(),
            filters: Vec::new(),
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
        page_size: 2,
        after: None,
    };
    let live_page = service
        .paginate_documents_async(tenant_id.clone(), paginated.clone())
        .await
        .expect("live page should succeed");
    let replica_page = replica
        .paginate_documents(&paginated)
        .expect("replica page should succeed");
    assert_eq!(replica_page, live_page);
}

#[tokio::test]
async fn embedded_replica_catches_up_after_reconnection() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("before"))]),
        )
        .await
        .expect("initial insert should succeed");

    let mut replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");
    assert_eq!(replica.sequence_cursor(), SequenceNumber(1));

    service
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("after"))]),
        )
        .await
        .expect("follow-up insert should succeed");

    let stale_documents = replica
        .query_documents(&query_for("tasks"))
        .expect("stale replica query should succeed");
    assert_eq!(stale_documents.len(), 1);

    replica
        .catch_up(&service, 1)
        .await
        .expect("replica catch-up should succeed");
    assert_eq!(replica.sequence_cursor(), SequenceNumber(2));

    let live_documents = service
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("live query should succeed");
    let replica_documents = replica
        .query_documents(&query_for("tasks"))
        .expect("replica query should succeed");
    assert_eq!(replica_documents, live_documents);
}

#[tokio::test]
async fn embedded_replica_catch_up_refreshes_policy_only_schema_changes() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = messages_table("messages_replica_policy");
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let principal = principal_with_subject("user-123");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("authorized fixture insert should succeed");
    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Grace")),
            ]),
        )
        .expect("fixture insert should succeed");

    let mut replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");
    assert_eq!(replica.sequence_cursor(), SequenceNumber(2));

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_replica_policy",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    replica
        .catch_up(&service, 1)
        .await
        .expect("replica catch-up should refresh schema even without new journal records");
    assert_eq!(replica.sequence_cursor(), SequenceNumber(2));

    let live_documents = service
        .query_documents_with_principal(&tenant_id, &query, &principal)
        .expect("live principal query should succeed");
    let replica_documents = replica
        .query_documents_with_principal(&query, &principal)
        .expect("replica principal query should succeed");
    assert_eq!(document_bodies(&replica_documents), vec!["Ada"]);
    assert_eq!(replica_documents, live_documents);

    let live_anonymous = service
        .query_documents(&tenant_id, &query)
        .expect("live anonymous query should succeed");
    let replica_anonymous = replica
        .query_documents(&query)
        .expect("replica anonymous query should succeed");
    assert!(live_anonymous.is_empty());
    assert_eq!(replica_anonymous, live_anonymous);
}

#[tokio::test]
async fn embedded_replica_catch_up_rebuilds_indexes_for_schema_only_changes() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for rank in [1, 2, 3] {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let mut replica = EmbeddedReplica::bootstrap_in_memory(&service, tenant_id.clone())
        .await
        .expect("replica should bootstrap");

    service
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: tasks_table(),
                fields: vec![FieldSchema {
                    name: "rank".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                }],
                indexes: vec![IndexDefinition {
                    name: "by_rank".to_string(),
                    fields: vec!["rank".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("schema should save");

    replica
        .catch_up(&service, 1)
        .await
        .expect("replica catch-up should refresh schema and indexes");

    let query = Query {
        table: tasks_table(),
        filters: vec![filter("rank", FilterOp::Eq, json!(2))],
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let live_documents = service
        .query_documents(&tenant_id, &query)
        .expect("live indexed query should succeed");
    let replica_documents = replica
        .query_documents(&query)
        .expect("replica indexed query should succeed");
    assert_eq!(replica_documents, live_documents);
    assert_eq!(replica_documents.len(), 1);
    assert_eq!(replica_documents[0].fields.get("rank"), Some(&json!(2)));
}

#[tokio::test]
async fn shadow_materializer_queries_match_live_service_path() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    for (title, rank) in [("alpha", 1), ("beta", 2), ("gamma", 3)] {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([
                    ("title".to_string(), json!(title)),
                    ("rank".to_string(), json!(rank)),
                ]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let shadow = service
        .build_shadow_materializer_async(
            tenant_id.clone(),
            ShadowMaterializerConfig {
                compaction_threshold_records: 2,
            },
        )
        .await
        .expect("shadow materializer should build");
    assert_eq!(shadow.manifest().current_sequence, SequenceNumber(3));
    assert_eq!(
        shadow.current_snapshot().applied_sequence,
        SequenceNumber(3)
    );
    let snapshot = shadow.current_snapshot();

    let ordered_query = Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let live_query = service
        .query_documents_async(tenant_id.clone(), ordered_query.clone())
        .await
        .expect("live query should succeed");
    let shadow_query = query_documents_for_docs_with_principal(
        snapshot.documents.clone(),
        &snapshot.schema,
        &ordered_query,
        &PrincipalContext::anonymous(),
    )
    .expect("shadow query should succeed");
    assert_eq!(shadow_query, live_query);

    let paginated = PaginatedQuery {
        query: ordered_query,
        page_size: 2,
        after: None,
    };
    let live_page = service
        .paginate_documents_async(tenant_id, paginated.clone())
        .await
        .expect("live page should succeed");
    let shadow_page = paginate_documents_for_docs_with_principal(
        snapshot.documents.clone(),
        &snapshot.schema,
        &paginated,
        &PrincipalContext::anonymous(),
    )
    .expect("shadow page should succeed");
    assert_eq!(shadow_page, live_page);
}

#[tokio::test]
async fn shadow_materializer_schema_aware_queries_match_live_service_path() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = messages_table("messages_shadow_schema");
    let principal = principal_with_subject("user-123");
    let hidden_owner = principal_with_subject("user-456");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_shadow_schema",
                vec![IndexDefinition {
                    name: "by_owner".to_string(),
                    fields: vec!["owner".to_string()],
                }],
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    for (owner, body) in [
        ("user-123", "Ada"),
        ("user-123", "Beta"),
        ("user-456", "Hidden"),
    ] {
        let principal = if owner == "user-123" {
            principal.clone()
        } else {
            hidden_owner.clone()
        };
        service
            .insert_document_with_principal(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
                &principal,
            )
            .expect("seed insert should succeed");
    }

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

    let indexed_query = Query {
        table: table.clone(),
        filters: vec![filter("owner", FilterOp::Eq, json!("user-123"))],
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let live_query = service
        .query_documents_async_with_principal(
            tenant_id.clone(),
            indexed_query.clone(),
            principal.clone(),
        )
        .await
        .expect("live schema-aware query should succeed");
    let shadow_query = query_documents_for_docs_with_principal(
        snapshot.documents.clone(),
        &snapshot.schema,
        &indexed_query,
        &principal,
    )
    .expect("shadow schema-aware query should succeed");
    assert_eq!(document_bodies(&shadow_query), vec!["Ada", "Beta"]);
    assert_eq!(shadow_query, live_query);

    let paginated = PaginatedQuery {
        query: Query {
            table,
            filters: Vec::new(),
            order: Some(OrderBy {
                field: "body".to_string(),
                direction: OrderDirection::Asc,
            }),
            limit: None,
        },
        page_size: 1,
        after: None,
    };
    let live_page = service
        .paginate_documents_async_with_principal(tenant_id, paginated.clone(), principal.clone())
        .await
        .expect("live schema-aware page should succeed");
    let shadow_page = paginate_documents_for_docs_with_principal(
        snapshot.documents,
        &snapshot.schema,
        &paginated,
        &principal,
    )
    .expect("shadow schema-aware page should succeed");
    assert_eq!(subscription_bodies(&shadow_page.data), vec!["Ada"]);
    assert_eq!(shadow_page, live_page);
}

#[tokio::test]
async fn online_consistency_verifier_matches_authoritative_shadow_and_replica_state() {
    let data_dir = tempdir().expect("service tempdir should build");
    let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    service
        .set_table_schema(
            &tenant_id,
            TableSchema {
                table: tasks_table(),
                fields: vec![FieldSchema {
                    name: "rank".to_string(),
                    field_type: FieldType::Number,
                    required: false,
                }],
                indexes: vec![IndexDefinition {
                    name: "by_rank".to_string(),
                    fields: vec!["rank".to_string()],
                }],
                access_policy: None,
            },
        )
        .expect("schema should save");

    for rank in [1, 2, 3] {
        service
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(rank))]),
            )
            .await
            .expect("seed insert should succeed");
    }

    let report = service
        .verify_consistency_async(tenant_id.clone())
        .await
        .expect("consistency verification should succeed");
    assert!(report.ok, "{report:#?}");
    assert!(report.mismatches.is_empty());
    assert_eq!(report.authoritative.document_count, 3);
    assert_eq!(report.authoritative.schema_table_count, 1);
    assert_eq!(
        report.authoritative.applied_sequence,
        report.authoritative.durable_head
    );
    assert_eq!(report.authoritative.digest, report.shadow.digest);
    assert_eq!(report.authoritative.digest, report.embedded_replica.digest);
    assert!(report.bootstrap.resume_after_sequence <= report.bootstrap.bootstrap_cut_sequence);
    assert_eq!(
        report.bootstrap.bootstrap_cut_sequence,
        report.authoritative.durable_head
    );
    assert!(!report.bootstrap.snapshot_digest.is_empty());
}

#[test]
fn snapshot_comparison_reports_document_field_differences_with_identifier() {
    let document = neovex_core::Document::new(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("alpha"))]),
    );
    let left = materialized_snapshot_with_documents(vec![document.clone()]);
    let mut changed_document = document.clone();
    changed_document
        .fields
        .insert("title".to_string(), json!("beta"));
    let right = materialized_snapshot_with_documents(vec![changed_document]);

    let mismatch = compare_materialized_journal_snapshots(
        ConsistencyScope::AuthoritativeSnapshot,
        &left,
        ConsistencyScope::ShadowMaterializer,
        &right,
    )
    .expect("document mismatch should be reported");

    assert_eq!(mismatch.invariant, "materialized_snapshot_match");
    assert_eq!(mismatch.path, format!("documents.tasks/{}", document.id));
    assert_eq!(mismatch.left_scope, ConsistencyScope::AuthoritativeSnapshot);
    assert_eq!(mismatch.right_scope, ConsistencyScope::ShadowMaterializer);
    assert!(mismatch.left_description.contains("alpha"));
    assert!(mismatch.right_description.contains("beta"));
}

#[test]
fn durable_journal_bootstrap_verifier_reports_resume_after_mismatch() {
    let snapshot = materialized_snapshot_with_documents(Vec::new());
    let bootstrap = DurableJournalBootstrap {
        snapshot: snapshot.clone(),
        resume_after: SequenceNumber(4),
        bootstrap_cut: snapshot.durable_head,
        cursor_floor: SequenceNumber(0),
    };

    let mismatches = collect_durable_journal_bootstrap_mismatches(&snapshot, &bootstrap);
    let resume_after = mismatches
        .iter()
        .find(|mismatch| mismatch.path == "bootstrap.resume_after_sequence")
        .expect("resume_after mismatch should be reported");
    assert_eq!(resume_after.invariant, "bootstrap_metadata_match");
    assert_eq!(
        resume_after.left_scope,
        ConsistencyScope::AuthoritativeSnapshot
    );
    assert_eq!(resume_after.right_scope, ConsistencyScope::JournalBootstrap);
    assert!(resume_after.left_description.contains('1'));
    assert!(resume_after.right_description.contains('4'));
}

#[tokio::test]
async fn generated_task_history_matches_model_across_live_shadow_and_embedded_replica_surfaces() {
    let history = GeneratedTaskHistory::seeded("engine-generated-history", 41, 48);
    assert_generated_task_history_matches_model_across_surfaces(
        &history,
        None,
        "generated_task_history_matches_model_across_live_shadow_and_embedded_replica_surfaces",
    )
    .await;
}

#[tokio::test]
#[ignore = "run through verification harness pr mode"]
async fn verification_harness_pr_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::PullRequest)
        .expect("pull-request corpus should resolve")
    {
        let history = case.history("engine-generated-history");
        assert_generated_task_history_matches_model_across_surfaces(
            &history,
            Some(case),
            "verification_harness_pr_generated_history_seed_corpus_matches_model",
        )
        .await;
    }
}

#[tokio::test]
#[ignore = "run through verification harness nightly mode"]
async fn verification_harness_nightly_generated_history_seed_corpus_matches_model() {
    for case in selected_generated_task_history_seed_corpus(VerificationHarnessMode::Nightly)
        .expect("nightly corpus should resolve")
    {
        let history = case.history("engine-generated-history");
        assert_generated_task_history_matches_model_across_surfaces(
            &history,
            Some(case),
            "verification_harness_nightly_generated_history_seed_corpus_matches_model",
        )
        .await;
    }
}

#[tokio::test]
async fn schema_async_write_path_rebuilds_and_removes_indexes_durably() {
    let data_dir = tempdir().expect("service tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let schema = TableSchema {
        table: tasks_table(),
        fields: vec![FieldSchema {
            name: "rank".to_string(),
            field_type: FieldType::Number,
            required: false,
        }],
        indexes: vec![IndexDefinition {
            name: "by_rank".to_string(),
            fields: vec!["rank".to_string()],
        }],
        access_policy: None,
    };

    {
        let service = Arc::new(Service::new(data_dir.path()).expect("service should create"));
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(7))]),
            )
            .expect("insert should succeed");
        service
            .insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("rank".to_string(), json!(9))]),
            )
            .expect("insert should succeed");
        service
            .set_table_schema_async(tenant_id.clone(), schema.clone())
            .await
            .expect("schema should save");
    }

    let store = neovex_storage::TenantStore::open(
        data_dir.path().join(format!("{}.redb", tenant_id.as_str())),
    )
    .expect("tenant store should reopen");
    assert_eq!(
        store
            .index_scan_eq(&tasks_table(), "by_rank", &json!(7))
            .expect("index scan should succeed")
            .len(),
        1
    );
    drop(store);

    {
        let service = Arc::new(Service::new(data_dir.path()).expect("service should recreate"));
        service
            .delete_table_schema_async(tenant_id.clone(), tasks_table())
            .await
            .expect("schema should delete");
    }

    let store = neovex_storage::TenantStore::open(
        data_dir.path().join(format!("{}.redb", tenant_id.as_str())),
    )
    .expect("tenant store should reopen after deletion");
    assert!(
        store
            .index_scan_eq(&tasks_table(), "by_rank", &json!(7))
            .expect("index scan should succeed")
            .is_empty(),
        "async schema deletion should clear rebuilt index entries"
    );
}

#[test]
fn paginate_without_cursor_returns_first_page() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie", "delta", "echo"] {
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    assert_eq!(page.data.len(), 2);
    assert_eq!(page.data[0]["title"], json!("alpha"));
    assert_eq!(page.data[1]["title"], json!("bravo"));
    assert!(page.has_more);
    assert!(page.next_cursor.is_some());
}

#[test]
fn paginate_with_cursor_returns_next_page() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie", "delta", "echo"] {
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let first_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    let second_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: first_page.next_cursor.clone(),
        },
    )
    .expect("pagination should succeed");

    assert_eq!(second_page.data.len(), 2);
    assert_eq!(second_page.data[0]["title"], json!("charlie"));
    assert_eq!(second_page.data[1]["title"], json!("delta"));
    assert!(second_page.has_more);
    assert!(second_page.next_cursor.is_some());
}

#[test]
fn paginate_rejects_cursor_for_different_query_shape() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let first_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    let error = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Desc,
                }),
                limit: None,
            },
            page_size: 2,
            after: first_page.next_cursor,
        },
    )
    .expect_err("cursor should be rejected");

    assert!(matches!(error, Error::InvalidInput(_)));
}

#[test]
fn paginate_last_page_has_no_cursor() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for title in ["alpha", "bravo", "charlie"] {
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!(title))]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let first_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    let last_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: first_page.next_cursor,
        },
    )
    .expect("pagination should succeed");

    assert_eq!(last_page.data.len(), 1);
    assert_eq!(last_page.data[0]["title"], json!("charlie"));
    assert!(!last_page.has_more);
    assert!(last_page.next_cursor.is_none());
}

#[test]
fn paginate_empty_table_returns_empty_page() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");

    let page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            page_size: 2,
            after: None,
        },
    )
    .expect("pagination should succeed");

    assert!(page.data.is_empty());
    assert!(!page.has_more);
    assert!(page.next_cursor.is_none());
}

#[test]
fn paginate_with_filters_and_ordering() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for (title, status) in [
        ("a", "todo"),
        ("b", "done"),
        ("c", "todo"),
        ("d", "todo"),
        ("e", "done"),
        ("f", "todo"),
        ("g", "todo"),
    ] {
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([
                ("title".to_string(), json!(title)),
                ("status".to_string(), json!(status)),
            ]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let first_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("todo"))],
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Desc,
                }),
                limit: None,
            },
            page_size: 3,
            after: None,
        },
    )
    .expect("pagination should succeed");

    assert_eq!(
        first_page
            .data
            .iter()
            .map(|document| document["title"]
                .as_str()
                .expect("title should be a string"))
            .collect::<Vec<_>>(),
        vec!["g", "f", "d"]
    );
    assert!(first_page.has_more);

    let second_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("todo"))],
                order: Some(OrderBy {
                    field: "title".to_string(),
                    direction: OrderDirection::Desc,
                }),
                limit: None,
            },
            page_size: 3,
            after: first_page.next_cursor,
        },
    )
    .expect("pagination should succeed");

    assert_eq!(
        second_page
            .data
            .iter()
            .map(|document| document["title"]
                .as_str()
                .expect("title should be a string"))
            .collect::<Vec<_>>(),
        vec!["c", "a"]
    );
    assert!(!second_page.has_more);
}

#[test]
fn fallback_query_filters_during_scan_for_selective_match() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for rank in 0..512 {
        let status = if rank % 97 == 0 { "keep" } else { "skip" };
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([
                ("rank".to_string(), json!(rank)),
                ("status".to_string(), json!(status)),
            ]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let query = Query {
        table: tasks_table(),
        filters: vec![filter("status", FilterOp::Eq, json!("keep"))],
        order: Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    let documents = evaluate_query(&store, &query).expect("fallback query should evaluate");
    let ranks = documents
        .into_iter()
        .map(|document| {
            document
                .fields
                .get("rank")
                .and_then(serde_json::Value::as_i64)
                .expect("rank should be present")
        })
        .collect::<Vec<_>>();
    assert_eq!(ranks, vec![0, 97, 194, 291, 388, 485]);
}

#[test]
fn paginated_fallback_scan_preserves_cursor_and_ordering() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    for rank in 0..12 {
        let status = if rank % 2 == 0 { "todo" } else { "done" };
        let document = neovex_core::Document::new(
            tasks_table(),
            serde_json::Map::from_iter([
                ("rank".to_string(), json!(rank)),
                ("status".to_string(), json!(status)),
            ]),
        );
        store.insert(&document).expect("insert should succeed");
    }

    let paginated = PaginatedQuery {
        query: Query {
            table: tasks_table(),
            filters: vec![filter("status", FilterOp::Eq, json!("todo"))],
            order: Some(OrderBy {
                field: "rank".to_string(),
                direction: OrderDirection::Desc,
            }),
            limit: None,
        },
        page_size: 2,
        after: None,
    };

    let first_page =
        evaluate_paginated(&store, &paginated).expect("first fallback page should evaluate");
    assert_eq!(
        first_page
            .data
            .iter()
            .map(|document| {
                document
                    .get("rank")
                    .and_then(serde_json::Value::as_i64)
                    .expect("rank should be present")
            })
            .collect::<Vec<_>>(),
        vec![10, 8]
    );
    assert!(first_page.has_more);
    let second_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            after: first_page.next_cursor.clone(),
            ..paginated.clone()
        },
    )
    .expect("second fallback page should evaluate");
    assert_eq!(
        second_page
            .data
            .iter()
            .map(|document| {
                document
                    .get("rank")
                    .and_then(serde_json::Value::as_i64)
                    .expect("rank should be present")
            })
            .collect::<Vec<_>>(),
        vec![6, 4]
    );
    assert!(second_page.has_more);
    let third_page = evaluate_paginated(
        &store,
        &PaginatedQuery {
            after: second_page.next_cursor.clone(),
            ..paginated
        },
    )
    .expect("third fallback page should evaluate");
    assert_eq!(
        third_page
            .data
            .iter()
            .map(|document| {
                document
                    .get("rank")
                    .and_then(serde_json::Value::as_i64)
                    .expect("rank should be present")
            })
            .collect::<Vec<_>>(),
        vec![2, 0]
    );
    assert!(!third_page.has_more);
    assert!(third_page.next_cursor.is_none());
}

#[test]
fn paginate_rejects_zero_page_size() {
    let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
    let error = evaluate_paginated(
        &store,
        &PaginatedQuery {
            query: Query {
                table: tasks_table(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            page_size: 0,
            after: None,
        },
    )
    .expect_err("pagination should fail");

    assert!(matches!(error, Error::InvalidInput(_)));
}

#[tokio::test]
async fn service_read_policy_filters_indexed_queries_and_hides_unauthorized_gets() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_indexed");

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_indexed",
                vec![IndexDefinition {
                    name: "by_owner".to_string(),
                    fields: vec!["owner".to_string()],
                }],
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("authorized fixture insert should succeed");
    let unauthorized_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Grace")),
            ]),
        )
        .expect("fixture insert should succeed");

    let principal = principal_with_subject("user-123");
    let documents = service
        .query_documents_with_principal(
            &tenant_id,
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "body".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
            &principal,
        )
        .expect("query should succeed");

    assert_eq!(document_bodies(&documents), vec!["Ada"]);
    assert!(matches!(
        service.get_document_with_principal(&tenant_id, &table, unauthorized_id, &principal),
        Err(Error::DocumentNotFound(_))
    ));
}

#[tokio::test]
async fn service_read_policy_filters_full_scans_pagination_and_subscription_results() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_scanned");

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_scanned",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    for (owner, body) in [
        ("user-123", "Ada-1"),
        ("user-456", "Grace"),
        ("user-123", "Ada-2"),
    ] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("fixture insert should succeed");
    }

    let principal = principal_with_subject("user-123");
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };
    let documents = service
        .query_documents_with_principal(&tenant_id, &query, &principal)
        .expect("full-scan query should succeed");
    assert_eq!(document_bodies(&documents), vec!["Ada-1", "Ada-2"]);

    let first_page = service
        .paginate_documents_with_principal(
            &tenant_id,
            &PaginatedQuery {
                query: query.clone(),
                page_size: 1,
                after: None,
            },
            &principal,
        )
        .expect("first page should succeed");
    assert_eq!(subscription_bodies(&first_page.data), vec!["Ada-1"]);
    assert!(first_page.has_more);

    let second_page = service
        .paginate_documents_with_principal(
            &tenant_id,
            &PaginatedQuery {
                query: query.clone(),
                page_size: 1,
                after: first_page.next_cursor.clone(),
            },
            &principal,
        )
        .expect("second page should succeed");
    assert_eq!(subscription_bodies(&second_page.data), vec!["Ada-2"]);
    assert!(!second_page.has_more);

    let (tx, mut rx) = subscription_channel();
    let _subscription = service
        .subscribe_with_principal(&tenant_id, query, &principal, "req-1".to_string(), tx)
        .expect("subscription should succeed");

    match rx
        .recv()
        .await
        .expect("initial subscription event should arrive")
    {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(subscription_bodies(&data), vec!["Ada-1", "Ada-2"]);
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    service
        .insert_document(
            &tenant_id,
            table,
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-999")),
                ("body".to_string(), json!("Blocked")),
            ]),
        )
        .expect("unauthorized fixture insert should still commit for another owner");

    match rx.recv().await.expect("subscription update should arrive") {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(subscription_bodies(&data), vec!["Ada-1", "Ada-2"]);
        }
        other => panic!("unexpected subscription update: {other:?}"),
    }
}

#[tokio::test]
async fn materialized_surface_respects_read_policy_after_schema_change() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_materialized_schema_change");
    let query = Query {
        table: table.clone(),
        filters: Vec::new(),
        order: Some(OrderBy {
            field: "body".to_string(),
            direction: OrderDirection::Asc,
        }),
        limit: None,
    };

    for (owner, body) in [("user-123", "Ada"), ("user-456", "Grace")] {
        service
            .insert_document(
                &tenant_id,
                table.clone(),
                serde_json::Map::from_iter([
                    ("owner".to_string(), json!(owner)),
                    ("body".to_string(), json!(body)),
                ]),
            )
            .expect("fixture insert should succeed");
    }

    let warmed = service
        .query_documents(&tenant_id, &query)
        .expect("warming query should succeed");
    assert_eq!(document_bodies(&warmed), vec!["Ada", "Grace"]);

    let warmed_stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(warmed_stats.table_load_count, 1);

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_materialized_schema_change",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");

    let visible = service
        .query_documents_with_principal(&tenant_id, &query, &principal_with_subject("user-123"))
        .expect("authorized query should succeed after schema change");
    assert_eq!(document_bodies(&visible), vec!["Ada"]);

    let post_change_stats = service
        .materialized_read_surface_stats_for_testing(&tenant_id)
        .expect("materialized surface stats should load");
    assert_eq!(post_change_stats.table_load_count, 2);
    assert_eq!(post_change_stats.loaded_table_count, 1);
    assert!(post_change_stats.evaluation_count > warmed_stats.evaluation_count);
}

#[tokio::test]
async fn service_write_policy_rejects_create_update_and_delete_before_commit() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_writes");

    service
        .set_table_schema(
            &tenant_id,
            messages_schema("messages_writes", Vec::new(), Some(owner_write_policy())),
        )
        .expect("schema should save");

    let owner_principal = principal_with_subject("user-123");
    let intruder = principal_with_subject("user-999");
    let initial_sequence = service
        .latest_sequence(&tenant_id)
        .expect("latest sequence should load");

    let create_error = service
        .insert_document_with_principal(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Blocked create")),
            ]),
            &intruder,
        )
        .expect_err("create should be denied");
    assert!(matches!(create_error, Error::PermissionDenied(_)));
    assert_eq!(
        service
            .latest_sequence(&tenant_id)
            .expect("latest sequence should remain unchanged"),
        initial_sequence
    );
    assert!(
        service
            .list_documents(&tenant_id, &table)
            .expect("list should succeed")
            .is_empty(),
        "denied create should not commit"
    );

    let document_id = service
        .insert_document_with_principal(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Allowed")),
            ]),
            &owner_principal,
        )
        .expect("authorized create should succeed");
    let committed_sequence = service
        .latest_sequence(&tenant_id)
        .expect("latest sequence should advance after authorized insert");

    let update_error = service
        .update_document_with_principal(
            &tenant_id,
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Intruder edit"))]),
            &intruder,
        )
        .expect_err("update should be denied");
    assert!(matches!(update_error, Error::PermissionDenied(_)));
    assert_eq!(
        service
            .latest_sequence(&tenant_id)
            .expect("latest sequence should remain unchanged"),
        committed_sequence
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id)
            .expect("document should still exist")
            .get_field("body")
            .expect("body should be present"),
        &json!("Allowed")
    );

    let delete_error = service
        .delete_document_with_principal(&tenant_id, table.clone(), document_id, &intruder)
        .expect_err("delete should be denied");
    assert!(matches!(delete_error, Error::PermissionDenied(_)));
    assert_eq!(
        service
            .latest_sequence(&tenant_id)
            .expect("latest sequence should remain unchanged"),
        committed_sequence
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id)
            .expect("document should still exist")
            .get_field("body")
            .expect("body should be present"),
        &json!("Allowed")
    );
}

#[test]
fn mutation_execution_unit_aborts_on_overlapping_document_conflict() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_doc");

    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document = execution_unit
        .get_document(&table, document_id)
        .expect("point read should succeed")
        .expect("document should exist");
    assert_eq!(document.get_field("body"), Some(&json!("Initial")));
    execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("concurrent update should commit");

    let error = execution_unit
        .commit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(error, Error::Conflict(_)));
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id)
            .expect("document should remain committed")
            .get_field("body"),
        Some(&json!("Outside update"))
    );
}

#[test]
fn mutation_execution_unit_commits_when_concurrent_write_is_disjoint() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_disjoint");

    let first_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("First")),
            ]),
        )
        .expect("first fixture insert should succeed");
    let second_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Second")),
            ]),
        )
        .expect("second fixture insert should succeed");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let read_back = execution_unit
        .get_document(&table, first_id)
        .expect("point read should succeed")
        .expect("document should exist");
    assert_eq!(read_back.get_field("body"), Some(&json!("First")));
    execution_unit
        .update_document(
            table.clone(),
            first_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            second_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("disjoint update should commit");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(commit.writes.len(), 1);
    assert_eq!(
        service
            .get_document(&tenant_id, &table, first_id)
            .expect("first document should exist")
            .get_field("body"),
        Some(&json!("Tx update"))
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, second_id)
            .expect("second document should exist")
            .get_field("body"),
        Some(&json!("Outside update"))
    );
}

#[test]
fn mutation_execution_unit_insert_then_update_commits_as_single_insert() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_insert_update");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("staged insert should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Updated"))]),
        )
        .expect("staged update should succeed");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(commit.writes.len(), 1);
    assert!(commit.writes[0].previous.is_none());
    assert_eq!(
        commit.writes[0]
            .current
            .as_ref()
            .and_then(|document| document.get_field("body")),
        Some(&json!("Updated"))
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id)
            .expect("inserted document should exist")
            .get_field("body"),
        Some(&json!("Updated"))
    );
}

#[test]
fn mutation_execution_unit_insert_then_delete_commits_as_noop() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_insert_delete");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Transient")),
            ]),
        )
        .expect("staged insert should succeed");
    execution_unit
        .delete_document(table.clone(), document_id)
        .expect("staged delete should succeed");

    let commit = execution_unit.commit().expect("commit should succeed");
    assert!(
        commit.is_none(),
        "insert followed by delete should collapse to a no-op"
    );
    let error = service
        .get_document(&tenant_id, &table, document_id)
        .expect_err("transient document should not exist");
    assert!(matches!(error, Error::DocumentNotFound(_)));
}

#[test]
fn mutation_execution_unit_restage_after_revert_commits_once() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_restage");

    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("First"))]),
        )
        .expect("first staged update should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Initial"))]),
        )
        .expect("revert staged update should succeed");
    execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Second"))]),
        )
        .expect("restaged update should succeed");

    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(
        commit.writes.len(),
        1,
        "restaging after a revert should only produce one final write"
    );
    assert_eq!(
        service
            .get_document(&tenant_id, &table, document_id)
            .expect("document should exist")
            .get_field("body"),
        Some(&json!("Second"))
    );
}

#[tokio::test]
async fn mutation_execution_unit_conflicts_with_durable_unapplied_write() {
    let data_dir = tempdir().expect("service tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let service = Arc::new(
        Service::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(92_000))),
            faults.clone(),
        )
        .expect("service should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    service
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    let table = messages_table("messages_occ_apply_lag");

    let mut outside_update = tokio::spawn({
        let service = service.clone();
        let tenant_id = tenant_id.clone();
        let table = table.clone();
        async move {
            service
                .insert_document_async(
                    tenant_id,
                    table,
                    serde_json::Map::from_iter([
                        ("owner".to_string(), json!("user-456")),
                        ("body".to_string(), json!("Outside insert")),
                    ]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert!(
        timeout(Duration::from_millis(100), &mut outside_update)
            .await
            .is_err(),
        "outside update should remain pending while apply is blocked"
    );

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let visible = execution_unit
        .query_documents_cancellable(
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            &mut || Ok(()),
        )
        .expect("query should succeed");
    assert!(
        visible.is_empty(),
        "execution unit should still see the applied snapshot while the outside write lags"
    );
    execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Tx insert")),
            ]),
        )
        .expect("staged insert should succeed");

    let commit_handle = tokio::task::spawn_blocking({
        let execution_unit = execution_unit.clone();
        move || execution_unit.commit()
    });

    let commit_result = timeout(Duration::from_secs(1), commit_handle)
        .await
        .expect("commit should resolve promptly while the journal worker is still blocked")
        .expect("commit task should join successfully");
    faults.release();
    timeout(Duration::from_secs(1), outside_update)
        .await
        .expect("outside update should finish after apply resumes")
        .expect("outside update task should join successfully")
        .expect("outside update should succeed");

    let error = commit_result.expect_err(
        "commit should conflict with the durable journal write that was not part of the applied snapshot",
    );
    assert!(matches!(error, Error::Conflict(_)));
    let documents = service
        .query_documents(
            &tenant_id,
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: Some(OrderBy {
                    field: "body".to_string(),
                    direction: OrderDirection::Asc,
                }),
                limit: None,
            },
        )
        .expect("query should succeed after apply");
    assert_eq!(documents.len(), 1);
    assert_eq!(
        documents[0].get_field("body"),
        Some(&json!("Outside insert"))
    );
}

#[test]
fn mutation_execution_unit_conflicts_when_auth_filtered_visibility_changes() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_auth");

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_occ_auth",
                vec![IndexDefinition {
                    name: "by_owner".to_string(),
                    fields: vec!["owner".to_string()],
                }],
                Some(owner_read_write_policy()),
            ),
        )
        .expect("schema should save");
    let hidden_owner = principal_with_subject("user-456");

    let hidden_id = service
        .insert_document_with_principal(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-456")),
                ("body".to_string(), json!("Hidden")),
            ]),
            &hidden_owner,
        )
        .expect("hidden document insert should succeed");

    let principal = principal_with_subject("user-123");
    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), principal.clone())
        .expect("execution unit should start");
    let visible = execution_unit
        .query_documents_cancellable(
            &Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            &mut || Ok(()),
        )
        .expect("authorized query should succeed");
    assert!(visible.is_empty(), "hidden row should not be visible yet");

    execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Tx insert")),
            ]),
        )
        .expect("authorized staged insert should succeed");

    service
        .update_document_with_principal(
            &tenant_id,
            table.clone(),
            hidden_id,
            serde_json::Map::from_iter([("owner".to_string(), json!("user-123"))]),
            &hidden_owner,
        )
        .expect("external update should make the hidden row visible");

    let error = execution_unit
        .commit()
        .expect_err("commit should detect the auth-filtered visibility change");
    assert!(matches!(error, Error::Conflict(_)));
}

#[test]
fn mutation_execution_unit_rejects_reuse_after_successful_commit() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_finalize_success");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    let document_id = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Committed")),
            ]),
        )
        .expect("staged insert should succeed");
    let commit = execution_unit
        .commit()
        .expect("commit should succeed")
        .expect("commit entry should be returned");
    assert_eq!(commit.writes.len(), 1);

    let read_error = execution_unit
        .get_document(&table, document_id)
        .expect_err("finalized execution unit should reject further reads");
    assert!(matches!(read_error, Error::InvalidInput(message) if message.contains("finalized")));

    let write_error = execution_unit
        .insert_document(
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Second")),
            ]),
        )
        .expect_err("finalized execution unit should reject further writes");
    assert!(matches!(write_error, Error::InvalidInput(message) if message.contains("finalized")));

    let commit_error = execution_unit
        .commit()
        .expect_err("finalized execution unit should reject a second commit");
    assert!(matches!(commit_error, Error::InvalidInput(message) if message.contains("finalized")));
}

#[test]
fn mutation_execution_unit_rejects_reuse_after_failed_commit_attempt() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_occ_finalize_failure");

    let document_id = service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Initial")),
            ]),
        )
        .expect("fixture insert should succeed");

    let execution_unit = service
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should start");
    execution_unit
        .get_document(&table, document_id)
        .expect("point read should succeed")
        .expect("document should exist");
    execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Tx update"))]),
        )
        .expect("staged update should succeed");

    service
        .update_document(
            &tenant_id,
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Outside update"))]),
        )
        .expect("concurrent update should commit");

    let commit_error = execution_unit
        .commit()
        .expect_err("commit should detect the conflict");
    assert!(matches!(commit_error, Error::Conflict(_)));

    let read_error = execution_unit
        .get_document(&table, document_id)
        .expect_err("conflicted execution unit should reject further reads");
    assert!(matches!(read_error, Error::InvalidInput(message) if message.contains("finalized")));

    let write_error = execution_unit
        .update_document(
            table.clone(),
            document_id,
            serde_json::Map::from_iter([("body".to_string(), json!("Retry"))]),
        )
        .expect_err("conflicted execution unit should reject further writes");
    assert!(matches!(write_error, Error::InvalidInput(message) if message.contains("finalized")));

    let second_commit_error = execution_unit
        .commit()
        .expect_err("conflicted execution unit should reject a second commit");
    assert!(
        matches!(second_commit_error, Error::InvalidInput(message) if message.contains("finalized"))
    );
}

#[tokio::test]
async fn policy_revision_changes_terminate_active_authorized_subscriptions() {
    let fixture = ServiceFixture::new(|path| Service::new(path));
    let service = fixture.service();
    let tenant_id = fixture.create_tenant("demo", Service::create_tenant);
    let table = messages_table("messages_policy");

    service
        .set_table_schema(
            &tenant_id,
            messages_schema(
                "messages_policy",
                Vec::new(),
                Some(read_only_owner_policy()),
            ),
        )
        .expect("schema should save");
    service
        .insert_document(
            &tenant_id,
            table.clone(),
            serde_json::Map::from_iter([
                ("owner".to_string(), json!("user-123")),
                ("body".to_string(), json!("Ada")),
            ]),
        )
        .expect("fixture insert should succeed");

    let (tx, mut rx) = subscription_channel();
    let principal = principal_with_subject("user-123");
    let _subscription = service
        .subscribe_with_principal(
            &tenant_id,
            Query {
                table: table.clone(),
                filters: Vec::new(),
                order: None,
                limit: None,
            },
            &principal,
            "req-1".to_string(),
            tx,
        )
        .expect("subscription should succeed");
    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        1
    );

    match rx
        .recv()
        .await
        .expect("initial subscription event should arrive")
    {
        SubscriptionUpdate::Result { data, .. } => {
            assert_eq!(subscription_bodies(&data), vec!["Ada"]);
        }
        other => panic!("unexpected initial subscription event: {other:?}"),
    }

    let changed_policy = TableAccessPolicy {
        read: owner_matches_subject_rule(AccessValue::DocumentField {
            field: "body".to_string(),
        }),
        ..TableAccessPolicy::default()
    };
    service
        .set_table_schema(
            &tenant_id,
            messages_schema("messages_policy", Vec::new(), Some(changed_policy)),
        )
        .expect("updated schema should save");

    match rx.recv().await.expect("policy-change error should arrive") {
        SubscriptionUpdate::Error { message, .. } => {
            assert!(
                message.contains("authorization policy changed; resubscribe"),
                "unexpected message: {message}"
            );
        }
        other => panic!("unexpected post-policy-change event: {other:?}"),
    }
    assert_eq!(
        service
            .active_subscription_count(&tenant_id)
            .expect("subscription count should load"),
        0
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn evaluator_gt_results_are_subset_of_gte(
        values in prop::collection::vec(-50i64..50, 0..20),
        threshold in -50i64..50,
    ) {
        let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
        for value in values {
            store.insert(&rank_document(value)).expect("insert should succeed");
        }

        let mut gt_query = query_for("tasks");
        gt_query.filters = vec![filter("rank", FilterOp::Gt, json!(threshold))];
        let gt_documents = evaluate_query(&store, &gt_query).expect("gt query should evaluate");

        let mut gte_query = query_for("tasks");
        gte_query.filters = vec![filter("rank", FilterOp::Gte, json!(threshold))];
        let gte_documents = evaluate_query(&store, &gte_query).expect("gte query should evaluate");

        let gt_ids = gt_documents
            .iter()
            .map(|document| document.id.to_string())
            .collect::<BTreeSet<_>>();
        let gte_ids = gte_documents
            .iter()
            .map(|document| document.id.to_string())
            .collect::<BTreeSet<_>>();

        prop_assert!(gt_ids.is_subset(&gte_ids));
        for document in gt_documents {
            prop_assert!(
                document.fields["rank"]
                    .as_i64()
                    .expect("rank should be an i64")
                    > threshold
            );
        }
    }

    #[test]
    fn evaluator_descending_matches_reversed_ascending_for_unique_values(
        values in prop::collection::btree_set(-50i64..50, 0..20),
    ) {
        let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
        for value in &values {
            store.insert(&rank_document(*value)).expect("insert should succeed");
        }

        let mut asc_query = query_for("tasks");
        asc_query.order = Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Asc,
        });
        let asc = evaluate_query(&store, &asc_query)
            .expect("ascending query should evaluate")
            .into_iter()
            .map(|document| {
                document.fields["rank"]
                    .as_i64()
                    .expect("rank should be an i64")
            })
            .collect::<Vec<_>>();

        let mut desc_query = query_for("tasks");
        desc_query.order = Some(OrderBy {
            field: "rank".to_string(),
            direction: OrderDirection::Desc,
        });
        let desc = evaluate_query(&store, &desc_query)
            .expect("descending query should evaluate")
            .into_iter()
            .map(|document| {
                document.fields["rank"]
                    .as_i64()
                    .expect("rank should be an i64")
            })
            .collect::<Vec<_>>();

        let mut expected = asc.clone();
        expected.reverse();
        prop_assert_eq!(desc, expected);
    }

    #[test]
    fn evaluator_limit_never_exceeds_requested_size(
        values in prop::collection::vec(-50i64..50, 0..20),
        limit in 0usize..30,
    ) {
        let store = neovex_storage::TenantStore::create_in_memory().expect("store should open");
        for value in &values {
            store.insert(&rank_document(*value)).expect("insert should succeed");
        }

        let mut query = query_for("tasks");
        query.limit = Some(limit);
        let documents = evaluate_query(&store, &query).expect("query should evaluate");

        prop_assert!(documents.len() <= limit);
        prop_assert!(documents.len() <= values.len());
    }
}
