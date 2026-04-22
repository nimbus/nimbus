use neovex_core::Error;
use neovex_runtime::{HostCallCancellation, InvocationRequest};
use serde_json::Value;

use crate::adapters::convex::RuntimeReadSet;

use super::context::RuntimeInvocationContext;

pub(in crate::adapters::convex) async fn invoke_named_convex_function_async_cancellable(
    context: &RuntimeInvocationContext<'_>,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
    server_request_id: Option<String>,
) -> Result<Value, Error> {
    invoke_named_convex_function_with_trace_async_cancellable(
        context,
        request,
        cancellation,
        server_request_id,
    )
    .await
    .map(|(value, _)| value)
}

pub(in crate::adapters::convex) async fn invoke_named_convex_function_with_trace_async_cancellable(
    context: &RuntimeInvocationContext<'_>,
    request: InvocationRequest,
    cancellation: HostCallCancellation,
    server_request_id: Option<String>,
) -> Result<(Value, RuntimeReadSet), Error> {
    context
        .invoke_with_trace_async_cancellable(request, cancellation, server_request_id)
        .await
}
