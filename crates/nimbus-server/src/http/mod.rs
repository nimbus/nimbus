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
    CreateTenantRequest, DataResponse, DocumentDataResponse, DocumentResponse, HealthResponse,
    InsertDocumentRequest, JournalBootstrapResponse, JournalStreamRequest, JournalStreamResponse,
    MaterializedJournalSnapshotResponse, RuntimeDiagnosticsResponse,
    RuntimeLaneDiagnosticsResponse, RuntimeLimitsResponse, RuntimeTenantBudgetResponse,
    TenantEngineDiagnosticsResponse, TenantListResponse, TenantResponse, UpdateDocumentRequest,
    VersionInfoResponse,
};
use crate::state::{AppError, AppState, RequestCancellationGuard};
use crate::tenant::TenantIsolationContext;
use nimbus_compute::scheduling::{
    CronJobsResponse, ScheduleResponse, ScheduledJobResultResponse, ScheduledJobsResponse,
};

mod authz;
mod deploy;
mod documents;
mod graph;
mod local_admin;
mod machines;
mod metadata;
mod queries;
mod resource_control;
mod sandboxes;
mod scheduling;
mod schema;
mod service_grants;
mod services;
mod sessions;
mod source;
mod tenants;
mod ui;
mod version_info;

pub(crate) use deploy::deploy_app;
pub(crate) use documents::{
    delete_document, get_document, insert_document, list_documents, update_document,
};
pub(crate) use graph::call_graph;
pub(crate) use local_admin::{rotate_local_admin_token, shutdown_system};
pub(crate) use machines::{
    create_machine, delete_machine, restart_machine, start_machine, stop_machine, update_machine,
};
pub(crate) use metadata::{
    clear_tenant_consistency_session, encryption_status, examples_redirect, health, license_status,
    runtime_diagnostics, tenant_consistency_report, tenant_engine_diagnostics,
};
pub(crate) use queries::{
    bootstrap_journal, query_documents, query_documents_paginated, read_journal,
};
pub(crate) use sandboxes::{create_sandbox, get_sandbox, list_sandboxes, stop_sandbox};
pub(crate) use scheduling::{
    cancel_scheduled_job, create_cron_job, delete_cron_job, get_scheduled_job_result,
    list_cron_jobs, list_scheduled_jobs, schedule_mutation,
};
pub(crate) use schema::{delete_table_schema, get_schema, get_table_schema, set_table_schema};
pub(crate) use services::{
    create_service_definition, delete_service_definition, get_service, list_service_definitions,
    restart_service, start_service, stop_service, update_service_definition,
};
pub(crate) use sessions::{close_session, get_session, list_sessions, open_session};
pub(crate) use source::module_source;
pub(crate) use tenants::{create_tenant, delete_tenant, list_tenants};
pub(crate) use ui::{
    consume_ui_launch_ticket, create_ui_session, mint_ui_launch_ticket, ui_auth, ui_auth_script,
    ui_csp_middleware, ui_path, ui_root,
};
pub(crate) use version_info::version_info;

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

fn parse_operator_tenant_context(
    value: impl Into<String>,
    surface: &'static str,
) -> Result<TenantIsolationContext, AppError> {
    parse_user_tenant_id(value)
        .map(|tenant_id| TenantIsolationContext::operator(tenant_id, surface))
}
