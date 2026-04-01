use neovex_runtime::{HostCallCancellation, NeovexRuntimeError, RuntimeMetrics};
use serde_json::Value;

pub(crate) fn record_host_operation_result(
    metrics: &RuntimeMetrics,
    operation: &str,
    result: &std::result::Result<Value, NeovexRuntimeError>,
) {
    match result {
        Ok(_) => metrics.record_host_operation_succeeded(operation),
        Err(NeovexRuntimeError::Cancelled) => {
            metrics.record_host_operation_canceled_in_flight(operation);
        }
        Err(_) => metrics.record_host_operation_failed(operation),
    }
}

pub(crate) fn execute_host_call(
    metrics: &RuntimeMetrics,
    operation: &str,
    dispatch: impl FnOnce() -> std::result::Result<Value, NeovexRuntimeError>,
) -> std::result::Result<Value, NeovexRuntimeError> {
    metrics.record_host_operation_started(operation);
    let result = dispatch();
    record_host_operation_result(metrics, operation, &result);
    result
}

pub(crate) fn execute_host_call_cancellable(
    metrics: &RuntimeMetrics,
    operation: &str,
    cancellation: &HostCallCancellation,
    dispatch: impl FnOnce() -> std::result::Result<Value, NeovexRuntimeError>,
) -> std::result::Result<Value, NeovexRuntimeError> {
    if cancellation.is_cancelled() {
        metrics.record_host_operation_canceled_before_start(operation);
        return Err(NeovexRuntimeError::Cancelled);
    }
    execute_host_call(metrics, operation, dispatch)
}
