use super::support::{expect_blocking_wait_reaches_state, expect_catch_up_future_within};
use super::*;

fn assert_quiescent_contiguous(stats: &crate::tenant::MutationJournalStats) {
    assert!(stats.frontiers.is_causally_ordered());
    assert_eq!(stats.active_assigned_head, stats.durable_head);
    assert_eq!(stats.durable_head, stats.storage_applied_head);
    assert_eq!(stats.storage_applied_head, stats.published_head);
    assert_eq!(stats.published_head, stats.applied_head);
    assert_eq!(
        (
            stats.assignment_lag,
            stats.apply_lag,
            stats.publication_lag,
            stats.visibility_lag,
        ),
        (0, 0, 0, 0)
    );
}

async fn settled_frontier(
    engine: &Engine,
    tenant_id: &TenantId,
) -> crate::tenant::MutationJournalStats {
    // Every document commit may restart the trigger-candidate worker. Its
    // zero-write cursor job is legitimate publisher work, so drain it before
    // asserting a quiescent frontier rather than racing that accepted suffix.
    engine
        .shutdown_trigger_candidates_for_testing(tenant_id)
        .expect("trigger cursor should stop before a quiescent sample");
    engine
        .flush_tenant_committer_for_testing(tenant_id)
        .await
        .expect("accepted trigger cursor work should drain");
    engine
        .mutation_journal_stats_for_testing(tenant_id)
        .expect("settled frontier sample should load")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn provider_publisher_frontiers_are_monotonic_and_contiguous() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("publisher-frontier-contract", Engine::create_tenant);
    engine
        .shutdown_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");

    let mut samples = vec![settled_frontier(&engine, &tenant_id).await];
    engine
        .set_table_schema_async(
            tenant_id.clone(),
            TableSchema {
                table: tasks_table(),
                fields: Vec::new(),
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .await
        .expect("schema route should commit");
    samples.push(settled_frontier(&engine, &tenant_id).await);
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("route".to_string(), json!("queued"))]),
        )
        .await
        .expect("queued route should commit");
    samples.push(settled_frontier(&engine, &tenant_id).await);
    tokio::task::spawn_blocking({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        move || {
            engine.insert_document(
                &tenant_id,
                tasks_table(),
                serde_json::Map::from_iter([("route".to_string(), json!("direct"))]),
            )
        }
    })
    .await
    .expect("direct route task should join")
    .expect("direct route should commit");
    samples.push(settled_frontier(&engine, &tenant_id).await);
    let unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("execution unit should begin");
    unit.insert_document(
        tasks_table(),
        serde_json::Map::from_iter([("route".to_string(), json!("execution-unit"))]),
    )
    .expect("execution-unit write should stage");
    tokio::task::spawn_blocking(move || unit.commit())
        .await
        .expect("execution-unit task should join")
        .expect("execution-unit route should commit")
        .expect("execution-unit route should append a record");
    samples.push(settled_frontier(&engine, &tenant_id).await);

    for sample in &samples {
        assert_quiescent_contiguous(sample);
    }
    for pair in samples.windows(2) {
        assert!(pair[1].assigned_high_water >= pair[0].assigned_high_water);
        assert!(pair[1].durable_head >= pair[0].durable_head);
        assert!(pair[1].storage_applied_head >= pair[0].storage_applied_head);
        assert!(pair[1].published_head >= pair[0].published_head);
        assert!(pair[1].applied_head >= pair[0].applied_head);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn publisher_stall_diagnostics_distinguish_assignment_apply_and_publication_lag() {
    let assignment_fixture = EngineFixture::new(|path| Engine::new(path));
    let assignment_engine = assignment_fixture.engine();
    let assignment_tenant =
        assignment_fixture.create_tenant("assignment-stall", Engine::create_tenant);
    assignment_engine
        .shutdown_trigger_candidates_for_testing(&assignment_tenant)
        .expect("assignment trigger cursor should stop");
    let assignment_baseline = assignment_engine
        .mutation_journal_stats_for_testing(&assignment_tenant)
        .expect("assignment baseline should load")
        .durable_head;
    let assignment_faults = assignment_engine.commit_fault_handle_for_testing();
    let assignment_pause = crate::engine::commit_fault_labels::JOURNAL_ASSIGN_AFTER_STAGE;
    assignment_faults.arm(assignment_pause);
    let assignment_write = tokio::spawn({
        let engine = assignment_engine.clone();
        let tenant_id = assignment_tenant.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("phase".to_string(), json!("assignment"))]),
                )
                .await
        }
    });
    expect_blocking_wait_reaches_state("assignment should pause after suffix staging", {
        let faults = assignment_faults.clone();
        move |timeout| faults.wait_until_entered(assignment_pause, timeout)
    })
    .await;
    let assignment = assignment_engine
        .mutation_journal_stats_for_testing(&assignment_tenant)
        .expect("assignment-stall diagnostics should load");
    assert_eq!(assignment.assignment_lag, 1);
    assert_eq!(assignment.active_assigned_head.0, assignment_baseline.0 + 1);
    assert_eq!(assignment.durable_head, assignment_baseline);
    assert_eq!((assignment.apply_lag, assignment.publication_lag), (0, 0));
    assignment_faults.release(assignment_pause);
    expect_catch_up_future_within(assignment_write, "assignment-stall write should finish")
        .await
        .expect("assignment write task should join")
        .expect("assignment write should commit");

    let (_apply_dir, apply_engine, apply_tenant, apply_faults) =
        super::support::new_faulted_engine(91_000);
    apply_engine
        .shutdown_trigger_candidates_for_testing(&apply_tenant)
        .expect("apply trigger cursor should stop");
    let apply_write = tokio::spawn({
        let engine = apply_engine.clone();
        let tenant_id = apply_tenant.clone();
        async move {
            engine
                .insert_document_async(
                    tenant_id,
                    tasks_table(),
                    serde_json::Map::from_iter([("phase".to_string(), json!("apply"))]),
                )
                .await
        }
    });
    timeout(Duration::from_secs(5), apply_faults.wait_until_entered())
        .await
        .expect("storage apply should pause after durable append");
    let apply = apply_engine
        .mutation_journal_stats_for_testing(&apply_tenant)
        .expect("apply-stall diagnostics should load");
    assert_eq!(apply.assignment_lag, 0);
    assert_eq!(apply.apply_lag, 1);
    assert_eq!(apply.publication_lag, 0);
    apply_faults.release();
    expect_catch_up_future_within(apply_write, "apply-stall write should finish")
        .await
        .expect("apply write task should join")
        .expect("apply write should commit");

    let publication_fixture = EngineFixture::new(|path| Engine::new(path));
    let publication_engine = publication_fixture.engine();
    let publication_tenant =
        publication_fixture.create_tenant("publication-stall", Engine::create_tenant);
    // The seed commit below dispatches to the trigger-candidate feed, which
    // would restart a lifecycle-shutdown worker and let its cursor record race
    // the exact publication-lag and quiescence assertions.
    publication_engine
        .disable_trigger_candidates_for_testing(&publication_tenant)
        .expect("publication trigger cursor should stop");
    let document_id = publication_engine
        .insert_document_async(
            publication_tenant.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("phase".to_string(), json!("seed"))]),
        )
        .await
        .expect("publication seed should commit");
    let pending = publication_engine
        .stage_assigned_pending_update_for_testing(
            &publication_tenant,
            &tasks_table(),
            &document_id,
            "phase",
            json!("publication"),
        )
        .expect("pending publication record should stage and become durable");
    publication_engine
        .apply_assigned_pending_record_without_publish_for_testing(&publication_tenant, &pending)
        .expect("pending publication record should apply in storage");
    publication_engine
        .sync_mutation_journal_progress_for_testing(
            &publication_tenant,
            nimbus_storage::JournalProgress {
                durable_head: pending.sequence,
                applied_head: pending.sequence,
            },
        )
        .expect("storage progress should be observed behind the publish barrier");
    let publication = publication_engine
        .mutation_journal_stats_for_testing(&publication_tenant)
        .expect("publication-stall diagnostics should load");
    assert_eq!((publication.assignment_lag, publication.apply_lag), (0, 0));
    assert_eq!(publication.publication_lag, 1);
    assert_eq!(publication.storage_applied_head, pending.sequence);
    assert!(publication.published_head < pending.sequence);
    publication_engine
        .publish_assigned_pending_record_for_testing(&publication_tenant, &pending)
        .expect("pending publication should release");
    assert_quiescent_contiguous(
        &publication_engine
            .mutation_journal_stats_for_testing(&publication_tenant)
            .expect("released publication diagnostics should load"),
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn frontier_diagnostics_remain_ordered_under_concurrent_sampling() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("concurrent-frontier-sampling", Engine::create_tenant);
    // The document stream below restarts a lifecycle-shutdown worker on every
    // commit dispatch, and this test samples the raw frontier rather than
    // draining through `settled_frontier`. Suppress the producer permanently so
    // the final quiescent sample cannot race an accepted cursor record.
    engine
        .disable_trigger_candidates_for_testing(&tenant_id)
        .expect("trigger cursor should not add unrelated records");
    engine
        .set_table_schema_async(
            tenant_id.clone(),
            TableSchema {
                table: tasks_table(),
                fields: Vec::new(),
                indexes: Vec::new(),
                access_policy: None,
            },
        )
        .await
        .expect("sampling schema should commit");

    let finished = Arc::new(AtomicBool::new(false));
    let sampler = tokio::spawn({
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let finished = finished.clone();
        async move {
            let mut sample_count = 0usize;
            let mut previous = SequenceNumber(0);
            loop {
                let stats = engine
                    .mutation_journal_stats_for_testing(&tenant_id)
                    .expect("concurrent frontier sample should load");
                assert!(stats.frontiers.is_causally_ordered(), "{stats:?}");
                assert!(stats.assigned_high_water >= previous);
                previous = stats.assigned_high_water;
                sample_count += 1;
                if finished.load(Ordering::Acquire) {
                    return sample_count;
                }
                tokio::task::yield_now().await;
            }
        }
    });
    for index in 0..32 {
        engine
            .insert_document_async(
                tenant_id.clone(),
                tasks_table(),
                serde_json::Map::from_iter([("index".to_string(), json!(index))]),
            )
            .await
            .expect("sampled mutation should commit");
    }
    finished.store(true, Ordering::Release);
    let sample_count = timeout(Duration::from_secs(5), sampler)
        .await
        .expect("frontier sampler should finish")
        .expect("frontier sampler task should join");
    assert!(
        sample_count >= 32,
        "sampler should overlap the mutation stream"
    );
    assert_quiescent_contiguous(
        &engine
            .mutation_journal_stats_for_testing(&tenant_id)
            .expect("final frontier sample should load"),
    );
}
