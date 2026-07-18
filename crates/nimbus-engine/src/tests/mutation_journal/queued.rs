use super::support::{
    assert_future_stays_pending, expect_blocking_wait_reaches_state, expect_catch_up_future_within,
    expect_future_within, new_faulted_engine,
};
use super::*;
use nimbus_core::{ScheduleRequest, TriggerDeliveryCursor};
use nimbus_storage::NoopFaultInjector;

#[tokio::test]
async fn async_schema_write_advances_runtime_journal_before_next_queued_document_write() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .set_table_schema_async(
            tenant_id.clone(),
            TableSchema {
                table: tasks_table(),
                fields: vec![FieldSchema {
                    name: "title".to_string(),
                    field_type: FieldType::String,
                    required: true,
                }],
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .await
        .expect("async schema write should succeed");

    let after_schema = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after schema write");
    assert!(after_schema.durable_head.0 >= 1);
    assert_eq!(after_schema.applied_head, after_schema.durable_head);
    assert_eq!(after_schema.apply_lag, 0);
    let schema_head = after_schema.durable_head;

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("after-schema"))]),
        )
        .await
        .expect("queued document write should follow the schema commit sequence");

    let after_insert = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "queued insert should advance after schema commit",
        |stats| stats.durable_head.0 > schema_head.0 && stats.applied_head == stats.durable_head,
    )
    .await;
    assert_eq!(
        durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1
    );
    assert_eq!(after_insert.queue_depth, 0);
    assert_eq!(after_insert.worker_failure_count, 0);
}

/// Liveness smoke for the real `insert_document_async` path under concurrency:
/// several tasks issue rapid mutations in true parallelism and every one must
/// drain within a bound. This exercises the mutation journal end-to-end and
/// catches gross liveness regressions.
///
/// It is deliberately NOT presented as a reliable reproducer of the specific
/// old lost-wakeup race it was born from. The committer actor never retires or
/// re-arms, so that interleaving is structurally absent; the loom handoff model
/// permanently contrasts the old protocol with the actor topology. Counts are
/// modest because `cargo test` builds unoptimized (durable commits are ~10-50x
/// slower than the release build the benchmark uses).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_mutations_do_not_strand_the_journal_worker() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    const TASKS: usize = 4;
    const MUTATIONS_PER_TASK: usize = 200;

    let workload = {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            let mut handles = Vec::with_capacity(TASKS);
            for task in 0..TASKS {
                let engine = engine.clone();
                let tenant_id = tenant_id.clone();
                handles.push(tokio::spawn(async move {
                    for index in 0..MUTATIONS_PER_TASK {
                        engine
                            .insert_document_async(
                                tenant_id.clone(),
                                tasks_table(),
                                serde_json::Map::from_iter([(
                                    "title".to_string(),
                                    json!(format!("t{task}-{index}")),
                                )]),
                            )
                            .await
                            .expect("concurrent insert should not fail");
                    }
                }));
            }
            for handle in handles {
                handle.await.expect("mutation task should not panic");
            }
        }
    };

    tokio::time::timeout(std::time::Duration::from_secs(45), workload)
        .await
        .expect(
            "every concurrent mutation must drain — a hang here means the journal-worker \
             lost-wakeup deadlock has regressed",
        );

    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load");
    assert_eq!(stats.queue_depth, 0, "all mutations drained");
    assert_eq!(stats.worker_failure_count, 0, "no worker failures");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_disjoint_queued_commits_all_succeed_without_retry() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("prepared-disjoint", Engine::create_tenant);
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;

    run_paused_insert_burst(&engine, &tenant_id, 32).await;

    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;
    assert_eq!(
        after.reprepare_total.saturating_sub(before.reprepare_total),
        0,
        "disjoint prepared writes must not amplify into retries"
    );
    assert_eq!(after.prepared_payload_bytes_current, 0);
    assert!(after.prepared_payload_bytes_peak > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_disjoint_direct_commits_all_succeed_without_retry() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("prepared-direct-disjoint", Engine::create_tenant);
    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("table-seed"))]),
        )
        .expect("seed insert should establish the table identity");
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;

    let mut inserts = Vec::new();
    for index in 0..32 {
        inserts.push(tokio::task::spawn_blocking({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            move || {
                engine.insert_document(
                    &tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!(format!("direct-{index}")),
                    )]),
                )
            }
        }));
    }
    for insert in inserts {
        insert
            .await
            .expect("direct insert task should join")
            .expect("disjoint direct insert should succeed");
    }

    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;
    assert_eq!(after.reprepare_total - before.reprepare_total, 0);
    assert_eq!(after.prepared_payload_bytes_current, 0);
    assert_eq!(
        engine
            .query_documents(&tenant_id, &query_for("tasks"))
            .expect("direct documents should query")
            .len(),
        33
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_disjoint_execution_unit_commits_all_succeed_without_retry() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(56_000))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(56_000)),
        )
        .expect("memory engine should create"),
    );
    let tenant_id = TenantId::new("prepared-execution-disjoint").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    let seed = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("seed execution unit should begin");
    seed.insert_document(
        tasks_table(),
        serde_json::Map::from_iter([("title".to_string(), json!("table-seed"))]),
    )
    .expect("seed insert should stage");
    tokio::task::spawn_blocking(move || seed.commit())
        .await
        .expect("seed task should join")
        .expect("seed commit should succeed")
        .expect("seed insert should establish the table identity");
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;

    let mut units = Vec::new();
    for index in 0..32 {
        let unit = engine
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("execution unit should begin");
        unit.insert_document(
            tasks_table(),
            serde_json::Map::from_iter([(
                "title".to_string(),
                json!(format!("execution-{index}")),
            )]),
        )
        .expect("disjoint execution-unit insert should stage");
        units.push(unit);
    }
    let commits = units
        .into_iter()
        .map(|unit| tokio::task::spawn_blocking(move || unit.commit()))
        .collect::<Vec<_>>();
    for commit in commits {
        commit
            .await
            .expect("execution-unit task should join")
            .expect("disjoint execution-unit commit should succeed")
            .expect("document insert should produce a commit");
    }

    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;
    assert_eq!(after.reprepare_total - before.reprepare_total, 0);
    assert_eq!(after.prepared_payload_bytes_current, 0);
    assert_eq!(
        engine
            .query_documents_async(tenant_id.clone(), query_for("tasks"))
            .await
            .expect("execution-unit documents should query")
            .len(),
        33
    );
}

struct AssignedPendingUpdate {
    engine: Arc<Engine>,
    tenant_id: TenantId,
    record: nimbus_core::TenantEventRecord,
}

impl AssignedPendingUpdate {
    fn stage(
        engine: &Arc<Engine>,
        tenant_id: &TenantId,
        document_id: &DocumentId,
        field: &str,
        value: serde_json::Value,
    ) -> Self {
        // This is the exact state between assignment/durable append and apply:
        // registered in the pending window, durable, and not yet published.
        let record = engine
            .stage_assigned_pending_update_for_testing(
                tenant_id,
                &tasks_table(),
                document_id,
                field,
                value,
            )
            .expect("pending fixture should stage and append durably");
        Self {
            engine: engine.clone(),
            tenant_id: tenant_id.clone(),
            record,
        }
    }

    fn apply_and_publish(self) {
        self.engine
            .apply_assigned_pending_record_for_testing(&self.tenant_id, &self.record)
            .expect("pending fixture record should apply and publish");
    }

    fn apply_without_publish(&self) {
        self.engine
            .apply_assigned_pending_record_without_publish_for_testing(
                &self.tenant_id,
                &self.record,
            )
            .expect("pending fixture record should apply without publishing");
    }

    fn publish(self) {
        self.engine
            .publish_assigned_pending_record_for_testing(&self.tenant_id, &self.record)
            .expect("applied pending fixture record should publish");
    }
}

async fn wait_for_prepared_payload(engine: &Arc<Engine>, tenant_id: &TenantId, description: &str) {
    wait_for_value(
        description,
        Duration::from_secs(1),
        Duration::ZERO,
        || async {
            engine
                .tenant_engine_diagnostics(tenant_id)
                .expect("diagnostics should load")
                .commit_phases
                .prepared_payload_bytes_current
        },
        |bytes| *bytes > 0,
    )
    .await;
}

async fn wait_for_reprepare_count(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    expected: u64,
    description: &str,
) {
    wait_for_value(
        description,
        Duration::from_secs(1),
        Duration::ZERO,
        || async {
            engine
                .tenant_engine_diagnostics(tenant_id)
                .expect("diagnostics should load")
                .commit_phases
                .reprepare_total
        },
        |count| *count >= expected,
    )
    .await;
}

async fn run_direct_pending_reprepare_race() -> (
    EngineFixture<Engine>,
    Arc<Engine>,
    TenantId,
    DocumentId,
    u64,
    u64,
) {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("direct-pending-reprepare", Engine::create_tenant);
    let document_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("original"))]),
        )
        .expect("seed insert should succeed");
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load");
    let pending = AssignedPendingUpdate::stage(
        &engine,
        &tenant_id,
        &document_id,
        "assigned_pending",
        json!(1),
    );

    let mut direct = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        move || {
            engine.update_document(
                &tenant_id,
                tasks_table(),
                document_id,
                serde_json::Map::from_iter([("direct".to_string(), json!(2))]),
            )
        }
    });
    wait_for_prepared_payload(
        &engine,
        &tenant_id,
        "direct prepare should remain accounted while it waits for the pending sequence",
    )
    .await;
    assert_future_stays_pending(
        &mut direct,
        "direct retry must remain blocked until the pending sequence applies",
    )
    .await;
    pending.apply_and_publish();
    expect_future_within(direct, "direct retry should finish after pending apply")
        .await
        .expect("direct task should join")
        .expect("direct update should transparently re-prepare");

    (
        fixture,
        engine,
        tenant_id,
        document_id,
        before.mutation_journal.read_wait_count,
        before.commit_phases.reprepare_total,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_prepared_write_detects_pending_then_waits_and_reprepares() {
    let (_fixture, engine, tenant_id, _, waits_before, retries_before) =
        run_direct_pending_reprepare_race().await;
    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load");
    assert!(after.mutation_journal.read_wait_count > waits_before);
    assert_eq!(after.commit_phases.reprepare_total - retries_before, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_sequential_same_doc_writes_match_overlay_semantics() {
    let (_fixture, engine, tenant_id, document_id, _, _) =
        run_direct_pending_reprepare_race().await;
    let document = engine
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("directly updated document should remain visible");
    assert_eq!(document.fields.get("assigned_pending"), Some(&json!(1)));
    assert_eq!(document.fields.get("direct"), Some(&json!(2)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn progress_sync_cannot_leapfrog_pending_write_or_stale_window_reprepare() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("progress-pending-reprepare", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("background trigger candidate worker should shut down");
    let document_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("original"))]),
        )
        .expect("seed insert should succeed");
    let pending = AssignedPendingUpdate::stage(
        &engine,
        &tenant_id,
        &document_id,
        "assigned_pending",
        json!(1),
    );
    let pending_sequence = pending.record.sequence;
    pending.apply_without_publish();

    let mut direct = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        move || {
            engine.update_document(
                &tenant_id,
                tasks_table(),
                document_id,
                serde_json::Map::from_iter([("direct".to_string(), json!(2))]),
            )
        }
    });
    wait_for_prepared_payload(
        &engine,
        &tenant_id,
        "direct prepare should wait on the held pending sequence",
    )
    .await;

    engine
        .set_trigger_delivery_cursor_for_testing(
            &tenant_id,
            TriggerDeliveryCursor::new(pending_sequence),
        )
        .expect("later zero-write cursor record should synchronize progress");
    let observed_applied = SequenceNumber(pending_sequence.0 + 1);
    let held = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("held journal stats should load");
    assert_eq!(held.durable_head, observed_applied);
    assert!(
        held.applied_head < pending_sequence,
        "progress sync must stop the engine applied watermark before the held pending sequence"
    );
    assert_future_stays_pending(
        &mut direct,
        "direct retry must not wake from progress beyond a held pending sequence",
    )
    .await;

    pending.publish();
    expect_future_within(direct, "direct retry should finish after pending apply")
        .await
        .expect("direct task should join")
        .expect("direct update should re-prepare from the published window image");
    let released = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("released journal stats should load");
    assert!(
        released.applied_head >= observed_applied,
        "releasing the pending barrier must expose the observed zero-write suffix"
    );

    let document = engine
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("re-prepared document should remain visible");
    assert_eq!(document.fields.get("assigned_pending"), Some(&json!(1)));
    assert_eq!(document.fields.get("direct"), Some(&json!(2)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn uncovered_progress_advances_applied_watermark_without_local_window_image() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("uncovered-progress", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("background trigger candidate worker should shut down");
    engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("covered"))]),
        )
        .expect("covered seed insert should succeed");
    let before = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("initial journal stats should load");
    let uncovered = SequenceNumber(before.applied_head.0 + 1);

    engine
        .sync_mutation_journal_progress_for_testing(
            &tenant_id,
            nimbus_storage::JournalProgress {
                durable_head: uncovered,
                applied_head: uncovered,
            },
        )
        .expect("uncovered provider-style progress should synchronize");

    let after = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("updated journal stats should load");
    assert_eq!(after.durable_head, uncovered);
    assert_eq!(
        after.applied_head, uncovered,
        "an uncovered sequence with no lower pending owner must not stall the engine watermark"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn execution_unit_prepared_write_detects_pending_then_waits_and_reprepares() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("execution-pending-reprepare", Engine::create_tenant);
    let document_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("original"))]),
        )
        .expect("seed insert should succeed");
    let stale_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("stale execution unit should begin");
    stale_unit
        .update_document(
            tasks_table(),
            document_id.clone(),
            serde_json::Map::from_iter([("execution".to_string(), json!(2))]),
        )
        .expect("stale execution-unit update should stage");
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load");
    let pending = AssignedPendingUpdate::stage(
        &engine,
        &tenant_id,
        &document_id,
        "assigned_pending",
        json!(1),
    );

    let error = tokio::task::spawn_blocking(move || stale_unit.commit())
        .await
        .expect("stale execution-unit task should join")
        .expect_err("assigned pending write must conflict with the stale unit");
    let conflicting_sequence = error
        .conflicting_sequence()
        .expect("pending conflict should name its assigned sequence");
    assert_eq!(conflicting_sequence, pending.record.sequence);

    let mut retry = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        move || {
            engine.record_mutation_conflict_retry(&tenant_id)?;
            engine.wait_for_applied_sequence_blocking(&tenant_id, conflicting_sequence)?;
            let unit =
                engine.begin_mutation_execution_unit(tenant_id, PrincipalContext::anonymous())?;
            unit.update_document(
                tasks_table(),
                document_id,
                serde_json::Map::from_iter([("execution".to_string(), json!(2))]),
            )?;
            unit.commit()
        }
    });
    wait_for_reprepare_count(
        &engine,
        &tenant_id,
        before.commit_phases.reprepare_total + 1,
        "execution-unit caller should record its retry before waiting for apply",
    )
    .await;
    assert_future_stays_pending(
        &mut retry,
        "execution-unit retry must remain blocked until the pending sequence applies",
    )
    .await;
    pending.apply_and_publish();
    expect_future_within(
        retry,
        "execution-unit retry should finish after pending apply",
    )
    .await
    .expect("execution-unit retry task should join")
    .expect("execution-unit retry should succeed")
    .expect("execution-unit retry should produce a commit");

    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load");
    assert!(after.mutation_journal.read_wait_count > before.mutation_journal.read_wait_count);
    assert_eq!(
        after.commit_phases.reprepare_total - before.commit_phases.reprepare_total,
        1
    );
    let document = engine
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("execution-unit document should remain visible");
    assert_eq!(document.fields.get("assigned_pending"), Some(&json!(1)));
    assert_eq!(document.fields.get("execution"), Some(&json!(2)));
}

async fn run_same_document_prepare_race() -> (
    EngineFixture<Engine>,
    Arc<Engine>,
    TenantId,
    nimbus_core::DocumentId,
    u64,
    u64,
) {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("prepared-hot-key", Engine::create_tenant);
    let document_id = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("original"))]),
        )
        .await
        .expect("seed insert should succeed");
    let before_diagnostics = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load");
    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("pause handle should load");
    pause.arm();

    let first = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("first".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state(
        "first same-document prepare should reach the paused drainer",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;
    let second = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("second".to_string(), json!(2))]),
                )
                .await
        }
    });
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "second same-document prepare should queue behind the paused drainer",
        |stats| stats.queue_depth == 1,
    )
    .await;
    pause.release();
    first
        .await
        .expect("first task should join")
        .expect("first update should succeed");
    second
        .await
        .expect("second task should join")
        .expect("second update should re-prepare");

    (
        fixture,
        engine,
        tenant_id,
        document_id,
        before_diagnostics.mutation_journal.read_wait_count,
        before_diagnostics.commit_phases.reprepare_total,
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_prepared_write_detects_pending_and_reprepares_inline() {
    let (_fixture, engine, tenant_id, document_id, waits_before, retries_before) =
        run_same_document_prepare_race().await;
    let document = engine
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("final same-document read should succeed");
    assert_eq!(document.fields.get("first"), Some(&json!(1)));
    assert_eq!(document.fields.get("second"), Some(&json!(2)));
    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load");
    assert_eq!(after.mutation_journal.read_wait_count, waits_before);
    assert_eq!(after.commit_phases.reprepare_total - retries_before, 0);
    assert!(after.commit_phases.inline_reprepare_total > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn hot_key_direct_writes_reprepare_inline_without_caller_retry() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("direct-inline-hot-key", Engine::create_tenant);
    let document_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("seed"))]),
        )
        .expect("seed insert should succeed");
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;

    let writes = (0..32)
        .map(|index| {
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            let document_id = document_id.clone();
            tokio::task::spawn_blocking(move || {
                engine.update_document(
                    &tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([(format!("field_{index}"), json!(index))]),
                )
            })
        })
        .collect::<Vec<_>>();
    for write in writes {
        write
            .await
            .expect("hot-key task should join")
            .expect("hot-key write should succeed");
    }
    let document = engine
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("final hot-key read should succeed");
    for index in 0..32 {
        assert_eq!(
            document.fields.get(&format!("field_{index}")),
            Some(&json!(index)),
            "inline re-prepare must retain every serialized patch"
        );
    }

    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;
    assert!(
        after.inline_reprepare_total > before.inline_reprepare_total,
        "same-document burst must exercise actor-local re-prepare"
    );
    assert_eq!(
        after.reprepare_total - before.reprepare_total,
        0,
        "same-document blind writes must never reach caller retry"
    );
    assert_eq!(
        after.window_prepare_total - before.window_prepare_total,
        32,
        "every hot-key caller should prepare from the published in-memory image"
    );
    assert_eq!(
        after.storage_prepare_total - before.storage_prepare_total,
        0,
        "the direct hot path must not acquire the storage-backed prepare permit"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn out_of_window_stale_prepare_falls_back_to_caller_wait() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("direct-window-fallback", Engine::create_tenant);
    let document_id = engine
        .insert_document(
            &tenant_id,
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("seed"))]),
        )
        .expect("seed insert should succeed");
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(crate::engine::commit_fault_labels::PREPARE_COMPLETE);
    let update = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        move || {
            engine.update_document(
                &tenant_id,
                tasks_table(),
                document_id,
                serde_json::Map::from_iter([("fallback".to_string(), json!(true))]),
            )
        }
    });
    expect_blocking_wait_reaches_state("direct prepare should pause before actor admission", {
        let faults = faults.clone();
        move |timeout| {
            faults.wait_until_entered(
                crate::engine::commit_fault_labels::PREPARE_COMPLETE,
                timeout,
            )
        }
    })
    .await;
    engine
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            document_id.clone(),
            serde_json::Map::from_iter([("racer".to_string(), json!(true))]),
        )
        .await
        .expect("a racing write should make the paused prepare stale");
    engine
        .force_write_log_storage_fallback_for_testing(&tenant_id)
        .expect("test should force an out-of-window validation source");
    faults.release(crate::engine::commit_fault_labels::PREPARE_COMPLETE);
    expect_catch_up_future_within(update, "caller fallback should re-prepare and commit")
        .await
        .expect("fallback task should join")
        .expect("fallback update should succeed");

    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("diagnostics should load")
        .commit_phases;
    assert_eq!(after.inline_reprepare_total, before.inline_reprepare_total);
    assert_eq!(after.reprepare_total - before.reprepare_total, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queued_sequential_same_doc_writes_match_overlay_semantics() {
    let (_fixture, engine, tenant_id, document_id, _, _) = run_same_document_prepare_race().await;
    let visible = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    let document = visible
        .iter()
        .find(|document| document.id == document_id)
        .expect("updated document should remain visible");
    assert_eq!(document.fields.get("first"), Some(&json!(1)));
    assert_eq!(document.fields.get("second"), Some(&json!(2)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn assign_time_stamping_is_monotonic_under_concurrent_prepares() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(55_000))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(55_000)),
        )
        .expect("memory engine should create"),
    );
    let tenant_id = TenantId::new("prepared-stamps").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    run_paused_insert_burst(&engine, &tenant_id, 16).await;
    let commits = durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0));
    assert_eq!(commits.len(), 16);
    assert!(commits.windows(2).all(|pair| {
        pair[0].sequence < pair[1].sequence && pair[0].timestamp.0 <= pair[1].timestamp.0
    }));
    assert!(
        commits
            .iter()
            .all(|commit| commit.timestamp == Timestamp(55_000))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bounded_committer_inbox_times_out_with_typed_retryable_error_and_reports_depth() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("bounded-committer").expect("tenant id should build");
    crate::tenant::configure_committer_limits_for_testing(
        tenant_id.clone(),
        2,
        Duration::from_millis(25),
    );
    engine
        .create_tenant(tenant_id.clone())
        .expect("bounded-committer tenant should create");
    let inbox_capacity = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("committer diagnostics should load")
        .mutation_journal
        .committer_inbox_capacity;
    assert_eq!(
        inbox_capacity, 2,
        "the per-tenant test override must be in effect; a default-capacity \
         inbox would let this test pass without exercising the bounded path"
    );

    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("committer pause handle should load");
    pause.arm();
    let spawn_insert = |title: &'static str| {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!(title))]),
                )
                .await
        })
    };

    let first = spawn_insert("actor-held");
    expect_blocking_wait_reaches_state(
        "the first mutation should hold the committer at the pause seam",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;

    let spawn_schema = |index: usize| {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .set_table_schema_async(
                    tenant_id,
                    TableSchema {
                        table: TableName::new(format!("inbox_{index}"))
                            .expect("test table name should build"),
                        fields: vec![],
                        indexes: vec![],
                        access_policy: None,
                    },
                )
                .await
        })
    };
    let accepted = (0..inbox_capacity).map(spawn_schema).collect::<Vec<_>>();
    let full = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "all bounded committer slots should become observable",
        |stats| stats.committer_inbox_depth == inbox_capacity,
    )
    .await;
    assert_eq!(full.committer_inbox_capacity, inbox_capacity);

    let started = std::time::Instant::now();
    let rejected = [spawn_insert("rejected_one"), spawn_insert("rejected_two")];
    let mut errors = Vec::new();
    for rejected_insert in rejected {
        errors.push(
            expect_future_within(rejected_insert, "a full-inbox sender must time out")
                .await
                .expect("rejected insert task should join")
                .expect_err("each sender beyond the bounded inbox must time out"),
        );
    }
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the bounded send timeout must not turn into unbounded queueing"
    );
    for error in &errors {
        assert!(
            matches!(error, nimbus_core::Error::CommitterFull { capacity, .. } if *capacity == inbox_capacity)
        );
        assert_eq!(
            error.retryability(),
            nimbus_core::Retryability::RetryableAfterBackoff
        );
    }

    let diagnostics = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("committer diagnostics should load while saturated");
    assert_eq!(
        diagnostics.mutation_journal.committer_inbox_depth,
        inbox_capacity
    );
    assert_eq!(diagnostics.mutation_journal.committer_send_timeout_count, 2);
    assert_eq!(
        diagnostics.commit_phases.committer_inbox_depth,
        u64::try_from(inbox_capacity).expect("committer capacity should fit diagnostics")
    );
    assert_eq!(diagnostics.commit_phases.committer_send_timeout_total, 2);

    pause.release();
    expect_future_within(first, "the held queued mutation should drain after release")
        .await
        .expect("accepted insert task should join")
        .expect("accepted insert should commit");
    for schema_write in accepted {
        expect_future_within(
            schema_write,
            "accepted direct committer work should drain after release",
        )
        .await
        .expect("accepted schema task should join")
        .expect("accepted schema write should commit");
    }
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("cleanup-wake"))]),
        )
        .await
        .expect("a later queued wake should drain cancelled timed-out requests");
    let drained = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "the bounded committer inbox should fully drain",
        |stats| stats.committer_inbox_depth == 0 && stats.pending_response_count == 0,
    )
    .await;
    assert_eq!(drained.committer_send_timeout_count, 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejected_serial_job_returns_typed_retryable_publisher_error() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("serial-job-rejection").expect("tenant id should build");
    crate::tenant::configure_publisher_limits_for_testing(
        tenant_id.clone(),
        1,
        Duration::from_millis(50),
    );
    engine
        .create_tenant(tenant_id.clone())
        .expect("serial-job-rejection tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH);

    let first = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("first publisher batch should pause", {
        let faults = faults.clone();
        move |timeout| {
            faults.wait_until_entered(
                crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH,
                timeout,
            )
        }
    })
    .await;
    let second = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(2))]),
                )
                .await
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "second batch should fill the publisher queue",
        |stats| stats.publisher_queue_depth == 1,
    )
    .await;

    let schema_error = engine
        .set_table_schema_async(
            tenant_id.clone(),
            TableSchema {
                table: TableName::new("serial_rejected").expect("table name should build"),
                fields: Vec::new(),
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .await
        .expect_err("full publisher queue should reject the opaque serial job");
    assert!(matches!(schema_error, Error::CommitterFull { .. }));
    assert_eq!(
        schema_error.retryability(),
        nimbus_core::Retryability::RetryableAfterBackoff
    );

    faults.release(crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH);
    expect_catch_up_future_within(first, "first batch should finish after release")
        .await
        .expect("first task should join")
        .expect("first batch should succeed");
    expect_catch_up_future_within(second, "second batch should finish after release")
        .await
        .expect("second task should join")
        .expect("second batch should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pipeline_mode_reconcile_timeout_processes_client_batch_in_current_mode() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("mode-reconcile-timeout").expect("tenant id should build");
    crate::tenant::configure_publisher_limits_for_testing(
        tenant_id.clone(),
        1,
        Duration::from_millis(100),
    );
    engine
        .create_tenant(tenant_id.clone())
        .expect("mode-reconcile-timeout tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH);
    let spawn_insert = |index: usize| {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
        })
    };
    let first = spawn_insert(1);
    expect_blocking_wait_reaches_state("first publisher batch should pause", {
        let faults = faults.clone();
        move |timeout| {
            faults.wait_until_entered(
                crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH,
                timeout,
            )
        }
    })
    .await;
    let second = spawn_insert(2);
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "second batch should fill the publisher queue",
        |stats| stats.publisher_queue_depth == 1,
    )
    .await;
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request serial mode");
    let third = spawn_insert(3);
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "mode barrier should time out without failing the client batch",
        |stats| stats.publisher_mode_transition_failure_count == 1,
    )
    .await;
    faults.release(crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH);

    for write in [first, second, third] {
        expect_catch_up_future_within(write, "client batch should finish in current mode")
            .await
            .expect("insert task should join")
            .expect("mode reconciliation failure must not fail client writes");
    }
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("mode diagnostics should load");
    assert_eq!(stats.publisher_mode_transition_failure_count, 1);
    assert_eq!(stats.durable_head, stats.applied_head);
}

async fn run_paused_insert_burst(engine: &Arc<Engine>, tenant_id: &TenantId, count: usize) {
    assert!(count > 0, "a burst must contain at least one mutation");
    engine
        .set_mutation_admission_codel_for_testing(
            tenant_id,
            Duration::from_secs(60),
            Duration::from_secs(60),
        )
        .expect("the burst should not be shed by CoDel");
    let pause = engine
        .mutation_journal_pause_handle_for_testing(tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let mut inserts = Vec::with_capacity(count);
    inserts.push(tokio::spawn({
        let engine = Arc::clone(engine);
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("burst-0"))]),
                )
                .await
        }
    }));
    expect_blocking_wait_reaches_state(
        "journal worker should pause with the first burst mutation admitted",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;

    for index in 1..count {
        inserts.push(tokio::spawn({
            let engine = Arc::clone(engine);
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        serde_json::Map::from_iter([(
                            "title".to_string(),
                            json!(format!("burst-{index}")),
                        )]),
                    )
                    .await
            }
        }));
    }
    if count > 1 {
        wait_for_mutation_admission_stats(
            engine,
            tenant_id,
            "the rest of the burst should be queued behind the paused drainer",
            |stats| stats.queue_depth == count - 1,
        )
        .await;
    }
    pause.release();

    for insert in inserts {
        expect_catch_up_future_within(insert, "every mutation in the paused burst should commit")
            .await
            .expect("burst task should join")
            .expect("burst mutation should succeed");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adaptive_batch_grows_under_backlog_and_shrinks_when_idle() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("adaptive-batch", Engine::create_tenant);
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics before burst should load")
        .commit_phases;

    run_paused_insert_burst(&engine, &tenant_id, 96).await;
    let after_burst = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics after burst should load")
        .commit_phases;
    let burst_size_sum = after_burst
        .journal_batch_size_sum
        .saturating_sub(before.journal_batch_size_sum);
    let burst_count = after_burst
        .journal_batch_count
        .saturating_sub(before.journal_batch_count);
    assert_eq!(
        burst_count, 1,
        "the paused backlog should drain as one batch"
    );
    assert_eq!(burst_size_sum, 96);
    assert!(
        burst_size_sum / burst_count > 32,
        "backlog should raise the effective batch above the base cap"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("idle"))]),
        )
        .await
        .expect("idle mutation should succeed");
    let after_idle = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics after idle mutation should load")
        .commit_phases;
    assert_eq!(
        after_idle
            .journal_batch_count
            .saturating_sub(after_burst.journal_batch_count),
        1
    );
    assert_eq!(
        after_idle
            .journal_batch_size_sum
            .saturating_sub(after_burst.journal_batch_size_sum),
        1,
        "an idle arrival should retain base behavior rather than waiting for a max batch"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn adaptive_batch_never_splits_the_durable_round_trip() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults =
        CountedFaultInjector::fail_nth_call(FaultPoint::JournalDurableAppendBeforeApply, u64::MAX);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(43_500))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(43_500)),
        )
        .expect("memory engine should create"),
    );
    let tenant_id = TenantId::new("adaptive-round-trip").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    let visits_before = faults.visit_count();
    let metrics_before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics before burst should load")
        .commit_phases;

    run_paused_insert_burst(&engine, &tenant_id, 65).await;

    let metrics_after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics after burst should load")
        .commit_phases;
    let drained_batches = metrics_after
        .journal_batch_count
        .saturating_sub(metrics_before.journal_batch_count);
    let append_boundaries = faults.visit_count().saturating_sub(visits_before);
    assert_eq!(drained_batches, 1);
    assert_eq!(
        append_boundaries, drained_batches,
        "each adaptive drain must cross the post-append fault boundary exactly once; the record slice is passed to one append_durable_records_batch call"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_accumulator_preserves_fsync_amortization_when_assignment_gets_ahead() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("publisher-accumulator", Engine::create_tenant);
    let before = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics before publisher backlog should load")
        .commit_phases;
    let faults = engine.commit_fault_handle_for_testing();
    let pause_label = crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH;
    faults.arm(pause_label);

    let mut inserts = vec![tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(0))]),
                )
                .await
        }
    })];
    expect_blocking_wait_reaches_state("first publisher batch should pause after append", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_entered(pause_label, timeout)
    })
    .await;

    const QUEUED_BATCHES: usize = 16;
    for index in 1..=QUEUED_BATCHES {
        inserts.push(tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                    )
                    .await
            }
        }));
        wait_for_mutation_journal_stats(
            &engine,
            &tenant_id,
            "each assigned singleton should reach the bounded publisher queue",
            |stats| stats.publisher_queue_depth == index,
        )
        .await;
    }

    faults.release(pause_label);
    for insert in inserts {
        expect_catch_up_future_within(insert, "accumulated mutation should publish")
            .await
            .expect("accumulated insert task should join")
            .expect("accumulated insert should succeed");
    }

    let after = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("phase metrics after publisher backlog should load")
        .commit_phases;
    assert_eq!(
        after
            .journal_batch_count
            .saturating_sub(before.journal_batch_count),
        2,
        "the paused singleton plus all queued assignments should require two fsyncs"
    );
    assert_eq!(
        after
            .journal_batch_size_sum
            .saturating_sub(before.journal_batch_size_sum),
        (QUEUED_BATCHES + 1) as u64
    );
}

struct RetryableThenBlockingAppendFaultInjector {
    append_visits: std::sync::atomic::AtomicU64,
    retry_entered: (Mutex<bool>, Condvar),
    retry_released: (Mutex<bool>, Condvar),
}

struct BlockingDefinitiveAppendFaultInjector {
    append_visits: std::sync::atomic::AtomicU64,
    fail_on_visit: u64,
    failed: AtomicBool,
    entered: (Mutex<bool>, Condvar),
    released: (Mutex<bool>, Condvar),
}

#[derive(Default)]
struct DurableAppendThenRecoveryFaultInjector {
    armed: AtomicBool,
    append_failed: AtomicBool,
    recovery_failed: AtomicBool,
}

impl DurableAppendThenRecoveryFaultInjector {
    fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }
}

impl nimbus_storage::FaultInjector for DurableAppendThenRecoveryFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if !self.armed.load(Ordering::Acquire) {
            return Ok(());
        }
        if point == FaultPoint::JournalFlushBeforeVisibility
            && !self.append_failed.swap(true, Ordering::AcqRel)
        {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Unavailable,
                "injected append acknowledgement failure after durable visibility",
            ));
        }
        if point == FaultPoint::StorageCommitAfterVisibilityBeforeReturn
            && !self.recovery_failed.swap(true, Ordering::AcqRel)
        {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Unavailable,
                "injected durable journal recovery failure",
            ));
        }
        Ok(())
    }
}

impl BlockingDefinitiveAppendFaultInjector {
    fn new() -> Arc<Self> {
        Self::new_on_visit(1)
    }

    fn new_on_visit(fail_on_visit: u64) -> Arc<Self> {
        Arc::new(Self {
            append_visits: std::sync::atomic::AtomicU64::new(0),
            fail_on_visit,
            failed: AtomicBool::new(false),
            entered: (Mutex::new(false), Condvar::new()),
            released: (Mutex::new(false), Condvar::new()),
        })
    }

    fn wait_until_blocked(&self, timeout: Duration) -> bool {
        let (lock, condvar) = &self.entered;
        let entered = lock.lock().expect("definitive entered lock should acquire");
        if *entered {
            return true;
        }
        let (entered, _) = condvar
            .wait_timeout_while(entered, timeout, |entered| !*entered)
            .expect("definitive entered wait should succeed");
        *entered
    }

    fn release_failure(&self) {
        let (lock, condvar) = &self.released;
        *lock.lock().expect("definitive release lock should acquire") = true;
        condvar.notify_all();
    }
}

impl nimbus_storage::FaultInjector for BlockingDefinitiveAppendFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != FaultPoint::JournalAppendBeforeDurableFlush {
            return Ok(());
        }
        let visit = self.append_visits.fetch_add(1, Ordering::AcqRel) + 1;
        if visit != self.fail_on_visit || self.failed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let (entered_lock, entered_condvar) = &self.entered;
        *entered_lock
            .lock()
            .expect("definitive entered lock should acquire") = true;
        entered_condvar.notify_all();
        let (release_lock, release_condvar) = &self.released;
        let mut released = release_lock
            .lock()
            .expect("definitive release lock should acquire");
        while !*released {
            released = release_condvar
                .wait(released)
                .expect("definitive release wait should succeed");
        }
        Err(Error::InvalidInput(
            "injected definitive publisher failure".to_string(),
        ))
    }
}

struct BlockingAmbiguousApplyFaultInjector {
    failed: AtomicBool,
    entered: (Mutex<bool>, Condvar),
    released: (Mutex<bool>, Condvar),
}

impl BlockingAmbiguousApplyFaultInjector {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            failed: AtomicBool::new(false),
            entered: (Mutex::new(false), Condvar::new()),
            released: (Mutex::new(false), Condvar::new()),
        })
    }

    fn wait_until_blocked(&self, timeout: Duration) -> bool {
        let (lock, condvar) = &self.entered;
        let entered = lock.lock().expect("ambiguous entered lock should acquire");
        if *entered {
            return true;
        }
        let (entered, _) = condvar
            .wait_timeout_while(entered, timeout, |entered| !*entered)
            .expect("ambiguous entered wait should succeed");
        *entered
    }

    fn release_failure(&self) {
        let (lock, condvar) = &self.released;
        *lock.lock().expect("ambiguous release lock should acquire") = true;
        condvar.notify_all();
    }
}

impl nimbus_storage::FaultInjector for BlockingAmbiguousApplyFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != FaultPoint::JournalDurableAppendBeforeApply
            || self.failed.swap(true, Ordering::AcqRel)
        {
            return Ok(());
        }
        let (entered_lock, entered_condvar) = &self.entered;
        *entered_lock
            .lock()
            .expect("ambiguous entered lock should acquire") = true;
        entered_condvar.notify_all();
        let (release_lock, release_condvar) = &self.released;
        let mut released = release_lock
            .lock()
            .expect("ambiguous release lock should acquire");
        while !*released {
            released = release_condvar
                .wait(released)
                .expect("ambiguous release wait should succeed");
        }
        Err(Error::Internal(
            "injected ambiguous publisher apply failure".to_string(),
        ))
    }
}

impl RetryableThenBlockingAppendFaultInjector {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            append_visits: std::sync::atomic::AtomicU64::new(0),
            retry_entered: (Mutex::new(false), Condvar::new()),
            retry_released: (Mutex::new(false), Condvar::new()),
        })
    }

    fn wait_until_retry_blocked(&self, timeout: Duration) -> bool {
        let (lock, condvar) = &self.retry_entered;
        let entered = lock.lock().expect("retry-entered lock should acquire");
        if *entered {
            return true;
        }
        let (entered, _) = condvar
            .wait_timeout_while(entered, timeout, |entered| !*entered)
            .expect("retry-entered wait should succeed");
        *entered
    }

    fn release_retry(&self) {
        let (lock, condvar) = &self.retry_released;
        *lock.lock().expect("retry-release lock should acquire") = true;
        condvar.notify_all();
    }
}

impl nimbus_storage::FaultInjector for RetryableThenBlockingAppendFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point != FaultPoint::JournalAppendBeforeDurableFlush {
            return Ok(());
        }
        let visit = self.append_visits.fetch_add(1, Ordering::AcqRel) + 1;
        if visit == 2 {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Transient,
                "injected transient publisher append failure",
            ));
        }
        if visit == 3 {
            let (entered_lock, entered_condvar) = &self.retry_entered;
            *entered_lock
                .lock()
                .expect("retry-entered lock should acquire") = true;
            entered_condvar.notify_all();
            let (release_lock, release_condvar) = &self.retry_released;
            let mut released = release_lock
                .lock()
                .expect("retry-release lock should acquire");
            while !*released {
                released = release_condvar
                    .wait(released)
                    .expect("retry-release wait should succeed");
            }
        }
        Ok(())
    }
}

struct RetryExhaustionThenHealthyAppendFaultInjector {
    visits: std::sync::atomic::AtomicU64,
}

struct ArmedOneShotDirectFaultInjector {
    point: FaultPoint,
    armed: AtomicBool,
    failed: AtomicBool,
}

impl ArmedOneShotDirectFaultInjector {
    fn new(point: FaultPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            armed: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        })
    }

    fn arm(&self) {
        self.armed.store(true, Ordering::Release);
    }
}

impl nimbus_storage::FaultInjector for ArmedOneShotDirectFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point == self.point
            && self.armed.load(Ordering::Acquire)
            && !self.failed.swap(true, Ordering::AcqRel)
        {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Transient,
                format!("injected one-shot direct fault at {}", point.as_str()),
            ));
        }
        Ok(())
    }
}

impl RetryExhaustionThenHealthyAppendFaultInjector {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            visits: std::sync::atomic::AtomicU64::new(0),
        })
    }
}

impl nimbus_storage::FaultInjector for RetryExhaustionThenHealthyAppendFaultInjector {
    fn check(&self, point: FaultPoint) -> nimbus_core::Result<()> {
        if point == FaultPoint::JournalAppendBeforeDurableFlush
            && self.visits.fetch_add(1, Ordering::AcqRel) < 4
        {
            return Err(Error::storage(
                nimbus_core::StorageErrorKind::Transient,
                "injected retry exhaustion before durable advance",
            ));
        }
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_preserves_sequence_order_across_transient_retry() {
    let data_dir = tempdir().expect("transient publisher tempdir should build");
    let faults = RetryableThenBlockingAppendFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_000))),
            faults.clone(),
        )
        .expect("transient publisher engine should create"),
    );
    let tenant_id = TenantId::new("publisher-retry-order").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("transient publisher tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("first publisher batch should commit");

    let second = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(2))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("retry of batch N should block before persistence", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_retry_blocked(timeout)
    })
    .await;

    let third = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(3))]),
                )
                .await
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "batch N+1 should wait in the publisher queue behind retrying batch N",
        |stats| stats.publisher_queue_depth == 1,
    )
    .await;
    let persisted_while_retrying = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("durable prefix should read while retry is blocked");
    assert_eq!(
        persisted_while_retrying
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1)],
        "batch N+1 must not persist around the retrying batch N"
    );
    assert_eq!(
        engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("retry diagnostics should load")
            .publisher_transient_error_count,
        1
    );

    faults.release_retry();
    expect_catch_up_future_within(second, "retrying publisher batch should complete")
        .await
        .expect("second insert task should join")
        .expect("second insert should succeed after retry");
    expect_catch_up_future_within(third, "following publisher batch should complete")
        .await
        .expect("third insert task should join")
        .expect("third insert should succeed");
    assert_eq!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("final durable prefix should read")
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2), SequenceNumber(3)]
    );
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("final retry diagnostics should load");
    assert_eq!(stats.publisher_transient_error_count, 1);
    assert_eq!(stats.publisher_fatal_error_count, 0);
    assert_eq!(stats.publisher_ambiguous_error_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn assignment_failure_mid_batch_discards_staged_suffix_and_keeps_tenant_live() {
    let data_dir = tempdir().expect("assignment failure tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_500))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_500)),
        )
        .expect("assignment failure engine should create"),
    );
    let tenant_id = TenantId::new("assignment-failure-recovery").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    let faults = engine.commit_fault_handle_for_testing();
    faults.inject_error_on_nth_hit(
        crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
        1,
        Error::InvalidInput("injected mid-batch assignment failure".to_string()),
    );

    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("assignment drain pause should load");
    pause.arm();
    let mut writes = Vec::new();
    for index in 0..3 {
        writes.push(tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                    )
                    .await
            }
        }));
        if index == 0 {
            expect_blocking_wait_reaches_state("assignment batch should pause before drain", {
                let pause = pause.clone();
                move |timeout| pause.wait_until_entered(timeout)
            })
            .await;
        }
    }
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "assignment batch followers should collect behind the pause",
        |stats| stats.queue_depth == 2,
    )
    .await;
    pause.release();
    let mut typed_failures = 0usize;
    let mut successful_followers = 0usize;
    for write in writes {
        match expect_catch_up_future_within(write, "assignment caller should resolve")
            .await
            .expect("assignment task should join")
        {
            Err(Error::InvalidInput(message))
                if message == "injected mid-batch assignment failure" =>
            {
                typed_failures += 1;
            }
            Ok(_) => successful_followers += 1,
            Err(other) => panic!("assignment caller received the wrong error: {other}"),
        }
    }
    let assignment_hits =
        faults.hit_count(crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE);
    assert!(
        typed_failures >= 2,
        "the staged request and injected-failure request must both receive the typed error; typed={typed_failures}, successful={successful_followers}, hits={assignment_hits}"
    );
    let journal_before_follow_up = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("journal should remain readable");
    assert_eq!(journal_before_follow_up.len(), successful_followers);
    assert_eq!(
        journal_before_follow_up
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        (1..=u64::try_from(successful_followers).unwrap())
            .map(SequenceNumber)
            .collect::<Vec<_>>(),
        "a mid-batch assignment error must not leave a phantom sequence"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(99))]),
        )
        .await
        .expect("the next batch should publish normally after assignment recovery");
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("runtime should remain loaded"),
        runtime_before,
        "recoverable assignment failure must not evict the tenant"
    );
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("recovered journal should read");
    assert_eq!(journal.len(), successful_followers + 1);
    assert_eq!(
        journal
            .last()
            .expect("follow-up record should exist")
            .sequence,
        SequenceNumber(u64::try_from(successful_followers + 1).unwrap())
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_assignment_failure_discards_staged_suffix_and_keeps_tenant_live() {
    let data_dir = tempdir().expect("serial assignment failure tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_575))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_575)),
        )
        .expect("serial assignment failure engine should create"),
    );
    let tenant_id = TenantId::new("serial-assignment-failure").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial kill-switch arm");
    engine
        .commit_fault_handle_for_testing()
        .inject_error_on_nth_hit(
            crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
            1,
            Error::InvalidInput("injected serial assignment failure".to_string()),
        );

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("the serial assignment fault should fail its caller");
    assert!(
        matches!(error, Error::InvalidInput(ref message) if message == "injected serial assignment failure")
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("journal should remain readable")
            .is_empty(),
        "assignment failure must not create a durable record"
    );
    let (assigned_through, pending) = engine
        .write_log_assignment_for_testing(&tenant_id)
        .expect("serial write-log assignment state should load");
    assert_eq!(assigned_through, SequenceNumber(0));
    assert!(
        pending.is_empty(),
        "serial assignment recovery must remove the phantom staged suffix"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("the next serial batch should assign and commit without panicking");
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("runtime should remain loaded"),
        runtime_before,
        "recoverable serial assignment failure must not evict the tenant"
    );
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("serial journal diagnostics should load");
    assert_eq!(
        stats.publisher_mode,
        crate::tenant::CommitterPipelineMode::Serial
    );
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(1));
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("serial journal should read after recovery");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_lost_ack_evicts_and_replays_without_retryable_error() {
    let data_dir = tempdir().expect("serial recovery fallback tempdir should build");
    let faults = Arc::new(DurableAppendThenRecoveryFaultInjector::default());
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_580))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_580)),
        )
        .expect("serial recovery fallback engine should create"),
    );
    let tenant_id = TenantId::new("serial-recovery-fallback").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("serial runtime identity should load");
    engine.fail_serial_recovery_reads_for_testing(tenant_id.clone(), false, true);
    faults.arm();

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("the lost append acknowledgement must force crash-and-replay");
    assert_eq!(
        error.retryability(),
        nimbus_core::Retryability::Terminal,
        "a write that may have landed must never receive a safe-retry marker"
    );
    assert!(
        matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")),
        "the terminal error must identify the replay policy: {error}"
    );

    let replayed = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("the replacement runtime should replay the landed record");
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].fields.get("index"), Some(&json!(1)));
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("replacement runtime identity should load"),
        runtime_before,
        "an uncertain serial append must evict its runtime"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("the next serial assignment should heal without a write-log assertion");
    let after_heal = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("healed diagnostics should load");
    assert_eq!(after_heal.durable_head, SequenceNumber(2));
    assert_eq!(after_heal.applied_head, SequenceNumber(2));
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("healed durable journal should read");
    assert_eq!(
        journal
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2)]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn quiesce_racing_serial_crash_replay_completes_without_panic() {
    let data_dir = tempdir().expect("serial quiesce race tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_581))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_581)),
        )
        .expect("serial quiesce race engine should create"),
    );
    let tenant_id = TenantId::new("serial-quiesce-race").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");

    let writer = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("serial append should block before ambiguous apply", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let quiesce = tokio::spawn({
        let engine = engine.clone();
        async move { engine.quiesce().await }
    });
    expect_future_within(
        async {
            while !engine.background_shutdown_started() {
                tokio::task::yield_now().await;
            }
        },
        "quiesce should close the background spawn gate",
    )
    .await;
    faults.release_failure();

    let error = expect_catch_up_future_within(
        writer,
        "serial crash-and-replay should resolve during quiesce",
    )
    .await
    .expect("writer task should join without a background-task panic")
    .expect_err("ambiguous serial write should fail for replay");
    assert!(matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")));
    expect_catch_up_future_within(quiesce, "quiesce should await inline eviction completion")
        .await
        .expect("quiesce task should join without a spawn rejection panic");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_crash_replay_rejects_residual_direct_commit_without_running_it() {
    let data_dir = tempdir().expect("serial residual direct tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_581))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_582)),
        )
        .expect("serial residual direct engine should create"),
    );
    let tenant_id = TenantId::new("serial-residual-direct").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");

    let crashing = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("serial batch should block after its durable append", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let residual = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "direct commit should queue behind the crashing serial batch",
        |stats| stats.committer_inbox_depth == 1,
    )
    .await;
    faults.release_failure();

    let crash_error = expect_catch_up_future_within(crashing, "crashing batch should resolve")
        .await
        .expect("crashing batch task should join")
        .expect_err("ambiguous serial write should fail for replay");
    assert!(
        matches!(crash_error, Error::Internal(ref message) if message.contains("crash-and-replay"))
    );
    let residual_error = expect_catch_up_future_within(
        residual,
        "residual direct commit should fail before the old runtime executes it",
    )
    .await
    .expect("direct commit task should join without panicking")
    .expect_err("residual direct commit should receive the typed eviction error");
    assert_eq!(
        residual_error.storage_kind(),
        Some(nimbus_core::StorageErrorKind::Unavailable)
    );
    assert!(
        residual_error
            .to_string()
            .contains("restarting after durable recovery")
    );

    let documents = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("replacement runtime should replay only the durable batch");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("index"), Some(&json!(1)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_lost_ack_with_unreadable_progress_evicts_and_replays_once() {
    let data_dir = tempdir().expect("direct lost-ack tempdir should build");
    let faults =
        ArmedOneShotDirectFaultInjector::new(FaultPoint::StorageCommitAfterVisibilityBeforeReturn);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_582))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_582)),
        )
        .expect("direct lost-ack engine should create"),
    );
    let tenant_id = TenantId::new("direct-lost-ack").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("direct runtime identity should load");
    engine.fail_direct_recovery_read_for_testing(tenant_id.clone());
    faults.arm();

    let error = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(1))]),
            )
        }
    })
    .await
    .expect("direct lost-ack task should join")
    .expect_err("an unknowable landed direct write must require replay");
    assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);
    assert!(matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")));

    let replayed = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("replacement runtime should replay the landed direct write");
    assert_eq!(replayed.len(), 1);
    assert_eq!(replayed[0].fields.get("index"), Some(&json!(1)));
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("replacement runtime identity should load"),
        runtime_before
    );
    let replayed_journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("replayed direct journal should read");
    assert_eq!(replayed_journal.len(), 1);
    assert_eq!(replayed_journal[0].sequence, SequenceNumber(1));

    tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
        }
    })
    .await
    .expect("replacement direct task should join")
    .expect("the replacement runtime should accept the next direct commit");
    let healed_journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("healed direct journal should read");
    assert_eq!(
        healed_journal
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        vec![SequenceNumber(1), SequenceNumber(2)]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn direct_unchanged_head_failure_remains_retryable_and_batch_scoped() {
    let data_dir = tempdir().expect("direct definitive tempdir should build");
    let faults = ArmedOneShotDirectFaultInjector::new(FaultPoint::StorageCommitBeforeVisibility);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_583))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_583)),
        )
        .expect("direct definitive engine should create"),
    );
    let tenant_id = TenantId::new("direct-definitive").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("direct runtime identity should load");
    faults.arm();

    let error = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(1))]),
            )
        }
    })
    .await
    .expect("direct definitive task should join")
    .expect_err("the pre-visibility direct fault should fail only its batch");
    assert_eq!(
        error.retryability(),
        nimbus_core::Retryability::RetryableAfterBackoff
    );
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("definitive failure must keep the runtime loaded"),
        runtime_before
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("definitive direct journal should read")
            .is_empty()
    );
    let (_, pending) = engine
        .write_log_assignment_for_testing(&tenant_id)
        .expect("definitive direct assignment state should load");
    assert!(pending.is_empty());

    tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
        }
    })
    .await
    .expect("follow-up direct task should join")
    .expect("the next direct commit should reuse the discarded suffix safely");
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("follow-up direct journal should read");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_err_err_recovery_evicts_when_append_did_not_land() {
    let data_dir = tempdir().expect("serial unlanded ambiguity tempdir should build");
    let faults = Arc::new(nimbus_storage::ScriptedFaultInjector::new([
        nimbus_storage::FaultOccurrence {
            point: FaultPoint::JournalAppendBeforeDurableFlush,
            visit: 1,
        },
    ]));
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_585))),
            faults,
            Arc::new(nimbus_core::SeededIdSource::new(46_585)),
        )
        .expect("serial unlanded ambiguity engine should create"),
    );
    let tenant_id = TenantId::new("serial-unlanded-ambiguity").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("serial runtime identity should load");
    engine.fail_serial_recovery_reads_for_testing(tenant_id.clone(), true, true);

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("an unknowable serial append must force crash-and-replay");
    assert_eq!(error.retryability(), nimbus_core::Retryability::Terminal);
    assert!(matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")));

    let replayed = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("the replacement runtime should load the durable truth");
    assert!(
        replayed.is_empty(),
        "the replacement runtime must not invent the unlanded record"
    );
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("replacement runtime identity should load"),
        runtime_before,
        "Err/Err recovery must evict even when replay later proves no record landed"
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("reloaded journal should remain readable")
            .is_empty()
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("the replacement runtime should reuse sequence one safely");
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("replacement journal should read");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn assignment_worker_panic_discards_staged_suffix_and_keeps_tenant_live() {
    let data_dir = tempdir().expect("assignment panic tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_625))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_625)),
        )
        .expect("assignment panic engine should create"),
    );
    let tenant_id = TenantId::new("assignment-panic-recovery").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    engine
        .commit_fault_handle_for_testing()
        .inject_panic_on_nth_hit(
            crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
            1,
        );

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("the injected assignment panic should fail its caller");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("assignment batch panicked")),
        "the assignment join error should reach the caller"
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("journal should remain readable")
            .is_empty(),
        "the staged record from a panicked assignment worker must be discarded"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("a follow-up batch should publish after panic recovery");
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("runtime should remain loaded"),
        runtime_before,
        "assignment panics are recoverable and must not evict the tenant"
    );
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("journal should read after recovery");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_assignment_worker_panic_discards_staged_suffix_and_keeps_tenant_live() {
    let data_dir = tempdir().expect("serial assignment panic tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_650))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(46_650)),
        )
        .expect("serial assignment panic engine should create"),
    );
    let tenant_id = TenantId::new("serial-assignment-panic").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial kill-switch arm");
    engine
        .commit_fault_handle_for_testing()
        .inject_panic_on_nth_hit(
            crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE,
            1,
        );

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("the injected serial assignment panic should fail its caller");
    assert!(
        matches!(error, Error::Internal(message) if message.contains("serial committer queued batch panicked")),
        "the serial join error should reach the caller"
    );
    assert!(
        engine
            .read_durable_journal(&tenant_id, SequenceNumber(0))
            .expect("serial journal should remain readable")
            .is_empty(),
        "the staged record from a panicked serial worker must not become durable"
    );
    let (assigned_through, pending) = engine
        .write_log_assignment_for_testing(&tenant_id)
        .expect("serial write-log assignment state should load");
    assert_eq!(assigned_through, SequenceNumber(0));
    assert!(
        pending.is_empty(),
        "serial panic must discard staged suffix"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("a follow-up serial batch should commit after panic recovery");
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("runtime should remain loaded"),
        runtime_before
    );
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("serial journal should read after recovery");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn definitive_publisher_append_error_is_batch_scoped_and_tenant_stays_loaded() {
    let data_dir = tempdir().expect("definitive publisher tempdir should build");
    let faults = Arc::new(nimbus_storage::ScriptedFaultInjector::new([
        nimbus_storage::FaultOccurrence {
            point: FaultPoint::JournalAppendBeforeDurableFlush,
            visit: 1,
        },
    ]));
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_750))),
            faults,
            Arc::new(nimbus_core::SeededIdSource::new(46_750)),
        )
        .expect("definitive publisher engine should create"),
    );
    let tenant_id = TenantId::new("definitive-publisher-failure").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("pre-durability append fault should fail only its batch");
    assert!(
        error
            .to_string()
            .contains("journal_append_before_durable_flush")
    );
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("tenant should still be loaded"),
        runtime_before
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("next batch should publish on the same runtime");
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("publisher diagnostics should load");
    assert_eq!(stats.publisher_fatal_error_count, 1);
    assert_eq!(stats.publisher_ambiguous_error_count, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn definitive_recovery_drains_batches_behind_response_fences() {
    let data_dir = tempdir().expect("fenced recovery tempdir should build");
    let faults = BlockingDefinitiveAppendFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_775))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_775)),
        )
        .expect("fenced recovery engine should create"),
    );
    let tenant_id = TenantId::new("definitive-fenced-recovery").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let first = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("the first append should block before failing", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let fence = engine
        .enqueue_publisher_response_fence_for_testing(&tenant_id)
        .await
        .expect("response fence should enqueue behind the failed batch");
    let second = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(2))]),
                )
                .await
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "the response fence and following assigned batch should both queue",
        |stats| stats.publisher_queue_depth == 2,
    )
    .await;

    faults.release_failure();
    for failed in [first, second] {
        let error = expect_catch_up_future_within(
            failed,
            "every batch in the failed assigned suffix should resolve",
        )
        .await
        .expect("failed insert task should join")
        .expect_err("the definitive suffix should fail");
        assert!(
            matches!(error, Error::InvalidInput(ref message) if message == "injected definitive publisher failure"),
            "fence-stranded batches must retain the original typed error: {error}"
        );
    }
    assert!(matches!(
        fence
            .await
            .expect("response fence should complete")
            .expect("independent deferred response should retain its result"),
        crate::tenant::QueuedMutationResult::Scheduled(false)
    ));

    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("recovered publisher stats should load");
    let (assigned_through, pending) = engine
        .write_log_assignment_for_testing(&tenant_id)
        .expect("recovered write-log state should load");
    assert_eq!(stats.durable_head, SequenceNumber(0));
    assert_eq!(assigned_through, stats.durable_head);
    assert!(
        pending.is_empty(),
        "recovery must remove every phantom staged record"
    );

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(3))]),
        )
        .await
        .expect("a follow-up insert should succeed on the recovered runtime");
    let journal = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("recovered journal should read");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn definitive_recovery_retries_fence_conflict_whose_sequence_was_discarded() {
    let data_dir = tempdir().expect("discarded conflict tempdir should build");
    let faults = BlockingDefinitiveAppendFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_787))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_787)),
        )
        .expect("discarded conflict engine should create"),
    );
    let tenant_id = TenantId::new("discarded-conflict-retry").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let failing = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("value".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("assigned sequence 1 should block before failing", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let fence = engine
        .enqueue_publisher_conflict_response_fence_for_testing(&tenant_id, SequenceNumber(1))
        .await
        .expect("conflict response fence should enqueue");
    let retrying = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            let response = fence.await.map_err(|_| {
                Error::Internal("publisher dropped the conflict response fence".to_string())
            })?;
            match response {
                Err(error)
                    if error.retryability() == nimbus_core::Retryability::Retryable
                        && error.conflicting_sequence().is_none() =>
                {
                    engine
                        .insert_document_async(
                            tenant_id,
                            tasks_table(),
                            serde_json::Map::from_iter([("value".to_string(), json!(2))]),
                        )
                        .await
                }
                Err(error) => Err(error),
                Ok(_) => Err(Error::Internal(
                    "discarded conflict fence unexpectedly succeeded".to_string(),
                )),
            }
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "conflict response fence should queue behind the failing assignment",
        |stats| stats.publisher_queue_depth == 1,
    )
    .await;

    faults.release_failure();
    let failing_error = expect_catch_up_future_within(
        failing,
        "the definitive batch should fail without durable advance",
    )
    .await
    .expect("failing update task should join")
    .expect_err("the injected definitive append should fail");
    assert!(
        matches!(failing_error, Error::InvalidInput(ref message) if message == "injected definitive publisher failure")
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        expect_catch_up_future_within(retrying, "discarded conflict waiter should retry"),
    )
    .await
    .expect("discarded conflict target must not hang the caller")
    .expect("retrying update task should join")
    .expect("retrying update should re-prepare and commit");

    let documents = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("updated document should query");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("value"), Some(&json!(2)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn serial_discard_rewrites_same_batch_conflict_before_retry() {
    let data_dir = tempdir().expect("serial deferred conflict tempdir should build");
    let faults = BlockingDefinitiveAppendFaultInjector::new_on_visit(2);
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_790))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_790)),
        )
        .expect("serial deferred conflict engine should create"),
    );
    let tenant_id = TenantId::new("serial-deferred-conflict").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial arm");
    let document_id = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("value".to_string(), json!("seed"))]),
        )
        .await
        .expect("seed write should consume the healthy first append");

    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();
    let first = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("first".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("first serial update should reach the paused drainer", {
        let pause = pause.clone();
        move |timeout| pause.wait_until_entered(timeout)
    })
    .await;

    // Force the second prepared write through the ordinary CallerWait branch
    // so its dependency resolves against M1's staged B+1 image. Production
    // callers without an inline plan use this exact branch.
    engine.strip_next_inline_reprepare_for_testing(&tenant_id);
    let mut second = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(
                    tenant_id,
                    tasks_table(),
                    document_id,
                    serde_json::Map::from_iter([("second".to_string(), json!(2))]),
                )
                .await
        }
    });
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "second same-document prepare should join M1's serial batch",
        |stats| stats.queue_depth == 1,
    )
    .await;
    pause.release();
    expect_blocking_wait_reaches_state(
        "the same-document batch should block before append failure",
        {
            let faults = faults.clone();
            move |timeout| faults.wait_until_blocked(timeout)
        },
    )
    .await;
    assert_future_stays_pending(
        &mut second,
        "deferred CallerWait must not complete before the append outcome is known",
    )
    .await;

    faults.release_failure();
    let first_error = expect_catch_up_future_within(first, "M1 should receive the append failure")
        .await
        .expect("first update task should join")
        .expect_err("M1 should fail with the definitive append error");
    assert!(
        matches!(first_error, Error::InvalidInput(ref message) if message == "injected definitive publisher failure")
    );
    tokio::time::timeout(
        Duration::from_secs(5),
        expect_catch_up_future_within(second, "rewritten M2 conflict should retry"),
    )
    .await
    .expect("discarded B+1 must not strand M2")
    .expect("second update task should join")
    .expect("M2 should retry from a fresh snapshot and commit");

    let document = engine
        .get_document(&tenant_id, &tasks_table(), document_id)
        .expect("retried document should remain readable");
    assert_eq!(document.fields.get("first"), None);
    assert_eq!(document.fields.get("second"), Some(&json!(2)));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_retry_exhaustion_without_durable_advance_is_batch_scoped() {
    let data_dir = tempdir().expect("retry exhaustion tempdir should build");
    let faults = RetryExhaustionThenHealthyAppendFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_800))),
            faults,
            Arc::new(nimbus_core::SeededIdSource::new(46_800)),
        )
        .expect("retry exhaustion engine should create"),
    );
    let tenant_id = TenantId::new("retry-exhaustion-batch-scope").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");

    let error = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect_err("retry exhaustion should fail the batch");
    assert_eq!(
        error.retryability(),
        nimbus_core::Retryability::RetryableAfterBackoff
    );
    assert_eq!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("tenant should remain loaded"),
        runtime_before
    );
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("publisher should continue after bounded retry exhaustion");
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("publisher diagnostics should load");
    assert_eq!(stats.publisher_transient_error_count, 4);
    assert_eq!(stats.publisher_ambiguous_error_count, 0);
}

#[derive(Default)]
struct OrderedBlockingObserver {
    state: Mutex<(Vec<SequenceNumber>, usize, usize, bool)>,
    entered: Condvar,
    release: Condvar,
}

impl OrderedBlockingObserver {
    fn wait_for_first(&self, timeout: Duration) -> bool {
        let state = self.state.lock().expect("observer state should lock");
        let (state, _) = self
            .entered
            .wait_timeout_while(state, timeout, |state| state.0.is_empty())
            .expect("observer wait should succeed");
        !state.0.is_empty()
    }

    fn release_first(&self) {
        let mut state = self.state.lock().expect("observer state should lock");
        state.3 = true;
        self.release.notify_all();
    }
}

impl crate::CommittedMutationObserver for OrderedBlockingObserver {
    fn committed_mutation_applied(&self, event: crate::CommittedMutationEvent) {
        let mut state = self.state.lock().expect("observer state should lock");
        state.1 += 1;
        state.2 = state.2.max(state.1);
        state.0.push(event.commit.sequence);
        self.entered.notify_all();
        if state.0.len() == 1 {
            while !state.3 {
                state = self
                    .release
                    .wait(state)
                    .expect("observer release should wait");
            }
        }
        state.1 -= 1;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_observers_are_strictly_ordered_and_quiesce_drains_them() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("ordered-observers", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("ordered-test", observer.clone());

    for index in 0..2 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("publisher response must not wait for the observer");
    }
    expect_blocking_wait_reaches_state("first observer callback should block", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    {
        let state = observer.state.lock().expect("observer state should lock");
        assert_eq!(state.0, vec![SequenceNumber(1)]);
        assert_eq!(state.2, 1, "observer callbacks must never overlap");
    }
    let observer_stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("observer queue diagnostics should load");
    assert_eq!(observer_stats.observer_queue_depth, 2);
    assert_eq!(observer_stats.observer_queue_capacity, 4_096);
    assert_eq!(observer_stats.observer_queue_high_watermark, 3_072);
    assert!(!observer_stats.observer_dispatch_poisoned);

    let mut quiesce = tokio::spawn({
        let engine = engine.clone();
        async move { engine.quiesce().await }
    });
    assert_future_stays_pending(
        &mut quiesce,
        "engine quiesce must wait for the ordered observer queue to drain",
    )
    .await;
    observer.release_first();
    expect_future_within(quiesce, "observer queue should drain during quiesce")
        .await
        .expect("quiesce task should join");
    let state = observer.state.lock().expect("observer state should lock");
    assert_eq!(state.0.len(), 2);
    assert!(
        state.0[0] < state.0[1],
        "observer commits must stay ordered"
    );
    assert_eq!(state.2, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn observer_queue_cap_breach_poison_is_nonblocking_and_visible() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("observer-cap-poison").expect("tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 1, 1, 1, 1);
    let created = fixture.create_tenant("observer-cap-poison", Engine::create_tenant);
    assert_eq!(created, tenant_id);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("cap-poison-test", observer.clone());

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("the first commit should enqueue its observer event");
    expect_blocking_wait_reaches_state("first observer callback should hold the queue budget", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    let at_capacity = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("observer queue diagnostics should load at capacity");
    assert_eq!(at_capacity.observer_queue_depth, 1);
    assert_eq!(at_capacity.observer_queue_high_water_warning_count, 1);
    assert_eq!(at_capacity.observer_queue_cap_breach_count, 0);

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(2))]),
        )
        .await
        .expect("observer saturation must never block or fail a durable mutation response");
    let poisoned = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "observer hard-cap breach should poison the dispatcher",
        |stats| stats.observer_dispatch_poisoned,
    )
    .await;
    assert_eq!(poisoned.observer_queue_depth, 1);
    assert_eq!(poisoned.observer_queue_capacity, 1);
    assert_eq!(poisoned.observer_queue_high_watermark, 1);
    assert_eq!(poisoned.observer_queue_high_water_warning_count, 1);
    assert_eq!(poisoned.observer_queue_cap_breach_count, 1);

    observer.release_first();
    engine.quiesce().await;
    let state = observer.state.lock().expect("observer state should lock");
    assert_eq!(
        state.0,
        vec![SequenceNumber(1)],
        "the poison policy must drain accepted work without accepting events beyond the cap"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn observer_queue_capacity_clamps_to_serial_journal_dispatch_max() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("observer-cap-clamp").expect("tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 1, 1, 1, 4);
    let created = fixture.create_tenant("observer-cap-clamp", Engine::create_tenant);
    assert_eq!(created, tenant_id);
    engine
        .set_committer_pipeline_requested_for_testing(&tenant_id, false)
        .expect("test should request the serial journal arm");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("cap-clamp-test", observer.clone());

    run_paused_insert_burst(&engine, &tenant_id, 4).await;
    expect_blocking_wait_reaches_state("full-batch observer dispatch should block", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("clamped observer diagnostics should load");
    assert_eq!(stats.observer_queue_depth, 4);
    assert_eq!(stats.observer_queue_capacity, 4);
    assert_eq!(stats.observer_queue_cap_breach_count, 0);
    assert!(!stats.observer_dispatch_poisoned);

    observer.release_first();
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("full single dispatch should drain without poison");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_catch_up_chunks_observers_to_capacity_in_sequence_order() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("provider-observer-chunks").expect("tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 2, 1, 1, 1);
    let created = fixture.create_tenant("provider-observer-chunks", Engine::create_tenant);
    assert_eq!(created, tenant_id);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    for index in 0..5 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("provider-tail fixture write should commit");
    }
    let records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("provider-tail fixture journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    let expected_sequences = records
        .iter()
        .filter(|record| !record.writes.is_empty())
        .map(|record| record.sequence)
        .collect::<Vec<_>>();
    assert_eq!(expected_sequences.len(), 5);

    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-chunk-order-test", observer.clone());
    let mut catch_up = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &records)
                .await
        }
    });
    expect_blocking_wait_reaches_state("the first provider observer chunk should block", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    assert_future_stays_pending(
        &mut catch_up,
        "provider catch-up must wait for capacity before handing off its next chunk",
    )
    .await;
    let at_capacity = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("provider observer diagnostics should load");
    assert_eq!(at_capacity.observer_queue_depth, 2);
    assert_eq!(at_capacity.observer_queue_capacity, 2);
    assert_eq!(at_capacity.observer_queue_cap_breach_count, 0);
    assert!(!at_capacity.observer_dispatch_poisoned);

    observer.release_first();
    expect_catch_up_future_within(catch_up, "provider observer chunks should drain")
        .await
        .expect("provider catch-up task should join")
        .expect("provider observer catch-up should succeed");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("provider observer tail should flush");
    let state = observer.state.lock().expect("observer state should lock");
    assert_eq!(state.0, expected_sequences);
    assert_eq!(state.2, 1, "provider observer chunks must remain serial");
    drop(state);
    let drained = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("drained provider observer diagnostics should load");
    assert_eq!(drained.observer_queue_depth, 0);
    assert_eq!(drained.observer_queue_cap_breach_count, 0);
    assert!(!drained.observer_dispatch_poisoned);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rapid_provider_catch_up_triggers_coalesce_into_one_tail_reader() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = TenantId::new("provider-observer-coalescing").expect("tenant id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_id.clone(), 1, 1, 1, 1);
    assert_eq!(
        fixture.create_tenant("provider-observer-coalescing", Engine::create_tenant),
        tenant_id
    );
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    const RECORDS: usize = 12;
    for index in 0..RECORDS {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("provider-tail fixture write should commit");
    }
    let records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("provider-tail fixture journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    assert_eq!(records.len(), RECORDS);

    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-coalescing-test", observer.clone());
    assert!(
        engine
            .trigger_provider_catch_up_observers_for_testing(&tenant_id, &records[..2])
            .expect("initial catch-up trigger should start"),
        "the first trigger must own the tenant's sole catch-up task"
    );
    expect_blocking_wait_reaches_state("the first coalesced catch-up callback should block", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;

    for record in &records[2..] {
        assert!(
            !engine
                .trigger_provider_catch_up_observers_for_testing(
                    &tenant_id,
                    std::slice::from_ref(record),
                )
                .expect("later catch-up trigger should coalesce"),
            "a later frontier must not spawn another parked catch-up task"
        );
    }
    assert_eq!(
        engine
            .provider_catch_up_observer_task_count_for_testing(&tenant_id)
            .expect("catch-up task count should load"),
        1,
        "one stalled tenant must retain only one catch-up task regardless of trigger count"
    );

    observer.release_first();
    engine
        .enqueue_provider_catch_up_observers_for_testing(
            &tenant_id,
            std::slice::from_ref(records.last().expect("journal should have a tail")),
        )
        .await
        .expect("coalesced catch-up should reach the latest requested frontier");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_id)
        .await
        .expect("coalesced observer work should drain");
    let state = observer.state.lock().expect("observer state should lock");
    assert_eq!(
        state.0,
        records
            .iter()
            .map(|record| record.sequence)
            .collect::<Vec<_>>(),
        "the sole tail reader must deliver the latest durable frontier exactly once and in order"
    );
    assert_eq!(
        state.2, 1,
        "coalesced observer callbacks must remain serial"
    );
}

#[derive(Default)]
struct TenantSelectiveBlockingObserver {
    blocked_tenant: Mutex<Option<TenantId>>,
    state: Mutex<(
        std::collections::HashMap<TenantId, Vec<SequenceNumber>>,
        bool,
        bool,
    )>,
    entered: Condvar,
    release: Condvar,
}

impl TenantSelectiveBlockingObserver {
    fn block_tenant(&self, tenant_id: TenantId) {
        *self
            .blocked_tenant
            .lock()
            .expect("blocked tenant lock should acquire") = Some(tenant_id);
    }

    fn wait_until_blocked(&self, timeout: Duration) -> bool {
        let state = self
            .state
            .lock()
            .expect("selective observer state should lock");
        let (state, _) = self
            .entered
            .wait_timeout_while(state, timeout, |state| !state.1)
            .expect("selective observer entered wait should succeed");
        state.1
    }

    fn release_blocked_tenant(&self) {
        let mut state = self
            .state
            .lock()
            .expect("selective observer state should lock");
        state.2 = true;
        self.release.notify_all();
    }

    fn sequences(&self, tenant_id: &TenantId) -> Vec<SequenceNumber> {
        self.state
            .lock()
            .expect("selective observer state should lock")
            .0
            .get(tenant_id)
            .cloned()
            .unwrap_or_default()
    }
}

impl crate::CommittedMutationObserver for TenantSelectiveBlockingObserver {
    fn committed_mutation_applied(&self, event: crate::CommittedMutationEvent) {
        let should_block = self
            .blocked_tenant
            .lock()
            .expect("blocked tenant lock should acquire")
            .as_ref()
            == Some(&event.tenant_id);
        let mut state = self
            .state
            .lock()
            .expect("selective observer state should lock");
        let first_for_tenant = state
            .0
            .entry(event.tenant_id.clone())
            .or_default()
            .is_empty();
        state
            .0
            .get_mut(&event.tenant_id)
            .expect("tenant observer sequence should exist")
            .push(event.commit.sequence);
        if should_block && first_for_tenant {
            state.1 = true;
            self.entered.notify_all();
            while !state.2 {
                state = self
                    .release
                    .wait(state)
                    .expect("selective observer release wait should succeed");
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn poisoned_provider_catch_up_tenant_does_not_block_fresh_tenant() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = fixture.create_tenant("provider-poison-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("provider-poison-b", Engine::create_tenant);
    let scheduled_tenant =
        fixture.create_tenant("provider-poison-scheduled", Engine::create_tenant);
    for tenant_id in [&tenant_a, &tenant_b] {
        engine
            .shutdown_trigger_candidates_for_testing(tenant_id)
            .expect("trigger cursor should not add unrelated records");
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(1))]),
            )
            .await
            .expect("provider catch-up fixture write should commit");
    }
    let records_a = engine
        .read_durable_journal(&tenant_a, SequenceNumber(0))
        .expect("tenant A journal should read");
    let records_b = engine
        .read_durable_journal(&tenant_b, SequenceNumber(0))
        .expect("tenant B journal should read");
    let observer = Arc::new(TenantSelectiveBlockingObserver::default());
    engine.install_committed_mutation_observer("provider-poison-isolation-test", observer.clone());
    engine
        .schedule_mutation_async(
            scheduled_tenant.clone(),
            ScheduleRequest {
                run_after_ms: 60_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: serde_json::Map::from_iter([("index".to_string(), json!(99))]),
                },
            },
        )
        .await
        .expect("scheduled fixture work should persist");
    engine
        .evict_runtime_without_deleting_for_testing(&scheduled_tenant)
        .await
        .expect("scheduled tenant should unload without deleting durable work");
    assert!(!engine.loaded_tenant_ids().contains(&scheduled_tenant));
    engine
        .poison_committed_mutation_observers_for_testing(&tenant_a)
        .expect("tenant A observer dispatcher should poison");

    let error = engine
        .enqueue_provider_catch_up_observers_for_testing(&tenant_a, &records_a)
        .await
        .expect_err("poisoned tenant A must refuse catch-up observers");
    assert!(error.to_string().contains("poisoned"));
    let stats_a = engine
        .mutation_journal_stats_for_testing(&tenant_a)
        .expect("tenant A observer diagnostics should load");
    assert_eq!(stats_a.observer_catch_up_enqueue_failure_count, 1);

    tokio::time::timeout(
        Duration::from_secs(2),
        engine.enqueue_provider_catch_up_observers_for_testing(&tenant_b, &records_b),
    )
    .await
    .expect("tenant B catch-up must not wait behind poisoned tenant A")
    .expect("tenant B catch-up should succeed");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_b)
        .await
        .expect("tenant B observers should flush");
    assert_eq!(observer.sequences(&tenant_b), vec![SequenceNumber(1)]);
    let stats_b = engine
        .mutation_journal_stats_for_testing(&tenant_b)
        .expect("tenant B observer diagnostics should load");
    assert_eq!(stats_b.observer_catch_up_enqueue_failure_count, 0);
    assert!(!stats_b.observer_dispatch_poisoned);

    engine
        .catch_up_provider_after_listener_attach_for_testing()
        .await
        .expect("listener-attach catch-up should continue into scheduled-work loading");
    assert!(
        engine.loaded_tenant_ids().contains(&scheduled_tenant),
        "poisoned tenant A must not prevent an unloaded scheduled tenant from loading"
    );
    assert_eq!(
        engine
            .list_scheduled_jobs(&scheduled_tenant)
            .expect("reloaded scheduled work should remain queryable")
            .len(),
        1
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_attach_contains_mid_eviction_tenant_and_loads_remaining_work() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = fixture.create_tenant("provider-evicting-a", Engine::create_tenant);
    let tenant_b = fixture.create_tenant("provider-refresh-b", Engine::create_tenant);
    let scheduled_tenant =
        fixture.create_tenant("provider-refresh-scheduled", Engine::create_tenant);
    for tenant_id in [&tenant_a, &tenant_b] {
        engine
            .shutdown_trigger_candidates_for_testing(tenant_id)
            .expect("trigger cursor should not add unrelated records");
    }

    let document_id = engine
        .insert_document_async(
            tenant_b.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(0))]),
        )
        .await
        .expect("tenant B seed should commit");
    engine
        .schedule_mutation_async(
            scheduled_tenant.clone(),
            ScheduleRequest {
                run_after_ms: 60_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: serde_json::Map::from_iter([("index".to_string(), json!(99))]),
                },
            },
        )
        .await
        .expect("scheduled fixture work should persist");
    engine
        .evict_runtime_without_deleting_for_testing(&scheduled_tenant)
        .await
        .expect("scheduled tenant should unload without deleting durable work");

    let observer = Arc::new(TenantSelectiveBlockingObserver::default());
    engine.install_committed_mutation_observer(
        "provider-whole-body-isolation-test",
        observer.clone(),
    );
    let pending_b = engine
        .stage_assigned_pending_update_for_testing(
            &tenant_b,
            &tasks_table(),
            &document_id,
            "index",
            json!(1),
        )
        .expect("tenant B provider-style update should stage");
    engine
        .apply_assigned_pending_record_without_publish_for_testing(&tenant_b, &pending_b)
        .expect("tenant B durable state should advance without its runtime watermark");
    let evicting_runtime = engine
        .begin_runtime_eviction_for_testing(&tenant_a)
        .expect("tenant A should enter the mid-eviction state");

    engine
        .catch_up_provider_after_listener_attach_for_testing()
        .await
        .expect("tenant A failure must not abort the remaining attach catch-up");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if observer.sequences(&tenant_b) == vec![pending_b.sequence] {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("tenant B observer catch-up should reach its durable tail");
    assert!(
        engine
            .provider_catch_up_failure_count_for_testing(&tenant_a)
            .expect("tenant A failure count should remain inspectable during eviction")
            >= 1,
        "the contained tenant failure must be visible in per-tenant diagnostics"
    );
    assert!(
        engine.loaded_tenant_ids().contains(&scheduled_tenant),
        "tenant A must not prevent a later scheduled-work tenant from loading"
    );
    assert_eq!(
        engine
            .list_scheduled_jobs(&scheduled_tenant)
            .expect("reloaded scheduled work should remain queryable")
            .len(),
        1
    );
    drop(evicting_runtime);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn busy_provider_catch_up_tenant_does_not_head_of_line_block_other_tenant() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_a = TenantId::new("provider-busy-a").expect("tenant A id should build");
    let tenant_b = TenantId::new("provider-busy-b").expect("tenant B id should build");
    crate::tenant::configure_observer_limits_for_testing(tenant_a.clone(), 2, 1, 1, 1);
    crate::tenant::configure_observer_limits_for_testing(tenant_b.clone(), 2, 1, 1, 1);
    assert_eq!(
        fixture.create_tenant("provider-busy-a", Engine::create_tenant),
        tenant_a
    );
    assert_eq!(
        fixture.create_tenant("provider-busy-b", Engine::create_tenant),
        tenant_b
    );
    for tenant_id in [&tenant_a, &tenant_b] {
        engine
            .shutdown_trigger_candidates_for_testing(tenant_id)
            .expect("trigger cursor should not add unrelated records");
    }
    for index in 0..5 {
        engine
            .insert_document_async(
                tenant_a.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("tenant A provider tail should commit");
    }
    engine
        .insert_document_async(
            tenant_b.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(10))]),
        )
        .await
        .expect("tenant B provider tail should commit");
    let records_a = engine
        .read_durable_journal(&tenant_a, SequenceNumber(0))
        .expect("tenant A journal should read");
    let expected_a = records_a
        .iter()
        .filter(|record| !record.writes.is_empty())
        .map(|record| record.sequence)
        .collect::<Vec<_>>();
    let records_b = engine
        .read_durable_journal(&tenant_b, SequenceNumber(0))
        .expect("tenant B journal should read");
    let observer = Arc::new(TenantSelectiveBlockingObserver::default());
    observer.block_tenant(tenant_a.clone());
    engine.install_committed_mutation_observer("provider-busy-isolation-test", observer.clone());

    let mut catch_up_a = tokio::spawn({
        let engine = engine.clone();
        let tenant_a = tenant_a.clone();
        async move {
            engine
                .enqueue_provider_catch_up_observers_for_testing(&tenant_a, &records_a)
                .await
        }
    });
    expect_blocking_wait_reaches_state("tenant A observer should sustain a full backlog", {
        let observer = observer.clone();
        move |timeout| observer.wait_until_blocked(timeout)
    })
    .await;
    assert_future_stays_pending(
        &mut catch_up_a,
        "tenant A catch-up should wait for its own partial-capacity chunks",
    )
    .await;

    tokio::time::timeout(
        Duration::from_secs(2),
        engine.enqueue_provider_catch_up_observers_for_testing(&tenant_b, &records_b),
    )
    .await
    .expect("tenant B hint must stay responsive while tenant A is saturated")
    .expect("tenant B catch-up should enqueue");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_b)
        .await
        .expect("tenant B observer should drain");
    assert_eq!(observer.sequences(&tenant_b), vec![SequenceNumber(1)]);

    observer.release_blocked_tenant();
    expect_catch_up_future_within(catch_up_a, "tenant A chunks should eventually drain")
        .await
        .expect("tenant A catch-up task should join")
        .expect("tenant A catch-up should succeed");
    engine
        .flush_committed_mutation_observers_for_testing(&tenant_a)
        .await
        .expect("tenant A observer should flush");
    assert_eq!(
        observer.sequences(&tenant_a),
        expected_a,
        "tenant A catch-up chunks must preserve commit order"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_catch_up_spawn_rejection_releases_task_state_after_quiesce() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("provider-catch-up-quiesce", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("provider catch-up fixture write should commit");
    let records = engine
        .read_durable_journal(&tenant_id, SequenceNumber(0))
        .expect("provider catch-up fixture journal should read")
        .into_iter()
        .filter(|record| !record.writes.is_empty())
        .collect::<Vec<_>>();
    engine.install_committed_mutation_observer(
        "provider-catch-up-quiesce-test",
        Arc::new(TenantSelectiveBlockingObserver::default()),
    );
    engine.quiesce().await;

    for _ in 0..2 {
        let error = engine
            .enqueue_provider_catch_up_observers_for_testing(&tenant_id, &records)
            .await
            .expect_err("quiesce must reject new provider catch-up tasks without panicking");
        assert!(matches!(error, Error::ResourceExhausted(_)));
        assert_eq!(
            engine
                .provider_catch_up_observer_task_count_for_testing(&tenant_id)
                .expect("catch-up task count should load"),
            0,
            "a rejected spawn must release the tenant's sole-task state"
        );
    }
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("catch-up spawn rejection diagnostics should load");
    assert_eq!(stats.observer_catch_up_enqueue_failure_count, 2);
}

#[derive(Default)]
struct BlockingPanickingObserver {
    state: Mutex<(bool, bool)>,
    entered: Condvar,
    release: Condvar,
}

impl BlockingPanickingObserver {
    fn wait_until_entered(&self, timeout: Duration) -> bool {
        let state = self.state.lock().expect("observer state should lock");
        let (state, _) = self
            .entered
            .wait_timeout_while(state, timeout, |state| !state.0)
            .expect("observer wait should succeed");
        state.0
    }

    fn release_to_panic(&self) {
        let mut state = self.state.lock().expect("observer state should lock");
        state.1 = true;
        self.release.notify_all();
    }
}

impl crate::CommittedMutationObserver for BlockingPanickingObserver {
    fn committed_mutation_applied(&self, _event: crate::CommittedMutationEvent) {
        let mut state = self.state.lock().expect("observer state should lock");
        state.0 = true;
        self.entered.notify_all();
        while !state.1 {
            state = self
                .release
                .wait(state)
                .expect("observer release should wait");
        }
        drop(state);
        panic!("injected committed observer panic");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn panicking_observer_poison_drops_queued_depth_without_phantom_backlog() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("observer-panic", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(BlockingPanickingObserver::default());
    engine.install_committed_mutation_observer("panic-test", observer.clone());

    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .await
        .expect("first insert should dispatch to the blocking observer");
    expect_blocking_wait_reaches_state("first observer dispatch should block before panicking", {
        let observer = observer.clone();
        move |timeout| observer.wait_until_entered(timeout)
    })
    .await;
    for index in 2..=3 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("later inserts should queue behind the blocked observer dispatch");
    }
    let queued = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("queued observer diagnostics should load");
    assert_eq!(queued.observer_queue_depth, 3);
    assert!(!queued.observer_dispatch_poisoned);

    observer.release_to_panic();
    let stats = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "panicking observer should poison with an accurate drained depth",
        |stats| stats.observer_dispatch_poisoned && stats.observer_queue_depth == 0,
    )
    .await;
    assert_eq!(stats.observer_queue_cap_breach_count, 0);
    tokio::time::timeout(Duration::from_secs(5), engine.quiesce())
        .await
        .expect("observer panic must not strand engine quiesce");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn committed_observers_preserve_order_across_execution_unit_and_direct_handoffs() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("mixed-ordered-observers", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let observer = Arc::new(OrderedBlockingObserver::default());
    engine.install_committed_mutation_observer("mixed-ordered-test", observer.clone());

    let first = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("first execution unit should begin");
    first
        .insert_document(
            tasks_table(),
            serde_json::Map::from_iter([("index".to_string(), json!(1))]),
        )
        .expect("first insert should stage");
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT);
    let first_commit = tokio::task::spawn_blocking(move || first.commit());
    let wait_faults = faults.clone();
    assert!(
        tokio::task::spawn_blocking(move || {
            wait_faults.wait_until_entered(
                crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT,
                Duration::from_secs(5),
            )
        })
        .await
        .expect("commit pause waiter should join"),
        "first commit should pause after its serialized observer enqueue"
    );

    tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(2))]),
            )
        }
    })
    .await
    .expect("second direct commit task should join")
    .expect("second direct commit should succeed");

    expect_blocking_wait_reaches_state("first observer callback should arrive", {
        let observer = observer.clone();
        move |timeout| observer.wait_for_first(timeout)
    })
    .await;
    {
        let state = observer.state.lock().expect("observer state should lock");
        assert_eq!(
            state.0,
            vec![SequenceNumber(1)],
            "the later direct caller must not enqueue ahead of the earlier execution-unit commit"
        );
    }

    observer.release_first();
    faults.release(crate::engine::commit_fault_labels::POST_PUBLISH_PRE_FANOUT);
    first_commit
        .await
        .expect("first execution-unit task should join")
        .expect("first execution-unit commit should succeed")
        .expect("first execution-unit insert should commit");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ambiguous_publisher_eviction_drains_then_reopens_a_distinct_runtime() {
    let data_dir = tempdir().expect("guarded eviction tempdir should build");
    let faults = Arc::new(nimbus_storage::ScriptedFaultInjector::new([
        nimbus_storage::FaultOccurrence {
            point: FaultPoint::JournalDurableAppendBeforeApply,
            visit: 1,
        },
    ]));
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_900))),
            faults,
        )
        .expect("guarded eviction engine should create"),
    );
    let tenant_id = TenantId::new("guarded-ambiguous-eviction").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let runtime_before = engine
        .tenant_runtime_identity_for_testing(&tenant_id)
        .expect("runtime identity should load");
    let eviction_blocker = engine
        .tenant_operation_guard_for_testing(&tenant_id)
        .expect("test operation should hold eviction before load-gate acquisition");

    timeout(
        Duration::from_secs(5),
        engine.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("recover"))]),
        ),
    )
    .await
    .expect("ambiguous writer should resolve within the eviction timeout")
    .expect_err("post-durable fault should require guarded eviction");
    let mut reload = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    let mut reload_writer = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("after-recovery"))]),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut reload,
        "reload should wait for the failed runtime to finish eviction",
    )
    .await;
    assert_future_stays_pending(
        &mut reload_writer,
        "writer reload should wait for the failed runtime to finish eviction",
    )
    .await;
    drop(eviction_blocker);
    let documents = expect_catch_up_future_within(
        reload,
        "tenant reload should finish after the old operation guard drops",
    )
    .await
    .expect("tenant reload task should join")
    .expect("reopen should wait for the old store handle to drain and recover");
    assert!(
        documents.iter().any(|document| {
            document
                .fields
                .get("title")
                .is_some_and(|title| title == "recover")
        }),
        "the reload must retain the durably committed document"
    );
    expect_catch_up_future_within(
        reload_writer,
        "writer should transparently reopen after eviction",
    )
    .await
    .expect("writer reload task should join")
    .expect("writer should succeed after the failed runtime finishes eviction");
    assert_eq!(
        engine
            .query_documents_async(tenant_id.clone(), query_for("tasks"))
            .await
            .expect("reopened tenant should remain queryable")
            .len(),
        2
    );
    assert_ne!(
        engine
            .tenant_runtime_identity_for_testing(&tenant_id)
            .expect("reopened runtime identity should load"),
        runtime_before
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn applied_sequence_waiter_returns_retryable_error_when_runtime_is_evicted() {
    let data_dir = tempdir().expect("applied-wait eviction tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_925))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_925)),
        )
        .expect("applied-wait eviction engine should create"),
    );
    let tenant_id = TenantId::new("applied-wait-eviction").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let writer = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(1))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("durable append should pause before ambiguous apply", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let mut waiter = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .count_table_documents_async(tenant_id, tasks_table())
                .await
        }
    });
    assert_future_stays_pending(
        &mut waiter,
        "table-count waiter should park behind the durable-but-unapplied sequence",
    )
    .await;

    faults.release_failure();
    expect_catch_up_future_within(writer, "ambiguous writer should enter eviction")
        .await
        .expect("writer task should join")
        .expect_err("ambiguous writer should fail for crash-and-replay");
    let wait_error = tokio::time::timeout(
        Duration::from_secs(5),
        expect_catch_up_future_within(waiter, "applied waiter should wake on eviction"),
    )
    .await
    .expect("eviction must wake the applied waiter within the bound")
    .expect("waiter task should join")
    .expect_err("the dead runtime cannot satisfy its applied target");
    assert_eq!(
        wait_error.storage_kind(),
        Some(nimbus_core::StorageErrorKind::Unavailable)
    );
    assert_eq!(
        wait_error.retryability(),
        nimbus_core::Retryability::RetryableAfterBackoff
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ambiguous_eviction_and_explicit_delete_complete_without_lock_inversion() {
    let data_dir = tempdir().expect("delete race tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_925))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_925)),
        )
        .expect("delete race engine should create"),
    );
    let tenant_id = TenantId::new("ambiguous-delete-race").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let writer = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("delete-race"))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("publisher should block after durable append", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let mut delete = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move { engine.delete_tenant_async(tenant_id).await }
    });
    assert_future_stays_pending(
        &mut delete,
        "explicit deletion should wait for the publisher-owned operation guard",
    )
    .await;
    faults.release_failure();

    let writer_error = expect_catch_up_future_within(
        writer,
        "ambiguous writer should resolve while deletion owns the load gate",
    )
    .await
    .expect("writer task should join")
    .expect_err("ambiguous writer should fail for replay");
    assert!(
        matches!(writer_error, Error::Internal(ref message) if message.contains("crash-and-replay"))
    );
    expect_catch_up_future_within(
        delete,
        "explicit deletion should complete after guard drain",
    )
    .await
    .expect("delete task should join")
    .expect("explicit deletion should succeed");
    assert!(matches!(
        engine
            .query_documents_async(tenant_id, query_for("tasks"))
            .await,
        Err(Error::TenantNotFound(_))
    ));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ambiguous_eviction_fails_and_drains_stranded_mutation_queues_before_reload() {
    let data_dir = tempdir().expect("stranded eviction tempdir should build");
    let faults = BlockingAmbiguousApplyFaultInjector::new();
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(46_950))),
            faults.clone(),
            Arc::new(nimbus_core::SeededIdSource::new(46_950)),
        )
        .expect("stranded eviction engine should create"),
    );
    let tenant_id = TenantId::new("ambiguous-stranded-queues").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let spawn_insert = |index: usize| {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
        })
    };
    let durable = spawn_insert(1);
    expect_blocking_wait_reaches_state("first publisher batch should block after append", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_blocked(timeout)
    })
    .await;

    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause should load");
    pause.arm();
    let journal_stranded = spawn_insert(2);
    expect_blocking_wait_reaches_state("second request should strand in the journal queue", {
        let pause = pause.clone();
        move |timeout| pause.wait_until_entered(timeout)
    })
    .await;
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "second request should remain in the paused journal queue",
        |stats| stats.queue_depth == 1,
    )
    .await;

    let admission_stranded = spawn_insert(3);
    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "third request should remain in admission behind the paused actor",
        |stats| stats.queue_depth == 1,
    )
    .await;
    faults.release_failure();

    for failed in [durable, journal_stranded, admission_stranded] {
        let error = expect_catch_up_future_within(
            failed,
            "eviction should resolve every accepted mutation request",
        )
        .await
        .expect("evicted mutation task should join")
        .expect_err("evicted mutation should receive the typed replay error");
        assert!(
            matches!(error, Error::Internal(ref message) if message.contains("crash-and-replay")),
            "stranded callers should retain the eviction error: {error}"
        );
    }
    pause.release();

    let documents = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("the tenant should reload and recover its durable prefix");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("index"), Some(&json!(1)));
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("reloaded journal stats should load");
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_torn_tail_recovery_replays_exactly_one_contiguous_prefix() {
    let data_dir = tempdir().expect("torn-tail publisher tempdir should build");
    let faults = Arc::new(nimbus_storage::ScriptedFaultInjector::new([
        nimbus_storage::FaultOccurrence {
            point: FaultPoint::JournalDurableAppendBeforeApply,
            visit: 1,
        },
    ]));
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(45_000))),
            faults,
            Arc::new(nimbus_core::SeededIdSource::new(45_000)),
        )
        .expect("torn-tail publisher engine should create"),
    );
    let tenant_id = TenantId::new("publisher-torn-tail").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("torn-tail publisher tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let error = timeout(
        Duration::from_secs(5),
        engine.insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("replay-me"))]),
        ),
    )
    .await
    .expect("torn-tail writer should resolve within the eviction timeout")
    .expect_err("post-append fault must force crash-and-replay");
    assert!(
        error.to_string().contains("crash-and-replay"),
        "ambiguous publisher failure should identify replay recovery: {error}"
    );

    // Failure completion deliberately precedes load-gate acquisition. The
    // access waits for eviction completion, then opens a fresh runtime and
    // applies the durable tail.
    let documents = timeout(
        Duration::from_secs(5),
        engine.query_documents_async(tenant_id.clone(), query_for("tasks")),
    )
    .await
    .expect("torn-tail reload should resolve within the eviction timeout")
    .expect("next access should recover the durable torn tail");
    assert_eq!(documents.len(), 1);
    assert_eq!(documents[0].fields.get("title"), Some(&json!("replay-me")));
    let journal = engine
        .read_durable_journal_async(tenant_id.clone(), SequenceNumber(0))
        .await
        .expect("recovered durable prefix should read");
    assert_eq!(journal.len(), 1);
    assert_eq!(journal[0].sequence, SequenceNumber(1));
    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("recovered journal stats should load");
    assert_eq!(stats.durable_head, SequenceNumber(1));
    assert_eq!(stats.applied_head, SequenceNumber(1));
    assert_eq!(stats.apply_lag, 0);
    assert_eq!(stats.publisher_ambiguous_error_count, 1);
}

#[derive(Clone, Copy)]
enum KillSwitchWorkloadMode {
    Pipeline,
    Serial,
    FlipMidLoad,
}

async fn run_kill_switch_workload(mode: KillSwitchWorkloadMode) -> (Vec<u8>, Vec<u8>) {
    let data_dir = tempdir().expect("kill-switch engine tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(47_000))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(47_000)),
        )
        .expect("kill-switch engine should create"),
    );
    let tenant_id = TenantId::new("kill-switch").expect("tenant id should build");
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("kill-switch tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("background cursor commits should be disabled for byte comparison");
    engine
        .set_prepared_table_id_for_testing(
            &tenant_id,
            &tasks_table(),
            nimbus_core::TableId::try_from("kill-switch-table".to_string())
                .expect("fixed table id should be valid"),
        )
        .expect("kill-switch table identity should be deterministic");

    if matches!(mode, KillSwitchWorkloadMode::Serial) {
        engine
            .set_committer_pipeline_requested_for_testing(&tenant_id, false)
            .expect("serial mode should be requested");
    }

    const WRITE_COUNT: usize = 6;
    if matches!(mode, KillSwitchWorkloadMode::FlipMidLoad) {
        let faults = engine.commit_fault_handle_for_testing();
        let pause_label = crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH;
        faults.arm(pause_label);
        let mut inserts = vec![tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        serde_json::Map::from_iter([("index".to_string(), json!(0))]),
                    )
                    .await
            }
        })];
        expect_blocking_wait_reaches_state("flip workload publisher should pause after append", {
            let faults = faults.clone();
            move |timeout| faults.wait_until_entered(pause_label, timeout)
        })
        .await;

        for index in 1..WRITE_COUNT - 1 {
            inserts.push(tokio::spawn({
                let engine = engine.clone();
                let tenant_id = tenant_id.clone();
                async move {
                    engine
                        .insert_document_async(
                            tenant_id,
                            tasks_table(),
                            serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                        )
                        .await
                }
            }));
            wait_for_mutation_journal_stats(
                &engine,
                &tenant_id,
                "pre-flip assignment should enter the publisher queue",
                |stats| stats.publisher_queue_depth == index,
            )
            .await;
        }

        engine
            .set_committer_pipeline_requested_for_testing(&tenant_id, false)
            .expect("mid-load serial mode should be requested");
        inserts.push(tokio::spawn({
            let engine = engine.clone();
            let tenant_id = tenant_id.clone();
            async move {
                engine
                    .insert_document_async(
                        tenant_id,
                        tasks_table(),
                        serde_json::Map::from_iter([("index".to_string(), json!(WRITE_COUNT - 1))]),
                    )
                    .await
            }
        }));
        wait_for_mutation_journal_stats(
            &engine,
            &tenant_id,
            "kill switch should expose its draining state while prior publish is paused",
            |stats| stats.publisher_mode == crate::tenant::CommitterPipelineMode::DrainingToSerial,
        )
        .await;
        faults.release(pause_label);
        for insert in inserts {
            expect_catch_up_future_within(insert, "kill-switch mutation should complete")
                .await
                .expect("kill-switch insert task should join")
                .expect("kill-switch insert should succeed");
        }
    } else {
        for index in 0..WRITE_COUNT {
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([("index".to_string(), json!(index))]),
                )
                .await
                .expect("kill-switch baseline insert should succeed");
        }
    }

    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("kill-switch diagnostics should load");
    match mode {
        KillSwitchWorkloadMode::Pipeline => {
            assert_eq!(
                stats.publisher_mode,
                crate::tenant::CommitterPipelineMode::Pipeline
            );
            assert_eq!(stats.publisher_mode_transition_count, 0);
        }
        KillSwitchWorkloadMode::Serial | KillSwitchWorkloadMode::FlipMidLoad => {
            assert_eq!(
                stats.publisher_mode,
                crate::tenant::CommitterPipelineMode::Serial
            );
            assert_eq!(stats.publisher_mode_transition_count, 1);
        }
    }

    let mut documents = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("kill-switch documents should query");
    documents.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    let document_bytes = serde_json::to_vec(&documents).expect("documents should serialize");
    let journal_bytes = serde_json::to_vec(
        &engine
            .read_durable_journal_async(tenant_id, SequenceNumber(0))
            .await
            .expect("kill-switch durable journal should read"),
    )
    .expect("durable journal should serialize");
    (document_bytes, journal_bytes)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kill_switch_mid_load_produces_identical_state() {
    let pipeline = run_kill_switch_workload(KillSwitchWorkloadMode::Pipeline).await;
    let serial = run_kill_switch_workload(KillSwitchWorkloadMode::Serial).await;
    let flipped = run_kill_switch_workload(KillSwitchWorkloadMode::FlipMidLoad).await;

    assert_eq!(pipeline.0, serial.0, "pipeline and serial documents differ");
    assert_eq!(pipeline.0, flipped.0, "mid-flip documents differ");
    assert_eq!(
        pipeline.1, serial.1,
        "pipeline and serial durable journal prefixes differ byte-for-byte"
    );
    assert_eq!(
        pipeline.1, flipped.1,
        "mid-flip durable journal prefix differs byte-for-byte"
    );
}

#[tokio::test]
async fn mutation_admission_gate_buffers_while_journal_is_paused_without_losing_in_flight_response()
{
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .set_mutation_journal_queue_capacity_for_testing(&tenant_id, 1)
        .expect("queue capacity should be configurable for tests");
    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let first_insert = {
        let engine = Arc::clone(&engine);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("queued-first"))]),
                )
                .await
        })
    };

    expect_blocking_wait_reaches_state(
        "journal worker should pause before draining the queued request",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;

    let blocked_stats = engine
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
        let engine = Arc::clone(&engine);
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("queued-second"))]),
                )
                .await
        }
    });

    wait_for_mutation_admission_stats(
        &engine,
        &tenant_id,
        "second mutation should remain buffered at the admission gate",
        |stats| stats.queue_depth == 1,
    )
    .await;

    assert_future_stays_pending(
        &mut second_insert,
        "second mutation should stay pending while the journal worker is paused",
    )
    .await;

    let buffered_stats = engine
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

    let first_id = expect_future_within(
        first_insert,
        "first mutation should resolve after the pause is released",
    )
    .await
    .expect("first mutation task should join successfully")
    .expect("first mutation should succeed");
    let second_id = expect_future_within(
        second_insert,
        "second mutation should resolve after the journal drains",
    )
    .await
    .expect("second mutation task should join successfully")
    .expect("second mutation should succeed");

    let visible = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("final query should succeed after the buffered mutation drains");
    assert_eq!(visible.len(), 2);
    assert_eq!(
        visible
            .into_iter()
            .map(|document| document.id.clone())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([first_id, second_id])
    );

    let final_stats = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "mutation journal worker to go idle after the buffered queue drains",
        |stats| !stats.worker_running,
    )
    .await;
    assert!(final_stats.durable_head.0 >= 2);
    assert_eq!(final_stats.applied_head, final_stats.durable_head);
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
    assert_eq!(
        durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        2
    );

    let final_admission_stats = engine
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the gate drains");
    assert_eq!(final_admission_stats.queue_depth, 0);
    assert_eq!(final_admission_stats.shed_count, 0);
    assert_eq!(final_admission_stats.queue_rejection_count, 0);
}

#[tokio::test]
async fn mutation_journal_never_expires_admitted_work() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("demo", Engine::create_tenant);

    engine
        .set_mutation_admission_codel_for_testing(
            &tenant_id,
            Duration::from_millis(5),
            Duration::from_millis(10),
        )
        .expect("admission CoDel should be configurable for tests");
    let pause = engine
        .mutation_journal_pause_handle_for_testing(&tenant_id)
        .expect("journal pause handle should load");
    pause.arm();

    let mut admitted_insert = {
        let engine = Arc::clone(&engine);
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("admitted"))]),
                )
                .await
        })
    };

    expect_blocking_wait_reaches_state(
        "journal worker should pause after admitting the mutation to the journal queue",
        {
            let pause = pause.clone();
            move |timeout| pause.wait_until_entered(timeout)
        },
    )
    .await;

    assert_future_stays_pending(
        &mut admitted_insert,
        "admitted mutation should remain pending while the journal worker pause is armed",
    )
    .await;
    pause.release();

    let document_id = expect_future_within(
        admitted_insert,
        "admitted mutation should resolve after the pause is released",
    )
    .await
    .expect("admitted mutation task should join successfully")
    .expect("admitted mutation should still succeed");

    let visible = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("final query should succeed after the admitted mutation drains");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id, document_id);

    let admission_stats = engine
        .mutation_admission_stats_for_testing(&tenant_id)
        .expect("admission stats should load after the queue drains");
    assert_eq!(admission_stats.queue_depth, 0);
    assert_eq!(admission_stats.shed_count, 0);
    assert_eq!(admission_stats.queue_rejection_count, 0);

    let journal_stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("journal stats should load after the admitted mutation commits");
    assert!(journal_stats.durable_head.0 >= 1);
    assert_eq!(journal_stats.applied_head, journal_stats.durable_head);
    assert_eq!(journal_stats.apply_lag, 0);
    assert_eq!(journal_stats.queue_depth, 0);
    assert_eq!(
        durable_journal_commits(engine.as_ref(), &tenant_id, SequenceNumber(0)).len(),
        1
    );
}

#[tokio::test]
async fn queued_mutation_response_still_resolves_after_blocked_read_catches_up() {
    let (_data_dir, engine, tenant_id, faults) = new_faulted_engine(42_500);

    let mut first_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("first"))]),
                )
                .await
        }
    });

    expect_future_within(
        faults.wait_until_entered(),
        "journal worker should block after durable append",
    )
    .await;
    assert_future_stays_pending(
        &mut first_insert,
        "first mutation should remain pending while apply is blocked",
    )
    .await;

    let mut blocked_query = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    assert_future_stays_pending(
        &mut blocked_query,
        "query should remain pending while the first durable write is not yet applied",
    )
    .await;

    let mut second_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("second"))]),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut second_insert,
        "queued follow-up mutation should remain pending until the blocked apply resumes",
    )
    .await;

    faults.release();

    let first_id = expect_future_within(
        first_insert,
        "first mutation should resolve after apply resumes",
    )
    .await
    .expect("first mutation task should join successfully")
    .expect("first mutation should succeed");
    let query_results = expect_future_within(
        blocked_query,
        "blocked query should resolve after apply resumes",
    )
    .await
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
            let visible = engine
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

    let visible = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_cancellable_mutation_response_still_resolves_after_blocked_read_catches_up() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_750))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async_with(
                    tenant_id,
                    tasks_table(),
                    None,
                    serde_json::Map::from_iter([("title".to_string(), json!("first-cancellable"))]),
                    crate::AsyncMutationContext::anonymous(std::future::pending::<()>(), || Ok(())),
                )
                .await
        }
    });

    expect_future_within(
        faults.wait_until_entered(),
        "journal worker should block after durable append",
    )
    .await;
    assert_future_stays_pending(
        &mut first_insert,
        "first cancellable mutation should remain pending while apply is blocked",
    )
    .await;

    let mut blocked_query = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .query_documents_async(tenant_id, query_for("tasks"))
                .await
        }
    });
    assert_future_stays_pending(
        &mut blocked_query,
        "query should remain pending while the first durable write is not yet applied",
    )
    .await;

    let mut second_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async_with(
                    tenant_id,
                    tasks_table(),
                    None,
                    serde_json::Map::from_iter([(
                        "title".to_string(),
                        json!("second-cancellable"),
                    )]),
                    crate::AsyncMutationContext::anonymous(std::future::pending::<()>(), || Ok(())),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut second_insert,
        "queued follow-up cancellable mutation should remain pending until the blocked apply resumes",
    )
    .await;

    faults.release();

    let first_id = expect_future_within(
        first_insert,
        "first cancellable mutation should resolve after apply resumes",
    )
    .await
    .expect("first cancellable mutation task should join successfully")
    .expect("first cancellable mutation should succeed");
    let query_results = expect_future_within(
        blocked_query,
        "blocked query should resolve after apply resumes",
    )
    .await
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
            let visible = engine
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

    let visible = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}

#[tokio::test]
async fn queued_mutation_response_still_resolves_after_blocked_cancellable_read_catches_up() {
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(42_900))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let mut first_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
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

    expect_future_within(
        faults.wait_until_entered(),
        "journal worker should block after durable append",
    )
    .await;
    assert_future_stays_pending(
        &mut first_insert,
        "first mutation should remain pending while apply is blocked",
    )
    .await;

    let mut blocked_query = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .query_documents_async_cancellable(
                    tenant_id,
                    query_for("tasks"),
                    std::future::pending::<()>(),
                    || Ok(()),
                )
                .await
        }
    });
    assert_future_stays_pending(
        &mut blocked_query,
        "cancellable query should remain pending while the first durable write is not yet applied",
    )
    .await;

    let mut second_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
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
    assert_future_stays_pending(
        &mut second_insert,
        "queued follow-up mutation should remain pending until the blocked apply resumes",
    )
    .await;

    faults.release();

    let first_id = expect_future_within(
        first_insert,
        "first mutation should resolve after apply resumes",
    )
    .await
    .expect("first mutation task should join successfully")
    .expect("first mutation should succeed");
    let query_results = expect_future_within(
        blocked_query,
        "blocked query should resolve after apply resumes",
    )
    .await
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
            let visible = engine
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

    let visible = engine
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
    let data_dir = tempdir().expect("engine tempdir should build");
    let faults = BlockingFaultInjector::new(FaultPoint::JournalDurableAppendBeforeApply);
    let engine = Arc::new(
        Engine::new_with_simulation(
            data_dir.path(),
            Arc::new(ManualClock::new(Timestamp(43_050))),
            faults.clone(),
        )
        .expect("engine should create"),
    );
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    engine
        .create_tenant(tenant_id.clone())
        .expect("tenant should create");

    let first_runtime = std::thread::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("ephemeral current-thread runtime should build");
            runtime.block_on(async move {
                engine
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

    expect_future_within(
        faults.wait_until_entered(),
        "journal worker should block after durable append",
    )
    .await;

    let mut second_insert = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
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
    assert_future_stays_pending(
        &mut second_insert,
        "queued follow-up mutation should remain pending until the blocked apply resumes",
    )
    .await;

    faults.release();

    let first_id = tokio::task::spawn_blocking(move || {
        first_runtime
            .join()
            .expect("ephemeral runtime thread should join successfully")
    })
    .await
    .expect("join worker should finish")
    .expect("first mutation should succeed");
    let second_id = expect_catch_up_future_within(
        second_insert,
        "queued follow-up mutation should still resolve after the ephemeral runtime exits",
    )
    .await
    .expect("second mutation task should join successfully")
    .expect("second mutation should succeed");

    let visible = engine
        .query_documents_async(tenant_id, query_for("tasks"))
        .await
        .expect("final query should succeed");
    assert_eq!(visible.len(), 2);
    assert!(visible.iter().any(|document| document.id == first_id));
    assert!(visible.iter().any(|document| document.id == second_id));
}
