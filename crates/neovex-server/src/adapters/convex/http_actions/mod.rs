#![cfg_attr(test, allow(dead_code))]

use std::collections::HashMap;
use std::sync::Arc;

use axum::Json;
use axum::body::Bytes;
use axum::extract::OriginalUri;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use neovex_core::{Error, TenantId};
use neovex_runtime::{HostCallCancellation, InvocationAuth, InvocationKind, InvocationRequest};
use serde_json::{Value, json};

#[cfg(test)]
use super::execution::execute_convex_action;
use super::execution::{
    check_host_cancellation, execute_convex_action_async,
    execute_convex_action_cancellable_with_auth, invoke_named_convex_function_async_cancellable,
    next_runtime_server_request_id,
};
use super::*;
use crate::state::RequestCancellationGuard;

mod dispatch;
mod execution;
mod request_context;
mod response;

pub(in crate::adapters::convex) use dispatch::dispatch_http_route;
pub(in crate::adapters::convex) use execution::prepare_http_action_response_async;
pub(in crate::adapters::convex) use execution::prepare_http_action_response_cancellable;
