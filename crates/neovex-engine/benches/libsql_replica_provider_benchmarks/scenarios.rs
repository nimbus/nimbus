use super::common::{filter, tasks_table};
use super::fixtures::{PeerCatchUpFixture, TenantState};
use super::*;

pub(super) async fn exercise_crud_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    document_count: usize,
) -> BenchResult<()> {
    let mut ids = Vec::with_capacity(document_count);
    for rank in 0..document_count {
        ids.push(
            service
                .insert_document_async(
                    tenant_id.clone(),
                    tasks_table(),
                    serde_json::Map::from_iter([
                        ("status".to_string(), json!("open")),
                        ("rank".to_string(), json!(rank)),
                        ("title".to_string(), json!(format!("task-{rank:05}"))),
                    ]),
                )
                .await?,
        );
    }
    for (rank, id) in ids.iter().copied().enumerate() {
        let _ = service
            .update_document_async(
                tenant_id.clone(),
                tasks_table(),
                id,
                serde_json::Map::from_iter([("rank".to_string(), json!(rank + document_count))]),
            )
            .await?;
    }
    for id in ids {
        service
            .delete_document_async(tenant_id.clone(), tasks_table(), id)
            .await?;
    }
    Ok(())
}

pub(super) async fn exercise_point_read_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    ids: &[DocumentId],
    batch_size: usize,
) -> BenchResult<()> {
    for step in 0..batch_size {
        let id = ids[(step * 17) % ids.len()];
        let document = service
            .get_document_async(tenant_id.clone(), tasks_table(), id)
            .await?;
        black_box(document);
    }
    Ok(())
}

pub(super) async fn exercise_query_sample(
    service: &Arc<Service>,
    tenant_id: &TenantId,
    query: &Query,
    batch_size: usize,
) -> BenchResult<()> {
    for _ in 0..batch_size {
        let documents = service
            .query_documents_async(tenant_id.clone(), query.clone())
            .await?;
        black_box(documents);
    }
    Ok(())
}

pub(super) async fn exercise_mixed_load_sample(
    service: &Arc<Service>,
    tenant_states: &[TenantState],
    tenant_limit: usize,
    ops_per_tenant: usize,
) -> BenchResult<()> {
    let selected = tenant_states
        .iter()
        .take(tenant_limit)
        .cloned()
        .collect::<Vec<_>>();
    let mut handles = Vec::with_capacity(selected.len());
    for (task_index, state) in selected.into_iter().enumerate() {
        let service = service.clone();
        handles.push(tokio::spawn(async move {
            let query = Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                order: None,
                limit: Some(25),
            };
            for step in 0..ops_per_tenant {
                let id = state.ids[step % state.ids.len()];
                match step % 4 {
                    0 => {
                        let document = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            service.get_document_async(state.tenant_id.clone(), tasks_table(), id),
                        )
                        .await
                        .map_err(|_| {
                            NeovexError::Internal(format!(
                                "mixed-load point read timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                        black_box(document);
                    }
                    1 => {
                        let documents = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            service.query_documents_async(state.tenant_id.clone(), query.clone()),
                        )
                        .await
                        .map_err(|_| {
                            NeovexError::Internal(format!(
                                "mixed-load indexed query timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                        black_box(documents);
                    }
                    2 => {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            service.insert_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                serde_json::Map::from_iter([
                                    ("status".to_string(), json!("open")),
                                    (
                                        "rank".to_string(),
                                        json!(task_index * ops_per_tenant + step),
                                    ),
                                    (
                                        "title".to_string(),
                                        json!(format!("tenant-{task_index}-insert-{step}")),
                                    ),
                                ]),
                            ),
                        )
                        .await
                        .map_err(|_| {
                            NeovexError::Internal(format!(
                                "mixed-load insert timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                    }
                    _ => {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(MIXED_LOAD_OPERATION_TIMEOUT_SECS),
                            service.update_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                id,
                                serde_json::Map::from_iter([(
                                    "rank".to_string(),
                                    json!(step + ops_per_tenant),
                                )]),
                            ),
                        )
                        .await
                        .map_err(|_| {
                            NeovexError::Internal(format!(
                                "mixed-load update timed out for tenant {} at step {step}",
                                state.tenant_id
                            ))
                        })??;
                    }
                }
            }
            Ok::<(), NeovexError>(())
        }));
    }
    for handle in handles {
        handle.await??;
    }
    Ok(())
}

pub(super) async fn run_mixed_load_sample<F>(context: &str, future: F) -> BenchResult<()>
where
    F: std::future::Future<Output = BenchResult<()>>,
{
    tokio::time::timeout(Duration::from_secs(MIXED_LOAD_SAMPLE_TIMEOUT_SECS), future)
        .await
        .map_err(|_| -> Box<dyn std::error::Error> {
            format!("{context} exceeded {MIXED_LOAD_SAMPLE_TIMEOUT_SECS}s").into()
        })?
}

pub(super) async fn exercise_peer_catch_up_sample(
    fixture: &PeerCatchUpFixture,
) -> BenchResult<Duration> {
    let inserted_id = fixture
        .creator_service
        .insert_document_async(
            fixture.tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([
                ("status".to_string(), json!("open")),
                (
                    "title".to_string(),
                    json!(format!(
                        "peer-catch-up-{}",
                        BENCH_COUNTER.fetch_add(1, Ordering::SeqCst)
                    )),
                ),
            ]),
        )
        .await?;
    let started = Instant::now();
    loop {
        match fixture
            .opener_service
            .get_document_async(fixture.tenant_id.clone(), tasks_table(), inserted_id)
            .await
        {
            Ok(document) => {
                black_box(document);
                return Ok(started.elapsed());
            }
            Err(NeovexError::DocumentNotFound(_)) => {}
            Err(error) => return Err(Box::new(error)),
        }
        if started.elapsed() >= Duration::from_secs(PEER_CATCH_UP_TIMEOUT_SECS) {
            return Err(format!(
                "peer catch-up did not surface the delegated write within {}s",
                PEER_CATCH_UP_TIMEOUT_SECS
            )
            .into());
        }
        tokio::time::sleep(Duration::from_millis(PEER_CATCH_UP_POLL_INTERVAL_MS)).await;
    }
}
