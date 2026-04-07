use std::time::Instant;

use serde_json::Value;
use tokio::sync::oneshot;

use crate::context::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::runtime::{InvocationRequest, NeovexRuntime, RuntimeBundle, RuntimeInvocationExecution};
use crate::watchdog::WatchdogTimer;

use super::admission::{RuntimeExecutorAdmissionDecision, SharedInvocationPermit};
use super::facade::{BLOCKING_RESULT_POLL_INTERVAL, RuntimeExecutor};
use super::lifecycle::run_invocation_lifecycle;
use super::queue::{RuntimeWorkerJob, RuntimeWorkerResultSender, RuntimeWorkerRouter};

impl RuntimeExecutor {
    async fn dispatch_admitted_job_async(&self, job: RuntimeWorkerJob) -> Result<()> {
        self.inner.router.dispatch_job(job).await
    }

    fn dispatch_admitted_job_blocking(&self, job: RuntimeWorkerJob) -> Result<()> {
        self.inner
            .router
            .dispatch_job_blocking(job)
            .map_err(|_| RuntimeWorkerRouter::closed_error())
    }

    async fn invoke_job(
        watchdog: WatchdogTimer,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
        queue_started_at: Instant,
    ) -> Result<Value> {
        let policy = runtime.policy();
        let permit = SharedInvocationPermit::new(
            policy.clone(),
            context.tenant_label.clone(),
            None,
            runtime.bypasses_concurrency_limit(),
            cancellation.clone(),
        );
        let (result, _ready_jobs) = run_invocation_lifecycle(
            permit,
            policy,
            context.clone(),
            cancellation.clone(),
            queue_started_at,
            None,
            |permit| async move {
                runtime
                    .invoke_bundle_unmanaged(
                        None,
                        RuntimeInvocationExecution {
                            watchdog: watchdog.clone(),
                            bundle: bundle.clone(),
                            request: request.clone(),
                            context: context.clone(),
                            external_cancellation: cancellation,
                            permit,
                        },
                    )
                    .await
            },
        )
        .await;
        result
    }

    pub async fn invoke(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
    ) -> Result<Value> {
        self.invoke_with_cancellation(runtime, bundle, request, context, None)
            .await
    }

    pub async fn invoke_with_cancellation(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.inner
            .policy
            .metrics()
            .record_request_correlation(&context);
        Self::invoke_job(
            self.inner.watchdog.clone(),
            runtime.into_policy(self.inner.policy.clone()),
            bundle,
            request,
            context,
            cancellation,
            Instant::now(),
        )
        .await
    }

    pub async fn invoke_on_worker(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.inner
            .policy
            .metrics()
            .record_request_correlation(&context);
        if cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            self.inner
                .policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(
                    context.tenant_label.as_deref(),
                    cancellation.as_ref().and_then(HostCallCancellation::cause),
                );
            return Err(NeovexRuntimeError::Cancelled);
        }

        let (result_tx, result_rx) = oneshot::channel();
        let admission = self.inner.admission.admit_job(RuntimeWorkerJob {
            runtime,
            bundle,
            request,
            context,
            cancellation: cancellation.clone(),
            enqueued_at: Instant::now(),
            result_tx: RuntimeWorkerResultSender::Async(result_tx),
            dispatch_handle: None,
        })?;
        if let RuntimeExecutorAdmissionDecision::Dispatch(job) = admission {
            self.dispatch_admitted_job_async(*job).await?;
        }

        match cancellation {
            Some(cancellation) => {
                tokio::select! {
                    _ = cancellation.cancelled() => Err(NeovexRuntimeError::Cancelled),
                    result = result_rx => result.map_err(|_| {
                        NeovexRuntimeError::Contract(
                            "runtime executor dropped an invocation result".to_string(),
                        )
                    })?,
                }
            }
            None => result_rx.await.map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime executor dropped an invocation result".to_string(),
                )
            })?,
        }
    }

    pub fn invoke_blocking(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
    ) -> Result<Value> {
        self.invoke_blocking_with_cancellation(runtime, bundle, request, context, None)
    }

    pub fn invoke_blocking_with_cancellation(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        let executor = self.clone();
        let invoke = move || {
            executor.invoke_on_worker_blocking(runtime, bundle, request, context, cancellation)
        };

        if tokio::runtime::Handle::try_current().is_ok() {
            std::thread::spawn(invoke).join().map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime executor invocation thread panicked".to_string(),
                )
            })?
        } else {
            invoke()
        }
    }

    fn invoke_on_worker_blocking(
        &self,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        self.inner
            .policy
            .metrics()
            .record_request_correlation(&context);
        if cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            self.inner
                .policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(
                    context.tenant_label.as_deref(),
                    cancellation.as_ref().and_then(HostCallCancellation::cause),
                );
            return Err(NeovexRuntimeError::Cancelled);
        }

        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let admission = self.inner.admission.admit_job(RuntimeWorkerJob {
            runtime,
            bundle,
            request,
            context,
            cancellation: cancellation.clone(),
            enqueued_at: Instant::now(),
            result_tx: RuntimeWorkerResultSender::Blocking(result_tx),
            dispatch_handle: None,
        })?;
        if let RuntimeExecutorAdmissionDecision::Dispatch(job) = admission {
            self.dispatch_admitted_job_blocking(*job)?;
        }

        match cancellation {
            Some(cancellation) => loop {
                if cancellation.is_cancelled() {
                    return Err(NeovexRuntimeError::Cancelled);
                }
                match result_rx.recv_timeout(BLOCKING_RESULT_POLL_INTERVAL) {
                    Ok(result) => return result,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(NeovexRuntimeError::Contract(
                            "runtime executor dropped an invocation result".to_string(),
                        ));
                    }
                }
            },
            None => result_rx.recv().map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime executor dropped an invocation result".to_string(),
                )
            })?,
        }
    }
}
