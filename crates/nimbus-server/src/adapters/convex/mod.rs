use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Bytes;
use axum::extract::OriginalUri;
use axum::extract::ws::{Message, WebSocket};
use axum::http::{HeaderMap, Method};
use futures::future::BoxFuture;
use futures::{SinkExt, StreamExt};
use nimbus_core::{
    CommitEntry, Cursor, DocumentId, Error, Filter, FilterOp, Mutation, OrderBy, OrderDirection,
    PaginatedQuery, Query, ScheduleRequest, Schema, TableName, TenantId, Timestamp,
};
use nimbus_engine::SubscriptionUpdate;
#[cfg(test)]
use nimbus_runtime::HostCallOperation;
use nimbus_runtime::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallRequest, InvocationAuth,
    InvocationKind, InvocationRequest, NimbusRuntimeError, RuntimeBundle,
    RuntimeCompatibilityTarget, RuntimeExecutor, RuntimeLimits, RuntimePolicy, RuntimePreset,
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

use self::execution::{ConvexHttpRequestContext, ConvexHttpRouteRequest, ConvexSubscriptionEvent};
pub(crate) use self::handlers::{
    action, cancel_scheduled_job, http_route, http_route_root, mutation, paginated_query, query,
    schedule_after, schedule_at, ws,
};
use self::host_bridge::*;
pub(crate) use self::host_bridge::{
    ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope,
    ConvexRuntimeResponseEnvelope,
};
use self::manifest::*;
pub(crate) use self::registry::{
    ConvexFunctionDeploySummary, ConvexHttpRouteDeploySummary, ConvexRegistryDeploySummary,
};
use self::requests::*;
use self::templates::{empty_args, resolve_http_template};

use crate::application_auth::ApplicationAuthVerifier;
use crate::execution::read_tracking::{
    RuntimeIndexRangeRead, RuntimeReadSet, synthesize_runtime_subscription_base_queries,
};
use crate::protocol::ServerMessage;
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
    node20_runtime_policy: Arc<RuntimePolicy>,
    node20_runtime_executor: Arc<RuntimeExecutor>,
    node22_runtime_policy: Arc<RuntimePolicy>,
    node22_runtime_executor: Arc<RuntimeExecutor>,
    node24_runtime_policy: Arc<RuntimePolicy>,
    node24_runtime_executor: Arc<RuntimeExecutor>,
}

impl Default for ConvexRegistry {
    fn default() -> Self {
        let runtime_policy = Arc::new(RuntimePolicy::default());
        let runtime_executor = Arc::new(RuntimeExecutor::new(runtime_policy.clone()));
        let (node20_runtime_policy, node20_runtime_executor) =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node20);
        let (node22_runtime_policy, node22_runtime_executor) =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node22);
        let (node24_runtime_policy, node24_runtime_executor) =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node24);
        Self {
            functions: HashMap::new(),
            http_routes: Vec::new(),
            schema: None,
            runtime_bundle: None,
            auth_verifier: Arc::new(auth::ConvexAuthVerifier::empty()),
            runtime_policy,
            runtime_executor,
            node20_runtime_policy,
            node20_runtime_executor,
            node22_runtime_policy,
            node22_runtime_executor,
            node24_runtime_policy,
            node24_runtime_executor,
        }
    }
}

fn convex_node_runtime_lane(
    mut base_limits: RuntimeLimits,
    target: RuntimeCompatibilityTarget,
) -> (Arc<RuntimePolicy>, Arc<RuntimeExecutor>) {
    base_limits.compatibility_target = target;
    base_limits.preset = RuntimePreset::Application;
    base_limits.grants = nimbus_runtime::RuntimeGrants::application_node();
    let policy = Arc::new(RuntimePolicy::new(base_limits));
    let executor = Arc::new(RuntimeExecutor::new(policy.clone()));
    (policy, executor)
}

impl ApplicationAuthVerifier for ConvexRegistry {
    fn verify_bearer_token<'a>(
        &'a self,
        token: &'a str,
    ) -> BoxFuture<'a, Result<InvocationAuth, AppError>> {
        Box::pin(async move { ConvexRegistry::verify_bearer_token(self, token).await })
    }
}
