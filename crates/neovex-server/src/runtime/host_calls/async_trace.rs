use std::time::Instant;

use neovex_runtime::{HostCallCancellationCause, NeovexRuntimeError};
use serde_json::Value;

#[derive(Clone)]
pub(crate) struct RuntimeAsyncHostCallTrace {
    span: tracing::Span,
    label: &'static str,
    enqueued_at: Instant,
}

impl RuntimeAsyncHostCallTrace {
    pub(crate) fn new(span: tracing::Span, label: &'static str) -> Self {
        let trace = Self {
            span,
            label,
            enqueued_at: Instant::now(),
        };
        tracing::debug!(parent: &trace.span, "{} enqueued", trace.label);
        trace
    }

    pub(crate) fn record_canceled_before_start(&self, cause: Option<HostCallCancellationCause>) {
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

    pub(crate) fn record_started(&self) -> Instant {
        let started_at = Instant::now();
        tracing::debug!(
            parent: &self.span,
            queue_wait_ms = started_at.duration_since(self.enqueued_at).as_secs_f64() * 1000.0,
            "{} started",
            self.label
        );
        started_at
    }

    pub(crate) fn record_finished(
        &self,
        started_at: Instant,
        result: &std::result::Result<Value, NeovexRuntimeError>,
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
            Err(NeovexRuntimeError::Cancelled) => match cancellation_cause {
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
