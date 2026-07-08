//! Scheduled-mutation and cron orchestration extracted from `nimbus-server`'s
//! `http::scheduling` handlers (CP3). The handler still parses the route's
//! tenant id into an operator `TenantIsolationContext` (an `AppError`-typed
//! parse) before calling in here; everything past that — the engine call
//! plus the corresponding scheduler-state sync/record-keeping call — lives
//! in this module.

use nimbus_core::{
    CreateCronRequest, CronJob, DocumentId, ScheduleRequest, ScheduledJob, ScheduledJobResult,
    TenantId,
};
use serde::Serialize;

use crate::state::{ComputeError, ComputeState};

#[derive(Debug, Serialize)]
pub struct ScheduleResponse {
    pub job_id: String,
}

#[derive(Debug, Serialize)]
pub struct ScheduledJobsResponse {
    pub jobs: Vec<ScheduledJob>,
}

#[derive(Debug, Serialize)]
pub struct ScheduledJobResultResponse {
    pub result: ScheduledJobResult,
}

#[derive(Debug, Serialize)]
pub struct CronJobsResponse {
    pub crons: Vec<CronJob>,
}

pub async fn schedule_mutation(
    compute: &ComputeState,
    tenant_id: &TenantId,
    request: ScheduleRequest,
) -> Result<ScheduleResponse, ComputeError> {
    let service = compute.engine.clone();
    let job_id = service
        .schedule_mutation_async(tenant_id.clone(), request)
        .await?;
    nimbus_system::sync_scheduler_state_for_tenant_async(&service, tenant_id)
        .await
        .map_err(ComputeError::from)?;
    Ok(ScheduleResponse {
        job_id: job_id.to_string(),
    })
}

pub async fn list_scheduled_jobs(
    compute: &ComputeState,
    tenant_id: &TenantId,
) -> Result<ScheduledJobsResponse, ComputeError> {
    let service = compute.engine.clone();
    nimbus_system::sync_scheduler_state_for_tenant_async(&service, tenant_id)
        .await
        .map_err(ComputeError::from)?;
    let jobs = service.list_scheduled_jobs_async(tenant_id.clone()).await?;
    Ok(ScheduledJobsResponse { jobs })
}

pub async fn get_scheduled_job_result(
    compute: &ComputeState,
    tenant_id: &TenantId,
    job_id: DocumentId,
) -> Result<ScheduledJobResultResponse, ComputeError> {
    let service = compute.engine.clone();
    let result = service
        .get_scheduled_job_result_async(tenant_id.clone(), job_id)
        .await?;
    nimbus_system::record_scheduled_job_result_state_async(&service, tenant_id, &result)
        .await
        .map_err(ComputeError::from)?;
    Ok(ScheduledJobResultResponse { result })
}

pub async fn cancel_scheduled_job(
    compute: &ComputeState,
    tenant_id: &TenantId,
    job_id: DocumentId,
) -> Result<(), ComputeError> {
    let service = compute.engine.clone();
    service
        .cancel_scheduled_job_async(tenant_id.clone(), job_id.clone())
        .await?;
    nimbus_system::delete_scheduled_job_state_async(&service, tenant_id, &job_id)
        .await
        .map_err(ComputeError::from)
}

pub async fn create_cron_job(
    compute: &ComputeState,
    tenant_id: &TenantId,
    request: CreateCronRequest,
) -> Result<(), ComputeError> {
    let service = compute.engine.clone();
    service
        .create_cron_job_async(tenant_id.clone(), request)
        .await?;
    nimbus_system::sync_scheduler_state_for_tenant_async(&service, tenant_id)
        .await
        .map_err(ComputeError::from)
}

pub async fn list_cron_jobs(
    compute: &ComputeState,
    tenant_id: &TenantId,
) -> Result<CronJobsResponse, ComputeError> {
    let service = compute.engine.clone();
    nimbus_system::sync_scheduler_state_for_tenant_async(&service, tenant_id)
        .await
        .map_err(ComputeError::from)?;
    let crons = service.load_cron_jobs_async(tenant_id.clone()).await?;
    Ok(CronJobsResponse { crons })
}

pub async fn delete_cron_job(
    compute: &ComputeState,
    tenant_id: &TenantId,
    name: &str,
) -> Result<(), ComputeError> {
    let service = compute.engine.clone();
    service
        .delete_cron_job_async(tenant_id.clone(), name.to_owned())
        .await?;
    nimbus_system::delete_cron_job_state_async(&service, tenant_id, name)
        .await
        .map_err(ComputeError::from)
}
