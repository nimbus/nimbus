use std::future::Future;
use std::sync::Arc;

use neovex_runtime::{HostCallCancellation, NeovexRuntimeError, RuntimeMetrics};
use serde_json::Value;

use super::async_trace::RuntimeAsyncHostCallTrace;
use super::sync::record_host_operation_result;

pub(crate) async fn execute_async_host_call<Fut>(
    trace: RuntimeAsyncHostCallTrace,
    metrics: Arc<RuntimeMetrics>,
    operation: &'static str,
    cancellation: HostCallCancellation,
    task: Fut,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    Fut: Future<Output = std::result::Result<Value, NeovexRuntimeError>> + Send,
{
    let cancellation_cause = cancellation.cause();
    if cancellation.is_cancelled() {
        metrics.record_host_operation_canceled_before_start(operation);
        trace.record_canceled_before_start(cancellation_cause);
        return Err(NeovexRuntimeError::Cancelled);
    }

    let started_at = trace.record_started();
    metrics.record_host_operation_started(operation);
    let result = task.await;
    trace.record_finished(started_at, &result, cancellation_cause);
    record_host_operation_result(metrics.as_ref(), operation, &result);
    result
}
