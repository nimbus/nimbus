use std::time::Instant;

use serde_json::Value;
use tokio::runtime::RuntimeFlavor;
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

fn bridge_blocking_invocation<T, F>(thread_panic_message: &'static str, task: F) -> Result<T>
where
    T: Send,
    F: FnOnce() -> Result<T> + Send,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return match handle.runtime_flavor() {
            RuntimeFlavor::MultiThread => tokio::task::block_in_place(task),
            RuntimeFlavor::CurrentThread | _ => std::thread::scope(|scope| {
                scope
                    .spawn(task)
                    .join()
                    .map_err(|_| NeovexRuntimeError::Contract(thread_panic_message.to_string()))
            })?,
        };
    }

    task()
}

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

        bridge_blocking_invocation("runtime executor invocation thread panicked", invoke)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn blocking_bridge_stays_on_current_thread_for_multi_thread_runtime() {
        let runtime_thread = std::thread::current().id();

        let bridged_thread = bridge_blocking_invocation("blocking bridge should not panic", || {
            Ok(std::thread::current().id())
        })
        .expect("blocking bridge should return the current thread id");

        assert_eq!(
            bridged_thread, runtime_thread,
            "multi-thread runtime bridge should use block_in_place instead of spawning a new thread"
        );
    }

    #[test]
    fn blocking_bridge_spawns_fallback_thread_for_current_thread_runtimes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime should build");

        let (runtime_thread, bridged_thread) = runtime.block_on(async {
            let runtime_thread = std::thread::current().id();
            let bridged_thread =
                bridge_blocking_invocation("blocking bridge should not panic", || {
                    Ok(std::thread::current().id())
                })
                .expect("blocking bridge should return a fallback thread id");
            (runtime_thread, bridged_thread)
        });

        assert_ne!(
            bridged_thread, runtime_thread,
            "current-thread runtimes should keep using the dedicated bridge-thread fallback"
        );
    }
}
