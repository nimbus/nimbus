use super::common::{filter, tasks_table};
use super::support::TenantState;
use super::*;

pub(super) async fn exercise_crud_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    let mut ids = Vec::with_capacity(CRUD_DOCUMENTS);
    for rank in 0..CRUD_DOCUMENTS {
        ids.push(
            engine
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
    for (rank, id) in ids.iter().cloned().enumerate() {
        let _ = engine
            .update_document_async(
                tenant_id.clone(),
                tasks_table(),
                id,
                serde_json::Map::from_iter([("rank".to_string(), json!(rank + CRUD_DOCUMENTS))]),
            )
            .await?;
    }
    for id in ids {
        engine
            .delete_document_async(tenant_id.clone(), tasks_table(), id)
            .await?;
    }
    Ok(())
}

pub(super) async fn exercise_point_read_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    ids: &[DocumentId],
) -> BenchResult<()> {
    for step in 0..POINT_READ_BATCH_SIZE {
        let id = ids[(step * 17) % ids.len()].clone();
        let document = engine
            .get_document_async(tenant_id.clone(), tasks_table(), id)
            .await?;
        black_box(document);
    }
    Ok(())
}

pub(super) async fn exercise_query_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    query: &Query,
    batch_size: usize,
) -> BenchResult<()> {
    for _ in 0..batch_size {
        let documents = engine
            .query_documents_async(tenant_id.clone(), query.clone())
            .await?;
        black_box(documents);
    }
    Ok(())
}

pub(super) async fn exercise_journal_stream_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    let page = engine
        .stream_durable_journal_async(tenant_id.clone(), SequenceNumber(0), JOURNAL_STREAM_LIMIT)
        .await?;
    black_box(page);
    Ok(())
}

pub(super) async fn exercise_journal_bootstrap_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    let bootstrap = engine
        .export_durable_journal_bootstrap_async(tenant_id.clone())
        .await?;
    black_box(bootstrap);
    Ok(())
}

pub(super) async fn seed_subscription_fixture(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<()> {
    engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([("title".to_string(), json!("seed"))]),
        )
        .await?;
    Ok(())
}

pub(super) async fn register_subscription_receivers(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
) -> BenchResult<(
    Vec<SubscriptionRegistration>,
    Vec<mpsc::Receiver<SubscriptionUpdate>>,
)> {
    let query = Query {
        table: tasks_table(),
        filters: Vec::new(),
        order: None,
        limit: None,
    };
    let mut registrations = Vec::with_capacity(SUBSCRIPTION_FANOUT_COUNT);
    let mut receivers = Vec::with_capacity(SUBSCRIPTION_FANOUT_COUNT);
    for index in 0..SUBSCRIPTION_FANOUT_COUNT {
        let (sender, mut receiver) = mpsc::channel(8);
        let registration = engine
            .subscribe_async(
                tenant_id.clone(),
                query.clone(),
                format!("fanout-{index}"),
                sender,
                SubscribeOptions::anonymous(),
            )
            .await?;
        let initial = receiver
            .recv()
            .await
            .ok_or("subscription bootstrap should arrive")?;
        black_box(initial);
        registrations.push(registration);
        receivers.push(receiver);
    }
    Ok((registrations, receivers))
}

pub(super) async fn exercise_subscription_fanout_sample(
    engine: &Arc<Engine>,
    tenant_id: &TenantId,
    receivers: &mut [mpsc::Receiver<SubscriptionUpdate>],
) -> BenchResult<()> {
    let _ = engine
        .insert_document_async(
            tenant_id.clone(),
            tasks_table(),
            serde_json::Map::from_iter([(
                "title".to_string(),
                json!(format!(
                    "fanout-{}",
                    BENCH_DIR_COUNTER.fetch_add(1, Ordering::SeqCst)
                )),
            )]),
        )
        .await?;
    for receiver in receivers {
        let update = receiver
            .recv()
            .await
            .ok_or("subscription update should arrive")?;
        match update {
            SubscriptionUpdate::Result { .. } => {}
            SubscriptionUpdate::Error { message, .. } => {
                return Err(format!("unexpected subscription error: {message}").into());
            }
        }
    }
    Ok(())
}

pub(super) async fn exercise_mixed_load_sample(
    engine: &Arc<Engine>,
    tenant_states: &[TenantState],
) -> BenchResult<()> {
    let mut handles = Vec::with_capacity(tenant_states.len());
    for (task_index, state) in tenant_states.iter().cloned().enumerate() {
        let engine = engine.clone();
        handles.push(tokio::spawn(async move {
            let query = Query {
                table: tasks_table(),
                filters: vec![filter("status", FilterOp::Eq, json!("open"))],
                order: None,
                limit: Some(25),
            };
            for step in 0..MIXED_LOAD_OPS_PER_TENANT {
                let id = state.ids[step % state.ids.len()].clone();
                match step % 4 {
                    0 => {
                        let document = engine
                            .get_document_async(state.tenant_id.clone(), tasks_table(), id)
                            .await?;
                        black_box(document);
                    }
                    1 => {
                        let documents = engine
                            .query_documents_async(state.tenant_id.clone(), query.clone())
                            .await?;
                        black_box(documents);
                    }
                    2 => {
                        let _ = engine
                            .insert_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                serde_json::Map::from_iter([
                                    ("status".to_string(), json!("open")),
                                    (
                                        "rank".to_string(),
                                        json!(task_index * MIXED_LOAD_OPS_PER_TENANT + step),
                                    ),
                                    (
                                        "title".to_string(),
                                        json!(format!("tenant-{task_index}-insert-{step}")),
                                    ),
                                ]),
                            )
                            .await?;
                    }
                    _ => {
                        let _ = engine
                            .update_document_async(
                                state.tenant_id.clone(),
                                tasks_table(),
                                id,
                                serde_json::Map::from_iter([(
                                    "rank".to_string(),
                                    json!(step + MIXED_LOAD_OPS_PER_TENANT),
                                )]),
                            )
                            .await?;
                    }
                }
            }
            Ok::<(), nimbus_core::Error>(())
        }));
    }
    for handle in handles {
        handle.await??;
    }
    Ok(())
}
