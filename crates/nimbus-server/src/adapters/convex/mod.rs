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
use crate::execution::invocations::RuntimeBundleProvenanceConfig;
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
    artifact_guard: Option<Arc<tempfile::TempDir>>,
    auth_verifier: Arc<auth::ConvexAuthVerifier>,
    runtime_policy: Arc<RuntimePolicy>,
    runtime_executor: Arc<RuntimeExecutor>,
    node20_runtime_policy: Arc<RuntimePolicy>,
    node20_runtime_executor: Arc<RuntimeExecutor>,
    node22_runtime_policy: Arc<RuntimePolicy>,
    node22_runtime_executor: Arc<RuntimeExecutor>,
    node24_runtime_policy: Arc<RuntimePolicy>,
    node24_runtime_executor: Arc<RuntimeExecutor>,
    bun_jsc_runtime_policy: Arc<RuntimePolicy>,
    bun_jsc_runtime_executor: Arc<RuntimeExecutor>,
    runtime_bundle_provenance: Option<RuntimeBundleProvenanceConfig>,
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
        let (bun_jsc_runtime_policy, bun_jsc_runtime_executor) =
            convex_bun_jsc_runtime_lane(RuntimeLimits::default());
        Self {
            functions: HashMap::new(),
            http_routes: Vec::new(),
            schema: None,
            runtime_bundle: None,
            artifact_guard: None,
            auth_verifier: Arc::new(auth::ConvexAuthVerifier::empty()),
            runtime_policy,
            runtime_executor,
            node20_runtime_policy,
            node20_runtime_executor,
            node22_runtime_policy,
            node22_runtime_executor,
            node24_runtime_policy,
            node24_runtime_executor,
            bun_jsc_runtime_policy,
            bun_jsc_runtime_executor,
            runtime_bundle_provenance: None,
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

fn convex_bun_jsc_runtime_lane(
    mut base_limits: RuntimeLimits,
) -> (Arc<RuntimePolicy>, Arc<RuntimeExecutor>) {
    let bun_defaults = RuntimeLimits::application_bun_jsc();
    base_limits.backend_kind = bun_defaults.backend_kind;
    base_limits.backend_trust_tier = bun_defaults.backend_trust_tier;
    base_limits.backend_lockdown_profile = bun_defaults.backend_lockdown_profile;
    base_limits.backend_lifecycle_policy = bun_defaults.backend_lifecycle_policy;
    base_limits.bundle_content_kind = bun_defaults.bundle_content_kind;
    base_limits.javascript_evaluation_format = bun_defaults.javascript_evaluation_format;
    base_limits.compatibility_target = bun_defaults.compatibility_target;
    base_limits.execution_model = bun_defaults.execution_model;
    base_limits.mode = bun_defaults.mode;
    base_limits.language = bun_defaults.language;
    base_limits.preset = bun_defaults.preset;
    base_limits.grants = bun_defaults.grants;
    base_limits.runtime_pool_kind = bun_defaults.runtime_pool_kind;
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
