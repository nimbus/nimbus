use super::*;

/// Evaluates a query for a tenant.
pub(crate) async fn query_documents(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(query): Json<Query>,
) -> Result<Json<DataResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let guard = RequestCancellationGuard::new();
    let cancellation = guard.token();
    let cancellation_check = cancellation.clone();
    let documents = service
        .query_documents_async_cancellable(tenant_id, query, cancellation.cancelled(), move || {
            if cancellation_check.is_cancelled() {
                Err(Error::Cancelled)
            } else {
                Ok(())
            }
        })
        .await?;
    Ok(Json(DataResponse {
        data: documents
            .into_iter()
            .map(|document| document.to_json())
            .collect(),
    }))
}

/// Evaluates a paginated query for a tenant.
pub(crate) async fn query_documents_paginated(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(query): Json<PaginatedQuery>,
) -> Result<Json<Page>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let guard = RequestCancellationGuard::new();
    let cancellation = guard.token();
    let cancellation_check = cancellation.clone();
    let page = service
        .paginate_documents_async_cancellable(
            tenant_id,
            query,
            cancellation.cancelled(),
            move || {
                if cancellation_check.is_cancelled() {
                    Err(Error::Cancelled)
                } else {
                    Ok(())
                }
            },
        )
        .await?;
    Ok(Json(page))
}

/// Reads commit log entries for a tenant.
pub(crate) async fn read_commit_log(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    QueryParams(request): QueryParams<CommitLogRequest>,
) -> Result<Json<CommitLogResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let after = SequenceNumber(request.after.unwrap_or(0));
    let service = state.service.clone();
    let commits = service
        .read_commit_log_async(tenant_id.clone(), after)
        .await?;
    let latest_sequence = service.latest_sequence_async(tenant_id).await?;

    Ok(Json(CommitLogResponse {
        commits,
        latest_sequence: latest_sequence.0,
    }))
}

/// Streams durable journal records for a tenant.
pub(crate) async fn read_journal(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    QueryParams(request): QueryParams<JournalStreamRequest>,
) -> Result<Json<JournalStreamResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let after = SequenceNumber(request.after.unwrap_or(0));
    let limit = request
        .limit
        .unwrap_or(DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT);
    let page = state
        .service
        .clone()
        .stream_durable_journal_async(tenant_id, after, limit)
        .await?;

    Ok(Json(JournalStreamResponse {
        records: page.records,
        next_cursor: page.next_cursor.0,
        latest_sequence: page.latest_sequence.0,
        cursor_floor: page.cursor_floor.0,
        has_more: page.has_more,
    }))
}

/// Exports bootstrap metadata for replaying a tenant from snapshot plus journal stream.
pub(crate) async fn bootstrap_journal(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<JournalBootstrapResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let bootstrap = state
        .service
        .clone()
        .export_durable_journal_bootstrap_async(tenant_id)
        .await?;

    Ok(Json(JournalBootstrapResponse {
        snapshot: MaterializedJournalSnapshotResponse::from(bootstrap.snapshot),
        resume_after_sequence: bootstrap.resume_after.0,
        bootstrap_cut_sequence: bootstrap.bootstrap_cut.0,
        cursor_floor_sequence: bootstrap.cursor_floor.0,
    }))
}
