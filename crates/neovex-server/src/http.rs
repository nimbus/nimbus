use axum::Json;
use axum::extract::{Path, Query as QueryParams, State};
use axum::http::StatusCode;
use axum::response::Redirect;
use neovex_core::{
    CreateCronRequest, DocumentId, Error, Page, PaginatedQuery, Query, ScheduleRequest, Schema,
    SequenceNumber, TableName, TableSchema, TenantId,
};
use std::sync::Arc;

use crate::protocol::{
    CommitLogRequest, CommitLogResponse, CreateTenantRequest, CronJobsResponse, DataResponse,
    DocumentDataResponse, DocumentResponse, HealthResponse, InsertDocumentRequest,
    RuntimeDiagnosticsResponse, RuntimeLimitsResponse, ScheduleResponse,
    ScheduledJobResultResponse, ScheduledJobsResponse, TenantListResponse, TenantResponse,
    UpdateDocumentRequest,
};
use crate::state::{AppError, AppState, run_blocking};

/// Health endpoint.
pub(crate) async fn health() -> Json<HealthResponse> {
    Json(HealthResponse { ok: true })
}

/// Returns the current Neovex license and entitlement status.
pub(crate) async fn license_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::license::LicenseSnapshot>, AppError> {
    let service = state.service.clone();
    let usage = run_blocking(move || service.current_monthly_active_users()).await?;
    Ok(Json(state.license_state.snapshot_with_usage(Some(usage))))
}

/// Returns runtime limits and live runtime metrics for diagnostics.
pub(crate) async fn runtime_diagnostics(
    State(state): State<Arc<AppState>>,
) -> Json<RuntimeDiagnosticsResponse> {
    let registry = state
        .convex_registry
        .clone()
        .expect("runtime diagnostics route requires Convex support state");
    let limits = registry.runtime_limits();
    Json(RuntimeDiagnosticsResponse {
        limits: RuntimeLimitsResponse {
            max_heap_mb: limits.max_heap_mb,
            initial_heap_mb: limits.initial_heap_mb,
            execution_timeout_ms: limits
                .execution_timeout
                .as_millis()
                .min(u128::from(u64::MAX)) as u64,
            max_concurrent_isolates: limits.max_concurrent_isolates,
            max_nested_runtime_invocations: limits.max_nested_runtime_invocations,
        },
        metrics: registry.runtime_metrics_snapshot(),
    })
}

/// Redirects to the repo-hosted demos index.
pub(crate) async fn demos_redirect() -> Redirect {
    Redirect::permanent("/demos/")
}

/// Creates a tenant explicitly.
pub(crate) async fn create_tenant(
    State(state): State<Arc<AppState>>,
    Json(request): Json<CreateTenantRequest>,
) -> Result<(StatusCode, Json<TenantResponse>), AppError> {
    let tenant_id = TenantId::new(request.id)?;
    let service = state.service.clone();
    let id = run_blocking(move || {
        service.create_tenant(tenant_id.clone())?;
        Ok(tenant_id.to_string())
    })
    .await?;
    Ok((StatusCode::CREATED, Json(TenantResponse { id })))
}

/// Lists known tenants.
pub(crate) async fn list_tenants(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TenantListResponse>, AppError> {
    let service = state.service.clone();
    let tenants = run_blocking(move || service.list_tenants()).await?;
    Ok(Json(TenantListResponse {
        tenants: tenants
            .into_iter()
            .map(|tenant| tenant.to_string())
            .collect(),
    }))
}

/// Deletes a tenant.
pub(crate) async fn delete_tenant(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    run_blocking(move || service.delete_tenant(&tenant_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Schedules a mutation to execute in the future.
pub(crate) async fn schedule_mutation(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(request): Json<ScheduleRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let job_id = run_blocking(move || service.schedule_mutation(&tenant_id, request)).await?;
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
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let jobs = run_blocking(move || service.list_scheduled_jobs(&tenant_id)).await?;
    Ok(Json(ScheduledJobsResponse { jobs }))
}

/// Loads the final result for an executed scheduled job.
pub(crate) async fn get_scheduled_job_result(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, job_id)): Path<(String, String)>,
) -> Result<Json<ScheduledJobResultResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let job_id = parse_document_id(&job_id)?;
    let service = state.service.clone();
    let result =
        run_blocking(move || service.get_scheduled_job_result(&tenant_id, &job_id)).await?;
    Ok(Json(ScheduledJobResultResponse { result }))
}

/// Cancels a pending scheduled job before it starts executing.
pub(crate) async fn cancel_scheduled_job(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, job_id)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let job_id = parse_document_id(&job_id)?;
    let service = state.service.clone();
    run_blocking(move || service.cancel_scheduled_job(&tenant_id, &job_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Creates a recurring cron job.
pub(crate) async fn create_cron_job(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(request): Json<CreateCronRequest>,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    run_blocking(move || service.create_cron_job(&tenant_id, request)).await?;
    Ok(StatusCode::CREATED)
}

/// Lists cron jobs for a tenant.
pub(crate) async fn list_cron_jobs(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<CronJobsResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let crons = run_blocking(move || service.list_cron_jobs(&tenant_id)).await?;
    Ok(Json(CronJobsResponse { crons }))
}

/// Deletes a cron job definition.
pub(crate) async fn delete_cron_job(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, name)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    run_blocking(move || service.delete_cron_job(&tenant_id, &name)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Stores or updates a table schema.
pub(crate) async fn set_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
    Json(table_schema): Json<TableSchema>,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let path_table = TableName::new(table)?;
    if table_schema.table != path_table {
        return Err(AppError::from(Error::InvalidInput(
            "schema table must match the path table".to_string(),
        )));
    }

    let service = state.service.clone();
    run_blocking(move || service.set_table_schema(&tenant_id, table_schema)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Returns the full tenant schema.
pub(crate) async fn get_schema(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
) -> Result<Json<Schema>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let schema = run_blocking(move || service.get_schema(&tenant_id)).await?;
    Ok(Json(schema))
}

/// Returns a single table schema.
pub(crate) async fn get_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
) -> Result<Json<TableSchema>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let service = state.service.clone();
    let table_schema = run_blocking(move || service.get_table_schema(&tenant_id, &table)).await?;
    Ok(Json(table_schema))
}

/// Deletes a single table schema.
pub(crate) async fn delete_table_schema(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let service = state.service.clone();
    run_blocking(move || service.delete_table_schema(&tenant_id, &table)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Inserts a document into a tenant table.
pub(crate) async fn insert_document(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(request): Json<InsertDocumentRequest>,
) -> Result<(StatusCode, Json<DocumentResponse>), AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(request.table)?;
    let service = state.service.clone();
    let document_id =
        run_blocking(move || service.insert_document(&tenant_id, table, request.fields)).await?;

    Ok((
        StatusCode::CREATED,
        Json(DocumentResponse {
            id: document_id.to_string(),
        }),
    ))
}

/// Updates a document within a tenant table.
pub(crate) async fn update_document(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table, document_id)): Path<(String, String, String)>,
    Json(request): Json<UpdateDocumentRequest>,
) -> Result<Json<DocumentResponse>, AppError> {
    let document_id = parse_document_id(&document_id)?;
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let service = state.service.clone();
    let document_id = run_blocking(move || {
        service.update_document(&tenant_id, table, document_id, request.patch)
    })
    .await?;

    Ok(Json(DocumentResponse {
        id: document_id.to_string(),
    }))
}

/// Deletes a document within a tenant table.
pub(crate) async fn delete_document(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table, document_id)): Path<(String, String, String)>,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let document_id = parse_document_id(&document_id)?;
    let service = state.service.clone();
    run_blocking(move || service.delete_document(&tenant_id, table, document_id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lists documents in a tenant table.
pub(crate) async fn list_documents(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table)): Path<(String, String)>,
) -> Result<Json<DataResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let service = state.service.clone();
    let documents = run_blocking(move || service.list_documents(&tenant_id, &table)).await?;
    Ok(Json(DataResponse {
        data: documents
            .into_iter()
            .map(|document| document.to_json())
            .collect(),
    }))
}

/// Fetches a single document in a tenant table.
pub(crate) async fn get_document(
    State(state): State<Arc<AppState>>,
    Path((tenant_id, table, document_id)): Path<(String, String, String)>,
) -> Result<Json<DocumentDataResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let table = TableName::new(table)?;
    let document_id = parse_document_id(&document_id)?;
    let service = state.service.clone();
    let document =
        run_blocking(move || service.get_document(&tenant_id, &table, document_id)).await?;
    Ok(Json(DocumentDataResponse {
        document: document.to_json(),
    }))
}

/// Evaluates a query for a tenant.
pub(crate) async fn query_documents(
    State(state): State<Arc<AppState>>,
    Path(tenant_id): Path<String>,
    Json(query): Json<Query>,
) -> Result<Json<DataResponse>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let documents = run_blocking(move || service.query_documents(&tenant_id, &query)).await?;
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
    let page = run_blocking(move || service.paginate_documents(&tenant_id, &query)).await?;
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
    let (commits, latest_sequence) = run_blocking(move || {
        let commits = service.read_commit_log(&tenant_id, after)?;
        let latest_sequence = service.latest_sequence(&tenant_id)?;
        Ok((commits, latest_sequence))
    })
    .await?;

    Ok(Json(CommitLogResponse {
        commits,
        latest_sequence: latest_sequence.0,
    }))
}

fn parse_document_id(value: &str) -> Result<DocumentId, AppError> {
    value.parse().map_err(|error| {
        AppError::from(Error::InvalidInput(format!(
            "invalid document id `{value}`: {error}"
        )))
    })
}
