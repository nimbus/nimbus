use super::*;

/// Schedules a mutation to execute in the future.
pub(crate) async fn schedule_mutation(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(request): Json<ScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.schedule.create")?;
    let response =
        nimbus_compute::scheduling::schedule_mutation(&state, tenant.tenant_id(), request).await?;
    Ok((StatusCode::CREATED, Json(response)))
}

/// Lists all pending scheduled jobs for a tenant.
pub(crate) async fn list_scheduled_jobs(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<ScheduledJobsResponse>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.schedule.list")?;
    let response =
        nimbus_compute::scheduling::list_scheduled_jobs(&state, tenant.tenant_id()).await?;
    Ok(Json(response))
}

/// Loads the final result for an executed scheduled job.
pub(crate) async fn get_scheduled_job_result(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, job_id)): Path<(String, String)>,
) -> Result<Json<ScheduledJobResultResponse>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.schedule.result")?;
    let job_id = parse_document_id(&job_id)?;
    let response =
        nimbus_compute::scheduling::get_scheduled_job_result(&state, tenant.tenant_id(), job_id)
            .await?;
    Ok(Json(response))
}

/// Cancels a pending scheduled job before it starts executing.
pub(crate) async fn cancel_scheduled_job(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, job_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.schedule.cancel")?;
    let job_id = parse_document_id(&job_id)?;
    nimbus_compute::scheduling::cancel_scheduled_job(&state, tenant.tenant_id(), job_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Creates a recurring cron job.
pub(crate) async fn create_cron_job(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(request): Json<CreateCronRequest>,
) -> Result<StatusCode, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.crons.create")?;
    nimbus_compute::scheduling::create_cron_job(&state, tenant.tenant_id(), request).await?;
    Ok(StatusCode::CREATED)
}

/// Lists cron jobs for a tenant.
pub(crate) async fn list_cron_jobs(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<CronJobsResponse>, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.crons.list")?;
    let response = nimbus_compute::scheduling::list_cron_jobs(&state, tenant.tenant_id()).await?;
    Ok(Json(response))
}

/// Deletes a cron job definition.
pub(crate) async fn delete_cron_job(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, name)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant = parse_operator_tenant_context(tenant_id, "native_http.crons.delete")?;
    nimbus_compute::scheduling::delete_cron_job(&state, tenant.tenant_id(), &name).await?;
    Ok(StatusCode::NO_CONTENT)
}
