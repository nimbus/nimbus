use super::execution::{
    check_host_cancellation, dispatch_convex_mutation_async,
    dispatch_convex_mutation_cancellable_with_auth, encode_runtime_core_result,
    ensure_runtime_host_not_cancelled, execute_convex_action_async,
    execute_convex_action_cancellable_with_auth, execute_query_result_async,
    execute_query_result_cancellable_with_auth, execute_schedule_command,
    execute_schedule_command_async, runtime_error_to_core,
};
use super::http_actions::{
    prepare_http_action_response_async, prepare_http_action_response_cancellable,
};
use super::registry::validate_runtime_http_route;
use super::subscriptions::{
    is_scalar_filter_value, should_replace_lower_bound, should_replace_upper_bound,
};
use super::*;

mod async_bridge;
mod bridge;
mod contract;
mod db_ops;
mod function_ops;
mod pagination;
mod payloads;
mod read_tracking;
mod responses;

pub(in crate::adapters::convex) use bridge::{
    ConvexHostBridge, ConvexHostBridgeInvocation, ConvexHostBridgeScope,
};
#[cfg(test)]
pub(in crate::adapters::convex) use contract::ConvexHostCallRequest;
pub(in crate::adapters::convex) use contract::convex_host_operation_name;
#[cfg(test)]
pub(in crate::adapters::convex) use contract::{ConvexHostCallFamily, ConvexHostCallOperation};
pub(in crate::adapters::convex) use pagination::synthesize_runtime_paginate_cursor;
pub(in crate::adapters::convex) use payloads::*;
pub(in crate::adapters::convex) use responses::*;

pub(in crate::adapters::convex) fn runtime_host_payload_value<T>(
    payload: T,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    T: serde::Serialize,
{
    serde_json::to_value(payload).map_err(NeovexRuntimeError::from)
}
