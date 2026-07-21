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
            Arc::new(ManualClock::new(Timestamp(47_000))),
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
    let serial = run_static_arm_workload(crate::tenant::CommitterArm::Serial).await;

    assert_eq!(
        ordered.0, serial.0,
        "ordered-publisher and serial documents differ"
    );
    assert_eq!(
        ordered.1, serial.1,
        "ordered-publisher and serial durable journal prefixes differ byte-for-byte"
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
async fn opaque_serial_job_cannot_overtake_ordered_publisher() {
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
        "schema serial job should queue behind the publisher-owned batch",
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
        "schema serial job should drain after publisher",
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
