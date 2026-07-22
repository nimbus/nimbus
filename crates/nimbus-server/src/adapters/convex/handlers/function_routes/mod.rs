use super::registry_auth::registry_and_auth_for_path;
use super::*;
use std::time::Instant;
use tracing::warn;

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
}

impl RunTrace {
    fn new(function_path: impl Into<String>, kind: &'static str) -> Self {
        Self {
            function_path: function_path.into(),
            kind,
            started_at: unix_time_millis_lossy(),
            started: Instant::now(),
        }
    }

    async fn record(
        self,
        service: &Arc<nimbus_engine::Engine>,
        tenant_id: &TenantId,
        status: &str,
        error: Option<&str>,
    ) {
        let record = nimbus_system::RunRecord {
            tenant_id,
            function_path: &self.function_path,
            kind: self.kind,
            started_at: self.started_at,
            duration_ms: self.started.elapsed().as_secs_f64() * 1000.0,
            status,
            error,
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
