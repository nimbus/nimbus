use crate::*;

mod contract;
mod pagination;
mod payloads;
mod responses;

pub use contract::{
    ConvexHostCallFamily, ConvexHostCallOperation, ConvexHostCallRequest,
    convex_host_operation_name,
};
pub use pagination::synthesize_runtime_paginate_cursor;
pub use payloads::*;
pub use responses::*;

pub fn runtime_host_payload_value<T>(payload: T) -> Result<Value, NimbusRuntimeError>
where
    T: serde::Serialize,
{
    serde_json::to_value(payload).map_err(NimbusRuntimeError::from)
}
