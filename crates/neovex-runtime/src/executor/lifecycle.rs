use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use tracing::debug;

use crate::context::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;

use super::admission::SharedInvocationPermit;
use super::queue::RuntimeWorkerJob;

pub(crate) async fn run_invocation_lifecycle<F, Fut>(
    mut permit: SharedInvocationPermit,
    policy: Arc<RuntimePolicy>,
    context: RuntimeInvocationContext,
    cancellation_for_metrics: Option<HostCallCancellation>,
    queue_started_at: Instant,
    worker_id: Option<usize>,
    invoke: F,
) -> (Result<Value>, Vec<RuntimeWorkerJob>)
where
    F: FnOnce(SharedInvocationPermit) -> Fut,
    Fut: Future<Output = Result<Value>>,
{
    let metrics = policy.metrics();
    let execution_started_at = Instant::now();
    let result = async {
        permit.acquire_initial(queue_started_at).await?;
        match worker_id {
            Some(worker_id) => {
                debug!(
                    worker_id,
                    invocation_id = context.invocation_id,
                    request_id = ?context.server_request_id,
                    tenant = context.tenant_label.as_deref().unwrap_or("unknown"),
                    function = %context.function_name,
                    kind = context.kind,
                    "runtime worker invocation started"
                );
            }
            None => {
                debug!(
                    invocation_id = context.invocation_id,
                    request_id = ?context.server_request_id,
                    tenant = context.tenant_label.as_deref().unwrap_or("unknown"),
                    function = %context.function_name,
                    kind = context.kind,
                    queued_invocations = metrics.snapshot().queued_invocations,
                    "runtime invocation admitted"
                );
            }
        }
        invoke(permit.clone()).await
    }
    .await
    .inspect_err(|error| match error {
        NeovexRuntimeError::ExecutionTimeout(_) => metrics.record_timeout(),
        NeovexRuntimeError::Cancelled => metrics.record_in_flight_canceled_invocation_for_tenant(
            context.tenant_label.as_deref(),
            cancellation_for_metrics
                .as_ref()
                .and_then(HostCallCancellation::cause),
        ),
        _ => {}
    })
    .inspect(|_| {
        let execution = execution_started_at.elapsed();
        metrics.record_execution_for_tenant(context.tenant_label.as_deref(), execution);
        match worker_id {
            Some(worker_id) => {
                debug!(
                    worker_id,
                    invocation_id = context.invocation_id,
                    request_id = ?context.server_request_id,
                    tenant = context.tenant_label.as_deref().unwrap_or("unknown"),
                    function = %context.function_name,
                    kind = context.kind,
                    execution_ms = execution.as_secs_f64() * 1000.0,
                    active_runtime_instances = metrics.snapshot().active_runtime_instances,
                    "runtime worker invocation completed"
                );
            }
            None => {
                debug!(
                    invocation_id = context.invocation_id,
                    request_id = ?context.server_request_id,
                    tenant = context.tenant_label.as_deref().unwrap_or("unknown"),
                    function = %context.function_name,
                    kind = context.kind,
                    execution_ms = execution.as_secs_f64() * 1000.0,
                    active_runtime_instances = metrics.snapshot().active_runtime_instances,
                    "runtime invocation completed"
                );
            }
        }
    })
    .inspect_err(|_| {
        let execution = execution_started_at.elapsed();
        metrics.record_execution_for_tenant(context.tenant_label.as_deref(), execution);
    });
    let ready_jobs = permit.finish_invocation().await;
    (result, ready_jobs)
}
