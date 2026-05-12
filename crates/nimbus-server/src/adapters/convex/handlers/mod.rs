use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::{
    OriginalUri, Path as AxumPath, Query as AxumQuery, State, ws::WebSocketUpgrade,
};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use nimbus_core::{ScheduleRequest, TenantId, Timestamp};
use nimbus_runtime::{InvocationAuth, InvocationKind, InvocationRequest};
use serde_json::Value;

use super::execution::{
    check_host_cancellation, dispatch_convex_mutation_async, execute_convex_action_async,
    execute_query_result_async, invoke_named_convex_function_async_cancellable,
    next_runtime_server_request_id,
};
use super::http_actions::dispatch_http_route;
use super::subscriptions::handle_convex_socket_for_tenant;
use super::templates::{normalize_http_request_path, parse_job_id};
use super::{
    ConvexActionRequest, ConvexExecutableAction, ConvexExecutableMutation, ConvexExecutableQuery,
    ConvexHttpRouteRequest, ConvexMutationRequest, ConvexPaginatedQueryRequest, ConvexQueryRequest,
    ConvexRegistry, ConvexScheduleAfterRequest, ConvexScheduleAtRequest,
};
use crate::protocol::ScheduleResponse;
use crate::state::{AppError, AppState, RequestCancellationGuard, record_authenticated_usage};

mod common;
mod function_routes;
mod http;
mod scheduling;
mod socket;

pub(crate) use function_routes::{action, mutation, paginated_query, query};
pub(crate) use http::{http_route, http_route_root};
pub(crate) use scheduling::{cancel_scheduled_job, schedule_after, schedule_at};
pub(crate) use socket::ws;
