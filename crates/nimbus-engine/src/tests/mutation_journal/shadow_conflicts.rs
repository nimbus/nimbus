use super::support::expect_blocking_wait_reaches_state;
use super::*;

fn title(value: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([("title".to_string(), json!(value))])
}

async fn pause_direct_update_after_prepare(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    document_id: &DocumentId,
    direct_value: &'static str,
) -> (
    crate::engine::CommitFaultHandle,
    tokio::task::JoinHandle<nimbus_core::Result<DocumentId>>,
) {
    let faults = engine.commit_fault_handle_for_testing();
    faults.arm(crate::engine::commit_fault_labels::PREPARE_COMPLETE);
    let direct = tokio::task::spawn_blocking({
        let engine = Arc::clone(engine);
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        move || engine.update_document(&tenant_id, tasks_table(), document_id, title(direct_value))
    });
    expect_blocking_wait_reaches_state("direct update should finish caller-side prepare", {
        let faults = faults.clone();
        move |timeout| {
            faults.wait_until_entered(
                crate::engine::commit_fault_labels::PREPARE_COMPLETE,
                timeout,
            )
        }
    })
    .await;
    (faults, direct)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shadow_conflict_total_increments_for_conflicting_queued_and_direct_mutations_without_rejection()
 {
    // Observe every eligible batch: sampling defaults to 1-in-16, which
    // would make this small scenario nondeterministic. Safe under nextest
    // (process-per-test isolation).
    unsafe { std::env::set_var("NIMBUS_SHADOW_CONFLICT_SAMPLE_EVERY", "1") };
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("shadow-conflict", Engine::create_tenant);
    let document_id = engine
        .insert_document(&tenant_id, tasks_table(), title("initial"))
        .expect("initial direct insert should succeed");

    let (faults, direct_update) =
        pause_direct_update_after_prepare(&engine, &tenant_id, &document_id, "direct-racer").await;
    let queued_id = engine
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            document_id.clone(),
            title("queued-wins"),
        )
        .await
        .expect("shadow-conflicting queued mutation should still complete");
    assert_eq!(queued_id, document_id);
    faults.release(crate::engine::commit_fault_labels::PREPARE_COMPLETE);
    direct_update
        .await
        .expect("direct update task should join")
        .expect("the direct racing mutation should succeed");

    let visible = engine
        .query_documents(&tenant_id, &query_for("tasks"))
        .expect("final document query should succeed");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].fields.get("title"), Some(&json!("direct-racer")));

    let metrics = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("commit diagnostics should load")
        .commit_phases;
    assert!(
        metrics.shadow_conflict_total > 0,
        "the stale direct dependency should intersect the queued commit ahead of it: {metrics:?}"
    );
    assert!(
        metrics.shadow_window_size > 0,
        "the shadow checker should examine a non-empty recent window: {metrics:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shadow_conflict_total_stays_zero_for_disjoint_queued_and_direct_mutations() {
    // Observe every eligible batch (see the conflicting-workload test).
    unsafe { std::env::set_var("NIMBUS_SHADOW_CONFLICT_SAMPLE_EVERY", "1") };
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("shadow-disjoint", Engine::create_tenant);
    let queued_document_id = engine
        .insert_document(&tenant_id, tasks_table(), title("queued-target"))
        .expect("queued target insert should succeed");
    let direct_document_id = engine
        .insert_document(&tenant_id, tasks_table(), title("direct-target"))
        .expect("direct target insert should succeed");

    let (faults, direct_update) = pause_direct_update_after_prepare(
        &engine,
        &tenant_id,
        &direct_document_id,
        "direct-updated",
    )
    .await;
    engine
        .update_document_async(
            tenant_id.clone(),
            tasks_table(),
            queued_document_id,
            title("queued-updated"),
        )
        .await
        .expect("disjoint queued mutation should succeed");
    faults.release(crate::engine::commit_fault_labels::PREPARE_COMPLETE);
    direct_update
        .await
        .expect("direct update task should join")
        .expect("disjoint direct racing mutation should succeed");

    let metrics = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("commit diagnostics should load")
        .commit_phases;
    assert_eq!(
        metrics.shadow_conflict_total, 0,
        "non-overlapping document dependencies must not conflict: {metrics:?}"
    );
    assert!(
        metrics.shadow_window_size > 0,
        "the zero result must come from checking a non-empty disjoint window: {metrics:?}"
    );
}
