//! Convex host logic for Nimbus.
//!
//! This crate owns the Convex runtime host bridge, function execution, and
//! subscription transforms. It has no HTTP or WebSocket transport. The server
//! owns the routes and the socket loop and calls into this crate.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use http::Method;
use nimbus_bridge::read_tracking::{
    RuntimeIndexRangeRead, RuntimeReadSet, synthesize_runtime_subscription_base_queries,
};
use nimbus_convex::*;
use nimbus_core::{
    Cursor, DocumentId, Error, Filter, FilterOp, InvocationAuth, Mutation, OrderBy, PaginatedQuery,
    Query, ScheduleRequest, TableName, TenantId, Timestamp,
};
#[cfg(test)]
use nimbus_runtime::HostCallOperation;
use nimbus_runtime::{
    HostBridge, HostBridgeFuture, HostCallCancellation, HostCallRequest, InvocationKind,
    InvocationRequest, NimbusRuntimeError, RuntimeBundle,
};
use serde_json::Value;

mod execution;
mod host_bridge;
mod http_actions;
mod subscriptions;
#[cfg(test)]
mod tests;

use self::host_bridge::{ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope};

pub use self::execution::{
    RuntimeInvocationContext, bootstrap_runtime_named_subscription_async,
    dispatch_convex_mutation_async, execute_convex_action_async, execute_query_result_async,
    invoke_named_convex_function_async_cancellable,
};
pub use self::http_actions::prepare_http_action_response_async;
pub use self::subscriptions::{
    RuntimeTransformContext, apply_subscription_transform,
    next_runtime_subscription_server_request_id,
};

pub fn runtime_auth_payload(auth: &Option<InvocationAuth>) -> Option<Value> {
    auth.as_ref().map(InvocationAuth::to_runtime_payload)
}
