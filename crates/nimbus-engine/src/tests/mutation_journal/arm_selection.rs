use super::support::{
    assert_future_stays_pending, expect_blocking_wait_reaches_state, expect_catch_up_future_within,
};
use super::*;
use nimbus_storage::NoopFaultInjector;

async fn run_static_arm_workload(arm: crate::tenant::CommitterArm) -> (Vec<u8>, Vec<u8>) {
    let data_dir = tempdir().expect("static-arm engine tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualWallClock::new(Timestamp(47_000))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(47_000)),
        )
        .expect("static-arm engine should create"),
    );
    let tenant_id = TenantId::new("static-arm").expect("tenant id should build");
    crate::tenant::configure_committer_arm_for_testing(tenant_id.clone(), arm);
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("static-arm tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("background cursor commits should be disabled for byte comparison");
    engine
        .set_prepared_table_id_for_testing(
            &tenant_id,
            &tasks_table(),
            nimbus_core::TableId::try_from("static-arm-table".to_string())
                .expect("fixed table id should be valid"),
        )
        .expect("static-arm table identity should be deterministic");

    for index in 0..6 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("static-arm insert should succeed");
    }

    let stats = engine
        .mutation_journal_stats_for_testing(&tenant_id)
        .expect("static-arm diagnostics should load");
    assert_eq!(stats.committer_arm, arm);

    let mut documents = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("static-arm documents should query");
    documents.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    let document_bytes = serde_json::to_vec(&documents).expect("documents should serialize");
    let journal_bytes = serde_json::to_vec(
        &engine
            .read_durable_journal_async(tenant_id, SequenceNumber(0))
            .await
            .expect("static-arm durable journal should read"),
    )
    .expect("durable journal should serialize");
    (document_bytes, journal_bytes)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn construction_time_committer_arms_produce_identical_state() {
    let ordered = run_static_arm_workload(crate::tenant::CommitterArm::OrderedPublisher).await;
    let serial = run_static_arm_workload(crate::tenant::CommitterArm::SerialReference).await;

    assert_eq!(
        ordered.0, serial.0,
        "ordered-publisher and serial-reference documents differ"
    );
    assert_eq!(
        ordered.1, serial.1,
        "ordered-publisher and serial-reference durable journal prefixes differ byte-for-byte"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn journal_progress_sync_cannot_overtake_publisher() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("static-arm-progress-order", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let faults = engine.commit_fault_handle_for_testing();
    let pause = crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH;
    faults.arm(pause);

    let write = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("ordered"))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("ordered publisher should pause before apply", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_entered(pause, timeout)
    })
    .await;

    let mut progress_sync = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.sync_mutation_journal_progress_for_testing(
                &tenant_id,
                nimbus_storage::JournalProgress {
                    durable_head: SequenceNumber(2),
                    applied_head: SequenceNumber(2),
                },
            )
        }
    });
    assert_future_stays_pending(
        &mut progress_sync,
        "journal progress sync must queue behind the publisher-owned batch",
    )
    .await;
    assert_eq!(
        engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("paused diagnostics should load")
            .durable_head,
        SequenceNumber(1),
        "progress sync must not overtake the publisher's response fence"
    );

    faults.release(pause);
    expect_catch_up_future_within(write, "ordered write should finish after release")
        .await
        .expect("write task should join")
        .expect("ordered write should succeed");
    expect_catch_up_future_within(progress_sync, "progress sync should drain after publisher")
        .await
        .expect("progress-sync task should join")
        .expect("progress sync should succeed");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn opaque_internal_job_cannot_overtake_ordered_publisher() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("static-arm-serial-order", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    let faults = engine.commit_fault_handle_for_testing();
    let pause = crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH;
    faults.arm(pause);

    let write = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("title".to_string(), json!("ordered"))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("ordered publisher should pause before apply", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_entered(pause, timeout)
    })
    .await;

    let schema_write = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .set_table_schema_async(
                    tenant_id,
                    TableSchema {
                        table: tasks_table(),
                        fields: Vec::new(),
                        indexes: Vec::new(),
                        access_policy: None,
                    },
                )
                .await
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "schema internal job should queue behind the publisher-owned batch",
        |stats| stats.publisher_queue_depth == 1,
    )
    .await;
    assert!(
        !schema_write.is_finished(),
        "opaque schema work must not answer while it is observably queued behind the publisher-owned batch"
    );

    faults.release(pause);
    expect_catch_up_future_within(write, "ordered write should finish after release")
        .await
        .expect("write task should join")
        .expect("ordered write should succeed");
    expect_catch_up_future_within(
        schema_write,
        "schema internal job should drain after publisher",
    )
    .await
    .expect("schema task should join")
    .expect("schema write should succeed");

    let journal = engine
        .read_durable_journal_async(tenant_id.clone(), SequenceNumber(0))
        .await
        .expect("ordered journal should read");
    assert_eq!(journal.len(), 2);
    assert!(matches!(
        journal[0].events.as_slice(),
        [nimbus_core::TenantEventKind::DocumentWrite { .. }]
    ));
    assert!(matches!(
        journal[1].events.as_slice(),
        [nimbus_core::TenantEventKind::SchemaChange { .. }]
    ));
    assert_eq!(
        engine
            .get_table_schema_async(tenant_id, tasks_table())
            .await
            .expect("schema lookup should succeed")
            .table,
        tasks_table()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ordered_publisher_serializes_queued_direct_and_execution_unit_paths() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("three-commit-path-order", Engine::create_tenant);
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("route".to_string(), json!("seed"))]),
        )
        .await
        .expect("seed write should establish a stable table identity");
    // A document write restarts the trigger-candidate worker. Stop and drain
    // it after the seed so its cursor job cannot occupy the actor while this
    // test measures the three mutation-path handoffs.
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("seed trigger cursor should stop");
    engine
        .flush_tenant_committer_for_testing(&tenant_id)
        .await
        .expect("seed trigger cursor should drain");

    let faults = engine.commit_fault_handle_for_testing();
    let pause = crate::engine::commit_fault_labels::DURABLE_BEFORE_PUBLISH;
    faults.arm(pause);
    let queued = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("route".to_string(), json!("queued"))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("queued path should pause in the ordered publisher", {
        let faults = faults.clone();
        move |timeout| faults.wait_until_entered(pause, timeout)
    })
    .await;

    // The queued record is durable at this pause, so beginning now gives the
    // execution unit the current sequence while the publisher still owns the
    // unfinished commit. Its later actor admission is the ordering assertion.
    let execution_unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should begin");
    execution_unit
        .insert_document(
            tasks_table(),
            serde_json::Map::from_iter([("route".to_string(), json!("execution-unit"))]),
        )
        .expect("execution-unit write should stage");

    let direct = tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("route".to_string(), json!("direct"))]),
            )
        }
    });
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "direct path should queue behind the paused publisher batch",
        |stats| stats.committer_inbox_depth == 0 && stats.publisher_queue_depth == 1,
    )
    .await;

    let execution = tokio::task::spawn_blocking(move || execution_unit.commit());
    wait_for_mutation_journal_stats(
        &engine,
        &tenant_id,
        "execution-unit path should queue in the actor behind the direct handoff",
        |stats| stats.committer_inbox_depth == 1 && stats.publisher_queue_depth == 1,
    )
    .await;

    faults.release(pause);
    expect_catch_up_future_within(queued, "queued path should finish after release")
        .await
        .expect("queued task should join")
        .expect("queued mutation should commit");
    expect_catch_up_future_within(direct, "direct path should finish after the queued path")
        .await
        .expect("direct task should join")
        .expect("direct mutation should commit");
    expect_catch_up_future_within(
        execution,
        "execution-unit path should finish after the direct path",
    )
    .await
    .expect("execution-unit task should join")
    .expect("execution-unit mutation should commit")
    .expect("execution-unit mutation should append a record");

    let records = engine
        .read_durable_journal_async(tenant_id, SequenceNumber(0))
        .await
        .expect("three-path journal should read");
    let route_records = records
        .iter()
        .filter(|record| !record.writes.is_empty())
        .skip(1)
        .collect::<Vec<_>>();
    let routes = route_records
        .iter()
        .map(|record| {
            record.writes[0]
                .current
                .as_ref()
                .and_then(|document| document.fields.get("route"))
                .and_then(serde_json::Value::as_str)
                .expect("each route commit should retain its marker")
        })
        .collect::<Vec<_>>();
    assert_eq!(routes, ["queued", "direct", "execution-unit"]);
    assert!(
        route_records.windows(2).all(|pair| {
            pair[1].sequence == SequenceNumber(pair[0].sequence.0.saturating_add(1))
        }),
        "the three route records must occupy one contiguous durable prefix"
    );
}

#[derive(Debug, PartialEq, Eq)]
struct SeededHistorySnapshot {
    documents: Vec<u8>,
    schema: Vec<u8>,
    scheduled_jobs: Vec<u8>,
    journal: Vec<u8>,
}

async fn run_seeded_history(arm: crate::tenant::CommitterArm) -> SeededHistorySnapshot {
    let data_dir = tempdir().expect("seeded-history tempdir should build");
    let engine = Arc::new(
        Engine::new_with_simulation_and_memory_persistence(
            data_dir.path(),
            Arc::new(ManualWallClock::new(Timestamp(48_000))),
            Arc::new(NoopFaultInjector),
            Arc::new(nimbus_core::SeededIdSource::new(48_000)),
        )
        .expect("seeded-history engine should create"),
    );
    let tenant_id = TenantId::new("seeded-history").expect("tenant id should build");
    crate::tenant::configure_committer_arm_for_testing(tenant_id.clone(), arm);
    engine
        .create_tenant_async(tenant_id.clone())
        .await
        .expect("seeded-history tenant should create");
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add nondeterministic records");
    engine
        .set_prepared_table_id_for_testing(
            &tenant_id,
            &tasks_table(),
            nimbus_core::TableId::try_from("seeded-history-table".to_string())
                .expect("fixed table id should be valid"),
        )
        .expect("seeded table identity should install");
    let mut ids = vec![
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("value".to_string(), json!(0))]),
            )
            .await
            .expect("seeded insert should establish the fixed table identity"),
    ];
    engine
        .set_table_schema_async(
            tenant_id.clone(),
            TableSchema {
                table: tasks_table(),
                fields: vec![FieldSchema {
                    name: "value".to_string(),
                    field_type: FieldType::Number,
                    required: true,
                }],
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .await
        .expect("seeded schema should commit");

    for value in 1..4 {
        ids.push(
            engine
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([("value".to_string(), json!(value))]),
                )
                .await
                .expect("seeded insert should commit"),
        );
    }
    engine
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            ids[1].clone(),
            serde_json::Map::from_iter([("value".to_string(), json!(41))]),
        )
        .await
        .expect("seeded update should commit");
    engine
        .delete_document_async(tenant_id.clone(), tasks_table(), ids[2].clone())
        .await
        .expect("seeded delete should commit");
    engine
        .schedule_mutation_async(
            tenant_id.clone(),
            nimbus_core::ScheduleRequest {
                run_after_ms: 2_000,
                mutation: nimbus_core::Mutation::Insert {
                    table: tasks_table(),
                    id: None,
                    fields: serde_json::Map::from_iter([("value".to_string(), json!(99))]),
                },
            },
        )
        .await
        .expect("seeded scheduled state should persist");

    let mut documents = engine
        .query_documents_async(tenant_id.clone(), query_for("tasks"))
        .await
        .expect("seeded documents should query");
    documents.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    let mut scheduled_jobs = engine
        .list_scheduled_jobs_async(tenant_id.clone())
        .await
        .expect("seeded scheduled state should load");
    scheduled_jobs.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    SeededHistorySnapshot {
        documents: serde_json::to_vec(&documents).expect("documents should serialize"),
        schema: serde_json::to_vec(
            &engine
                .get_schema_async(tenant_id.clone())
                .await
                .expect("schema should load"),
        )
        .expect("schema should serialize"),
        scheduled_jobs: serde_json::to_vec(
            &scheduled_jobs
                .iter()
                .map(|job| (job.run_at, &job.mutation, job.created_at))
                .collect::<Vec<_>>(),
        )
        .expect("scheduled jobs should serialize without their nondeterministic IDs"),
        journal: serde_json::to_vec(
            &engine
                .read_durable_journal_async(tenant_id, SequenceNumber(0))
                .await
                .expect("journal should load"),
        )
        .expect("journal should serialize"),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_publisher_pipeline_matches_serial_reference_for_seeded_history() {
    let ordered = run_seeded_history(crate::tenant::CommitterArm::OrderedPublisher).await;
    let reference = run_seeded_history(crate::tenant::CommitterArm::SerialReference).await;
    assert_eq!(ordered, reference);
}
