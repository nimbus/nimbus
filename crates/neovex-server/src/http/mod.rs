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
use crate::state::{AppError, AppState, RequestCancellationGuard};

mod documents;
mod metadata;
mod queries;
mod scheduling;
mod schema;
mod tenants;

pub(crate) use documents::{
    delete_document, get_document, insert_document, list_documents, update_document,
};
pub(crate) use metadata::{demos_redirect, health, license_status, runtime_diagnostics};
pub(crate) use queries::{query_documents, query_documents_paginated, read_commit_log};
pub(crate) use scheduling::{
    cancel_scheduled_job, create_cron_job, delete_cron_job, get_scheduled_job_result,
    list_cron_jobs, list_scheduled_jobs, schedule_mutation,
};
pub(crate) use schema::{delete_table_schema, get_schema, get_table_schema, set_table_schema};
pub(crate) use tenants::{create_tenant, delete_tenant, list_tenants};

fn parse_document_id(value: &str) -> Result<DocumentId, AppError> {
    value.parse().map_err(|error| {
        AppError::from(Error::InvalidInput(format!(
            "invalid document id `{value}`: {error}"
        )))
    })
}
