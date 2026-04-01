use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::Json;
use axum::body::Bytes;
use axum::extract::OriginalUri;
use axum::extract::ws::{Message, WebSocket};
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
use serde_json::{Value, json};
use tokio::sync::mpsc;

mod auth;
mod dispatch;
mod handlers;
mod http_actions;
mod registry;
mod runtime_bridge;
mod runtime_reads;
mod subscriptions;
mod templates;
#[cfg(test)]
mod tests;

pub(crate) use self::handlers::{
    action, cancel_scheduled_job, http_route, http_route_root, mutation, paginated_query, query,
    schedule_after, schedule_at, ws,
};
use self::runtime_reads::{
    ConvexRuntimeIndexRangeRead, ConvexRuntimeReadSet, commit_intersects_runtime_read_set,
    synthesize_runtime_subscription_base_queries,
};
use self::templates::{empty_args, method_name, resolve_http_template, resolve_template};

use self::dispatch::{ConvexHttpRequestContext, ConvexHttpRouteRequest, ConvexSubscriptionEvent};
use crate::protocol::ServerMessage;
use crate::state::{AppError, AppState};
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
    server_request_id: Option<String>,
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
