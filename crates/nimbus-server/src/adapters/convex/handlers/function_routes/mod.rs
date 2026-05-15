use super::common::registry_and_auth;
use super::*;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tracing::warn;

mod actions;
mod mutations;
mod queries;

pub(crate) use actions::action;
pub(crate) use mutations::mutation;
pub(crate) use queries::{paginated_query, query};

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
        service: &Arc<nimbus_engine::Service>,
        tenant_id: &TenantId,
        status: &str,
        error: Option<&str>,
    ) {
        let record = crate::system_tenant::RunRecord {
            tenant_id,
            function_path: &self.function_path,
            kind: self.kind,
            started_at: self.started_at,
            duration_ms: self.started.elapsed().as_secs_f64() * 1000.0,
            status,
            error,
        };
        if let Err(record_error) = crate::system_tenant::record_run_async(service, record).await {
            warn!(
                function_path = %self.function_path,
                kind = self.kind,
                error = %record_error,
                "failed to record Convex invocation in _nimbus.runs"
            );
        }
    }
}

fn unix_time_millis_lossy() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}
