use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::Json;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket};
use axum::extract::{
    OriginalUri, Path as AxumPath, Query as AxumQuery, State, ws::WebSocketUpgrade,
};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use futures::{SinkExt, StreamExt};
use neovex_core::{
    CommitEntry, Cursor, DocumentId, Error, Filter, FilterOp, Mutation, OrderBy, OrderDirection,
    PaginatedQuery, Query, ScheduleRequest, TableName, TenantId, Timestamp,
};
use neovex_engine::SubscriptionUpdate;
use neovex_runtime::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallRequest, InvocationAuth,
    InvocationKind, InvocationRequest, NeovexRuntime, NeovexRuntimeError, RuntimeBundle,
    RuntimeExecutor, RuntimeInvocationContext, RuntimeLimits, RuntimePolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::sync::mpsc;

mod auth;
mod dispatch;
mod http_actions;
mod registry;
mod runtime_bridge;
mod runtime_reads;
mod subscriptions;

use self::runtime_reads::{
    ConvexRuntimeIndexRangeRead, ConvexRuntimeReadSet, commit_intersects_runtime_read_set,
    synthesize_runtime_subscription_base_queries,
};

use self::dispatch::{
    ConvexHttpRequestContext, ConvexHttpRouteRequest, ConvexSubscriptionEvent,
    check_host_cancellation, dispatch_convex_mutation_async, execute_convex_action_async,
    execute_query_result_async, invoke_named_convex_function_async_cancellable,
};
use self::http_actions::dispatch_http_route;
use self::subscriptions::handle_convex_socket_for_tenant;
use crate::protocol::{ScheduleResponse, ServerMessage};
use crate::state::{AppError, AppState, RequestCancellationGuard, record_authenticated_usage};
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexQueryRequest {
    Named(ConvexNamedRequest),
    Raw { query: Query },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexPaginatedQueryRequest {
    Named(ConvexNamedPaginatedQueryRequest),
    Raw { query: PaginatedQuery },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexMutationRequest {
    Named(ConvexNamedRequest),
    Raw { mutation: Mutation },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexActionRequest {
    Named(ConvexNamedRequest),
    Raw { action: ConvexAction },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexScheduleAfterRequest {
    Named(ConvexNamedScheduleAfterRequest),
    Raw {
        mutation: Mutation,
        run_after_ms: u64,
    },
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ConvexScheduleAtRequest {
    Named(ConvexNamedScheduleAtRequest),
    Raw { mutation: Mutation, run_at_ms: u64 },
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConvexNamedRequest {
    pub name: String,
    #[serde(default = "empty_args")]
    pub args: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConvexNamedPaginatedQueryRequest {
    pub name: String,
    #[serde(default = "empty_args")]
    pub args: Value,
    pub page_size: usize,
    #[serde(default)]
    pub cursor: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConvexNamedScheduleAfterRequest {
    pub name: String,
    #[serde(default = "empty_args")]
    pub args: Value,
    pub run_after_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ConvexNamedScheduleAtRequest {
    pub name: String,
    #[serde(default = "empty_args")]
    pub args: Value,
    pub run_at_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum ConvexAction {
    Query { query: Query },
    PaginatedQuery { query: PaginatedQuery },
    Mutation { mutation: Mutation },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ConvexExecutableQuery {
    Query(Query),
    Read(ConvexReadCommand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ConvexReadCommand {
    Get { table: TableName, id: DocumentId },
    First { query: Query },
    Unique { query: Query },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ConvexExecutableMutation {
    Mutation(Mutation),
    Query(ConvexExecutableQuery),
    Scheduled(ConvexScheduledCommand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ConvexExecutableAction {
    Action(ConvexAction),
    Scheduled(ConvexScheduledCommand),
    Call(ConvexFunctionCallCommand),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ConvexFunctionCallCommand {
    #[serde(rename = "call_query")]
    Query {
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
    #[serde(rename = "call_mutation")]
    Mutation {
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
    #[serde(rename = "call_action")]
    Action {
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ConvexScheduledCommand {
    #[serde(rename = "schedule_run_after")]
    RunAfter {
        delay_ms: u64,
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
    #[serde(rename = "schedule_run_at")]
    RunAt {
        timestamp_ms: u64,
        name: String,
        #[serde(default)]
        visibility: Option<ConvexFunctionVisibility>,
        #[serde(default = "empty_args")]
        args: Value,
    },
    #[serde(rename = "schedule_cancel")]
    Cancel { job_id: String },
}

#[derive(Debug, Clone)]
enum ConvexSubscriptionTransform {
    Identity,
    Get {
        document_id: DocumentId,
    },
    First,
    Unique,
    RuntimeNamedQuery {
        name: String,
        args: Value,
        auth: Option<InvocationAuth>,
        read_set: Option<ConvexRuntimeReadSet>,
    },
    RuntimeNamedPaginatedQuery {
        name: String,
        args: Value,
        page_size: usize,
        cursor: Option<String>,
        auth: Option<InvocationAuth>,
        read_set: Option<ConvexRuntimeReadSet>,
    },
}

#[derive(Debug)]
struct ConvexRuntimeSubscriptionSetup {
    initial_value: Value,
    base_queries: Vec<Query>,
    transform: ConvexSubscriptionTransform,
}

#[derive(Debug)]
struct ConvexRuntimeSubscriptionHandle {
    convex_subscription_id: u64,
    underlying_subscription_ids: Vec<u64>,
}

#[derive(Debug, Default)]
struct ConvexSubscriptionTransforms {
    by_id: HashMap<u64, ConvexSubscriptionTransform>,
    by_request: HashMap<String, ConvexSubscriptionTransform>,
}

#[derive(Debug, Clone)]
pub struct ConvexRegistry {
    functions: HashMap<String, ConvexFunctionDefinition>,
    http_routes: Vec<ConvexHttpRouteDefinition>,
    runtime_bundle: Option<RuntimeBundle>,
    auth_verifier: Arc<auth::ConvexAuthVerifier>,
    runtime_policy: Arc<RuntimePolicy>,
    runtime_executor: Arc<RuntimeExecutor>,
}

impl Default for ConvexRegistry {
    fn default() -> Self {
        let runtime_policy = Arc::new(RuntimePolicy::default());
        let runtime_executor = Arc::new(RuntimeExecutor::new(runtime_policy.clone()));
        Self {
            functions: HashMap::new(),
            http_routes: Vec::new(),
            runtime_bundle: None,
            auth_verifier: Arc::new(auth::ConvexAuthVerifier::empty()),
            runtime_policy,
            runtime_executor,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexManifest {
    functions: Vec<ConvexFunctionDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexHttpRouteManifest {
    routes: Vec<ConvexHttpRouteDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexFunctionDefinition {
    name: String,
    kind: ConvexFunctionKind,
    #[serde(default)]
    visibility: ConvexFunctionVisibility,
    #[serde(default)]
    schedulable: bool,
    #[serde(default)]
    runtime_handler: Option<String>,
    plan: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexHttpRouteDefinition {
    #[serde(default)]
    name: Option<String>,
    method: ConvexHttpMethod,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    path_prefix: Option<String>,
    plan: ConvexHttpActionPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConvexHttpActionPlan {
    #[serde(default)]
    operation: Option<Value>,
    response: ConvexHttpResponseTemplate,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConvexHttpResponseTemplate {
    kind: ConvexHttpResponseKind,
    body: Value,
    #[serde(default)]
    status: Option<Value>,
    #[serde(default)]
    headers: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConvexHttpResponseKind {
    Json,
    Text,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
enum ConvexHttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ConvexFunctionKind {
    Query,
    PaginatedQuery,
    Mutation,
    Action,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
enum ConvexFunctionVisibility {
    #[default]
    Public,
    Internal,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ConvexClientMessage {
    Authenticate {
        token: String,
    },
    ClearAuth,
    Subscribe {
        request_id: String,
        query: Query,
    },
    SubscribeNamed {
        request_id: String,
        name: String,
        #[serde(default = "empty_args")]
        args: Value,
        #[serde(default)]
        page_size: Option<usize>,
        #[serde(default)]
        cursor: Option<String>,
    },
    Unsubscribe {
        subscription_id: u64,
    },
}

#[derive(Debug, Deserialize)]
struct ConvexRuntimeInvokePayload {
    request: InvocationRequest,
    definition: ConvexFunctionDefinition,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeHttpRouteInvokePayload {
    request: InvocationRequest,
    route: ConvexHttpRouteDefinition,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeQueryPayload {
    query: ConvexExecutableQuery,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimePaginatedQueryPayload {
    query: Query,
    page_size: usize,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeMutationPayload {
    mutation: ConvexExecutableMutation,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeActionPayload {
    action: ConvexExecutableAction,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeFunctionCallPayload {
    name: String,
    #[serde(default)]
    visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    args: Value,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    auth: Option<InvocationAuth>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeDbGetPayload {
    table: TableName,
    id: DocumentId,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeDbInsertPayload {
    table: TableName,
    fields: serde_json::Map<String, Value>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeDbPatchPayload {
    table: TableName,
    id: DocumentId,
    patch: serde_json::Map<String, Value>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeDbDeletePayload {
    table: TableName,
    id: DocumentId,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeQueryStartPayload {
    table: TableName,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeQueryWithIndexPayload {
    builder_id: String,
    index_name: String,
    #[serde(default)]
    filters: Vec<Filter>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeQueryFilterPayload {
    builder_id: String,
    #[serde(default)]
    filters: Vec<Filter>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeQueryOrderPayload {
    builder_id: String,
    direction: OrderDirection,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeQueryTerminalPayload {
    builder_id: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeQueryTakePayload {
    builder_id: String,
    limit: usize,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeQueryPaginatePayload {
    builder_id: String,
    page_size: usize,
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeSchedulerRunAfterPayload {
    delay_ms: u64,
    name: String,
    #[serde(default)]
    visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    args: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeSchedulerRunAtPayload {
    timestamp_ms: u64,
    name: String,
    #[serde(default)]
    visibility: Option<ConvexFunctionVisibility>,
    #[serde(default = "empty_args")]
    args: Value,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ConvexRuntimeSchedulerCancelPayload {
    job_id: String,
    #[serde(default)]
    session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ConvexRuntimeResponseEnvelope {
    Ok { value: Value },
    Error { error: ConvexRuntimeEncodedError },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ConvexRuntimeEncodedError {
    Cancelled,
    TenantNotFound { tenant_id: String },
    DocumentNotFound { document_id: String },
    ScheduledJobNotFound { job_id: String },
    AlreadyExists { message: String },
    InvalidInput { message: String },
    SchemaValidation { message: String },
    SchemaNotFound { table: String },
    Storage { message: String },
    Serialization { message: String },
    Internal { message: String },
}

#[derive(Clone)]
struct ConvexRuntimeBridge {
    service: Arc<neovex_engine::Service>,
    registry: Arc<ConvexRegistry>,
    tenant_id: TenantId,
    session_id: String,
    max_nested_runtime_invocations: usize,
    remaining_nested_runtime_invocations: Arc<AtomicUsize>,
    query_builders: Arc<Mutex<ConvexRuntimeQueryBuilders>>,
    read_set: Arc<Mutex<ConvexRuntimeReadSet>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ConvexHttpResponseParts {
    kind: ConvexHttpResponseKind,
    body: Value,
    #[serde(default)]
    status: Option<Value>,
    #[serde(default)]
    headers: Option<Value>,
}

#[derive(Debug, Default)]
struct ConvexRuntimeQueryBuilders {
    next_builder_id: u64,
    builders: HashMap<String, ConvexRuntimeQueryBuilderState>,
}

#[derive(Debug, Clone)]
struct ConvexRuntimeQueryBuilderState {
    table: TableName,
    filters: Vec<Filter>,
    order: Option<OrderBy>,
    order_field_hint: Option<String>,
    index_name: Option<String>,
}

pub(crate) async fn query(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexQueryRequest>,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex query route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let data = match request {
        ConvexQueryRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            invoke_named_convex_function_async_cancellable(
                &service,
                &registry,
                &tenant_id,
                InvocationRequest {
                    kind: InvocationKind::Query,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                },
                request_cancellation.token(),
            )
            .await?
        }
        ConvexQueryRequest::Named(request) => {
            let query = registry.resolve_query(&request.name, &request.args)?;
            let request_cancellation = RequestCancellationGuard::new();
            execute_query_result_async(
                &service,
                &tenant_id,
                query,
                Some(request_cancellation.token()),
            )
            .await?
        }
        ConvexQueryRequest::Raw { query } => {
            let request_cancellation = RequestCancellationGuard::new();
            execute_query_result_async(
                &service,
                &tenant_id,
                ConvexExecutableQuery::Query(query),
                Some(request_cancellation.token()),
            )
            .await?
        }
    };
    Ok(Json(data))
}

/// Executes a Convex-style paginated query over Neovex's pagination engine.
pub(crate) async fn paginated_query(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexPaginatedQueryRequest>,
) -> Result<Json<neovex_core::Page>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex paginated query route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let page = match request {
        ConvexPaginatedQueryRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            let value = invoke_named_convex_function_async_cancellable(
                &service,
                &registry,
                &tenant_id,
                InvocationRequest {
                    kind: InvocationKind::PaginatedQuery,
                    function_name: request.name,
                    args: request.args,
                    page_size: Some(request.page_size),
                    cursor: request.cursor,
                    auth: auth.clone(),
                },
                request_cancellation.token(),
            )
            .await?;
            serde_json::from_value(value)
                .map_err(|error| AppError::from(Error::Serialization(error.to_string())))?
        }
        ConvexPaginatedQueryRequest::Named(request) => {
            let query = registry.resolve_paginated_query(
                &request.name,
                &request.args,
                request.page_size,
                request.cursor,
            )?;
            let request_cancellation = RequestCancellationGuard::new();
            let cancellation = request_cancellation.token();
            let cancellation_check = cancellation.clone();
            service
                .paginate_documents_async_cancellable(
                    tenant_id.clone(),
                    query,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&cancellation_check),
                )
                .await?
        }
        ConvexPaginatedQueryRequest::Raw { query } => {
            let request_cancellation = RequestCancellationGuard::new();
            let cancellation = request_cancellation.token();
            let cancellation_check = cancellation.clone();
            service
                .paginate_documents_async_cancellable(
                    tenant_id.clone(),
                    query,
                    cancellation.cancelled(),
                    move || check_host_cancellation(&cancellation_check),
                )
                .await?
        }
    };
    Ok(Json(page))
}

/// Executes a Convex-style mutation over Neovex's existing mutation engine.
pub(crate) async fn mutation(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexMutationRequest>,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex mutation route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let value = match request {
        ConvexMutationRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            invoke_named_convex_function_async_cancellable(
                &service,
                &registry,
                &tenant_id,
                InvocationRequest {
                    kind: InvocationKind::Mutation,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                },
                request_cancellation.token(),
            )
            .await?
        }
        ConvexMutationRequest::Named(request) => {
            let mutation = registry.resolve_mutation(&request.name, &request.args)?;
            dispatch_convex_mutation_async(&service, &registry, &tenant_id, mutation, None).await?
        }
        ConvexMutationRequest::Raw { mutation } => {
            dispatch_convex_mutation_async(
                &service,
                &registry,
                &tenant_id,
                ConvexExecutableMutation::Mutation(mutation),
                None,
            )
            .await?
        }
    };
    Ok(Json(value))
}

/// Executes a Convex-style action backed by an existing Neovex operation.
pub(crate) async fn action(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexActionRequest>,
) -> Result<Json<Value>, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex action route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let value = match request {
        ConvexActionRequest::Named(request) if registry.runtime_bundle().is_some() => {
            let request_cancellation = RequestCancellationGuard::new();
            invoke_named_convex_function_async_cancellable(
                &service,
                &registry,
                &tenant_id,
                InvocationRequest {
                    kind: InvocationKind::Action,
                    function_name: request.name,
                    args: request.args,
                    page_size: None,
                    cursor: None,
                    auth: auth.clone(),
                },
                request_cancellation.token(),
            )
            .await?
        }
        ConvexActionRequest::Named(request) => {
            let action = registry.resolve_action(&request.name, &request.args)?;
            execute_convex_action_async(&service, &registry, &tenant_id, action, None).await?
        }
        ConvexActionRequest::Raw { action } => {
            execute_convex_action_async(
                &service,
                &registry,
                &tenant_id,
                ConvexExecutableAction::Action(action),
                None,
            )
            .await?
        }
    };
    Ok(Json(value))
}

/// Executes a Convex-style httpAction route backed by the convex manifest.
pub(crate) async fn http_route_root(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    method: Method,
    headers: HeaderMap,
    original_uri: OriginalUri,
    query: AxumQuery<HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, AppError> {
    dispatch_http_route(
        state,
        tenant_id,
        ConvexHttpRouteRequest {
            request_path: "/".to_string(),
            method,
            headers,
            original_uri,
            query: query.0,
            body,
        },
    )
    .await
}

/// Executes a Convex-style httpAction route backed by the convex manifest.
pub(crate) async fn http_route(
    State(state): State<Arc<AppState>>,
    AxumPath((tenant_id, path)): AxumPath<(String, String)>,
    method: Method,
    headers: HeaderMap,
    original_uri: OriginalUri,
    query: AxumQuery<HashMap<String, String>>,
    body: Bytes,
) -> Result<Response, AppError> {
    dispatch_http_route(
        state,
        tenant_id,
        ConvexHttpRouteRequest {
            request_path: normalize_http_request_path(&path),
            method,
            headers,
            original_uri,
            query: query.0,
            body,
        },
    )
    .await
}

/// Schedules a public convex mutation by relative delay.
pub(crate) async fn schedule_after(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexScheduleAfterRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex schedule-after route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let request = match request {
        ConvexScheduleAfterRequest::Named(request) => ScheduleRequest {
            run_after_ms: request.run_after_ms,
            mutation: registry.resolve_scheduled_mutation(&request.name, &request.args)?,
        },
        ConvexScheduleAfterRequest::Raw {
            mutation,
            run_after_ms,
        } => ScheduleRequest {
            run_after_ms,
            mutation,
        },
    };

    let job_id = service.schedule_mutation_async(tenant_id, request).await?;
    Ok((
        StatusCode::CREATED,
        Json(ScheduleResponse {
            job_id: job_id.to_string(),
        }),
    ))
}

/// Schedules a public convex mutation for an absolute timestamp.
pub(crate) async fn schedule_at(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    Json(request): Json<ConvexScheduleAtRequest>,
) -> Result<(StatusCode, Json<ScheduleResponse>), AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex schedule-at route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    let (run_at_ms, mutation) = match request {
        ConvexScheduleAtRequest::Named(request) => (
            request.run_at_ms,
            registry.resolve_scheduled_mutation(&request.name, &request.args)?,
        ),
        ConvexScheduleAtRequest::Raw {
            mutation,
            run_at_ms,
        } => (run_at_ms, mutation),
    };
    let delay_ms = run_at_ms.saturating_sub(Timestamp::now().0);
    let request = ScheduleRequest {
        run_after_ms: delay_ms,
        mutation,
    };

    let job_id = service.schedule_mutation_async(tenant_id, request).await?;
    Ok((
        StatusCode::CREATED,
        Json(ScheduleResponse {
            job_id: job_id.to_string(),
        }),
    ))
}

/// Cancels a pending convex scheduled job.
pub(crate) async fn cancel_scheduled_job(
    State(state): State<Arc<AppState>>,
    AxumPath((tenant_id, job_id)): AxumPath<(String, String)>,
    headers: HeaderMap,
) -> Result<StatusCode, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let job_id = parse_job_id(&job_id)?;
    let service = state.service.clone();
    let registry = state
        .convex_registry
        .clone()
        .expect("convex scheduled job cancel route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;
    service
        .cancel_scheduled_job_async(tenant_id, job_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// WebSocket endpoint for Convex-style query subscriptions bound to a tenant in the URL.
pub(crate) async fn ws(
    State(state): State<Arc<AppState>>,
    AxumPath(tenant_id): AxumPath<String>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Result<Response, AppError> {
    let tenant_id = TenantId::new(tenant_id)?;
    let service = state.service.clone();
    let tenant_check = tenant_id.clone();
    service.ensure_tenant_exists_async(tenant_check).await?;
    let registry = state
        .convex_registry
        .clone()
        .expect("convex websocket route requires Convex support state");
    let auth = registry.verify_authorization_header(&headers).await?;
    record_authenticated_usage(&state, auth.as_ref()).await;

    Ok(
        ws.on_upgrade(move |socket| {
            handle_convex_socket_for_tenant(socket, state, tenant_id, auth)
        }),
    )
}

fn resolve_template(template: &Value, args: &Value) -> Result<Value, Error> {
    let args = args_object(args)?;
    match template {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(template.clone()),
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_template(item, &Value::Object(args.clone())))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(object) => {
            if let Some(argument_name) = placeholder_name(object) {
                return args.get(argument_name).cloned().ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "convex function argument missing: {argument_name}"
                    ))
                });
            }

            let mut resolved = Map::new();
            for (key, nested) in object {
                resolved.insert(
                    key.clone(),
                    resolve_template(nested, &Value::Object(args.clone()))?,
                );
            }
            Ok(Value::Object(resolved))
        }
    }
}

fn resolve_http_template(
    template: &Value,
    request: &ConvexHttpRequestContext,
    result: Option<&Value>,
) -> Result<Value, Error> {
    match template {
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(template.clone()),
        Value::Array(items) => items
            .iter()
            .map(|item| resolve_http_template(item, request, result))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::Array),
        Value::Object(object) => {
            if let Some(argument_name) = placeholder_name(object) {
                return Err(Error::InvalidInput(format!(
                    "convex httpAction does not support args placeholder: {argument_name}"
                )));
            }
            if let Some(request_descriptor) = object.get("$request") {
                return resolve_http_request_placeholder(request_descriptor, request);
            }
            if let Some(result_descriptor) = object.get("$result") {
                return resolve_http_result_placeholder(result_descriptor, result);
            }

            let mut resolved = Map::new();
            for (key, nested) in object {
                resolved.insert(key.clone(), resolve_http_template(nested, request, result)?);
            }
            Ok(Value::Object(resolved))
        }
    }
}

fn resolve_http_request_placeholder(
    descriptor: &Value,
    request: &ConvexHttpRequestContext,
) -> Result<Value, Error> {
    let Value::Object(object) = descriptor else {
        return Err(Error::InvalidInput(
            "convex httpAction request placeholder must be an object".to_string(),
        ));
    };
    let source = object
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::InvalidInput(
                "convex httpAction request placeholder is missing source".to_string(),
            )
        })?;
    match source {
        "method" => Ok(Value::String(request.method.clone())),
        "url" => Ok(Value::String(request.url.clone())),
        "pathname" => Ok(Value::String(request.pathname.clone())),
        "text" => Ok(Value::String(request.body_text.clone())),
        "header" => {
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    Error::InvalidInput(
                        "convex httpAction header placeholder is missing name".to_string(),
                    )
                })?
                .to_ascii_lowercase();
            Ok(request
                .headers
                .get(&name)
                .cloned()
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "query" => {
            let name = object.get("name").and_then(Value::as_str).ok_or_else(|| {
                Error::InvalidInput(
                    "convex httpAction query placeholder is missing name".to_string(),
                )
            })?;
            Ok(request
                .query
                .get(name)
                .cloned()
                .map(Value::String)
                .unwrap_or(Value::Null))
        }
        "json" => {
            let json_body = parse_http_json_body(request)?;
            let path = object
                .get("path")
                .and_then(Value::as_str)
                .unwrap_or_default();
            resolve_nested_value(&json_body, path)
        }
        _ => Err(Error::InvalidInput(format!(
            "unsupported convex httpAction request source: {source}"
        ))),
    }
}

fn resolve_http_result_placeholder(
    descriptor: &Value,
    result: Option<&Value>,
) -> Result<Value, Error> {
    let Some(result) = result else {
        return Err(Error::InvalidInput(
            "convex httpAction response referenced an operation result, but no operation ran"
                .to_string(),
        ));
    };
    let Value::Object(object) = descriptor else {
        return Err(Error::InvalidInput(
            "convex httpAction result placeholder must be an object".to_string(),
        ));
    };
    let path = object
        .get("path")
        .and_then(Value::as_str)
        .unwrap_or_default();
    resolve_nested_value(result, path)
}

fn parse_http_json_body(request: &ConvexHttpRequestContext) -> Result<Value, Error> {
    if request.body_bytes.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(&request.body_bytes).map_err(|error| {
        Error::InvalidInput(format!(
            "convex httpAction request body must be valid JSON: {error}"
        ))
    })
}

fn resolve_nested_value(value: &Value, path: &str) -> Result<Value, Error> {
    if path.is_empty() {
        return Ok(value.clone());
    }

    let mut current = value;
    for segment in path.split('.') {
        match current {
            Value::Object(object) => {
                current = object.get(segment).ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "convex httpAction placeholder path not found: {path}"
                    ))
                })?;
            }
            Value::Array(items) => {
                let index = segment.parse::<usize>().map_err(|_| {
                    Error::InvalidInput(format!(
                        "convex httpAction placeholder path segment is not a valid index: {segment}"
                    ))
                })?;
                current = items.get(index).ok_or_else(|| {
                    Error::InvalidInput(format!(
                        "convex httpAction placeholder index out of bounds: {path}"
                    ))
                })?;
            }
            _ => {
                return Err(Error::InvalidInput(format!(
                    "convex httpAction placeholder path not found: {path}"
                )));
            }
        }
    }
    Ok(current.clone())
}

fn placeholder_name(object: &Map<String, Value>) -> Option<&str> {
    if object.len() == 1 {
        object.get("$arg").and_then(Value::as_str)
    } else {
        None
    }
}

fn args_object(args: &Value) -> Result<Map<String, Value>, Error> {
    match args {
        Value::Null => Ok(Map::new()),
        Value::Object(object) => Ok(object.clone()),
        _ => Err(Error::InvalidInput(
            "convex function args must be a JSON object".to_string(),
        )),
    }
}

fn empty_args() -> Value {
    json!({})
}

fn normalize_http_request_path(path: &str) -> String {
    if path.is_empty() {
        "/".to_string()
    } else if path.starts_with('/') {
        path.to_string()
    } else {
        format!("/{path}")
    }
}

fn method_name(method: &Method) -> &str {
    method.as_str()
}

fn parse_job_id(value: &str) -> Result<DocumentId, AppError> {
    value.parse().map_err(|error| {
        AppError::from(Error::InvalidInput(format!("invalid document id: {error}")))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use neovex_engine::Service;
    use serde_json::json;
    use tempfile::{TempDir, tempdir};

    use super::dispatch::execute_convex_action_cancellable;
    use super::*;

    fn runtime_bridge_fixture() -> (TempDir, Arc<Service>, TenantId, ConvexRuntimeBridge) {
        let tempdir = tempdir().expect("runtime action tempdir should build");
        let service = Arc::new(Service::new(tempdir.path()).expect("service should build"));
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should be created");
        let registry = Arc::new(ConvexRegistry::empty());
        let bridge = ConvexRuntimeBridge::new(service.clone(), registry, tenant_id.clone());
        (tempdir, service, tenant_id, bridge)
    }

    #[test]
    fn execute_convex_action_cancellable_short_circuits_before_mutation_dispatch() {
        let tempdir = tempdir().expect("runtime action tempdir should build");
        let service = Service::new(tempdir.path()).expect("service should build");
        let tenant_id = TenantId::new("demo").expect("tenant id should build");
        service
            .create_tenant(tenant_id.clone())
            .expect("tenant should be created");
        let registry = ConvexRegistry::empty();
        let cancellation = HostCallCancellation::default();
        cancellation.cancel();

        let result = execute_convex_action_cancellable(
            &service,
            &registry,
            &tenant_id,
            ConvexExecutableAction::Action(ConvexAction::Mutation {
                mutation: Mutation::Insert {
                    table: TableName::new("messages").expect("table should build"),
                    fields: serde_json::Map::from_iter([("body".to_string(), json!("hello"))]),
                },
            }),
            &cancellation,
        );

        assert!(matches!(result, Err(Error::Cancelled)));
        let documents = service
            .query_documents(
                &tenant_id,
                &Query {
                    table: TableName::new("messages").expect("table should build"),
                    filters: Vec::new(),
                    order: None,
                    limit: None,
                },
            )
            .expect("document query should succeed");
        assert!(documents.is_empty());
    }

    #[test]
    fn runtime_cancellable_db_get_short_circuits_before_dispatch() {
        let (_tempdir, service, tenant_id, bridge) = runtime_bridge_fixture();
        let document_id = service
            .insert_document(
                &tenant_id,
                TableName::new("messages").expect("table should build"),
                serde_json::Map::from_iter([("body".to_string(), json!("hello"))]),
            )
            .expect("document insert should succeed");
        let cancellation = HostCallCancellation::default();
        cancellation.cancel();

        let result = bridge.dispatch_host_call_cancellable(
            HostCallRequest {
                operation: "convex.ctx.db.get".to_string(),
                payload: json!({
                    "table": "messages",
                    "id": document_id.to_string(),
                }),
            },
            &cancellation,
        );

        assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
    }

    #[tokio::test]
    async fn runtime_async_db_get_precancel_records_canceled_host_op_metric() {
        let (_tempdir, service, tenant_id, bridge) = runtime_bridge_fixture();
        let document_id = service
            .insert_document(
                &tenant_id,
                TableName::new("messages").expect("table should build"),
                serde_json::Map::from_iter([("body".to_string(), json!("hello"))]),
            )
            .expect("document insert should succeed");
        let cancellation = HostCallCancellation::default();
        cancellation.cancel();

        let result = bridge
            .call_async(
                HostCallRequest {
                    operation: "convex.ctx.db.get".to_string(),
                    payload: json!({
                        "table": "messages",
                        "id": document_id.to_string(),
                    }),
                },
                cancellation,
            )
            .await;

        assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
        assert_eq!(
            bridge.registry.runtime_metrics_snapshot().canceled_host_ops,
            1
        );
    }

    #[test]
    fn runtime_cancellable_http_route_short_circuits_before_mutation_dispatch() {
        let (_tempdir, service, tenant_id, bridge) = runtime_bridge_fixture();
        let cancellation = HostCallCancellation::default();
        cancellation.cancel();

        let result = bridge.dispatch_host_call_cancellable(
            HostCallRequest {
                operation: "convex.http_route".to_string(),
                payload: json!({
                    "request": {
                        "kind": "action",
                        "function_name": "messages:send",
                        "args": {
                            "method": "POST",
                            "url": "http://localhost/messages",
                            "pathname": "/messages",
                            "query": {},
                            "headers": {},
                            "body_bytes": [],
                            "body_text": ""
                        }
                    },
                    "route": {
                        "name": "messages:send",
                        "method": "POST",
                        "path": "/messages",
                        "plan": {
                            "operation": {
                                "type": "mutation",
                                "mutation": {
                                    "type": "insert",
                                    "table": "messages",
                                    "fields": {
                                        "body": "hello"
                                    }
                                }
                            },
                            "response": {
                                "kind": "json",
                                "body": {
                                    "ok": true
                                }
                            }
                        }
                    }
                }),
            },
            &cancellation,
        );

        assert!(matches!(result, Err(NeovexRuntimeError::Cancelled)));
        let documents = service
            .query_documents(
                &tenant_id,
                &Query {
                    table: TableName::new("messages").expect("table should build"),
                    filters: Vec::new(),
                    order: None,
                    limit: None,
                },
            )
            .expect("document query should succeed");
        assert!(documents.is_empty());
    }
}
