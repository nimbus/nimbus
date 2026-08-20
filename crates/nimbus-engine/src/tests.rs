pub(crate) use nimbus_core::{
    AccessValue, DocumentId, Error, FieldSchema, FieldType, Filter, FilterOp, IndexDefinition,
    ManualWallClock, OrderBy, OrderDirection, Page, PaginatedQuery, PrincipalContext, Query,
    SequenceNumber, TableAccessPolicy, TableName, TableSchema, TenantId, Timestamp,
};
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) use nimbus_testing::ppsc::PpscBackend;
pub(crate) use nimbus_testing::{
    BlockingFaultInjector, BoundedTestBarrier as Barrier, CountedFaultInjector, EngineFixture,
    GeneratedTaskHistory, GeneratedTaskHistorySeedCase, GeneratedTaskPageExpectation,
    GeneratedTaskRecord, VerificationHarnessMode, ci_or_local_duration,
    replay_generated_task_history_async, selected_generated_task_history_seed_corpus,
    wait_for_value,
};
pub(crate) use serde_json::json;
pub(crate) use std::collections::BTreeSet;
pub(crate) use std::future::Future;
pub(crate) use std::pin::Pin;
pub(crate) use std::sync::atomic::{AtomicBool, Ordering};
pub(crate) use std::sync::{Arc, Condvar, Mutex};
pub(crate) use std::task::{Context, Poll};
pub(crate) use tempfile::{TempDir, tempdir};
pub(crate) use tokio::sync::{Notify, mpsc};
pub(crate) use tokio::time::{Duration, timeout};

pub(crate) use crate::engine::{
    SubscribeOptions, SubscriptionBootstrapCancellation,
    paginate_documents_for_docs_with_principal, query_documents_for_docs_with_principal,
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
    EmbeddedReplica, Engine, EnginePersistenceConfig, ShadowMaterializerConfig, SubscriptionUpdate,
};
pub(crate) use nimbus_storage::{
    DurableJournalBootstrap, EmbeddedProviderKind, FaultPoint, SqliteTenantStore, TenantStore,
};

mod ambient_sources;
mod clocks;
mod committer_lease;
#[path = "../benches/support/concurrent_write_phase_split.rs"]
mod concurrent_write_phase_split;
mod concurrent_write_phase_split_tests;
mod consistency;
mod embedded_providers;
#[cfg(feature = "libsql")]
mod libsql_replica_provider;
mod materialized_serving;
mod mutation_journal;
#[cfg(feature = "mysql")]
mod mysql_provider;
mod objects;
mod policy;
#[cfg(feature = "postgres")]
mod postgres_provider;
mod ppsc;
// Bounded-timeout helpers for the PostgreSQL suite only; the embedded suites
// run against local storage and need no such budget, and the other two provider
// suites bound their own waits.
#[cfg(feature = "postgres")]
mod provider_fixtures;
mod provider_publisher_contract;
mod queries;
mod subscriptions;

// Shared exercisers for the three remote-provider suites. They follow the
// provider gates: the embedded suites cover these behaviours through their own
// tests, so nothing reaches them in a build without a remote provider.
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) use nimbus_storage::provider_test_fixtures::{
    ExternalProviderFixtureMode, external_provider_fixture_mode,
};
#[cfg(feature = "libsql")]
pub(crate) use ppsc::exercise_ppsc_provider_scenario_differential;
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) use ppsc::{
    exercise_ppsc_provider_authority_extension, exercise_ppsc_provider_retained_differential,
};
#[cfg(feature = "postgres")]
pub(crate) use provider_fixtures::expect_external_provider_future_within;
pub(crate) use provider_publisher_contract::exercise_provider_publisher_contract;
// Narrower than the surrounding provider gate: only the SQL provider suites
// assert on the write pipeline. The libSQL replica writes through its remote
// primary, so a libsql-only build has no consumer for this expectation.
#[cfg(any(feature = "mysql", feature = "postgres"))]
pub(crate) use provider_publisher_contract::ProviderPipelineExpectation;
#[cfg(any(feature = "libsql", feature = "mysql", feature = "postgres"))]
pub(crate) use provider_publisher_contract::{
    exercise_provider_schedule_only_execution_unit_fence_contract,
    exercise_provider_scheduler_fence_contract,
    exercise_provider_trigger_invocation_fence_contract,
};
#[cfg(feature = "postgres")]
pub(crate) use provider_publisher_contract::{
    exercise_provider_trigger_outcome_acknowledgement_loss_contract,
    exercise_provider_trigger_transition_serialization_contract,
};

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
    engine: &Engine,
    tenant_id: &TenantId,
    after: SequenceNumber,
) -> Vec<nimbus_core::CommitEntry> {
    engine
        .read_durable_journal(tenant_id, after)
        .expect("durable journal should read")
        .into_iter()
        .map(|record| record.as_commit_entry())
        .filter(|commit| !commit.writes.is_empty())
        .collect()
}

/// Reads the current high-water sequence of the *full* durable journal,
/// including zero-write cursor-advance commits. Unlike `durable_journal_commits`
/// (which filters those out), this reflects whatever the trigger-candidate
/// feed's own commit most recently appended, so it can be used right after
/// observing `trigger_delivery_cursor.materialized_through` reach a target to
/// pin down exactly which durable state that round produced -- see the
/// settle helpers below for why a fixed target isn't enough on its own.
fn latest_durable_journal_sequence(engine: &Engine, tenant_id: &TenantId) -> SequenceNumber {
    engine
        .read_durable_journal(tenant_id, SequenceNumber(0))
        .expect("durable journal should read")
        .into_iter()
        .map(|record| record.as_commit_entry().sequence)
        .max()
        .unwrap_or(SequenceNumber(0))
}

pub(crate) fn subscription_channel() -> (
    mpsc::Sender<SubscriptionUpdate>,
    mpsc::Receiver<SubscriptionUpdate>,
) {
    mpsc::channel(16)
}

/// Bounded blocking receive for synchronous tests, which have no runtime to
/// `.await` on. It parks on the channel itself rather than spin-polling, and a
/// subscription that never publishes fails with a diagnostic instead of
/// hanging the suite.
pub(crate) fn expect_subscription_update_within(
    receiver: &mut mpsc::Receiver<SubscriptionUpdate>,
    description: &str,
) -> SubscriptionUpdate {
    let timeout_budget = ci_or_local_duration(Duration::from_secs(5), Duration::from_secs(15));
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("bounded subscription receive runtime should build")
        .block_on(async {
            tokio::time::timeout(timeout_budget, receiver.recv())
                .await
                .unwrap_or_else(|_| {
                    panic!(
                        "{description} within the bounded state-transition timeout of {timeout_budget:?}"
                    )
                })
        })
        .unwrap_or_else(|| panic!("{description}; the subscription channel disconnected instead"))
}

pub(crate) async fn wait_for_mutation_journal_stats(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    description: &str,
    predicate: impl Fn(&crate::tenant::MutationJournalStats) -> bool,
) -> crate::tenant::MutationJournalStats {
    wait_for_value(
        description,
        Duration::from_secs(1),
        Duration::ZERO,
        || async {
            engine
                .mutation_journal_stats_for_testing(tenant_id)
                .expect("mutation journal stats should load")
        },
        predicate,
    )
    .await
}

pub(crate) async fn wait_for_mutation_admission_stats(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    description: &str,
    predicate: impl Fn(&crate::tenant::MutationAdmissionStats) -> bool,
) -> crate::tenant::MutationAdmissionStats {
    wait_for_value(
        description,
        Duration::from_secs(1),
        Duration::ZERO,
        || async {
            engine
                .mutation_admission_stats_for_testing(tenant_id)
                .expect("mutation admission stats should load")
        },
        predicate,
    )
    .await
}

pub(crate) async fn wait_for_active_subscription_count(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    description: &str,
    expected_count: usize,
) -> usize {
    wait_for_value(
        description,
        Duration::from_secs(1),
        Duration::ZERO,
        || async {
            engine
                .active_subscription_count(tenant_id)
                .expect("subscription count should load")
        },
        |count| *count == expected_count,
    )
    .await
}

/// Waits for the tenant's trigger-delivery cursor to catch up through the
/// last document-bearing commit, then returns that document commit's
/// sequence.
///
/// Every tenant runs a background `TriggerCandidateFeed` worker that, by
/// design, advances a durable trigger-delivery cursor after every commit --
/// including commits with zero matching trigger registrations -- by
/// appending its own empty-write commit to the same commit log and sequence
/// space real document writes use (see `trigger_candidates.rs`). That
/// cursor-advance commit lands asynchronously, on its own OS thread, so a
/// test that captures `latest_sequence` and later compares it against an
/// independently observed read (a subscription snapshot's covered_sequence,
/// a materialized publication's covered_sequence, an applied-head stat,
/// etc.) can otherwise race against that background commit landing between
/// the two observations. Settling here first closes that window.
///
/// Two properties make the DOCUMENT commit (not the raw `latest_sequence`)
/// the only sound settle target and return value:
/// - `materialized_through` only ever advances through commits the worker
///   processes, and the worker never re-processes its own cursor-advance
///   commits. Targeting a raw `latest_sequence` that happens to be a
///   cursor-advance commit therefore waits on a predicate that can never
///   become true.
/// - Reactive subscription deliveries stamp `covered_sequence` from the
///   document commit's sequence (`QueuedSubscriptionWork::delivery_sequence`)
///   and cursor-advance commits generate no deliveries, so a subscription
///   coverage wait pinned to a cursor-advance sequence never completes.
///
/// Also waits for the tenant's in-memory `durable_head` stat to catch up to
/// the durable journal's high-water sequence *observed at the moment the
/// cursor settles*, not just past `target`: `materialize_trigger_invocations_and_sync`
/// durably commits the cursor-advance commit (and its cursor value) in one
/// storage transaction, but only calls `sync_mutation_journal_progress`
/// (which updates the in-memory stat `mutation_journal_stats_for_testing`
/// reads) as a *separate*, later step. Under scheduling pressure the worker
/// can be preempted between those two steps, so a caller that settles on the
/// cursor alone and immediately reads `mutation_journal_stats_for_testing`
/// can observe a stale `durable_head`. Comparing against a fixed `target`
/// doesn't close that window either: when document commits land close
/// together, an *earlier* round's cursor-advance commit can already have
/// pushed `durable_head` past a *later* document's own sequence, so
/// `durable_head > target` can be trivially true before the later round's
/// own cursor-advance commit has been synced. Re-reading the durable
/// journal's high-water mark right after the cursor observation pins down
/// exactly which commit that specific round produced, so waiting for
/// `durable_head` to reach *that* value can't be satisfied by a round that
/// already ran before it.
pub(crate) async fn settled_latest_document_sequence(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> SequenceNumber {
    // No document commit has landed yet, so there is nothing to settle
    // against: any cursor-advance commit would require a document commit to
    // advance through first.
    let Some(target) = durable_journal_commits(engine, tenant_id, SequenceNumber(0))
        .last()
        .map(|commit| commit.sequence)
    else {
        return SequenceNumber(0);
    };
    wait_for_value(
        "trigger delivery cursor should settle through the last document commit",
        ci_or_local_duration(Duration::from_secs(1), Duration::from_secs(3)),
        Duration::ZERO,
        || async {
            engine
                .trigger_delivery_cursor_for_testing(tenant_id)
                .expect("trigger delivery cursor should load")
        },
        move |cursor| cursor.materialized_through.0 >= target.0,
    )
    .await;
    let settled_head = latest_durable_journal_sequence(engine, tenant_id);
    wait_for_mutation_journal_stats(
        engine,
        tenant_id,
        "durable_head stat should catch up to the settled cursor-advance commit",
        move |stats| stats.durable_head.0 >= settled_head.0,
    )
    .await;
    target
}

/// Blocking (non-async) form of the settle in
/// [`settled_latest_document_sequence`], for `#[test]` callers: waits until
/// the trigger-delivery cursor has caught up through the last
/// document-bearing commit and its cursor-advance commit's durable-head bump
/// is visible in the in-memory `mutation_journal_stats_for_testing` stat, so
/// no further background sequence consumption can race the caller until the
/// next document write.
///
/// The cursor-advance commit is durably committed (cursor value included)
/// in one storage transaction inside `materialize_trigger_invocations_and_sync`,
/// but that function only calls `sync_mutation_journal_progress` (which
/// updates the in-memory `durable_head` stat) as a separate, later step.
/// Settling on the cursor alone can therefore return while `durable_head`
/// is still stale under scheduling pressure. Waiting for `durable_head` to
/// pass a *fixed* `target` doesn't close that window either: when document
/// commits land close together, an earlier round's cursor-advance commit can
/// already have pushed `durable_head` past a later document's own sequence,
/// making `durable_head > target` trivially true before that later round's
/// own cursor-advance commit has synced. Re-reading the durable journal's
/// high-water mark right after the cursor catches up pins down exactly which
/// commit *this* round produced, so waiting for `durable_head` to reach that
/// freshly observed value can't be satisfied by a round that already ran
/// before it.
pub(crate) fn settle_trigger_cursor_blocking(engine: &Engine, tenant_id: &TenantId) {
    // No document commit has landed yet, so there is nothing to settle
    // against: any cursor-advance commit would require a document commit to
    // advance through first.
    let Some(target) = durable_journal_commits(engine, tenant_id, SequenceNumber(0))
        .last()
        .map(|commit| commit.sequence)
    else {
        return;
    };
    let timeout = ci_or_local_duration(Duration::from_millis(500), Duration::from_secs(5));
    let started_at = std::time::Instant::now();
    loop {
        let cursor = engine
            .trigger_delivery_cursor_for_testing(tenant_id)
            .expect("trigger delivery cursor should load");
        if cursor.materialized_through.0 >= target.0 {
            break;
        }
        assert!(
            started_at.elapsed() < timeout,
            "trigger delivery cursor should settle through the last document commit \
             (materialized_through {} < target {})",
            cursor.materialized_through.0,
            target.0,
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    let settled_head = latest_durable_journal_sequence(engine, tenant_id);
    loop {
        let stats = engine
            .mutation_journal_stats_for_testing(tenant_id)
            .expect("mutation journal stats should load");
        if stats.durable_head.0 >= settled_head.0 {
            return;
        }
        assert!(
            started_at.elapsed() < timeout,
            "durable_head stat should catch up to the settled cursor-advance commit \
             (durable_head {} < settled_head {})",
            stats.durable_head.0,
            settled_head.0,
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Settles like [`settled_latest_document_sequence`], then returns the raw
/// `latest_sequence` (which may be the sequence of the worker's own
/// cursor-advance commit). Stable once settled: the final cursor-advance
/// commit lands atomically with the cursor write the settle observes, and
/// nothing re-processes cursor-advance commits. Use this only to assert
/// against sequences stamped from live store state (e.g. a subscription
/// bootstrap snapshot's covered_sequence); coverage waits on reactive
/// deliveries must use [`settled_latest_document_sequence`] instead.
pub(crate) async fn settled_latest_sequence(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> SequenceNumber {
    settled_latest_document_sequence(engine, tenant_id).await;
    engine
        .latest_sequence(tenant_id)
        .expect("latest sequence should load")
}

pub(crate) fn filter(field: &str, op: FilterOp, value: serde_json::Value) -> Filter {
    Filter {
        field: field.to_string(),
        op,
        value,
    }
}

pub(crate) fn materialized_snapshot_with_documents(
    documents: Vec<nimbus_core::Document>,
) -> crate::MaterializedJournalSnapshot {
    let table_identities = documents
        .iter()
        .map(|document| document.table.clone())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .map(|table| {
            // Derive a stable table_id from the table name so two snapshots
            // built from the same logical documents share identical table
            // identities. A random `TableId::new()` per call would make the
            // identity verifier (correctly) report spurious table_id drift.
            let table_id = nimbus_core::TableId::try_from(format!("tableid-{table}"))
                .expect("derived table id should be a valid logical name");
            crate::TableIdentitySnapshotEntry::default_namespace(table, table_id)
        })
        .collect();
    crate::MaterializedJournalSnapshot {
        version: nimbus_storage::MATERIALIZED_JOURNAL_SNAPSHOT_VERSION,
        applied_sequence: SequenceNumber(1),
        durable_head: SequenceNumber(1),
        table_identities,
        schema: nimbus_core::Schema::default(),
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
        case.map(|case| case.failure_context("nimbus-engine", test_name, invariant))
            .unwrap_or_else(|| history.failure_context(invariant, None))
    };

    let model = history.model();
    let expected_query = model.query_result();
    assert!(
        expected_query.len() > history.page_size(),
        "history seed should produce at least two query pages: {}",
        context("generated-history seed should produce at least two query pages")
    );

    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let table = TableName::new(history.table()).expect("generated task table should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    replay_generated_task_history_async(
        history,
        {
            let engine = Arc::clone(&engine);
            let tenant_id = tenant_id.clone();
            let table = table.clone();
            move |_slot, record| {
                let engine = Arc::clone(&engine);
                let tenant_id = tenant_id.clone();
                let table = table.clone();
                let fields = record.fields();
                async move { engine.insert_document_async(tenant_id, table, fields).await }
            }
        },
        {
            let engine = Arc::clone(&engine);
            let tenant_id = tenant_id.clone();
            let table = table.clone();
            move |_slot, document_id, record| {
                let engine = Arc::clone(&engine);
                let tenant_id = tenant_id.clone();
                let table = table.clone();
                let fields = record.fields();
                async move {
                    engine
                        .update_document_async(tenant_id, table, document_id, fields)
                        .await
                        .map(|_| ())
                }
            }
        },
        {
            let engine = Arc::clone(&engine);
            let tenant_id = tenant_id.clone();
            let table = table.clone();
            move |_slot, document_id| {
                let engine = Arc::clone(&engine);
                let tenant_id = tenant_id.clone();
                let table = table.clone();
                async move {
                    engine
                        .delete_document_async(tenant_id, table, document_id)
                        .await
                }
            }
        },
    )
    .await
    .expect("generated history replay should succeed");

    let live_documents = normalize_generated_task_documents(
        engine
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
        engine
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

    let live_first_page = engine
        .paginate_documents_async(tenant_id.clone(), history.paginated_query(None))
        .await
        .expect("live first page should succeed");
    assert_generated_task_page_matches(
        &live_first_page,
        &model.first_page(),
        &context("live first page should match the generated-history oracle"),
    );
    let live_second_page = engine
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

    let shadow = engine
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

    let replica = EmbeddedReplica::bootstrap_in_memory(&engine, tenant_id.clone())
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

pub(crate) fn document_bodies(documents: &[nimbus_core::Document]) -> Vec<&str> {
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
    documents: Vec<nimbus_core::Document>,
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

const BLOCKING_CANCELLATION_RELEASE_TIMEOUT: Duration = Duration::from_secs(60);

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

    pub(crate) fn check(self: Arc<Self>) -> impl Fn() -> nimbus_core::Result<()> + Send + 'static {
        move || {
            if self.first_check.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                let (lock, cvar) = &self.release_gate;
                let mut released = lock
                    .lock()
                    .expect("blocking cancellation probe should acquire release lock");
                let (next, _) = cvar
                    .wait_timeout_while(
                        released,
                        BLOCKING_CANCELLATION_RELEASE_TIMEOUT,
                        |released| !*released,
                    )
                    .expect("blocking cancellation probe should wait for release");
                released = next;
                assert!(
                    *released,
                    "blocking cancellation probe was not released within \
                     {BLOCKING_CANCELLATION_RELEASE_TIMEOUT:?}; the test likely exited before \
                     calling release()"
                );
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

pub(crate) async fn create_engine_with_durable_unapplied_task(
    timestamp_ms: u64,
    title: &str,
) -> (
    TempDir,
    Arc<Engine>,
    TenantId,
    Arc<BlockingFaultInjector>,
    DocumentId,
) {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualWallClock::new(Timestamp(timestamp_ms))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let insert_handle = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let title = title.to_string();
        async move {
            engine
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
    let document_id = durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0))
        .first()
        .and_then(|commit| commit.writes.first())
        .map(|write| write.doc_id.clone())
        .expect("durable commit should include the inserted document id");

    (data_dir, engine, tenant_id, faults, document_id)
}

#[tokio::test]
async fn engine_create_duplicate_tenant_returns_already_exists() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let error = engine
        .create_tenant(tenant_id)
        .expect_err("duplicate tenant should fail");
    assert!(matches!(error, Error::AlreadyExists(_)));
}

#[tokio::test]
async fn engine_delete_nonexistent_tenant_returns_not_found() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");

    let error = engine
        .delete_tenant(&tenant_id)
        .expect_err("missing tenant should fail");
    assert!(matches!(error, Error::TenantNotFound(_)));
}

#[tokio::test]
async fn engine_missing_document_operations_return_not_found() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let missing_id = nimbus_core::DocumentId::new();

    let get_error = engine
        .get_document(&tenant_id, &tasks_table(), missing_id.clone())
        .expect_err("missing get should fail");
    assert!(matches!(get_error, Error::DocumentNotFound(_)));

    let update_error = engine
        .update_document(
            &tenant_id,
            tasks_table(),
            missing_id.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("After"))]),
        )
        .expect_err("missing update should fail");
    assert!(matches!(update_error, Error::DocumentNotFound(_)));

    let delete_error = engine
        .delete_document(&tenant_id, tasks_table(), missing_id.clone())
        .expect_err("missing delete should fail");
    assert!(matches!(delete_error, Error::DocumentNotFound(_)));
}

#[tokio::test]
async fn engine_tenant_data_is_isolated_across_tenants() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let alpha_tenant = fixture.create_tenant("alpha", Engine::create_tenant);
    let beta_tenant = fixture.create_tenant("beta", Engine::create_tenant);

    let alpha_id = engine
        .insert_document(
            &alpha_tenant,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Alpha"))]),
        )
        .expect("insert should succeed");
    let beta_id = engine
        .insert_document(
            &beta_tenant,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Beta"))]),
        )
        .expect("insert should succeed");

    let alpha_docs = engine
        .list_documents(&alpha_tenant, &tasks_table())
        .expect("list should succeed");
    let beta_docs = engine
        .list_documents(&beta_tenant, &tasks_table())
        .expect("list should succeed");

    assert_eq!(alpha_docs.len(), 1);
    assert_eq!(beta_docs.len(), 1);
    assert_eq!(alpha_docs[0].fields.get("title"), Some(&json!("Alpha")));
    assert_eq!(beta_docs[0].fields.get("title"), Some(&json!("Beta")));

    // A positive per-tenant listing alone would not catch a bug that resolved
    // documents by ID without checking which tenant's store the ID belongs to.
    // Fetch each tenant's known document ID through the *other* tenant's
    // context and require the same not-found refusal used for a document that
    // never existed at all.
    let cross_fetch_beta_id_as_alpha = engine
        .get_document(&alpha_tenant, &tasks_table(), beta_id.clone())
        .expect_err("alpha tenant must not resolve beta's document id");
    assert!(matches!(
        cross_fetch_beta_id_as_alpha,
        Error::DocumentNotFound(_)
    ));
    let cross_fetch_alpha_id_as_beta = engine
        .get_document(&beta_tenant, &tasks_table(), alpha_id.clone())
        .expect_err("beta tenant must not resolve alpha's document id");
    assert!(matches!(
        cross_fetch_alpha_id_as_beta,
        Error::DocumentNotFound(_)
    ));
}

#[tokio::test]
async fn engine_insert_document_with_explicit_id_round_trips_firestore_style_key() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let explicit_id =
        DocumentId::from_key("cities.SF-42".to_string()).expect("explicit id should be valid");

    let inserted_id = engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            explicit_id.clone(),
            serde_json::Map::from_iter([("title".to_string(), json!("San Francisco"))]),
        )
        .expect("explicit insert should succeed");

    assert_eq!(inserted_id, explicit_id);
    let document = engine
        .get_document(&tenant_id, &tasks_table(), explicit_id.clone())
        .expect("explicitly keyed document should exist");
    assert_eq!(document.id, explicit_id);
    assert_eq!(document.get_field("title"), Some(&json!("San Francisco")));
}

#[tokio::test]
async fn engine_lazy_loads_tenant_from_disk() {
    let data_dir = tempdir().expect("tempdir should create");
    let engine = Engine::new(data_dir.path()).expect("engine should create");
    let tenant_id = TenantId::new("demo").expect("tenant id should be valid");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("Persisted"))]),
        )
        .expect("insert should succeed");

    drop(engine);

    let reloaded = Engine::new(data_dir.path()).expect("engine should reopen");
    let documents = reloaded
        .list_documents(&tenant_id, &tasks_table())
        .expect("list should succeed");

    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("Persisted")));
}

#[tokio::test]
async fn engine_unsubscribe_stops_notifications() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let (tx, mut rx) = subscription_channel();
    let subscription = engine
        .subscribe(
            &tenant_id,
            query_for("tasks"),
            "req-unsub".to_string(),
            tx,
            SubscribeOptions::anonymous(),
        )
        .expect("subscribe should succeed");
    let subscription_id = subscription.id();
    let _ = rx.recv().await.expect("initial update should arrive");

    engine
        .unsubscribe(&tenant_id, subscription_id)
        .expect("unsubscribe should succeed");
    engine
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
async fn engine_skips_the_durable_write_for_an_unchanged_table_schema() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let durable_head = |label: &str| {
        engine
            .tenant_engine_diagnostics(&tenant_id)
            .unwrap_or_else(|error| panic!("{label} diagnostics should load: {error}"))
            .mutation_journal
            .frontiers
            .durable_head
    };

    let before_first = durable_head("pre-declaration");
    engine
        .set_table_schema(&tenant_id, users_schema())
        .expect("first schema declaration should save");
    let after_first = durable_head("first declaration");
    assert_eq!(
        after_first.0,
        before_first.0 + 1,
        "a new table schema should append exactly one durable record"
    );

    for _ in 0..5 {
        engine
            .set_table_schema(&tenant_id, users_schema())
            .expect("redeclaring the stored schema should succeed");
    }
    assert_eq!(
        durable_head("redeclaration"),
        after_first,
        "redeclaring the stored schema must not append durable records"
    );
    assert_eq!(
        engine
            .get_table_schema(
                &tenant_id,
                &TableName::new("users").expect("table name should be valid")
            )
            .expect("stored schema should load"),
        users_schema(),
        "the stored schema should survive redeclaration unchanged"
    );

    let mut widened = users_schema();
    widened.fields.push(FieldSchema {
        name: "nickname".to_string(),
        field_type: FieldType::String,
        required: false,
    });
    engine
        .set_table_schema(&tenant_id, widened.clone())
        .expect("changed schema should save");
    assert_eq!(
        durable_head("changed declaration").0,
        after_first.0 + 1,
        "a changed table schema should still append exactly one durable record"
    );
    assert_eq!(
        engine
            .get_table_schema(
                &tenant_id,
                &TableName::new("users").expect("table name should be valid")
            )
            .expect("changed schema should load"),
        widened,
        "the changed schema should replace the stored one"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_identical_schema_declarations_append_one_durable_record() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("schema-idempotency", Engine::create_tenant);
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("pre-declaration diagnostics should load")
        .mutation_journal
        .frontiers
        .durable_head;

    let mut declarations = tokio::task::JoinSet::new();
    for _ in 0..16 {
        let engine = Arc::clone(&engine);
        let tenant_id = tenant_id.clone();
        declarations.spawn(async move {
            engine
                .set_table_schema_async(tenant_id, users_schema())
                .await
        });
    }
    while let Some(declaration) = declarations.join_next().await {
        declaration
            .expect("schema declaration task should join")
            .expect("identical schema declaration should succeed");
    }

    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("post-declaration diagnostics should load")
        .mutation_journal
        .frontiers
        .durable_head;
    assert_eq!(
        after.0,
        before.0 + 1,
        "concurrent identical declarations must append one schema record"
    );
}

#[tokio::test]
async fn engine_validates_insert_against_schema() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .set_table_schema(&tenant_id, users_schema())
        .expect("schema should save");

    let missing_name = engine
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("age".to_string(), json!(30))]),
        )
        .expect_err("insert should fail");
    assert!(matches!(missing_name, Error::SchemaValidation(_)));

    let wrong_type = engine
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([("name".to_string(), json!(123))]),
        )
        .expect_err("insert should fail");
    assert!(matches!(wrong_type, Error::SchemaValidation(_)));

    engine
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
async fn engine_validates_update_against_full_document() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .set_table_schema(&tenant_id, users_schema())
        .expect("schema should save");
    let document_id = engine
        .insert_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            serde_json::Map::from_iter([
                ("name".to_string(), json!("Alice")),
                ("age".to_string(), json!(30)),
            ]),
        )
        .expect("insert should succeed");

    let wrong_type = engine
        .update_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            document_id.clone(),
            serde_json::Map::from_iter([("age".to_string(), json!("not a number"))]),
        )
        .expect_err("update should fail");
    assert!(matches!(wrong_type, Error::SchemaValidation(_)));

    engine
        .update_document(
            &tenant_id,
            TableName::new("users").expect("table name should be valid"),
            document_id,
            serde_json::Map::from_iter([("age".to_string(), json!(31))]),
        )
        .expect("update should succeed");
}

/// [`Engine::enter_object_blob_operation`] is the guard entry point the
/// object byte-plane resolver (`nimbus-object-storage`, which this crate
/// cannot depend on) must enter before opening any per-tenant blob-plane
/// state. It must reject exactly like every other guarded tenant-scoped
/// call — [`ensure_tenant_exists_async`](Engine::ensure_tenant_exists_async)
/// among them — once a deletion has started: rejected as soon as deletion is
/// fenced, even while an older operation is still draining. This mirrors
/// `delete_tenant_async_fences_new_work_before_draining_in_flight_operations`
/// in `tests/subscriptions/lifecycle.rs`, substituting the blob-operation
/// guard for the "new work" being raced against deletion.
#[tokio::test]
async fn enter_object_blob_operation_rejects_while_in_flight_work_drains() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("blocker"))]),
        )
        .expect("seed insert should succeed");
    let probe = BlockingCancellationProbe::new();

    let read_task: tokio::task::JoinHandle<nimbus_core::Result<Vec<nimbus_core::Document>>> =
        tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            let probe = probe.clone();
            async move {
                engine
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

    let mut delete_task = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move { engine.delete_tenant_async(tenant_id).await }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut delete_task)
            .await
            .is_err(),
        "tenant deletion should wait for the in-flight operation"
    );

    let error = timeout(
        Duration::from_millis(100),
        engine.enter_object_blob_operation(&tenant_id),
    )
    .await
    .expect("new blob-plane work should reject once deletion is fenced")
    .err()
    .expect("new blob-plane work should fail after deletion begins");
    assert!(matches!(error, Error::TenantNotFound(_)));

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
}

#[tokio::test]
async fn tenant_runtime_lease_projects_incarnation_and_holds_delete_fence() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let lease = engine
        .enter_tenant_runtime_async(tenant_id.clone())
        .await
        .expect("runtime lease should enter the tenant operation fence");

    assert_eq!(lease.tenant_id(), &tenant_id);
    assert_eq!(lease.tenant_incarnation().get(), 1);

    let mut delete_task = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move { engine.delete_tenant_async(tenant_id).await }
    });
    assert!(
        timeout(Duration::from_millis(100), &mut delete_task)
            .await
            .is_err(),
        "tenant deletion must wait while a runtime invocation lease is held"
    );

    drop(lease);
    timeout(Duration::from_secs(1), async {
        delete_task
            .await
            .expect("delete task should join")
            .expect("tenant delete should succeed");
    })
    .await
    .expect("delete should complete after the runtime lease drops");

    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("same tenant ID should recreate with a new incarnation");
    let recreated = engine
        .enter_tenant_runtime_async(tenant_id)
        .await
        .expect("recreated tenant runtime lease should succeed");
    assert_eq!(recreated.tenant_incarnation().get(), 2);
}

#[tokio::test]
async fn expected_incarnation_delete_rejects_before_fencing_a_replacement() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    let error = engine
        .begin_tenant_incarnation_delete_async(
            tenant_id.clone(),
            std::num::NonZeroU64::new(2).expect("fixture incarnation is nonzero"),
        )
        .await
        .err()
        .expect("a stale expected incarnation must fail");
    assert!(matches!(error, Error::PreconditionFailed(_)));

    let live = engine
        .enter_tenant_runtime_async(tenant_id.clone())
        .await
        .expect("the rejected operation must not fence the live incarnation");
    assert_eq!(live.tenant_incarnation().get(), 1);
    drop(live);

    let deletion = engine
        .begin_tenant_incarnation_delete_async(
            tenant_id,
            std::num::NonZeroU64::new(1).expect("fixture incarnation is nonzero"),
        )
        .await
        .expect("the exact incarnation should be fenced");
    engine
        .finish_tenant_delete_async(deletion)
        .await
        .expect("fixture deletion should finish");
}

#[tokio::test]
async fn no_schema_allows_anything() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
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
