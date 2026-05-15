use super::*;

/// Schedules a mutation to execute in the future.
pub(crate) async fn schedule_mutation(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(request): Json<ScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let service = state.service.clone();
    let job_id = service
        .schedule_mutation_async(tenant_id.clone(), request)
        .await?;
    crate::system_tenant::sync_scheduler_state_for_tenant_async(&service, &tenant_id).await?;
    Ok((
        StatusCode::CREATED,
        Json(ScheduleResponse {
            job_id: job_id.to_string(),
        }),
    ))
}

/// Lists all pending scheduled jobs for a tenant.
pub(crate) async fn list_scheduled_jobs(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<ScheduledJobsResponse>, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let service = state.service.clone();
    crate::system_tenant::sync_scheduler_state_for_tenant_async(&service, &tenant_id).await?;
    let jobs = service.list_scheduled_jobs_async(tenant_id).await?;
    Ok(Json(ScheduledJobsResponse { jobs }))
}

/// Loads the final result for an executed scheduled job.
pub(crate) async fn get_scheduled_job_result(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, job_id)): Path<(String, String)>,
) -> Result<Json<ScheduledJobResultResponse>, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let job_id = parse_document_id(&job_id)?;
    let service = state.service.clone();
    let result = service
        .get_scheduled_job_result_async(tenant_id.clone(), job_id)
        .await?;
    crate::system_tenant::record_scheduled_job_result_state_async(&service, &tenant_id, &result)
        .await?;
    Ok(Json(ScheduledJobResultResponse { result }))
}

/// Cancels a pending scheduled job before it starts executing.
pub(crate) async fn cancel_scheduled_job(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, job_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let job_id = parse_document_id(&job_id)?;
    let service = state.service.clone();
    service
        .cancel_scheduled_job_async(tenant_id.clone(), job_id.clone())
        .await?;
    crate::system_tenant::delete_scheduled_job_state_async(&service, &tenant_id, &job_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Creates a recurring cron job.
pub(crate) async fn create_cron_job(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(request): Json<CreateCronRequest>,
) -> Result<StatusCode, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let service = state.service.clone();
    service
        .create_cron_job_async(tenant_id.clone(), request)
        .await?;
    crate::system_tenant::sync_scheduler_state_for_tenant_async(&service, &tenant_id).await?;
    Ok(StatusCode::CREATED)
}

/// Lists cron jobs for a tenant.
pub(crate) async fn list_cron_jobs(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<CronJobsResponse>, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let service = state.service.clone();
    crate::system_tenant::sync_scheduler_state_for_tenant_async(&service, &tenant_id).await?;
    let crons = service.list_cron_jobs_async(tenant_id).await?;
    Ok(Json(CronJobsResponse { crons }))
}

/// Deletes a cron job definition.
pub(crate) async fn delete_cron_job(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, name)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant_id = parse_user_tenant_id(tenant_id)?;
    let service = state.service.clone();
    service
        .delete_cron_job_async(tenant_id.clone(), name.clone())
        .await?;
    crate::system_tenant::delete_cron_job_state_async(&service, &tenant_id, &name).await?;
    Ok(StatusCode::NO_CONTENT)
}
