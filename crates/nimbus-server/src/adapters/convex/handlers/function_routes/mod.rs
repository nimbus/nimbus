use super::registry_auth::registry_and_auth_for_path;
use super::*;
use std::time::Instant;
use tracing::warn;

use nimbus_system::{OpenSpan, RunSpanRecorder};

use crate::error_envelope::StructuredHttpError;

mod actions;
mod mutations;
mod queries;

pub(crate) use actions::action;
pub(crate) use mutations::mutation;
pub(crate) use queries::{paginated_query, query};

fn convex_function_error(error: nimbus_core::Error) -> AppError {
    AppError::Structured(Box::new(StructuredHttpError::from_convex_core_error(error)))
}

struct RunTrace {
    function_path: String,
    kind: &'static str,
    started_at: u64,
    started: Instant,
    /// The run's spans. The host bridge records one span per host call
    /// under the root function span this trace opened.
    spans: Arc<RunSpanRecorder>,
    root: OpenSpan,
}

impl RunTrace {
    fn new(function_path: impl Into<String>, kind: &'static str) -> Self {
        let function_path = function_path.into();
        let spans = Arc::new(RunSpanRecorder::new());
        let root = spans.start("function", function_path.clone());
        Self {
            function_path,
            kind,
            started_at: unix_time_millis_lossy(),
            started: Instant::now(),
            spans,
            root,
        }
    }

    /// The recorder a runtime invocation context writes host-call spans into.
    fn recorder(&self) -> Arc<RunSpanRecorder> {
        Arc::clone(&self.spans)
    }

    async fn record(
        self,
        service: &Arc<nimbus_engine::Engine>,
        tenant_id: &TenantId,
        error: Option<&nimbus_core::Error>,
    ) {
        self.spans.finish(self.root, error.is_none());
        let (spans, dropped) = self.spans.snapshot();
        if dropped > 0 {
            warn!(
                function_path = %self.function_path,
                dropped,
                "run span limit reached; later spans were not recorded"
            );
        }
        let display = error.map(ToString::to_string);
        let error = error
            .zip(display.as_deref())
            .map(|(error, display)| nimbus_system::RunError::from_core_error(error, display));
        let record = nimbus_system::RunRecord {
            tenant_id,
            function_path: &self.function_path,
            kind: self.kind,
            started_at: self.started_at,
            duration_ms: self.started.elapsed().as_secs_f64() * 1000.0,
            status: if error.is_some() { "error" } else { "ok" },
            error,
            spans,
        };
        if let Err(record_error) = nimbus_system::record_run_async(service, record).await {
            warn!(
                function_path = %self.function_path,
                kind = self.kind,
                error = %record_error,
                "failed to record Convex invocation in _nimbus.runs"
            );
        }
    }
}

/// `RunTrace::started_at` stamp. `RunTrace` is a transient per-invocation
/// telemetry value (constructed fresh per request, discarded after
/// `record`), not unit-tested for its timing, so this is plumbing rather
/// than a site worth threading `Arc<dyn WallClock>` through every
/// action/mutation/query call site for.
fn unix_time_millis_lossy() -> u64 {
    nimbus_core::clock::system_now_millis()
}
