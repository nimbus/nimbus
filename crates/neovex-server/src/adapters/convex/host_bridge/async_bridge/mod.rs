use super::*;
use crate::execution::host_calls::{
    RuntimeAsyncHostCallTrace, execute_async_host_call, execute_host_call,
    execute_host_call_cancellable,
};

mod dispatch;
#[cfg(test)]
mod tests;

impl HostBridge for ConvexHostBridge {
    fn call(&self, request: HostCallRequest) -> std::result::Result<Value, NeovexRuntimeError> {
        let metrics = self.registry.runtime_policy().metrics();
        let operation = convex_host_operation_name(request.operation);
        execute_host_call(metrics.as_ref(), operation, || {
            self.dispatch_host_call(request)
        })
    }

    fn call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> std::result::Result<Value, NeovexRuntimeError> {
        let metrics = self.registry.runtime_policy().metrics();
        let operation = convex_host_operation_name(request.operation);
        execute_host_call_cancellable(metrics.as_ref(), operation, cancellation, || {
            self.dispatch_host_call_cancellable(request, cancellation)
        })
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let bridge = self.clone();
        static NEXT_ASYNC_HOST_CALL_ID: AtomicU64 = AtomicU64::new(1);
        let trace = RuntimeAsyncHostCallTrace::new(
            tracing::debug_span!(
                "convex_runtime_async_host_call",
                tenant = %bridge.tenant_id,
                server_request_id = ?bridge.server_request_id(),
                session_id = %bridge.session_id(),
                operation = %convex_host_operation_name(request.operation),
                host_call_id = NEXT_ASYNC_HOST_CALL_ID.fetch_add(1, Ordering::Relaxed),
            ),
            "convex runtime async host call",
        );
        let metrics = bridge.registry.runtime_policy().metrics();
        let operation = convex_host_operation_name(request.operation);
        Box::pin(execute_async_host_call(
            trace,
            metrics,
            operation,
            cancellation.clone(),
            async move {
                bridge
                    .dispatch_host_call_async(request, &cancellation)
                    .await
            },
        ))
    }
}
