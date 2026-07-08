use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use nimbus_runtime::{
    HostCallCancellation, HostCallCancellationCause, NimbusRuntimeError, RuntimeMetrics,
};
use serde_json::Value;

/// Tracing instrumentation for an async host call's lifecycle (enqueue, start,
/// finish). Sync host calls have no queueing delay worth spanning, so this
/// type is async-only.
#[derive(Clone)]
pub struct RuntimeAsyncHostCallTrace {
    span: tracing::Span,
    label: &'static str,
    enqueued_at: Instant,
}

impl RuntimeAsyncHostCallTrace {
    pub fn new(span: tracing::Span, label: &'static str) -> Self {
        let trace = Self {
            span,
            label,
            enqueued_at: Instant::now(),
        };
        tracing::debug!(parent: &trace.span, "{} enqueued", trace.label);
        trace
    }

    pub fn record_canceled_before_start(&self, cause: Option<HostCallCancellationCause>) {
        match cause {
            Some(cause) => tracing::debug!(
                parent: &self.span,
                queue_wait_ms = self.enqueued_at.elapsed().as_secs_f64() * 1000.0,
                cancellation_cause = cause.as_str(),
                "{} canceled before start",
                self.label
            ),
            None => tracing::debug!(
                parent: &self.span,
                queue_wait_ms = self.enqueued_at.elapsed().as_secs_f64() * 1000.0,
                "{} canceled before start",
                self.label
            ),
        }
    }

    pub fn record_started(&self) -> Instant {
        let started_at = Instant::now();
        tracing::debug!(
            parent: &self.span,
            queue_wait_ms = started_at.duration_since(self.enqueued_at).as_secs_f64() * 1000.0,
            "{} started",
            self.label
        );
        started_at
    }

    pub fn record_finished(
        &self,
        started_at: Instant,
        result: &std::result::Result<Value, NimbusRuntimeError>,
        cancellation_cause: Option<HostCallCancellationCause>,
    ) {
        let execution_ms = started_at.elapsed().as_secs_f64() * 1000.0;
        match result {
            Ok(_) => tracing::debug!(
                parent: &self.span,
                execution_ms,
                "{} finished",
                self.label
            ),
            Err(NimbusRuntimeError::Cancelled) => match cancellation_cause {
                Some(cause) => tracing::debug!(
                    parent: &self.span,
                    execution_ms,
                    cancellation_cause = cause.as_str(),
                    "{} canceled in flight",
                    self.label
                ),
                None => tracing::debug!(
                    parent: &self.span,
                    execution_ms,
                    "{} canceled in flight",
                    self.label
                ),
            },
            Err(error) => tracing::debug!(
                parent: &self.span,
                execution_ms,
                error = %error,
                "{} failed",
                self.label
            ),
        }
    }
}

/// Metrics bookkeeping shared by every call kind: sync, sync-cancellable, and
/// async all funnel their outcome through this once the dispatch has run.
fn record_host_operation_result(
    metrics: &RuntimeMetrics,
    operation: &str,
    result: &std::result::Result<Value, NimbusRuntimeError>,
) {
    match result {
        Ok(_) => metrics.record_host_operation_succeeded(operation),
        Err(NimbusRuntimeError::Cancelled) => {
            metrics.record_host_operation_canceled_in_flight(operation);
        }
        Err(_) => metrics.record_host_operation_failed(operation),
    }
}

/// Sync call kind: no cancellation check, no tracing span.
pub fn execute_host_call(
    metrics: &RuntimeMetrics,
    operation: &str,
    dispatch: impl FnOnce() -> std::result::Result<Value, NimbusRuntimeError>,
) -> std::result::Result<Value, NimbusRuntimeError> {
    metrics.record_host_operation_started(operation);
    let result = dispatch();
    record_host_operation_result(metrics, operation, &result);
    result
}

/// Sync-cancellable call kind: adds a pre-start cancellation check on top of
/// the plain sync call kind.
pub fn execute_host_call_cancellable(
    metrics: &RuntimeMetrics,
    operation: &str,
    cancellation: &HostCallCancellation,
    dispatch: impl FnOnce() -> std::result::Result<Value, NimbusRuntimeError>,
) -> std::result::Result<Value, NimbusRuntimeError> {
    if cancellation.is_cancelled() {
        metrics.record_host_operation_canceled_before_start(operation);
        return Err(NimbusRuntimeError::Cancelled);
    }
    execute_host_call(metrics, operation, dispatch)
}

/// Async call kind: pre-start cancellation check plus full trace
/// instrumentation (enqueue/start/finish) around the awaited future.
pub async fn execute_async_host_call<Fut>(
    trace: RuntimeAsyncHostCallTrace,
    metrics: Arc<RuntimeMetrics>,
    operation: &'static str,
    cancellation: HostCallCancellation,
    task: Fut,
) -> std::result::Result<Value, NimbusRuntimeError>
where
    Fut: Future<Output = std::result::Result<Value, NimbusRuntimeError>> + Send,
{
    let cancellation_cause = cancellation.cause();
    if cancellation.is_cancelled() {
        metrics.record_host_operation_canceled_before_start(operation);
        trace.record_canceled_before_start(cancellation_cause);
        return Err(NimbusRuntimeError::Cancelled);
    }

    let started_at = trace.record_started();
    metrics.record_host_operation_started(operation);
    let result = task.await;
    trace.record_finished(started_at, &result, cancellation_cause);
    record_host_operation_result(metrics.as_ref(), operation, &result);
    result
}
