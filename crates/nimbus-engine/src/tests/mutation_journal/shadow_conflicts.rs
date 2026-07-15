use super::support::{expect_blocking_wait_reaches_state, expect_future_within};
use super::*;

fn title(value: &str) -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([("title".to_string(), json!(value))])
}

async fn queue_update_behind_pause(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    document_id: &DocumentId,
    queued_value: &'static str,
) -> (
    crate::tenant::MutationJournalPauseHandle,
    tokio::task::JoinHandle<nimbus_core::Result<DocumentId>>,
) {
    let pause = engine
        .mutation_journal_pause_handle_for_testing(tenant_id)
        .expect("journal pause handle should load");
    pause.arm();
    let queued = tokio::spawn({
        let engine = Arc::clone(engine);
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(tenant_id, tasks_table(), document_id, title(queued_value))
                .await
        }
    });
    expect_blocking_wait_reaches_state("queued update should reach the armed pre-drain pause", {
        let pause = pause.clone();
        move |timeout| pause.wait_until_entered(timeout)
    })
    .await;
    (pause, queued)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shadow_conflict_total_increments_for_conflicting_queued_and_direct_mutations_without_rejection()
 {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("shadow-conflict", Engine::create_tenant);
    let document_id = engine
        .insert_document(&tenant_id, tasks_table(), title("initial"))
        .expect("initial direct insert should succeed");

    let (pause, queued_update) =
        queue_update_behind_pause(&engine, &tenant_id, &document_id, "queued-wins").await;
    engine
        .update_document(
            &tenant_id,
            tasks_table(),
            document_id.clone(),
            title("direct-racer"),
        )
        .expect("the direct racing mutation should succeed");
    pause.release();

    let queued_id = expect_future_within(
        queued_update,
        "shadow-conflicting queued mutation should still complete",
    )
    .await
    .expect("queued mutation task should join")
    .expect("shadow conflict observation must not reject the queued mutation");
    assert_eq!(queued_id, document_id);

    let visible = engine
        .query_documents(&tenant_id, &query_for("tasks"))
        .expect("final document query should succeed");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].fields.get("title"), Some(&json!("queued-wins")));

    let metrics = engine
        .tenant_engine_diagnostics(&tenant_id)
        .expect("commit diagnostics should load")
        .commit_phases;
    assert!(
        metrics.shadow_conflict_total > 0,
        "the stale queued dependency should intersect the direct racing commit: {metrics:?}"
    );
    assert!(
        metrics.shadow_window_size > 0,
        "the shadow checker should examine a non-empty recent window: {metrics:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shadow_conflict_total_stays_zero_for_disjoint_queued_and_direct_mutations() {
    let fixture = EngineFixture::new(|path| Engine::new(path));
    let engine = fixture.engine();
    let tenant_id = fixture.create_tenant("shadow-disjoint", Engine::create_tenant);
    let queued_document_id = engine
        .insert_document(&tenant_id, tasks_table(), title("queued-target"))
        .expect("queued target insert should succeed");
    let direct_document_id = engine
        .insert_document(&tenant_id, tasks_table(), title("direct-target"))
        .expect("direct target insert should succeed");

    let (pause, queued_update) =
        queue_update_behind_pause(&engine, &tenant_id, &queued_document_id, "queued-updated").await;
    engine
        .update_document(
            &tenant_id,
            tasks_table(),
            direct_document_id,
            title("direct-updated"),
        )
        .expect("disjoint direct racing mutation should succeed");
    pause.release();
    expect_future_within(
        queued_update,
        "disjoint queued mutation should complete after the pause",
    )
    .await
    .expect("queued mutation task should join")
    .expect("disjoint queued mutation should succeed");

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
