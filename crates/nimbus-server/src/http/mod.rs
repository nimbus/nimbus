use axum::Json;
use axum::extract::{Path, Query as QueryParams, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Redirect;
use nimbus_core::{
    CreateCronRequest, DocumentId, Error, Page, PaginatedQuery, Query, ScheduleRequest, Schema,
    SequenceNumber, TableName, TableSchema, TenantId,
};
use nimbus_engine::DEFAULT_DURABLE_JOURNAL_STREAM_LIMIT;
use std::sync::Arc;

use crate::protocol::{
    CreateTenantRequest, CronJobsResponse, DataResponse, DocumentDataResponse, DocumentResponse,
    HealthResponse, InsertDocumentRequest, JournalBootstrapResponse, JournalStreamRequest,
    JournalStreamResponse, MaterializedJournalSnapshotResponse, RuntimeDiagnosticsResponse,
    RuntimeLimitsResponse, ScheduleResponse, ScheduledJobResultResponse, ScheduledJobsResponse,
    TenantEngineDiagnosticsResponse, TenantListResponse, TenantResponse, UpdateDocumentRequest,
};
use crate::state::{AppError, AppState, RequestCancellationGuard};

mod deploy;
mod documents;
mod local_admin;
mod machines;
mod metadata;
mod queries;
mod scheduling;
mod schema;
mod services;
mod tenants;
mod ui;

pub(crate) use deploy::deploy_app;
pub(crate) use documents::{
    delete_document, get_document, insert_document, list_documents, update_document,
};
pub(crate) use local_admin::{rotate_local_admin_token, shutdown_system};
pub(crate) use machines::{
    create_machine, delete_machine, restart_machine, start_machine, stop_machine, update_machine,
};
pub(crate) use metadata::{
    demos_redirect, encryption_status, health, license_status, runtime_diagnostics,
    tenant_consistency_report, tenant_engine_diagnostics,
};
pub(crate) use queries::{
    bootstrap_journal, query_documents, query_documents_paginated, read_journal,
};
pub(crate) use scheduling::{
    cancel_scheduled_job, create_cron_job, delete_cron_job, get_scheduled_job_result,
    list_cron_jobs, list_scheduled_jobs, schedule_mutation,
};
pub(crate) use schema::{delete_table_schema, get_schema, get_table_schema, set_table_schema};
pub(crate) use services::{restart_service, start_service, stop_service};
pub(crate) use tenants::{create_tenant, delete_tenant, list_tenants};
pub(crate) use ui::{create_ui_session, ui_auth, ui_csp_middleware, ui_path, ui_root};

fn parse_document_id(value: &str) -> Result<DocumentId, AppError> {
    value.parse().map_err(|error| {
        AppError::from(Error::InvalidInput(format!(
            "invalid document id `{value}`: {error}"
        )))
    })
}

fn parse_user_tenant_id(value: impl Into<String>) -> Result<TenantId, AppError> {
    crate::system_tenant::user_tenant_id(value).map_err(AppError::from)
}
