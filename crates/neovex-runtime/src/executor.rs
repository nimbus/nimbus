#[cfg(test)]
use std::collections::HashMap;
use std::collections::{BTreeMap, VecDeque};
#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
#[cfg(test)]
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use tracing::debug;

use crate::context::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::{InvocationRequest, NeovexRuntime, RuntimeBundle, RuntimeWorkerIsolatePool};

struct RuntimeWorkerJob {
    runtime: NeovexRuntime,
    bundle: RuntimeBundle,
    request: InvocationRequest,
    context: RuntimeInvocationContext,
    cancellation: Option<HostCallCancellation>,
    enqueued_at: Instant,
    result_tx: RuntimeWorkerResultSender,
}

enum RuntimeWorkerResultSender {
    Async(oneshot::Sender<Result<Value>>),
    Blocking(std::sync::mpsc::SyncSender<Result<Value>>),
}

impl RuntimeWorkerResultSender {
    fn send(self, result: Result<Value>) {
        match self {
            Self::Async(result_tx) => {
                let _ = result_tx.send(result);
            }
            Self::Blocking(result_tx) => {
                let _ = result_tx.send(result);
            }
        }
    }
}

#[derive(Clone)]
pub struct RuntimeExecutor {
    inner: Arc<RuntimeExecutorInner>,
}

struct RuntimeExecutorInner {
    policy: Arc<RuntimePolicy>,
    sender: Arc<Mutex<Option<mpsc::Sender<RuntimeWorkerJob>>>>,
    admission: Arc<RuntimeExecutorAdmission>,
    worker_count: usize,
    queue_capacity: usize,
    worker_handles: Mutex<Vec<JoinHandle<()>>>,
    #[cfg(test)]
    test_state: Arc<RuntimeExecutorTestState>,
}

#[cfg(test)]
struct RuntimeExecutorTestState {
    next_worker_runtime_id: AtomicUsize,
    worker_runtime_builds: AtomicUsize,
    worker_thread_runtime_ids: Mutex<HashMap<ThreadId, usize>>,
}

#[cfg(test)]
impl RuntimeExecutorTestState {
    fn new() -> Self {
        Self {
            next_worker_runtime_id: AtomicUsize::new(1),
            worker_runtime_builds: AtomicUsize::new(0),
            worker_thread_runtime_ids: Mutex::new(HashMap::new()),
        }
    }

    fn register_current_worker_runtime(&self) {
        let worker_runtime_id = self.next_worker_runtime_id.fetch_add(1, Ordering::Relaxed);
        self.worker_runtime_builds.fetch_add(1, Ordering::Relaxed);
        self.worker_thread_runtime_ids
            .lock()
            .expect("runtime executor test state lock should not be poisoned")
            .insert(std::thread::current().id(), worker_runtime_id);
    }

    fn worker_runtime_builds(&self) -> usize {
        self.worker_runtime_builds.load(Ordering::Relaxed)
    }

    fn worker_runtime_id_for_current_thread(&self) -> Option<usize> {
        self.worker_thread_runtime_ids
            .lock()
            .expect("runtime executor test state lock should not be poisoned")
            .get(&std::thread::current().id())
            .copied()
    }
}

struct RuntimeExecutorAdmission {
    policy: Arc<RuntimePolicy>,
    state: Mutex<RuntimeExecutorAdmissionState>,
}

#[derive(Default)]
struct RuntimeExecutorAdmissionState {
    tenants: BTreeMap<String, RuntimeExecutorTenantAdmissionState>,
    queued_tenants: VecDeque<String>,
}

#[derive(Default)]
struct RuntimeExecutorTenantAdmissionState {
    in_flight: usize,
    queued_jobs: VecDeque<RuntimeWorkerJob>,
    queued_in_rotation: bool,
}

enum RuntimeExecutorAdmissionDecision {
    Dispatch(Box<RuntimeWorkerJob>),
    Queued,
}

impl RuntimeExecutorAdmission {
    fn new(policy: Arc<RuntimePolicy>) -> Self {
        Self {
            policy,
            state: Mutex::new(RuntimeExecutorAdmissionState::default()),
        }
    }

    fn admit_job(&self, job: RuntimeWorkerJob) -> Result<RuntimeExecutorAdmissionDecision> {
        let Some(tenant_label) = Self::fairness_tenant_label(&job).map(str::to_owned) else {
            return Ok(RuntimeExecutorAdmissionDecision::Dispatch(Box::new(job)));
        };

        let limits = self.policy.limits();
        let max_in_flight = limits.max_top_level_invocations_per_tenant;
        let max_queued = limits.max_queued_top_level_invocations_per_tenant;
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        let tenant_state = state.tenants.entry(tenant_label.clone()).or_default();
        if tenant_state.in_flight < max_in_flight && tenant_state.queued_jobs.is_empty() {
            tenant_state.in_flight += 1;
            return Ok(RuntimeExecutorAdmissionDecision::Dispatch(Box::new(job)));
        }
        if tenant_state.queued_jobs.len() >= max_queued {
            drop(state);
            self.policy
                .metrics()
                .record_rejected_invocation_for_tenant(Some(&tenant_label));
            return Err(NeovexRuntimeError::TenantQueueLimitExceeded {
                tenant_label,
                limit: max_queued,
            });
        }

        tenant_state.queued_jobs.push_back(job);
        if !tenant_state.queued_in_rotation {
            tenant_state.queued_in_rotation = true;
            state.queued_tenants.push_back(tenant_label);
        }
        Ok(RuntimeExecutorAdmissionDecision::Queued)
    }

    fn release_dispatched_job(
        &self,
        tenant_label: Option<&str>,
        bypasses_concurrency_limit: bool,
    ) -> Vec<RuntimeWorkerJob> {
        let Some(tenant_label) = (!bypasses_concurrency_limit)
            .then_some(tenant_label)
            .flatten()
        else {
            return Vec::new();
        };
        let max_in_flight = self.policy.limits().max_top_level_invocations_per_tenant;
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        if let Some(tenant_state) = state.tenants.get_mut(tenant_label) {
            tenant_state.in_flight = tenant_state.in_flight.saturating_sub(1);
        }
        Self::cleanup_tenant_locked(&mut state, tenant_label);
        Self::promote_ready_jobs_locked(&mut state, max_in_flight)
    }

    fn rollback_dispatched_job(
        &self,
        tenant_label: Option<&str>,
        bypasses_concurrency_limit: bool,
    ) {
        let Some(tenant_label) = (!bypasses_concurrency_limit)
            .then_some(tenant_label)
            .flatten()
        else {
            return;
        };
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        if let Some(tenant_state) = state.tenants.get_mut(tenant_label) {
            tenant_state.in_flight = tenant_state.in_flight.saturating_sub(1);
        }
        Self::cleanup_tenant_locked(&mut state, tenant_label);
    }

    fn drain_queued_jobs(&self) -> Vec<RuntimeWorkerJob> {
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        state.queued_tenants.clear();
        let mut queued_jobs = Vec::new();
        for tenant_state in state.tenants.values_mut() {
            tenant_state.queued_in_rotation = false;
            queued_jobs.extend(tenant_state.queued_jobs.drain(..));
        }
        queued_jobs
    }

    fn fairness_tenant_label(job: &RuntimeWorkerJob) -> Option<&str> {
        (!job.runtime.bypasses_concurrency_limit())
            .then_some(job.context.tenant_label.as_deref())
            .flatten()
    }

    fn promote_ready_jobs_locked(
        state: &mut RuntimeExecutorAdmissionState,
        max_in_flight: usize,
    ) -> Vec<RuntimeWorkerJob> {
        let mut promoted_jobs = Vec::new();
        loop {
            let cycle_len = state.queued_tenants.len();
            if cycle_len == 0 {
                break;
            }
            let mut promoted_this_cycle = false;
            for _ in 0..cycle_len {
                let Some(tenant_label) = state.queued_tenants.pop_front() else {
                    break;
                };
                let mut promoted_job = None;
                let mut requeue_tenant = false;
                let mut remove_tenant = false;
                if let Some(tenant_state) = state.tenants.get_mut(&tenant_label) {
                    if tenant_state.queued_jobs.is_empty() {
                        tenant_state.queued_in_rotation = false;
                        remove_tenant = tenant_state.in_flight == 0;
                    } else if tenant_state.in_flight < max_in_flight {
                        promoted_job = tenant_state.queued_jobs.pop_front();
                        tenant_state.in_flight += 1;
                        if tenant_state.queued_jobs.is_empty() {
                            tenant_state.queued_in_rotation = false;
                            remove_tenant = tenant_state.in_flight == 0;
                        } else {
                            requeue_tenant = true;
                        }
                    } else {
                        requeue_tenant = true;
                    }
                }
                if requeue_tenant {
                    state.queued_tenants.push_back(tenant_label.clone());
                }
                if remove_tenant {
                    state.tenants.remove(&tenant_label);
                }
                if let Some(job) = promoted_job {
                    promoted_jobs.push(job);
                    promoted_this_cycle = true;
                    break;
                }
            }
            if !promoted_this_cycle {
                break;
            }
        }
        promoted_jobs
    }

    fn cleanup_tenant_locked(state: &mut RuntimeExecutorAdmissionState, tenant_label: &str) {
        let should_remove = state.tenants.get(tenant_label).is_some_and(|tenant_state| {
            tenant_state.in_flight == 0
                && tenant_state.queued_jobs.is_empty()
                && !tenant_state.queued_in_rotation
        });
        if should_remove {
            state.tenants.remove(tenant_label);
        }
    }
}

const BLOCKING_RESULT_POLL_INTERVAL: Duration = Duration::from_millis(1);

impl std::fmt::Debug for RuntimeExecutor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeExecutor")
            .field("worker_count", &self.inner.worker_count)
            .field("queue_capacity", &self.inner.queue_capacity)
            .finish()
    }
}

impl RuntimeExecutor {
    pub fn new(policy: Arc<RuntimePolicy>) -> Self {
        let worker_count = policy.limits().max_concurrent_isolates.max(1);
        let queue_capacity = worker_count.saturating_mul(4).max(1);
        let (worker_sender, receiver) = mpsc::channel::<RuntimeWorkerJob>(queue_capacity);
        let sender = Arc::new(Mutex::new(Some(worker_sender)));
        let receiver = Arc::new(Mutex::new(receiver));
        let admission = Arc::new(RuntimeExecutorAdmission::new(policy.clone()));
        #[cfg(test)]
        let test_state = Arc::new(RuntimeExecutorTestState::new());
        let mut worker_handles = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let receiver = receiver.clone();
            let policy = policy.clone();
            let sender = sender.clone();
            let admission = admission.clone();
            #[cfg(test)]
            let test_state = test_state.clone();
            let handle = std::thread::Builder::new()
                .name(format!("neovex-runtime-worker-{worker_id}"))
                .spawn(move || {
                    let mut isolate_pool = RuntimeWorkerIsolatePool::new();
                    let worker_runtime = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .map_err(|error| {
                            format!("runtime worker failed to build tokio runtime: {error}")
                        });
                    #[cfg(test)]
                    if worker_runtime.is_ok() {
                        test_state.register_current_worker_runtime();
                    }

                    loop {
                        let job = {
                            let mut receiver = receiver
                                .lock()
                                .expect("runtime executor receiver lock should not be poisoned");
                            receiver.blocking_recv()
                        };
                        let Some(job) = job else {
                            break;
                        };
                        let tenant_label = job.context.tenant_label.clone();
                        let bypasses_concurrency_limit = job.runtime.bypasses_concurrency_limit();

                        if job
                            .cancellation
                            .as_ref()
                            .is_some_and(HostCallCancellation::is_cancelled)
                        {
                            policy
                                .metrics()
                                .record_queued_canceled_invocation_for_tenant(
                                    job.context.tenant_label.as_deref(),
                                    job.cancellation
                                        .as_ref()
                                        .and_then(HostCallCancellation::cause),
                                );
                            job.result_tx.send(Err(NeovexRuntimeError::Cancelled));
                            Self::dispatch_ready_jobs_blocking(
                                &sender,
                                &admission,
                                admission.release_dispatched_job(
                                    tenant_label.as_deref(),
                                    bypasses_concurrency_limit,
                                ),
                            );
                            continue;
                        }

                        policy.metrics().record_worker_dispatch();

                        let result = match &worker_runtime {
                            Ok(worker_runtime) => worker_runtime.block_on(Self::invoke_job(
                                Some(&mut isolate_pool),
                                job.runtime.into_policy(policy.clone()),
                                job.bundle,
                                job.request,
                                job.context,
                                job.cancellation,
                                job.enqueued_at,
                            )),
                            Err(error) => Err(NeovexRuntimeError::Contract(error.clone())),
                        };
                        job.result_tx.send(result);
                        Self::dispatch_ready_jobs_blocking(
                            &sender,
                            &admission,
                            admission.release_dispatched_job(
                                tenant_label.as_deref(),
                                bypasses_concurrency_limit,
                            ),
                        );
                    }
                })
                .expect("runtime executor worker thread should start");
            worker_handles.push(handle);
        }

        Self {
            inner: Arc::new(RuntimeExecutorInner {
                policy,
                sender,
                admission,
                worker_count,
                queue_capacity,
                worker_handles: Mutex::new(worker_handles),
                #[cfg(test)]
                test_state,
            }),
        }
    }

    pub fn policy(&self) -> Arc<RuntimePolicy> {
        self.inner.policy.clone()
    }

    #[cfg(test)]
    fn test_state(&self) -> Arc<RuntimeExecutorTestState> {
        self.inner.test_state.clone()
    }

    async fn dispatch_admitted_job_async(&self, job: RuntimeWorkerJob) -> Result<()> {
        let tenant_label = job.context.tenant_label.clone();
        let bypasses_concurrency_limit = job.runtime.bypasses_concurrency_limit();
        let sender = self
            .inner
            .sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
            })?;
        sender.send(job).await.map_err(|error| {
            self.inner
                .admission
                .rollback_dispatched_job(tenant_label.as_deref(), bypasses_concurrency_limit);
            drop(error);
            NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
        })
    }

    fn dispatch_admitted_job_blocking(&self, job: RuntimeWorkerJob) -> Result<()> {
        let tenant_label = job.context.tenant_label.clone();
        let bypasses_concurrency_limit = job.runtime.bypasses_concurrency_limit();
        let sender = self
            .inner
            .sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
            })?;
        sender.blocking_send(job).map_err(|error| {
            self.inner
                .admission
                .rollback_dispatched_job(tenant_label.as_deref(), bypasses_concurrency_limit);
            drop(error);
            NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
        })
    }

    fn dispatch_ready_jobs_blocking(
        sender: &Arc<Mutex<Option<mpsc::Sender<RuntimeWorkerJob>>>>,
        admission: &RuntimeExecutorAdmission,
        ready_jobs: Vec<RuntimeWorkerJob>,
    ) {
        for job in ready_jobs {
            let tenant_label = job.context.tenant_label.clone();
            let bypasses_concurrency_limit = job.runtime.bypasses_concurrency_limit();
            let dispatch_sender = sender
                .lock()
                .expect("runtime executor sender lock should not be poisoned")
                .as_ref()
                .cloned();
            match dispatch_sender {
                Some(dispatch_sender) => match dispatch_sender.blocking_send(job) {
                    Ok(()) => {}
                    Err(error) => {
                        let failed_job = error.0;
                        admission.rollback_dispatched_job(
                            tenant_label.as_deref(),
                            bypasses_concurrency_limit,
                        );
                        failed_job.result_tx.send(Err(NeovexRuntimeError::Contract(
                            "runtime executor unexpectedly closed".to_string(),
                        )));
                    }
                },
                None => {
                    admission.rollback_dispatched_job(
                        tenant_label.as_deref(),
                        bypasses_concurrency_limit,
                    );
                    job.result_tx.send(Err(NeovexRuntimeError::Contract(
                        "runtime executor unexpectedly closed".to_string(),
                    )));
                }
            }
        }
    }

    async fn invoke_job(
        isolate_pool: Option<&mut RuntimeWorkerIsolatePool>,
        runtime: NeovexRuntime,
        bundle: RuntimeBundle,
        request: InvocationRequest,
        context: RuntimeInvocationContext,
        cancellation: Option<HostCallCancellation>,
        queue_started_at: Instant,
    ) -> Result<Value> {
        let policy = runtime.policy();
        let metrics = policy.metrics();
        let _permit = if runtime.bypasses_concurrency_limit() {
            None
        } else {
            metrics.increment_queued_invocations();
            Some(
                policy
                    .isolate_semaphore()
                    .acquire_owned()
                    .await
                    .map_err(|_| {
                        NeovexRuntimeError::Contract(
                            "runtime isolate semaphore unexpectedly closed".to_string(),
                        )
                    })?,
            )
        };
        if !runtime.bypasses_concurrency_limit() {
            metrics.decrement_queued_invocations();
            let queue_wait = queue_started_at.elapsed();
            metrics.record_queue_wait_for_tenant(context.tenant_label.as_deref(), queue_wait);
            debug!(
                invocation_id = context.invocation_id,
                request_id = ?context.server_request_id,
                tenant = context.tenant_label.as_deref().unwrap_or("unknown"),
                function = %context.function_name,
                kind = context.kind,
                queue_wait_ms = queue_wait.as_secs_f64() * 1000.0,
                queued_invocations = metrics.snapshot().queued_invocations,
                "runtime invocation admitted"
            );
        }

        metrics.increment_active_isolates_for_tenant(context.tenant_label.as_deref());
        let execution_started_at = Instant::now();
        let cancellation_for_metrics = cancellation.clone();
        runtime
            .invoke_bundle_unmanaged(isolate_pool, &bundle, &request, &context, cancellation)
            .await
            .inspect_err(|error| match error {
                NeovexRuntimeError::ExecutionTimeout(_) => metrics.record_timeout(),
                NeovexRuntimeError::Cancelled => metrics
                    .record_in_flight_canceled_invocation_for_tenant(
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
                debug!(
                    invocation_id = context.invocation_id,
                    request_id = ?context.server_request_id,
                    tenant = context.tenant_label.as_deref().unwrap_or("unknown"),
                    function = %context.function_name,
                    kind = context.kind,
                    execution_ms = execution.as_secs_f64() * 1000.0,
                    active_isolates = metrics.snapshot().active_isolates,
                    "runtime invocation completed"
                );
            })
            .inspect_err(|_| {
                let execution = execution_started_at.elapsed();
                metrics.record_execution_for_tenant(context.tenant_label.as_deref(), execution);
            })
            .inspect(|_| {
                metrics.decrement_active_isolates_for_tenant(context.tenant_label.as_deref())
            })
            .inspect_err(|_| {
                metrics.decrement_active_isolates_for_tenant(context.tenant_label.as_deref())
            })
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
            None,
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

impl Drop for RuntimeExecutorInner {
    fn drop(&mut self) {
        self.sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .take();
        for queued_job in self.admission.drain_queued_jobs() {
            queued_job.result_tx.send(Err(NeovexRuntimeError::Contract(
                "runtime executor unexpectedly closed".to_string(),
            )));
        }
        let mut worker_handles = self
            .worker_handles
            .lock()
            .expect("runtime executor worker handle lock should not be poisoned");
        for handle in worker_handles.drain(..) {
            let _ = handle.join();
        }
    }
}

impl Default for RuntimeExecutor {
    fn default() -> Self {
        let policy = Arc::new(RuntimePolicy::default());
        Self::new(policy)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use std::sync::Mutex as StdMutex;
    use std::sync::{Arc, OnceLock};

    use serde_json::{Value, json};
    use tempfile::tempdir;
    use tokio::sync::{Mutex as TokioMutex, Notify};

    use super::*;
    use crate::host::{HostBridge, HostBridgeFuture, HostCallRequest};
    use crate::limits::RuntimeLimits;

    struct NoopHost;

    impl HostBridge for NoopHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Ok(Value::Null)
        }
    }

    struct WorkerRuntimeIdHost {
        test_state: Arc<RuntimeExecutorTestState>,
    }

    impl HostBridge for WorkerRuntimeIdHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            assert_eq!(request.operation, "convex.ctx.db.get");
            Ok(json!({
                "workerRuntimeId": self.test_state.worker_runtime_id_for_current_thread(),
            }))
        }
    }

    #[derive(Default)]
    struct TenantFairnessHost {
        started_ids: StdMutex<Vec<String>>,
        slow_started: Arc<Notify>,
        release_slow: Arc<Notify>,
    }

    impl TenantFairnessHost {
        fn started_ids(&self) -> Vec<String> {
            self.started_ids
                .lock()
                .expect("tenant fairness host lock should not be poisoned")
                .clone()
        }

        fn release_slow_job(&self) {
            self.release_slow.notify_waiters();
        }
    }

    impl HostBridge for TenantFairnessHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "tenant fairness host expects async db.get path".to_string(),
            ))
        }

        fn call_async(
            &self,
            request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            let document_id = request
                .payload
                .get("id")
                .and_then(Value::as_str)
                .expect("db.get payload should carry an id")
                .to_string();
            self.started_ids
                .lock()
                .expect("tenant fairness host lock should not be poisoned")
                .push(document_id.clone());
            let slow_started = self.slow_started.clone();
            let release_slow = self.release_slow.clone();
            Box::pin(async move {
                if document_id == "slow-1" {
                    slow_started.notify_waiters();
                    release_slow.notified().await;
                }
                Ok(json!({
                    "status": "ok",
                    "value": {
                        "id": document_id,
                    },
                }))
            })
        }
    }

    fn write_runtime_id_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", "doc-1");
};

export {};
"#,
        )
        .expect("bundle should write");
        (bundle_dir, bundle_path)
    }

    fn write_busy_loop_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = function () {
  while (true) {}
};

export {};
"#,
        )
        .expect("bundle should write");
        (bundle_dir, bundle_path)
    }

    fn write_function_named_get_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function (request) {
  const ctx = globalThis.__neovexCreateContext({
    request,
    sessionId: `${request.kind}:${request.function_name}`,
  });
  return await ctx.db.get("messages", request.function_name);
};

export {};
"#,
        )
        .expect("bundle should write");
        (bundle_dir, bundle_path)
    }

    fn write_constant_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  return "ok";
};

export {};
"#,
        )
        .expect("bundle should write");
        (bundle_dir, bundle_path)
    }

    fn test_request(function_name: &str) -> InvocationRequest {
        InvocationRequest {
            kind: crate::runtime::InvocationKind::Query,
            function_name: function_name.to_string(),
            args: Value::Null,
            page_size: None,
            cursor: None,
            auth: None,
        }
    }

    fn test_context_for_tenant(
        request: &InvocationRequest,
        tenant_label: &str,
        request_id: &str,
    ) -> RuntimeInvocationContext {
        RuntimeInvocationContext::top_level_for_tenant_and_request(
            request,
            tenant_label,
            request_id,
        )
    }

    fn test_context(request: &InvocationRequest, request_id: &str) -> RuntimeInvocationContext {
        test_context_for_tenant(request, "demo", request_id)
    }

    fn runtime_executor_test_lock() -> &'static TokioMutex<()> {
        static RUNTIME_EXECUTOR_TEST_LOCK: OnceLock<TokioMutex<()>> = OnceLock::new();
        RUNTIME_EXECUTOR_TEST_LOCK.get_or_init(|| TokioMutex::new(()))
    }

    #[tokio::test]
    async fn pre_canceled_worker_invocation_records_request_correlation() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let policy = Arc::new(RuntimePolicy::default());
        let executor = RuntimeExecutor::new(policy.clone());
        let request = test_request("messages:list");
        let context = test_context(&request, "req-pre-canceled");
        let cancellation = HostCallCancellation::default();
        cancellation.cancel_due_to_disconnect();

        let error = executor
            .invoke_on_worker(
                NeovexRuntime::new(Arc::new(NoopHost)),
                RuntimeBundle::new("unused.mjs"),
                request,
                context,
                Some(cancellation),
            )
            .await
            .expect_err("pre-canceled worker invocation should fail");

        assert!(matches!(error, NeovexRuntimeError::Cancelled));

        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.queued_canceled_invocations, 1);
        assert_eq!(snapshot.disconnect_canceled_invocations, 1);
        assert_eq!(snapshot.recent_request_correlations.len(), 1);
        let correlation = &snapshot.recent_request_correlations[0];
        assert_eq!(correlation.server_request_id, "req-pre-canceled");
        assert_eq!(correlation.function_name, "messages:list");
        assert_eq!(correlation.kind, "query");
        assert_eq!(correlation.tenant_label.as_deref(), Some("demo"));
        assert!(correlation.invocation_id > 0);
    }

    #[tokio::test]
    async fn worker_invocations_reuse_worker_local_tokio_runtime() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let host = Arc::new(WorkerRuntimeIdHost {
            test_state: test_state.clone(),
        });
        let request = test_request("messages:list");

        let first_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-worker-1"),
                None,
            )
            .await
            .expect("first worker invocation should succeed");
        let second_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host, policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-worker-2"),
                None,
            )
            .await
            .expect("second worker invocation should succeed");

        assert_eq!(first_result, json!({ "workerRuntimeId": 1 }));
        assert_eq!(second_result, json!({ "workerRuntimeId": 1 }));
        assert_eq!(test_state.worker_runtime_builds(), 1);
        let metrics = executor.policy().metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 1);
        assert_eq!(metrics.isolate_pool_replacements, 0);
    }

    #[test]
    fn sibling_threads_can_boot_runtime_executors_in_parallel() {
        let _test_lock = runtime_executor_test_lock().blocking_lock();
        let (_bundle_dir, bundle_path) = write_constant_bundle();
        let before = crate::runtime::bootstrap_snapshot_build_count_for_test();
        let barrier = Arc::new(Barrier::new(3));

        let worker =
            |request_id: &'static str, barrier: Arc<Barrier>, bundle_path: std::path::PathBuf| {
                std::thread::spawn(move || {
                    let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
                        max_concurrent_isolates: 1,
                        ..RuntimeLimits::default()
                    }));
                    let executor = RuntimeExecutor::new(policy.clone());
                    let request = test_request("messages:list");
                    barrier.wait();
                    executor.invoke_blocking_with_cancellation(
                        NeovexRuntime::with_policy(Arc::new(NoopHost), policy),
                        RuntimeBundle::new(bundle_path),
                        request.clone(),
                        test_context(&request, request_id),
                        None,
                    )
                })
            };

        let first = worker("req-sibling-1", barrier.clone(), bundle_path.clone());
        let second = worker("req-sibling-2", barrier.clone(), bundle_path);
        barrier.wait();

        assert_eq!(
            first
                .join()
                .expect("first sibling-thread executor should join")
                .expect("first sibling-thread invocation should succeed"),
            json!("ok")
        );
        assert_eq!(
            second
                .join()
                .expect("second sibling-thread executor should join")
                .expect("second sibling-thread invocation should succeed"),
            json!("ok")
        );

        let after = crate::runtime::bootstrap_snapshot_build_count_for_test();
        assert!(
            after.saturating_sub(before) <= 1,
            "parallel sibling-thread executor startups should reuse one process-global bootstrap snapshot"
        );
    }

    #[test]
    fn blocking_worker_invocation_succeeds_without_tokio_runtime_on_calling_thread() {
        let _test_lock = runtime_executor_test_lock().blocking_lock();
        assert!(
            tokio::runtime::Handle::try_current().is_err(),
            "plain #[test] should not already be inside a Tokio runtime"
        );

        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let test_state = executor.test_state();
        let request = test_request("messages:list");
        let cancellation = HostCallCancellation::default();

        let result = executor
            .invoke_blocking_with_cancellation(
                NeovexRuntime::with_policy(
                    Arc::new(WorkerRuntimeIdHost {
                        test_state: test_state.clone(),
                    }),
                    policy.clone(),
                ),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-blocking-worker"),
                Some(cancellation),
            )
            .expect("blocking worker invocation should succeed");

        assert_eq!(result, json!({ "workerRuntimeId": 1 }));
        assert_eq!(test_state.worker_runtime_builds(), 1);
    }

    #[tokio::test]
    async fn timed_out_worker_invocations_record_isolate_pool_replacements() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_timeout_bundle_dir, timeout_bundle_path) = write_busy_loop_bundle();
        let (_bundle_dir, bundle_path) = write_runtime_id_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_timeout: Duration::from_millis(50),
            max_concurrent_isolates: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let request = test_request("messages:list");

        let error = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(Arc::new(NoopHost), policy.clone()),
                RuntimeBundle::new(&timeout_bundle_path),
                request.clone(),
                test_context(&request, "req-timeout"),
                None,
            )
            .await
            .expect_err("busy-loop invocation should time out");
        assert!(matches!(error, NeovexRuntimeError::ExecutionTimeout(_)));

        let recovery_result = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(Arc::new(NoopHost), policy.clone()),
                RuntimeBundle::new(&bundle_path),
                request.clone(),
                test_context(&request, "req-recovery"),
                None,
            )
            .await
            .expect("follow-up invocation should succeed");
        assert_eq!(recovery_result, Value::Null);

        let metrics = policy.metrics_snapshot();
        assert_eq!(metrics.isolate_pool_misses, 1);
        assert_eq!(metrics.isolate_pool_hits, 1);
        assert_eq!(metrics.isolate_pool_replacements, 1);
    }

    #[tokio::test]
    async fn tenant_queue_limit_rejections_record_metrics() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            max_top_level_invocations_per_tenant: 1,
            max_queued_top_level_invocations_per_tenant: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(TenantFairnessHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);

        let slow_request = test_request("slow-1");
        let slow_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&slow_request, "tenant-a", "req-slow-1");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        slow_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(1), host.slow_started.notified())
            .await
            .expect("slow runtime invocation should start");

        let queued_request = test_request("slow-2");
        let queued_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&queued_request, "tenant-a", "req-slow-2");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        queued_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let rejected_request = test_request("slow-3");
        let error = executor
            .invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                bundle.clone(),
                rejected_request.clone(),
                test_context_for_tenant(&rejected_request, "tenant-a", "req-slow-3"),
                None,
            )
            .await
            .expect_err("third tenant-a invocation should be rejected");
        assert!(matches!(
            error,
            NeovexRuntimeError::TenantQueueLimitExceeded {
                ref tenant_label,
                limit: 1,
            } if tenant_label == "tenant-a"
        ));

        let metrics = policy.metrics_snapshot();
        assert_eq!(metrics.rejected_invocations, 1);
        assert_eq!(
            metrics
                .tenants
                .get("tenant-a")
                .expect("tenant metrics should be present")
                .rejected_invocations,
            1
        );

        host.release_slow_job();
        assert_eq!(
            slow_task
                .await
                .expect("slow task should join")
                .expect("slow invocation should succeed"),
            json!({ "id": "slow-1" })
        );
        assert_eq!(
            queued_task
                .await
                .expect("queued task should join")
                .expect("queued invocation should succeed"),
            json!({ "id": "slow-2" })
        );
    }

    #[tokio::test]
    async fn tenant_fairness_prevents_one_tenant_from_starving_another() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 2,
            max_top_level_invocations_per_tenant: 1,
            max_queued_top_level_invocations_per_tenant: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(TenantFairnessHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);

        let slow_request = test_request("slow-1");
        let slow_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&slow_request, "tenant-a", "req-tenant-a-1");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        slow_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(1), host.slow_started.notified())
            .await
            .expect("slow tenant-a invocation should start");

        let queued_request = test_request("slow-2");
        let queued_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&queued_request, "tenant-a", "req-tenant-a-2");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        queued_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;

        let fast_request = test_request("fast-1");
        let fast_result = tokio::time::timeout(
            Duration::from_secs(1),
            executor.invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                bundle.clone(),
                fast_request.clone(),
                test_context_for_tenant(&fast_request, "tenant-b", "req-tenant-b-1"),
                None,
            ),
        )
        .await
        .expect("tenant-b invocation should not be starved")
        .expect("tenant-b invocation should succeed");
        assert_eq!(fast_result, json!({ "id": "fast-1" }));
        assert!(
            !host.started_ids().iter().any(|id| id == "slow-2"),
            "tenant-a queued invocation should remain queued until tenant-a frees a slot"
        );

        host.release_slow_job();
        assert_eq!(
            slow_task
                .await
                .expect("slow task should join")
                .expect("slow invocation should succeed"),
            json!({ "id": "slow-1" })
        );
        assert_eq!(
            queued_task
                .await
                .expect("queued task should join")
                .expect("queued invocation should succeed"),
            json!({ "id": "slow-2" })
        );
    }
}
