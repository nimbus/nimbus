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
mod registry_types;
mod request_types;
mod runtime_bridge;
mod runtime_reads;
mod runtime_types;
mod subscription_types;
mod subscriptions;
mod templates;
#[cfg(test)]
mod tests;

use self::dispatch::{ConvexHttpRequestContext, ConvexHttpRouteRequest, ConvexSubscriptionEvent};
pub(crate) use self::handlers::{
    action, cancel_scheduled_job, http_route, http_route_root, mutation, paginated_query, query,
    schedule_after, schedule_at, ws,
};
use self::registry_types::*;
use self::request_types::*;
use self::runtime_reads::{
    ConvexRuntimeIndexRangeRead, ConvexRuntimeReadSet, commit_intersects_runtime_read_set,
    synthesize_runtime_subscription_base_queries,
};
use self::runtime_types::*;
use self::subscription_types::*;
use self::templates::{empty_args, method_name, resolve_http_template, resolve_template};

use crate::protocol::ServerMessage;
use crate::state::{AppError, AppState};

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
