use super::*;

mod actions;
mod mutations;
mod queries;
mod scheduling;

pub(in crate::adapters::convex) use actions::{
    execute_convex_action_cancellable_with_auth,
};
#[cfg(test)]
pub(in crate::adapters::convex) use actions::{
    execute_convex_action, execute_convex_action_cancellable, execute_named_action_request_direct,
};
#[cfg(test)]
pub(in crate::adapters::convex) use mutations::execute_named_mutation_request_direct;
pub(in crate::adapters::convex) use mutations::{
    dispatch_convex_mutation_cancellable_with_auth,
};
#[cfg(test)]
pub(in crate::adapters::convex) use mutations::dispatch_mutation;
pub(in crate::adapters::convex) use queries::{
    execute_query_result_cancellable_with_auth,
};
#[cfg(test)]
pub(in crate::adapters::convex) use queries::{
    execute_named_paginated_query_request_direct, execute_named_query_request_direct,
    execute_query_result_cancellable,
};
pub(in crate::adapters::convex) use scheduling::execute_schedule_command;

pub(in crate::adapters::convex) fn encode_runtime_core_result(
    result: Result<Value, Error>,
) -> std::result::Result<Value, NeovexRuntimeError> {
    match result {
        Ok(value) => serde_json::to_value(ConvexRuntimeResponseEnvelope::ok(value))
            .map_err(NeovexRuntimeError::from),
        Err(Error::Cancelled) => Err(NeovexRuntimeError::Cancelled),
        Err(error) => serde_json::to_value(ConvexRuntimeResponseEnvelope::from_core_error(error))
            .map_err(NeovexRuntimeError::from),
    }
}
