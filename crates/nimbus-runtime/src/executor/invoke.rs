use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;
use std::pin::Pin;
use tokio::runtime::RuntimeFlavor;
use tokio::sync::oneshot;

use crate::context::RuntimeInvocationContext;
use crate::error::{NimbusRuntimeError, Result};
use crate::execution_plan::RuntimeExecutionPlan;
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::{
    InvocationRequest, NimbusRuntime, RuntimeBundle, RuntimeHost, RuntimeInvocationExecution,
};
use crate::watchdog::WatchdogTimer;

use super::admission::{RuntimeExecutorAdmissionDecision, SharedInvocationPermit};
use super::facade::{BLOCKING_RESULT_POLL_INTERVAL, RuntimeExecutor};
use super::lifecycle::run_invocation_lifecycle;
use super::queue::{RuntimeWorkerJob, RuntimeWorkerResultSender};

pub struct RuntimeInvocationResponse {
    response: Value,
    completion: RuntimeInvocationCompletion,
}

enum RuntimeInvocationCompletion {
    Pending(Pin<Box<oneshot::Receiver<Result<Value>>>>),
    Complete,
}

impl RuntimeInvocationResponse {
    pub fn response(&self) -> &Value {
        &self.response
    }

    pub async fn wait_until_complete(self) -> Result<Value> {
        match self.completion {
            RuntimeInvocationCompletion::Pending(result_rx) => result_rx
                .await
                .map_err(|_| {
                    NimbusRuntimeError::Contract(
                        "runtime executor dropped an invocation completion".to_string(),
                    )
                })?
                .map(|_| self.response),
            RuntimeInvocationCompletion::Complete => Ok(self.response),
        }
    }
}

struct DirectRuntimeInvocation {
    watchdog: WatchdogTimer,
    host: RuntimeHost,
    policy: Arc<RuntimePolicy>,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    context: RuntimeInvocationContext,
    cancellation: Option<HostCallCancellation>,
    _retirement_guard: Option<super::retirement::RuntimeRetirementGuard>,
    queue_started_at: Instant,
}

fn execution_plan_for_invocation(
    policy: &RuntimePolicy,
    request: &InvocationRequest,
    context: &RuntimeInvocationContext,
) -> RuntimeExecutionPlan {
    let started_at = Instant::now();
    let plan = RuntimeExecutionPlan::for_invocation(policy, request, context);
    policy
        .metrics()
        .record_execution_plan_build(started_at.elapsed());
    plan
}

fn bridge_blocking_invocation<T, F>(thread_panic_message: &'static str, task: F) -> Result<T>
where
    T: Send,
    F: FnOnce() -> Result<T> + Send,
{
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        return match handle.runtime_flavor() {
            RuntimeFlavor::MultiThread => tokio::task::block_in_place(task),
            _ => std::thread::scope(|scope| {
                scope
                    .spawn(task)
                    .join()
                    .map_err(|_| NimbusRuntimeError::Contract(thread_panic_message.to_string()))
            })?,
        };
    }

    task()
}

impl RuntimeExecutor {
    fn prepare_worker_retirement_registration(
        &self,
        context: &RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<(
        Option<HostCallCancellation>,
        Option<super::retirement::RuntimeRetirementGuard>,
    )> {
        let cancellation = if context.runtime_owner_lease().is_some()
            || context.deployment_authority_lease().is_some()
        {
            Some(cancellation.unwrap_or_default())
        } else {
            cancellation
        };
        let retirement_guard = cancellation.as_ref().and_then(|cancellation| {
            self.inner
                .retirement
                .register(context, cancellation.clone())
        });
        // Close the validate-before-register race: retirement either observes
        // this guard and waits for it, or its revocation is visible here and
        // admission fails before dispatch/guest entry.
        if let Some(owner_lease) = context.runtime_owner_lease() {
            owner_lease.ensure_active()?;
        }
        if let Some(deployment_lease) = context.deployment_authority_lease() {
            deployment_lease.ensure_active()?;
        }
        Ok((cancellation, retirement_guard))
    }

    async fn dispatch_admitted_job_async(&self, job: RuntimeWorkerJob) -> Result<()> {
        self.inner.router.dispatch_job(job).await
    }

    fn dispatch_admitted_job_blocking(&self, job: RuntimeWorkerJob) -> Result<()> {
        self.inner
            .router
            .dispatch_job_blocking(job)
            .map_err(|failure| failure.into_error())
    }

    fn finish_canceled_queued_job_with_policy(policy: &RuntimePolicy, job: RuntimeWorkerJob) {
        policy
            .metrics()
            .record_queued_canceled_invocation_for_tenant(
                job.context.tenant_label.as_deref(),
                job.cancellation
                    .as_ref()
                    .and_then(HostCallCancellation::cause),
            );
        job.send_result(Err(NimbusRuntimeError::Cancelled));
    }

    fn register_queued_cancellation_listener(
        &self,
        invocation_id: u64,
        cancellation: &Option<HostCallCancellation>,
        policy: Arc<RuntimePolicy>,
    ) {
        let Some(cancellation) = cancellation else {
            return;
        };
        let admission = self.inner.admission.clone();
        cancellation.notify_on_cancel(move || {
            if let Some(job) = admission.cancel_queued_job(invocation_id) {
                Self::finish_canceled_queued_job_with_policy(policy.as_ref(), job);
            }
        });
    }

    async fn invoke_job(invocation: DirectRuntimeInvocation) -> Result<Value> {
        let DirectRuntimeInvocation {
            watchdog,
            host,
            policy,
            bundle,
            request,
            context,
            cancellation,
            _retirement_guard,
            queue_started_at,
        } = invocation;
        let permit = SharedInvocationPermit::new(
            policy.clone(),
            context.tenant_label.clone(),
            None,
            context.bypasses_concurrency_limit(),
            cancellation.clone(),
        );
        let runtime = host.runtime_with_policy(policy.clone());
        let execution_plan = execution_plan_for_invocation(&policy, &request, &context);
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
                            execution_plan,
                            external_cancellation: cancellation,
                            response_ready_tx: None,
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
        runtime: NimbusRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
    ) -> Result<Value> {
        self.invoke_with_cancellation(runtime, bundle, request, context, None)
            .await
    }

    pub async fn invoke_with_cancellation(
        &self,
        runtime: NimbusRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        let runtime_policy = runtime.policy();
        crate::retained_state::validate_retained_state_admission(&runtime_policy, &context)?;
        let (cancellation, retirement_guard) =
            self.prepare_worker_retirement_registration(&context, cancellation)?;
        runtime_policy
            .metrics()
            .record_request_correlation(&context);
        Self::invoke_job(DirectRuntimeInvocation {
            watchdog: self.inner.watchdog.clone(),
            host: runtime.invocation_host(),
            policy: runtime_policy,
            bundle,
            request,
            context,
            cancellation,
            _retirement_guard: retirement_guard,
            queue_started_at: Instant::now(),
        })
        .await
    }

    pub async fn invoke_on_worker(
        &self,
        runtime: NimbusRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        let runtime_policy = runtime.policy();
        crate::retained_state::validate_retained_state_admission(&runtime_policy, &context)?;
        let (cancellation, retirement_guard) =
            self.prepare_worker_retirement_registration(&context, cancellation)?;
        runtime_policy
            .metrics()
            .record_request_correlation(&context);
        if cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            runtime_policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(
                    context.tenant_label.as_deref(),
                    cancellation.as_ref().and_then(HostCallCancellation::cause),
                );
            return Err(NimbusRuntimeError::Cancelled);
        }

        let (result_tx, result_rx) = oneshot::channel();
        let execution_plan = execution_plan_for_invocation(&runtime_policy, &request, &context);
        let invocation_id = context.invocation_id;
        let admission = self.inner.admission.admit_job(RuntimeWorkerJob {
            host: runtime.invocation_host(),
            policy: runtime_policy.clone(),
            bundle,
            request,
            context,
            execution_plan,
            cancellation: cancellation.clone(),
            enqueued_at: Instant::now(),
            response_ready_tx: None,
            result_tx: RuntimeWorkerResultSender::Async(result_tx),
            dispatch_handle: None,
            _retirement_guard: retirement_guard,
        })?;
        let queued = matches!(&admission, RuntimeExecutorAdmissionDecision::Queued);
        if queued {
            self.register_queued_cancellation_listener(
                invocation_id,
                &cancellation,
                runtime_policy,
            );
        }
        if let RuntimeExecutorAdmissionDecision::Dispatch(job) = admission {
            self.dispatch_admitted_job_async(*job).await?;
        }

        match cancellation {
            Some(cancellation) => {
                tokio::select! {
                    _ = cancellation.cancelled() => Err(NimbusRuntimeError::Cancelled),
                    result = result_rx => result.map_err(|_| {
                        NimbusRuntimeError::Contract(
                            "runtime executor dropped an invocation result".to_string(),
                        )
                    })?,
                }
            }
            None => result_rx.await.map_err(|_| {
                NimbusRuntimeError::Contract(
                    "runtime executor dropped an invocation result".to_string(),
                )
            })?,
        }
    }

    pub async fn invoke_on_worker_response_ready(
        &self,
        runtime: NimbusRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<RuntimeInvocationResponse> {
        let runtime_policy = runtime.policy();
        crate::retained_state::validate_retained_state_admission(&runtime_policy, &context)?;
        let (cancellation, retirement_guard) =
            self.prepare_worker_retirement_registration(&context, cancellation)?;
        runtime_policy
            .metrics()
            .record_request_correlation(&context);
        if cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            runtime_policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(
                    context.tenant_label.as_deref(),
                    cancellation.as_ref().and_then(HostCallCancellation::cause),
                );
            return Err(NimbusRuntimeError::Cancelled);
        }

        let (result_tx, result_rx) = oneshot::channel();
        let (response_ready_tx, response_ready_rx) = oneshot::channel();
        let execution_plan = execution_plan_for_invocation(&runtime_policy, &request, &context);
        let invocation_id = context.invocation_id;
        let admission = self.inner.admission.admit_job(RuntimeWorkerJob {
            host: runtime.invocation_host(),
            policy: runtime_policy.clone(),
            bundle,
            request,
            context,
            execution_plan,
            cancellation: cancellation.clone(),
            enqueued_at: Instant::now(),
            response_ready_tx: Some(response_ready_tx),
            result_tx: RuntimeWorkerResultSender::Async(result_tx),
            dispatch_handle: None,
            _retirement_guard: retirement_guard,
        })?;
        let queued = matches!(&admission, RuntimeExecutorAdmissionDecision::Queued);
        if queued {
            self.register_queued_cancellation_listener(
                invocation_id,
                &cancellation,
                runtime_policy,
            );
        }
        if let RuntimeExecutorAdmissionDecision::Dispatch(job) = admission {
            self.dispatch_admitted_job_async(*job).await?;
        }

        let mut result_rx = Box::pin(result_rx);
        let mut response_ready_rx = Box::pin(response_ready_rx);
        let dropped_completion = || {
            NimbusRuntimeError::Contract(
                "runtime executor dropped an invocation completion".to_string(),
            )
        };

        match cancellation {
            Some(cancellation) => {
                tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => Err(NimbusRuntimeError::Cancelled),
                    response = &mut response_ready_rx => match response {
                        Ok(response) => Ok(RuntimeInvocationResponse {
                            response,
                            completion: RuntimeInvocationCompletion::Pending(result_rx),
                        }),
                        Err(_) => {
                            let response = result_rx.await.map_err(|_| dropped_completion())??;
                            Ok(RuntimeInvocationResponse {
                                response,
                                completion: RuntimeInvocationCompletion::Complete,
                            })
                        }
                    },
                    result = &mut result_rx => {
                        let response = result.map_err(|_| dropped_completion())??;
                        Ok(RuntimeInvocationResponse {
                            response,
                            completion: RuntimeInvocationCompletion::Complete,
                        })
                    }
                }
            }
            None => {
                tokio::select! {
                    biased;
                    response = &mut response_ready_rx => match response {
                        Ok(response) => Ok(RuntimeInvocationResponse {
                            response,
                            completion: RuntimeInvocationCompletion::Pending(result_rx),
                        }),
                        Err(_) => {
                            let response = result_rx.await.map_err(|_| dropped_completion())??;
                            Ok(RuntimeInvocationResponse {
                                response,
                                completion: RuntimeInvocationCompletion::Complete,
                            })
                        }
                    },
                    result = &mut result_rx => {
                        let response = result.map_err(|_| dropped_completion())??;
                        Ok(RuntimeInvocationResponse {
                            response,
                            completion: RuntimeInvocationCompletion::Complete,
                        })
                    }
                }
            }
        }
    }

    pub fn invoke_blocking(
        &self,
        runtime: NimbusRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
    ) -> Result<Value> {
        self.invoke_blocking_with_cancellation(runtime, bundle, request, context, None)
    }

    pub fn invoke_blocking_with_cancellation(
        &self,
        runtime: NimbusRuntime,
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
        runtime: NimbusRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
    ) -> Result<Value> {
        let runtime_policy = runtime.policy();
        crate::retained_state::validate_retained_state_admission(&runtime_policy, &context)?;
        let (cancellation, retirement_guard) =
            self.prepare_worker_retirement_registration(&context, cancellation)?;
        runtime_policy
            .metrics()
            .record_request_correlation(&context);
        if cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            runtime_policy
                .metrics()
                .record_queued_canceled_invocation_for_tenant(
                    context.tenant_label.as_deref(),
                    cancellation.as_ref().and_then(HostCallCancellation::cause),
                );
            return Err(NimbusRuntimeError::Cancelled);
        }

        let (result_tx, result_rx) = std::sync::mpsc::sync_channel(1);
        let execution_plan = execution_plan_for_invocation(&runtime_policy, &request, &context);
        let invocation_id = context.invocation_id;
        let admission = self.inner.admission.admit_job(RuntimeWorkerJob {
            host: runtime.invocation_host(),
            policy: runtime_policy.clone(),
            bundle,
            request,
            context,
            execution_plan,
            cancellation: cancellation.clone(),
            enqueued_at: Instant::now(),
            response_ready_tx: None,
            result_tx: RuntimeWorkerResultSender::Blocking(result_tx),
            dispatch_handle: None,
            _retirement_guard: retirement_guard,
        })?;
        let queued = matches!(&admission, RuntimeExecutorAdmissionDecision::Queued);
        if queued {
            self.register_queued_cancellation_listener(
                invocation_id,
                &cancellation,
                runtime_policy,
            );
        }
        if let RuntimeExecutorAdmissionDecision::Dispatch(job) = admission {
            self.dispatch_admitted_job_blocking(*job)?;
        }

        match cancellation {
            Some(cancellation) => loop {
                if cancellation.is_cancelled() {
                    return Err(NimbusRuntimeError::Cancelled);
                }
                match result_rx.recv_timeout(BLOCKING_RESULT_POLL_INTERVAL) {
                    Ok(result) => return result,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err(NimbusRuntimeError::Contract(
                            "runtime executor dropped an invocation result".to_string(),
                        ));
                    }
                }
            },
            None => result_rx.recv().map_err(|_| {
                NimbusRuntimeError::Contract(
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
