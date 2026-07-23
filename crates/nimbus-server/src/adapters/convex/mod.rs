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
pub use nimbus_convex::{ConvexSiloAuthRegistry, ConvexTenancyConfig, SiloTeamRegistry, TeamId};
use nimbus_core::{
    Cursor, DocumentId, Error, Filter, FilterOp, InvocationAuth, Mutation, OrderBy, PaginatedQuery,
    Query, ScheduleRequest, TableName, TenantId, Timestamp,
};
use nimbus_engine::SubscriptionUpdate;
#[cfg(test)]
use nimbus_runtime::HostCallOperation;
use nimbus_runtime::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallRequest, InvocationKind,
    InvocationRequest, NimbusRuntimeError, RuntimeBundle,
};
use serde_json::Value;
use tokio::sync::mpsc;

mod execution;
pub(in crate::adapters::convex) mod handlers;
mod host_bridge;
mod http_actions;
mod subscriptions;
#[cfg(test)]
mod tests;

pub(in crate::adapters::convex) use self::execution::ConvexHttpRouteRequest;

pub(in crate::adapters::convex) fn runtime_auth_payload(
    auth: &Option<InvocationAuth>,
) -> Option<Value> {
    auth.as_ref().map(InvocationAuth::to_runtime_payload)
}
pub(crate) use self::handlers::{
    action, cancel_scheduled_job, http_route, http_route_root, mutation, paginated_query, query,
    schedule_after, schedule_at, ws,
};
pub(crate) use self::host_bridge::{
    ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope,
};

use crate::protocol::ServerMessage;
use crate::state::{AppError, AppState};
use nimbus_bridge::read_tracking::{
    RuntimeIndexRangeRead, RuntimeReadSet, synthesize_runtime_subscription_base_queries,
};
