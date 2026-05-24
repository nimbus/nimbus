use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

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
    RuntimeCompatibilityTarget, RuntimeExecutionAdapterState, RuntimeExecutor, RuntimeLimits,
    RuntimeMetricsSnapshot, RuntimePolicy, RuntimeResetCapabilities,
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
struct ConvexRuntimeLane {
    policy: Arc<RuntimePolicy>,
    executor: Arc<OnceLock<Arc<RuntimeExecutor>>>,
    execution_adapter_state: RuntimeExecutionAdapterState,
}

#[derive(Debug, Clone)]
pub(crate) struct ConvexRuntimeLaneDiagnostics {
    pub lane_name: &'static str,
    pub default_lane: bool,
    pub executor_started: bool,
    pub execution_adapter_state: RuntimeExecutionAdapterState,
    pub limits: RuntimeLimits,
    pub reset_capabilities: RuntimeResetCapabilities,
    pub metrics: RuntimeMetricsSnapshot,
}

impl ConvexRuntimeLane {
    fn from_limits(
        limits: RuntimeLimits,
        execution_adapter_state: RuntimeExecutionAdapterState,
    ) -> Self {
        Self {
            policy: Arc::new(RuntimePolicy::new(limits)),
            executor: Arc::new(OnceLock::new()),
            execution_adapter_state,
        }
    }

    fn policy(&self) -> Arc<RuntimePolicy> {
        self.policy.clone()
    }

    fn executor(&self) -> Option<Arc<RuntimeExecutor>> {
        match self.execution_adapter_state {
            RuntimeExecutionAdapterState::Linked => Some(
                self.executor
                    .get_or_init(|| Arc::new(RuntimeExecutor::new(self.policy.clone())))
                    .clone(),
            ),
            RuntimeExecutionAdapterState::NotLinked => None,
        }
    }

    fn limits(&self) -> &RuntimeLimits {
        self.policy.limits()
    }

    fn diagnostics(
        &self,
        lane_name: &'static str,
        default_lane: bool,
    ) -> ConvexRuntimeLaneDiagnostics {
        ConvexRuntimeLaneDiagnostics {
            lane_name,
            default_lane,
            executor_started: self.executor.get().is_some(),
            execution_adapter_state: self.execution_adapter_state,
            limits: self.policy.limits().clone(),
            reset_capabilities: self.policy.limits().reset_capabilities(),
            metrics: self.policy.metrics_snapshot(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConvexRegistry {
    functions: HashMap<String, ConvexFunctionDefinition>,
    http_routes: Vec<ConvexHttpRouteDefinition>,
    schema: Option<Schema>,
    runtime_bundle: Option<RuntimeBundle>,
    artifact_guard: Option<Arc<tempfile::TempDir>>,
    auth_verifier: Arc<auth::ConvexAuthVerifier>,
    runtime_lane: ConvexRuntimeLane,
    node20_runtime_lane: ConvexRuntimeLane,
    node22_runtime_lane: ConvexRuntimeLane,
    node24_runtime_lane: ConvexRuntimeLane,
    bun_jsc_runtime_lane: ConvexRuntimeLane,
    runtime_bundle_provenance: Option<RuntimeBundleProvenanceConfig>,
}

impl Default for ConvexRegistry {
    fn default() -> Self {
        let runtime_lane = convex_default_runtime_lane(RuntimeLimits::default());
        let node20_runtime_lane =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node20);
        let node22_runtime_lane =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node22);
        let node24_runtime_lane =
            convex_node_runtime_lane(RuntimeLimits::default(), RuntimeCompatibilityTarget::Node24);
        let bun_jsc_runtime_lane = convex_bun_jsc_runtime_lane(RuntimeLimits::default());
        Self {
            functions: HashMap::new(),
            http_routes: Vec::new(),
            schema: None,
            runtime_bundle: None,
            artifact_guard: None,
            auth_verifier: Arc::new(auth::ConvexAuthVerifier::empty()),
            runtime_lane,
            node20_runtime_lane,
            node22_runtime_lane,
            node24_runtime_lane,
            bun_jsc_runtime_lane,
            runtime_bundle_provenance: None,
        }
    }
}

fn convex_default_runtime_lane(base_limits: RuntimeLimits) -> ConvexRuntimeLane {
    let mut limits = RuntimeLimits::default();
    if matches!(
        base_limits.backend_kind,
        nimbus_runtime::RuntimeBackendKind::V8
    ) {
        limits = base_limits;
    } else {
        limits.apply_resource_overrides_from(&base_limits);
    }
    ConvexRuntimeLane::from_limits(limits, RuntimeExecutionAdapterState::Linked)
}

fn convex_node_runtime_lane(
    base_limits: RuntimeLimits,
    target: RuntimeCompatibilityTarget,
) -> ConvexRuntimeLane {
    let mut limits = RuntimeLimits::application_node(target);
    limits.apply_resource_overrides_from(&base_limits);
    ConvexRuntimeLane::from_limits(limits, RuntimeExecutionAdapterState::Linked)
}

fn convex_bun_jsc_runtime_lane(base_limits: RuntimeLimits) -> ConvexRuntimeLane {
    let mut limits = RuntimeLimits::application_bun_jsc();
    limits.apply_resource_overrides_from(&base_limits);
    ConvexRuntimeLane::from_limits(limits, RuntimeExecutionAdapterState::NotLinked)
}

impl ApplicationAuthVerifier for ConvexRegistry {
    fn verify_bearer_token<'a>(
        &'a self,
        token: &'a str,
    ) -> BoxFuture<'a, Result<InvocationAuth, AppError>> {
        Box::pin(async move { ConvexRegistry::verify_bearer_token(self, token).await })
    }
}
