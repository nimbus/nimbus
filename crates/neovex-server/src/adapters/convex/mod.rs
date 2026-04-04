use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Bytes;
use axum::extract::OriginalUri;
use axum::extract::ws::{Message, WebSocket};
use axum::http::{HeaderMap, Method};
use futures::{SinkExt, StreamExt};
use neovex_core::{
    CommitEntry, Cursor, DocumentId, Error, Filter, FilterOp, Mutation, OrderBy, OrderDirection,
    PaginatedQuery, Query, ScheduleRequest, Schema, TableName, TenantId, Timestamp,
};
use neovex_engine::SubscriptionUpdate;
use neovex_runtime::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallOperation, HostCallRequest,
    InvocationAuth, InvocationKind, InvocationRequest, NeovexRuntimeError, RuntimeBundle,
    RuntimeExecutor, RuntimePolicy,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::mpsc;

mod auth;
mod execution;
mod handlers;
mod host_bridge;
mod http_actions;
mod manifest;
mod registry;
mod requests;
mod subscriptions;
mod templates;
#[cfg(test)]
mod tests;

use self::auth::normalize_principal_context;
use self::execution::{ConvexHttpRequestContext, ConvexHttpRouteRequest, ConvexSubscriptionEvent};
pub(crate) use self::handlers::{
    action, cancel_scheduled_job, http_route, http_route_root, mutation, paginated_query, query,
    schedule_after, schedule_at, ws,
};
use self::host_bridge::*;
use self::manifest::*;
use self::requests::*;
use self::templates::{empty_args, resolve_http_template};

use crate::protocol::ServerMessage;
use crate::runtime::read_tracking::{
    RuntimeIndexRangeRead, RuntimeReadSet, synthesize_runtime_subscription_base_queries,
};
use crate::state::{AppError, AppState};

#[derive(Debug, Clone)]
pub struct ConvexRegistry {
    functions: HashMap<String, ConvexFunctionDefinition>,
    http_routes: Vec<ConvexHttpRouteDefinition>,
    schema: Option<Schema>,
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
            schema: None,
            runtime_bundle: None,
            auth_verifier: Arc::new(auth::ConvexAuthVerifier::empty()),
            runtime_policy,
            runtime_executor,
        }
    }
}
