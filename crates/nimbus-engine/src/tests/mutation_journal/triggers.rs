use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

use crate::{Engine, TriggerInvocationExecution, TriggerInvocationExecutor, TriggerRegistration};
use nimbus_core::{
    DocumentId, DocumentLocator, DocumentPath, DocumentTriggerPattern, FirestoreCloudEventType,
    ResourcePathBinding, TenantId, Timestamp, TriggerDeliveryCursor, TriggerInvocationRecord,
    TriggerInvocationState,
};
use nimbus_storage::{
    FaultOccurrence, FaultPoint, ManualClock, NoopFaultInjector, ScriptedFaultInjector,
};
use tempfile::{TempDir, tempdir};

use super::support::{
    expect_blocking_wait_reaches_state, expect_future_within, mutation_journal_catch_up_timeout,
    mutation_journal_poll_interval, mutation_journal_progress_timeout, new_faulted_engine,
};
use super::*;

fn trigger_binding(document_id: &DocumentId) -> ResourcePathBinding {
    ResourcePathBinding::new(
        DocumentLocator::new(tasks_table(), document_id.clone()),
        DocumentPath::from_segments(["tasks".to_string(), document_id.to_string()])
            .expect("document path should build"),
    )
}

fn trigger_registration<I, S>(
    id: &str,
    event_type: FirestoreCloudEventType,
    pattern_segments: I,
) -> TriggerRegistration
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    TriggerRegistration::new(
        id,
        event_type,
        DocumentTriggerPattern::from_segments(pattern_segments)
            .expect("trigger pattern should parse"),
    )
    .expect("trigger registration should build")
}

#[derive(Default)]
struct RecordingTriggerExecutor {
    calls: Mutex<Vec<String>>,
    terminal_failure_registration_ids: Mutex<BTreeSet<String>>,
    retry_once_registration_ids: Mutex<BTreeSet<String>>,
    always_retry_registration_ids: Mutex<BTreeSet<String>>,
    attempts: Mutex<BTreeMap<String, u32>>,
}

impl RecordingTriggerExecutor {
    fn with_terminal_failure_for(registration_id: &str) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            terminal_failure_registration_ids: Mutex::new(BTreeSet::from([
                registration_id.to_string()
            ])),
            retry_once_registration_ids: Mutex::new(BTreeSet::new()),
            always_retry_registration_ids: Mutex::new(BTreeSet::new()),
            attempts: Mutex::new(BTreeMap::new()),
        }
    }

    fn with_retry_once_for(registration_id: &str) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            terminal_failure_registration_ids: Mutex::new(BTreeSet::new()),
            retry_once_registration_ids: Mutex::new(BTreeSet::from([registration_id.to_string()])),
            always_retry_registration_ids: Mutex::new(BTreeSet::new()),
            attempts: Mutex::new(BTreeMap::new()),
        }
    }

    fn with_always_retry_for(registration_id: &str) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            terminal_failure_registration_ids: Mutex::new(BTreeSet::new()),
            retry_once_registration_ids: Mutex::new(BTreeSet::new()),
            always_retry_registration_ids: Mutex::new(BTreeSet::from(
                [registration_id.to_string()],
            )),
            attempts: Mutex::new(BTreeMap::new()),
        }
    }

    fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("recording trigger executor calls lock should not be poisoned")
            .clone()
    }
}

impl TriggerInvocationExecutor for RecordingTriggerExecutor {
    fn execute_invocation(
        &self,
        _tenant_id: &TenantId,
        record: &TriggerInvocationRecord,
    ) -> TriggerInvocationExecution {
        self.calls
            .lock()
            .expect("recording trigger executor calls lock should not be poisoned")
            .push(record.key.registration_id.clone());
        let attempt = {
            let mut attempts = self
                .attempts
                .lock()
                .expect("recording trigger executor attempts lock should not be poisoned");
            let attempt = attempts
                .entry(record.key.registration_id.clone())
                .or_default();
            *attempt = attempt.saturating_add(1);
            *attempt
        };
        if self
            .terminal_failure_registration_ids
            .lock()
            .expect("recording trigger executor failure lock should not be poisoned")
            .contains(record.key.registration_id.as_str())
        {
            return TriggerInvocationExecution::terminal(format!(
                "forced trigger failure for {}",
                record.key.registration_id
            ));
        }
        if self
            .retry_once_registration_ids
            .lock()
            .expect("recording trigger executor retry-once lock should not be poisoned")
            .contains(record.key.registration_id.as_str())
            && attempt == 1
        {
            return TriggerInvocationExecution::retryable(format!(
                "forced retryable failure for {}",
                record.key.registration_id
            ));
        }
        if self
            .always_retry_registration_ids
            .lock()
            .expect("recording trigger executor retry lock should not be poisoned")
            .contains(record.key.registration_id.as_str())
        {
            return TriggerInvocationExecution::retryable(format!(
                "forced persistent retryable failure for {}",
                record.key.registration_id
            ));
        }
        TriggerInvocationExecution::completed()
    }
}

fn new_trigger_engine_with_manual_clock(
    timestamp_ms: u64,
) -> (TempDir, Arc<Engine>, TenantId, Arc<ManualClock>) {
    let data_dir = tempdir().expect("engine tempdir should build");
    let clock = Arc::new(ManualClock::new(Timestamp(timestamp_ms)));
    let engine = Arc::new(
        Engine::new_with_simulation(data_dir.path(), clock.clone(), Arc::new(NoopFaultInjector))
            .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    (data_dir, engine, tenant_id, clock)
}

#[tokio::test]
async fn trigger_candidates_publish_only_after_journal_apply() {
    let (_data_dir, engine, tenant_id, faults) = new_faulted_engine(61_000);
    let document_id = DocumentId::from_key("triggered").expect("document id should build");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    let insert_handle = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .insert_document_async_with_id(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("title".to_string(), json!("triggered"))]),
                )
                .await
        }
    });

    timeout(Duration::from_secs(1), faults.wait_until_entered())
        .await
        .expect("journal worker should block after durable append");
    assert_eq!(
        engine
            .pending_trigger_candidate_count_for_testing(&tenant_id)
            .expect("pending trigger candidate count should load"),
        0,
        "trigger candidates must not surface before the applied visibility boundary"
    );

    faults.release();

    expect_future_within(insert_handle, "mutation should finish after apply resumes")
        .await
        .expect("mutation task should join successfully")
        .expect("mutation should succeed");

    wait_for_value(
        "trigger candidates should arrive after apply",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .pending_trigger_candidate_count_for_testing(&tenant_id)
                .expect("pending trigger candidate count should load")
        },
        |count| *count == 2,
    )
    .await;

    let candidates = engine
        .drain_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger candidates should drain");
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].event_type, FirestoreCloudEventType::Created);
    assert_eq!(candidates[1].event_type, FirestoreCloudEventType::Written);
    assert_eq!(
        candidates[0].binding.document_path.to_string(),
        "tasks/triggered"
    );
}

#[tokio::test]
async fn trigger_candidate_worker_pause_does_not_block_mutation_completion() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let document_id = DocumentId::from_key("paused").expect("document id should build");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    let pause = engine
        .trigger_candidate_pause_handle_for_testing(&tenant_id)
        .expect("trigger candidate pause handle should load");
    pause.arm();

    let insert_handle = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .insert_document_async_with_id(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("title".to_string(), json!("paused"))]),
                )
                .await
        }
    });

    let pause_for_wait = pause.clone();
    expect_blocking_wait_reaches_state("trigger candidate worker should pause", move |timeout| {
        pause_for_wait.wait_until_entered(timeout)
    })
    .await;

    expect_future_within(
        insert_handle,
        "mutation should complete while trigger candidate worker is paused",
    )
    .await
    .expect("mutation task should join successfully")
    .expect("mutation should succeed");

    assert_eq!(
        engine
            .pending_trigger_candidate_count_for_testing(&tenant_id)
            .expect("pending trigger candidate count should load"),
        0,
        "paused worker should not publish candidates before release"
    );

    pause.release();

    wait_for_value(
        "trigger candidates should arrive after worker pause release",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .pending_trigger_candidate_count_for_testing(&tenant_id)
                .expect("pending trigger candidate count should load")
        },
        |count| *count == 2,
    )
    .await;
}

#[tokio::test]
async fn trigger_candidate_bootstrap_replays_commits_after_persisted_cursor() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let first_id = DocumentId::from_key("first").expect("first document id should build");
    let second_id = DocumentId::from_key("second").expect("second document id should build");

    {
        let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        engine
            .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&first_id))
            .expect("first resource path binding should persist");
        engine
            .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&second_id))
            .expect("second resource path binding should persist");
        engine
            .insert_document_with_id(
                &tenant_id,
                tasks_table(),
                first_id.clone(),
                serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
            )
            .expect("first insert should succeed");
        wait_for_value(
            "first trigger cursor should settle before pausing the second commit",
            mutation_journal_catch_up_timeout(),
            mutation_journal_poll_interval(),
            || async {
                engine
                    .trigger_delivery_cursor_for_testing(&tenant_id)
                    .expect("trigger delivery cursor should load")
            },
            |cursor| cursor.materialized_through == SequenceNumber(1),
        )
        .await;

        let pause = engine
            .trigger_candidate_pause_handle_for_testing(&tenant_id)
            .expect("trigger candidate pause handle should load");
        pause.arm();
        engine
            .insert_document_with_id(
                &tenant_id,
                tasks_table(),
                second_id.clone(),
                serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
            )
            .expect("second insert should succeed");
        let pause_for_wait = pause.clone();
        expect_blocking_wait_reaches_state(
            "second trigger candidate worker should pause before cursor advance",
            move |timeout| pause_for_wait.wait_until_entered(timeout),
        )
        .await;
        engine
            .shutdown_trigger_candidates_for_testing(&tenant_id)
            .expect("paused trigger candidate worker should shut down without advancing cursor");
        engine
            .set_trigger_delivery_cursor_for_testing(
                &tenant_id,
                TriggerDeliveryCursor::new(SequenceNumber(1)),
            )
            .expect("trigger delivery cursor should persist");
        engine.quiesce().await;
    }

    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should recreate"));
    engine
        .ensure_tenant_exists(&tenant_id)
        .expect("tenant should load and bootstrap trigger candidates");

    let replayed_count = wait_for_value(
        "trigger candidate bootstrap should replay commits after the persisted cursor",
        mutation_journal_catch_up_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .pending_trigger_candidate_count_for_testing(&tenant_id)
                .expect("pending trigger candidate count should load")
        },
        |count| *count >= 2,
    )
    .await;
    assert_eq!(
        replayed_count, 2,
        "trigger bootstrap should replay only the commit after the persisted cursor"
    );

    let candidates = engine
        .drain_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger candidates should drain");
    assert_eq!(candidates.len(), 2);
    let commit_prefixes = candidates
        .iter()
        .map(|candidate| {
            candidate
                .event_id
                .split(':')
                .take(2)
                .collect::<Vec<_>>()
                .join(":")
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        commit_prefixes.len(),
        1,
        "bootstrap should replay candidates from one document commit: {candidates:?}"
    );
    assert!(
        !commit_prefixes.contains("commit:1"),
        "bootstrap should only replay commits after the persisted cursor: {candidates:?}"
    );
}

#[tokio::test]
async fn trigger_invocations_materialize_exact_and_wildcard_matches() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let document_id = DocumentId::from_key("exact").expect("document id should build");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![
                trigger_registration(
                    "firebase:exactWritten",
                    FirestoreCloudEventType::Written,
                    ["tasks", "exact"],
                ),
                trigger_registration(
                    "firebase:wildWritten",
                    FirestoreCloudEventType::Written,
                    ["tasks", "{taskId}"],
                ),
                trigger_registration(
                    "firebase:wildCreated",
                    FirestoreCloudEventType::Created,
                    ["tasks", "{taskId}"],
                ),
            ],
        )
        .expect("trigger registrations should persist in runtime");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("exact"))]),
        )
        .expect("insert should succeed");

    wait_for_value(
        "trigger invocations should materialize for exact and wildcard matches",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
                .len()
        },
        |count| *count == 3,
    )
    .await;

    let invocations = engine
        .list_trigger_invocations_for_testing(&tenant_id)
        .expect("trigger invocations should load");
    assert_eq!(invocations.len(), 3);
    assert_eq!(
        engine
            .trigger_delivery_cursor_for_testing(&tenant_id)
            .expect("trigger delivery cursor should load"),
        TriggerDeliveryCursor::new(SequenceNumber(1))
    );

    let exact = invocations
        .iter()
        .find(|record| record.key.registration_id == "firebase:exactWritten")
        .expect("exact registration should materialize");
    assert_eq!(
        exact.event.cloud_event.event_type,
        FirestoreCloudEventType::Written
    );
    assert!(exact.event.firestore.params.is_empty());

    let wildcard_created = invocations
        .iter()
        .find(|record| record.key.registration_id == "firebase:wildCreated")
        .expect("wild created registration should materialize");
    assert_eq!(
        wildcard_created.event.cloud_event.event_type,
        FirestoreCloudEventType::Created
    );
    assert_eq!(
        wildcard_created
            .event
            .firestore
            .params
            .get("taskId")
            .map(String::as_str),
        Some("exact")
    );

    let wildcard_written = invocations
        .iter()
        .find(|record| record.key.registration_id == "firebase:wildWritten")
        .expect("wild written registration should materialize");
    assert_eq!(
        wildcard_written.event.cloud_event.event_type,
        FirestoreCloudEventType::Written
    );
    assert_eq!(
        wildcard_written
            .event
            .firestore
            .params
            .get("taskId")
            .map(String::as_str),
        Some("exact")
    );
}

#[tokio::test]
async fn trigger_candidate_worker_retries_transient_materialization_failure() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let document_id = DocumentId::from_key("materialize-retry").expect("document id should build");
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(50_000))),
            Arc::new(ScriptedFaultInjector::new([FaultOccurrence {
                point: FaultPoint::TriggerInvocationMaterializeBeforeCommit,
                visit: 1,
            }])),
        )
        .expect("engine should create"),
    );
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![trigger_registration(
                "firebase:materializeRetryWritten",
                FirestoreCloudEventType::Written,
                ["tasks", "{taskId}"],
            )],
        )
        .expect("trigger registrations should persist in runtime");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("materialize-retry"))]),
        )
        .expect("insert should succeed despite a transient trigger materialization failure");

    let (records, cursor, pending_count) = wait_for_value(
        "trigger candidate worker should retry transient materialization failure in-process",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            let records = engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load");
            let cursor = engine
                .trigger_delivery_cursor_for_testing(&tenant_id)
                .expect("trigger delivery cursor should load");
            let pending_count = engine
                .pending_trigger_candidate_count_for_testing(&tenant_id)
                .expect("pending trigger candidate count should load");
            (records, cursor, pending_count)
        },
        |(records, cursor, pending_count)| {
            records.len() == 1
                && *cursor == TriggerDeliveryCursor::new(SequenceNumber(1))
                && *pending_count == 2
        },
    )
    .await;

    assert_eq!(records.len(), 1);
    assert_eq!(
        records[0].key.registration_id,
        "firebase:materializeRetryWritten"
    );
    assert!(matches!(records[0].state, TriggerInvocationState::Pending));
    assert_eq!(cursor, TriggerDeliveryCursor::new(SequenceNumber(1)));
    assert_eq!(pending_count, 2);

    let candidates = engine
        .drain_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger candidates should drain");
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].event_type, FirestoreCloudEventType::Created);
    assert_eq!(candidates[1].event_type, FirestoreCloudEventType::Written);
}

#[tokio::test]
async fn trigger_invocations_advance_cursor_when_registry_is_ready_but_no_match_exists() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let document_id = DocumentId::from_key("nomatch").expect("document id should build");
    engine
        .replace_trigger_registrations_for_testing(&tenant_id, Vec::new())
        .expect("empty trigger registry should still mark readiness");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("nomatch"))]),
        )
        .expect("insert should succeed");

    wait_for_value(
        "trigger delivery cursor should advance even when no registrations match",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .trigger_delivery_cursor_for_testing(&tenant_id)
                .expect("trigger delivery cursor should load")
        },
        |cursor| *cursor == TriggerDeliveryCursor::new(SequenceNumber(1)),
    )
    .await;

    assert!(
        engine
            .list_trigger_invocations_for_testing(&tenant_id)
            .expect("trigger invocations should load")
            .is_empty()
    );
}

#[tokio::test]
async fn trigger_execution_claims_pending_invocations_and_marks_completion() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let document_id = DocumentId::from_key("complete").expect("document id should build");
    let executor = Arc::new(RecordingTriggerExecutor::default());
    engine
        .install_trigger_invocation_executor(executor.clone())
        .expect("trigger executor should install");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![trigger_registration(
                "firebase:completeWritten",
                FirestoreCloudEventType::Written,
                ["tasks", "{taskId}"],
            )],
        )
        .expect("trigger registrations should persist in runtime");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("complete"))]),
        )
        .expect("insert should succeed");

    wait_for_value(
        "trigger execution should mark the durable invocation as completed",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::Completed { attempt: 1, .. }
                )
        },
    )
    .await;

    let records = engine
        .list_trigger_invocations_for_testing(&tenant_id)
        .expect("trigger invocations should load");
    assert!(matches!(
        records[0].state,
        TriggerInvocationState::Completed { attempt: 1, .. }
    ));
    assert_eq!(
        executor.calls(),
        vec!["firebase:completeWritten".to_string()]
    );
}

#[tokio::test]
async fn trigger_execution_persists_terminal_failures() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);
    let document_id = DocumentId::from_key("terminal").expect("document id should build");
    let executor = Arc::new(RecordingTriggerExecutor::with_terminal_failure_for(
        "firebase:terminalWritten",
    ));
    engine
        .install_trigger_invocation_executor(executor.clone())
        .expect("trigger executor should install");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![trigger_registration(
                "firebase:terminalWritten",
                FirestoreCloudEventType::Written,
                ["tasks", "{taskId}"],
            )],
        )
        .expect("trigger registrations should persist in runtime");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("terminal"))]),
        )
        .expect("insert should succeed");

    wait_for_value(
        "trigger execution should persist terminal failure state",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::TerminalFailure { attempt: 1, .. }
                )
        },
    )
    .await;

    let records = engine
        .list_trigger_invocations_for_testing(&tenant_id)
        .expect("trigger invocations should load");
    assert!(matches!(
        records[0].state,
        TriggerInvocationState::TerminalFailure { attempt: 1, .. }
    ));
    assert_eq!(
        executor.calls(),
        vec!["firebase:terminalWritten".to_string()]
    );
}

#[tokio::test]
async fn trigger_execution_retries_retryable_failures_until_completion() {
    let (_data_dir, engine, tenant_id, clock) = new_trigger_engine_with_manual_clock(70_000);
    let document_id = DocumentId::from_key("retry-once").expect("document id should build");
    let executor = Arc::new(RecordingTriggerExecutor::with_retry_once_for(
        "firebase:retryWritten",
    ));
    engine
        .install_trigger_invocation_executor(executor.clone())
        .expect("trigger executor should install");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![trigger_registration(
                "firebase:retryWritten",
                FirestoreCloudEventType::Written,
                ["tasks", "{taskId}"],
            )],
        )
        .expect("trigger registrations should persist in runtime");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("retry"))]),
        )
        .expect("insert should succeed");

    wait_for_value(
        "trigger execution should persist retry-pending state after a retryable failure",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::RetryPending { attempt: 1, .. }
                )
        },
    )
    .await;

    clock.advance(Duration::from_secs(1));

    wait_for_value(
        "trigger execution should replay the retry and complete successfully",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::Completed { attempt: 2, .. }
                )
        },
    )
    .await;

    assert_eq!(
        executor.calls(),
        vec![
            "firebase:retryWritten".to_string(),
            "firebase:retryWritten".to_string()
        ]
    );
}

#[tokio::test]
async fn trigger_execution_promotes_exhausted_retries_to_terminal_failure() {
    let (_data_dir, engine, tenant_id, clock) = new_trigger_engine_with_manual_clock(75_000);
    let document_id = DocumentId::from_key("retry-terminal").expect("document id should build");
    let executor = Arc::new(RecordingTriggerExecutor::with_always_retry_for(
        "firebase:retryTerminalWritten",
    ));
    engine
        .install_trigger_invocation_executor(executor.clone())
        .expect("trigger executor should install");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![trigger_registration(
                "firebase:retryTerminalWritten",
                FirestoreCloudEventType::Written,
                ["tasks", "{taskId}"],
            )],
        )
        .expect("trigger registrations should persist in runtime");
    engine
        .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
        .expect("resource path binding should persist");

    engine
        .insert_document_with_id(
            &tenant_id,
            tasks_table(),
            document_id,
            serde_json::Map::from_iter([("title".to_string(), json!("retry-terminal"))]),
        )
        .expect("insert should succeed");

    for attempt in 1..5 {
        wait_for_value(
            "trigger execution should persist each retry-pending attempt before the next retry",
            mutation_journal_progress_timeout(),
            mutation_journal_poll_interval(),
            || async {
                engine
                    .list_trigger_invocations_for_testing(&tenant_id)
                    .expect("trigger invocations should load")
            },
            |records| {
                records.len() == 1
                    && matches!(
                        records[0].state,
                        TriggerInvocationState::RetryPending {
                            attempt: current_attempt,
                            ..
                        } if current_attempt == attempt
                    )
            },
        )
        .await;
        clock.advance(Duration::from_secs(1));
    }

    wait_for_value(
        "trigger execution should cap retryable failures at a terminal attempt budget",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::TerminalFailure { attempt: 5, .. }
                )
        },
    )
    .await;

    assert_eq!(executor.calls().len(), 5);
}

#[tokio::test]
async fn installing_executor_bootstraps_pending_invocations_after_restart() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let document_id = DocumentId::from_key("restart").expect("document id should build");

    {
        let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        engine
            .replace_trigger_registrations_for_testing(
                &tenant_id,
                vec![trigger_registration(
                    "firebase:restartWritten",
                    FirestoreCloudEventType::Written,
                    ["tasks", "{taskId}"],
                )],
            )
            .expect("trigger registrations should persist in runtime");
        engine
            .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
            .expect("resource path binding should persist");
        engine
            .insert_document_with_id(
                &tenant_id,
                tasks_table(),
                document_id.clone(),
                serde_json::Map::from_iter([("title".to_string(), json!("restart"))]),
            )
            .expect("insert should succeed");
        wait_for_value(
            "trigger materialization should persist pending invocation before restart",
            mutation_journal_progress_timeout(),
            mutation_journal_poll_interval(),
            || async {
                engine
                    .list_trigger_invocations_for_testing(&tenant_id)
                    .expect("trigger invocations should load")
            },
            |records| {
                records.len() == 1 && matches!(records[0].state, TriggerInvocationState::Pending)
            },
        )
        .await;
        engine.quiesce().await;
    }

    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should recreate"));
    engine
        .ensure_tenant_exists(&tenant_id)
        .expect("tenant should load");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![trigger_registration(
                "firebase:restartWritten",
                FirestoreCloudEventType::Written,
                ["tasks", "{taskId}"],
            )],
        )
        .expect("trigger registrations should persist in runtime");
    let executor = Arc::new(RecordingTriggerExecutor::default());
    engine
        .install_trigger_invocation_executor(executor.clone())
        .expect("trigger executor should install");

    wait_for_value(
        "installing the executor should replay pending durable trigger invocations",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::Completed { attempt: 1, .. }
                )
        },
    )
    .await;

    assert_eq!(
        executor.calls(),
        vec!["firebase:restartWritten".to_string()]
    );
}

#[tokio::test]
async fn installing_executor_bootstraps_due_retry_invocations_after_restart() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let document_id = DocumentId::from_key("retry-restart").expect("document id should build");
    let initial_clock = Arc::new(ManualClock::new(Timestamp(80_000)));

    {
        let engine = Arc::new(
            Engine::new_with_simulation(
                data_dir.path(),
                initial_clock.clone(),
                Arc::new(NoopFaultInjector),
            )
            .expect("engine should create"),
        );
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        engine
            .replace_trigger_registrations_for_testing(
                &tenant_id,
                vec![trigger_registration(
                    "firebase:retryRestartWritten",
                    FirestoreCloudEventType::Written,
                    ["tasks", "{taskId}"],
                )],
            )
            .expect("trigger registrations should persist in runtime");
        engine
            .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
            .expect("resource path binding should persist");
        engine
            .install_trigger_invocation_executor(Arc::new(
                RecordingTriggerExecutor::with_retry_once_for("firebase:retryRestartWritten"),
            ))
            .expect("trigger executor should install");

        engine
            .insert_document_with_id(
                &tenant_id,
                tasks_table(),
                document_id.clone(),
                serde_json::Map::from_iter([("title".to_string(), json!("retry-restart"))]),
            )
            .expect("insert should succeed");

        wait_for_value(
            "trigger execution should persist retry-pending state before restart",
            mutation_journal_progress_timeout(),
            mutation_journal_poll_interval(),
            || async {
                engine
                    .list_trigger_invocations_for_testing(&tenant_id)
                    .expect("trigger invocations should load")
            },
            |records| {
                records.len() == 1
                    && matches!(
                        records[0].state,
                        TriggerInvocationState::RetryPending { attempt: 1, .. }
                    )
            },
        )
        .await;
        engine.quiesce().await;
    }

    let restart_clock = Arc::new(ManualClock::new(Timestamp(81_000)));
    let engine = Arc::new(
        Engine::new_with_simulation(data_dir.path(), restart_clock, Arc::new(NoopFaultInjector))
            .expect("engine should recreate"),
    );
    engine
        .ensure_tenant_exists(&tenant_id)
        .expect("tenant should load");
    engine
        .replace_trigger_registrations_for_testing(
            &tenant_id,
            vec![trigger_registration(
                "firebase:retryRestartWritten",
                FirestoreCloudEventType::Written,
                ["tasks", "{taskId}"],
            )],
        )
        .expect("trigger registrations should persist in runtime");
    let executor = Arc::new(RecordingTriggerExecutor::default());
    engine
        .install_trigger_invocation_executor(executor.clone())
        .expect("trigger executor should install");

    wait_for_value(
        "installing the executor should replay due retry-pending invocations after restart",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::Completed { attempt: 2, .. }
                )
        },
    )
    .await;

    assert_eq!(
        executor.calls(),
        vec!["firebase:retryRestartWritten".to_string()]
    );
}

#[tokio::test]
async fn installing_executor_replays_running_invocations_after_restart() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let document_id = DocumentId::from_key("running-restart").expect("document id should build");
    let initial_clock = Arc::new(ManualClock::new(Timestamp(90_000)));

    {
        let engine = Arc::new(
            Engine::new_with_simulation(
                data_dir.path(),
                initial_clock.clone(),
                Arc::new(NoopFaultInjector),
            )
            .expect("engine should create"),
        );
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        engine
            .replace_trigger_registrations_for_testing(
                &tenant_id,
                vec![trigger_registration(
                    "firebase:runningRestartWritten",
                    FirestoreCloudEventType::Written,
                    ["tasks", "{taskId}"],
                )],
            )
            .expect("trigger registrations should persist in runtime");
        engine
            .upsert_resource_path_binding_for_testing(&tenant_id, trigger_binding(&document_id))
            .expect("resource path binding should persist");

        engine
            .insert_document_with_id(
                &tenant_id,
                tasks_table(),
                document_id.clone(),
                serde_json::Map::from_iter([("title".to_string(), json!("running-restart"))]),
            )
            .expect("insert should succeed");

        let mut records = wait_for_value(
            "trigger materialization should persist pending invocation before running restart",
            mutation_journal_progress_timeout(),
            mutation_journal_poll_interval(),
            || async {
                engine
                    .list_trigger_invocations_for_testing(&tenant_id)
                    .expect("trigger invocations should load")
            },
            |records| {
                records.len() == 1 && matches!(records[0].state, TriggerInvocationState::Pending)
            },
        )
        .await;
        let mut record = records.pop().expect("pending trigger should exist");
        record
            .begin_attempt(Timestamp(90_000))
            .expect("test should mark invocation running");
        engine
            .save_trigger_invocation_for_testing(&tenant_id, &record)
            .expect("running trigger invocation should persist");

        let records = engine
            .list_trigger_invocations_for_testing(&tenant_id)
            .expect("trigger invocations should load");
        assert!(
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::Running { attempt: 1, .. }
                ),
            "test setup should leave one durable running invocation before restart"
        );
        engine.quiesce().await;
    }

    let restart_clock = Arc::new(ManualClock::new(Timestamp(91_000)));
    let engine = Arc::new(
        Engine::new_with_simulation(data_dir.path(), restart_clock, Arc::new(NoopFaultInjector))
            .expect("engine should recreate"),
    );
    engine
        .ensure_tenant_exists(&tenant_id)
        .expect("tenant should load");
    let executor = Arc::new(RecordingTriggerExecutor::default());
    engine
        .install_trigger_invocation_executor(executor.clone())
        .expect("trigger executor should install");

    wait_for_value(
        "installing the executor should replay running durable trigger invocations after restart",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            engine
                .list_trigger_invocations_for_testing(&tenant_id)
                .expect("trigger invocations should load")
        },
        |records| {
            records.len() == 1
                && matches!(
                    records[0].state,
                    TriggerInvocationState::Completed { attempt: 1, .. }
                )
        },
    )
    .await;

    assert_eq!(
        executor.calls(),
        vec!["firebase:runningRestartWritten".to_string()]
    );
}
