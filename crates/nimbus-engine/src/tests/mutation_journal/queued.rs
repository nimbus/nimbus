use super::support::{
    assert_future_stays_pending, expect_blocking_wait_reaches_state, expect_catch_up_future_within,
    expect_future_within, new_faulted_engine,
};
use super::*;
use nimbus_core::TriggerDeliveryCursor;
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
    struct RestoreEnv {
        inbox: Option<std::ffi::OsString>,
        timeout: Option<std::ffi::OsString>,
    }
    impl Drop for RestoreEnv {
        fn drop(&mut self) {
            unsafe {
                match self.inbox.take() {
                    Some(value) => std::env::set_var("NIMBUS_COMMITTER_INBOX_SIZE", value),
                    None => std::env::remove_var("NIMBUS_COMMITTER_INBOX_SIZE"),
                }
                match self.timeout.take() {
                    Some(value) => std::env::set_var("NIMBUS_COMMITTER_SEND_TIMEOUT_MS", value),
                    None => std::env::remove_var("NIMBUS_COMMITTER_SEND_TIMEOUT_MS"),
                }
            }
        }
    }

    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let restore = RestoreEnv {
        inbox: std::env::var_os("NIMBUS_COMMITTER_INBOX_SIZE"),
        timeout: std::env::var_os("NIMBUS_COMMITTER_SEND_TIMEOUT_MS"),
    };
    unsafe {
        std::env::set_var("NIMBUS_COMMITTER_INBOX_SIZE", "2");
        std::env::set_var("NIMBUS_COMMITTER_SEND_TIMEOUT_MS", "25");
    }
    let tenant_id = fixture.create_tenant("bounded-committer", Engine::create_tenant);
    drop(restore);

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

    let schema = |table: &'static str| TableSchema {
        table: TableName::new(table).expect("test table name should build"),
        fields: vec![],
        indexes: vec![],
        access_policy: None,
    };
    let spawn_schema = |table: &'static str| {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        tokio::spawn(async move {
            engine
                .set_table_schema_async(tenant_id, schema(table))
                .await
        })
    };
    let second = spawn_schema("inbox_one");
    let third = spawn_schema("inbox_two");
    let full = wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "both bounded committer slots should become observable",
        |stats| stats.committer_inbox_depth == 2,
    )
    .await;
    assert_eq!(full.committer_inbox_capacity, 2);

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
        started.elapsed() < Duration::from_millis(250),
        "the configured 25ms send timeout must not turn into unbounded queueing"
    );
    for error in &errors {
        assert!(matches!(
            error,
            nimbus_core::Error::CommitterFull { capacity: 2, .. }
        ));
        assert_eq!(
            error.retryability(),
            nimbus_core::Retryability::RetryableAfterBackoff
        );
    }

    let diagnostics = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("committer diagnostics should load while saturated");
    assert_eq!(diagnostics.mutation_journal.committer_inbox_depth, 2);
    assert_eq!(diagnostics.mutation_journal.committer_send_timeout_count, 2);
    assert_eq!(diagnostics.commit_phases.committer_inbox_depth, 2);
    assert_eq!(diagnostics.commit_phases.committer_send_timeout_total, 2);

    pause.release();
    expect_future_within(first, "the held queued mutation should drain after release")
        .await
        .expect("accepted insert task should join")
        .expect("accepted insert should commit");
    for schema_write in [second, third] {
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
