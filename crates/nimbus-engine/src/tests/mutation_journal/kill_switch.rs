use super::support::{expect_blocking_wait_reaches_state, expect_catch_up_future_within};
use super::*;
use nimbus_storage::NoopFaultInjector;

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
