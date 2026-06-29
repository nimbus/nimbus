use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use axum::body::Bytes;
use axum::extract::OriginalUri;
use axum::extract::ws::{Message, WebSocket};
use axum::http::{HeaderMap, Method};
use futures::{SinkExt, StreamExt};
pub use nimbus_convex::ConvexRegistry;
pub(crate) use nimbus_convex::*;
use nimbus_core::{
    Cursor, DocumentId, Error, Filter, FilterOp, Mutation, OrderBy, PaginatedQuery, Query,
    ScheduleRequest, TableName, TenantId, Timestamp,
};
use nimbus_engine::SubscriptionUpdate;
#[cfg(test)]
use nimbus_runtime::HostCallOperation;
use nimbus_runtime::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallRequest, InvocationAuth,
    InvocationKind, InvocationRequest, NimbusRuntimeError, RuntimeBundle,
};
use serde_json::Value;
use tokio::sync::mpsc;

mod execution;
mod handlers;
mod host_bridge;
mod http_actions;
mod network_guard;
mod subscriptions;
#[cfg(test)]
mod tests;

pub(in crate::adapters::convex) use self::execution::ConvexHttpRouteRequest;
pub(crate) use self::handlers::{
    action, cancel_scheduled_job, http_route, http_route_root, mutation, paginated_query, query,
    schedule_after, schedule_at, ws,
};
pub(crate) use self::host_bridge::{
    ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope,
};
pub(crate) use self::network_guard::convex_application_network_bind_guard;

use crate::protocol::ServerMessage;
use crate::state::{AppError, AppState};
use nimbus_bridge::read_tracking::{
    RuntimeIndexRangeRead, RuntimeReadSet, synthesize_runtime_subscription_base_queries,
};

pub(crate) fn convex_system_deployment_record_input<'a>(
    summary: &'a ConvexRegistryDeploySummary,
    source_ref: &'a str,
) -> nimbus_system::SystemDeploymentRecordInput<'a> {
    nimbus_system::SystemDeploymentRecordInput {
        source_ref,
        functions: summary
            .functions
            .iter()
            .map(
                |function| nimbus_system::SystemDeploymentFunctionRecordInput {
                    name: function.name.as_str(),
                    kind: function.kind,
                    fingerprint: function.fingerprint.as_str(),
                },
            )
            .collect(),
        http_routes: summary
            .http_routes
            .iter()
            .map(
                |route| nimbus_system::SystemDeploymentHttpRouteRecordInput {
                    key: route.key.as_str(),
                    fingerprint: route.fingerprint.as_str(),
                },
            )
            .collect(),
        schema_fingerprint: summary.schema_fingerprint.as_deref(),
        index_fingerprint: summary.index_fingerprint.as_deref(),
        runtime_bundle_fingerprint: summary.runtime_bundle_fingerprint.as_deref(),
    }
}
