//! Update and delete writes carry the document's resource path binding into
//! the commit. The trigger candidate worker falls back to a store lookup only
//! for a write without one, and that lookup misses once a later commit (or
//! the delete itself) removes the binding.
//!
//! Each test restarts the engine after it creates the bound document. The
//! restarted write window retains no image of that document, so the journal
//! and direct paths prepare from storage instead of from the window.

use nimbus_core::{AtomicWrite, AtomicWriteBatch, WriteKey, WritePrecondition, WriteSetMode};

use super::*;

/// Creates a task at its resource path the way a Firestore `set` does, so the
/// insert commit itself carries the binding.
async fn insert_bound_task(engine: &Arc<Engine>, tenant_id: &TenantId, key: &str) -> DocumentId {
    let document_id = DocumentId::from_key(key).expect("document id should build");
    let batch = AtomicWriteBatch::new(vec![AtomicWrite::Set {
        key: WriteKey::from(trigger_binding(&document_id)),
        document: serde_json::Map::from_iter([("title".to_string(), json!(key))]),
        typed_fields: Default::default(),
        mode: WriteSetMode::Overwrite,
        precondition: WritePrecondition::default(),
        transforms: Vec::new(),
    }])
    .expect("atomic write batch should build");
    let unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("insert execution unit should begin");
    tokio::task::spawn_blocking(move || unit.execute_atomic_write_batch(batch))
        .await
        .expect("insert task should join")
        .expect("bound insert should commit");
    document_id
}

async fn restarted_engine_with_bound_task(
    key: &str,
) -> (TempDir, Arc<Engine>, TenantId, DocumentId) {
    let data_dir = tempdir().expect("engine tempdir should build");
    let tenant_id = TenantId::new("demo").expect("tenant id should build");
    let document_id = {
        let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should create"));
        engine
            .create_tenant(tenant_id.clone())
            .expect("tenant should create");
        let document_id = insert_bound_task(&engine, &tenant_id, key).await;
        engine.quiesce().await;
        document_id
    };
    let engine = Arc::new(Engine::new(data_dir.path()).expect("engine should recreate"));
    engine
        .ensure_tenant_exists(&tenant_id)
        .expect("tenant should load");
    (data_dir, engine, tenant_id, document_id)
}

/// Commits a bound sentinel insert and drains candidates until the sentinel
/// arrives. The worker publishes commits in order, so the result holds every
/// candidate for the earlier commits. Sentinel candidates are left out.
async fn drain_candidates_through_sentinel(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    sentinel_key: &str,
) -> Vec<(FirestoreCloudEventType, String)> {
    let sentinel_path = format!("tasks/{sentinel_key}");
    insert_bound_task(engine, tenant_id, sentinel_key).await;
    let drained = Mutex::new(Vec::new());
    wait_for_value(
        "sentinel trigger candidates should arrive",
        mutation_journal_progress_timeout(),
        mutation_journal_poll_interval(),
        || async {
            let mut drained = drained.lock().expect("drained candidates lock");
            drained.extend(
                engine
                    .drain_trigger_candidates_for_testing(tenant_id)
                    .expect("trigger candidates should drain")
                    .into_iter()
                    .map(|candidate| {
                        (
                            candidate.event_type,
                            candidate.binding.document_path.to_string(),
                        )
                    }),
            );
            drained.iter().any(|(event_type, path)| {
                *event_type == FirestoreCloudEventType::Written && *path == sentinel_path
            })
        },
        |sentinel_seen| *sentinel_seen,
    )
    .await;
    drained
        .into_inner()
        .expect("drained candidates lock")
        .into_iter()
        .filter(|(_, path)| *path != sentinel_path)
        .collect()
}

fn candidates_of_type(
    candidates: Vec<(FirestoreCloudEventType, String)>,
    event_type: FirestoreCloudEventType,
) -> Vec<(FirestoreCloudEventType, String)> {
    candidates
        .into_iter()
        .filter(|(candidate_type, _)| *candidate_type == event_type)
        .collect()
}

/// Parks the candidate worker on the update commit, deletes the document so
/// the store drops its binding, and then lets the worker build the update's
/// candidates.
async fn update_candidates_after_later_delete<F, Fut>(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    document_id: &DocumentId,
    update: F,
) -> Vec<(FirestoreCloudEventType, String)>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    drain_candidates_through_sentinel(engine, tenant_id, "setup-sentinel").await;
    let pause = engine
        .trigger_candidate_pause_handle_for_testing(tenant_id)
        .expect("trigger candidate pause handle should load");
    pause.arm();

    update().await;
    let pause_for_wait = pause.clone();
    expect_blocking_wait_reaches_state(
        "trigger candidate worker should park on the update commit",
        move |timeout| pause_for_wait.wait_until_entered(timeout),
    )
    .await;

    let unit = engine
        .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
        .expect("delete execution unit should begin");
    unit.delete_document(tasks_table(), document_id.clone())
        .expect("delete should stage");
    tokio::task::spawn_blocking(move || unit.commit())
        .await
        .expect("delete task should join")
        .expect("delete commit should succeed");

    pause.release();
    candidates_of_type(
        drain_candidates_through_sentinel(engine, tenant_id, "result-sentinel").await,
        FirestoreCloudEventType::Updated,
    )
}

fn title_patch() -> serde_json::Map<String, serde_json::Value> {
    serde_json::Map::from_iter([("title".to_string(), json!("updated"))])
}

#[tokio::test]
async fn journal_update_trigger_candidate_survives_a_later_delete() {
    let (_data_dir, engine, tenant_id, document_id) =
        restarted_engine_with_bound_task("journal-update").await;

    let updated = update_candidates_after_later_delete(&engine, &tenant_id, &document_id, || {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            engine
                .update_document_async(tenant_id, tasks_table(), document_id, title_patch())
                .await
                .expect("journal update should succeed");
        }
    })
    .await;

    assert_eq!(
        updated,
        vec![(
            FirestoreCloudEventType::Updated,
            "tasks/journal-update".to_string()
        )]
    );
}

#[tokio::test]
async fn direct_update_trigger_candidate_survives_a_later_delete() {
    let (_data_dir, engine, tenant_id, document_id) =
        restarted_engine_with_bound_task("direct-update").await;

    let updated = update_candidates_after_later_delete(&engine, &tenant_id, &document_id, || {
        let engine = engine.clone();
        let tenant_id = tenant_id.clone();
        let document_id = document_id.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                engine.update_document(&tenant_id, tasks_table(), document_id, title_patch())
            })
            .await
            .expect("update task should join")
            .expect("direct update should succeed");
        }
    })
    .await;

    assert_eq!(
        updated,
        vec![(
            FirestoreCloudEventType::Updated,
            "tasks/direct-update".to_string()
        )]
    );
}

#[tokio::test]
async fn execution_unit_update_trigger_candidate_survives_a_later_delete() {
    let (_data_dir, engine, tenant_id, document_id) =
        restarted_engine_with_bound_task("unit-update").await;

    let updated = update_candidates_after_later_delete(&engine, &tenant_id, &document_id, || {
        let unit = engine
            .begin_mutation_execution_unit(tenant_id.clone(), PrincipalContext::anonymous())
            .expect("update execution unit should begin");
        unit.update_document(tasks_table(), document_id.clone(), title_patch())
            .expect("update should stage");
        async move {
            tokio::task::spawn_blocking(move || unit.commit())
                .await
                .expect("update task should join")
                .expect("update commit should succeed");
        }
    })
    .await;

    assert_eq!(
        updated,
        vec![(
            FirestoreCloudEventType::Updated,
            "tasks/unit-update".to_string()
        )]
    );
}

#[tokio::test]
async fn journal_delete_trigger_candidate_keeps_the_removed_binding() {
    let (_data_dir, engine, tenant_id, document_id) =
        restarted_engine_with_bound_task("journal-delete").await;
    drain_candidates_through_sentinel(&engine, &tenant_id, "setup-sentinel").await;

    engine
        .delete_document_async(tenant_id.clone(), tasks_table(), document_id)
        .await
        .expect("journal delete should succeed");

    assert_eq!(
        candidates_of_type(
            drain_candidates_through_sentinel(&engine, &tenant_id, "result-sentinel").await,
            FirestoreCloudEventType::Deleted,
        ),
        vec![(
            FirestoreCloudEventType::Deleted,
            "tasks/journal-delete".to_string()
        )]
    );
}
