use std::sync::Arc;

use neovex_runtime::{HostCallCancellation, NeovexRuntimeError, RuntimeMetrics};
use serde_json::Value;

use super::async_trace::RuntimeAsyncHostCallTrace;
use super::sync::record_host_operation_result;

pub(crate) async fn execute_async_blocking_host_call<F>(
    trace: RuntimeAsyncHostCallTrace,
    metrics: Arc<RuntimeMetrics>,
    operation: String,
    cancellation: HostCallCancellation,
    task: F,
) -> std::result::Result<Value, NeovexRuntimeError>
where
    F: FnOnce(HostCallCancellation) -> std::result::Result<Value, NeovexRuntimeError>
        + Send
        + 'static,
{
    let cancellation_cause = cancellation.cause();
    if cancellation.is_cancelled() {
        metrics.record_host_operation_canceled_before_start(&operation);
        trace.record_canceled_before_start(cancellation_cause);
        return Err(NeovexRuntimeError::Cancelled);
    }

    let metrics_for_task = metrics.clone();
    let operation_for_task = operation.clone();
    let trace_for_task = trace.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let started_at = trace_for_task.record_started();
        metrics_for_task.record_host_operation_started(&operation_for_task);
        let result = task(cancellation);
        (started_at, result)
    });
    let (started_at, result) = match handle.await {
        Ok(output) => output,
        Err(error) => {
            trace.record_join_failure(&error);
            metrics.record_host_operation_failed(&operation);
            return Err(NeovexRuntimeError::Contract(format!(
                "runtime host bridge task failed: {error}"
            )));
        }
    };
    trace.record_finished(started_at, &result, cancellation_cause);
    record_host_operation_result(metrics.as_ref(), &operation, &result);
    result
}
