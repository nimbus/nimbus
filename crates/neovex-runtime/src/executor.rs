use std::cell::RefCell;
#[cfg(test)]
use std::collections::HashMap;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
#[cfg(test)]
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, mpsc, oneshot};
use tracing::debug;

use crate::context::RuntimeInvocationContext;
use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::{
    InvocationRequest, NeovexRuntime, RuntimeBundle, RuntimeInvocationExecution,
    RuntimeInvocationTimeoutController,
};
use crate::watchdog::WatchdogTimer;
use crate::worker_loop::{RunToCompletionWorkerLoopFactory, WorkerLoopFactory};

pub(crate) struct RuntimeWorkerJob {
    pub(crate) runtime: NeovexRuntime,
    pub(crate) bundle: RuntimeBundle,
    pub(crate) request: InvocationRequest,
    pub(crate) context: RuntimeInvocationContext,
    pub(crate) cancellation: Option<HostCallCancellation>,
    pub(crate) enqueued_at: Instant,
    pub(crate) result_tx: RuntimeWorkerResultSender,
    pub(crate) dispatch_handle: Option<RuntimeInvocationDispatchHandle>,
}

pub(crate) enum RuntimeWorkerResultSender {
    Async(oneshot::Sender<Result<Value>>),
    Blocking(std::sync::mpsc::SyncSender<Result<Value>>),
}

impl RuntimeWorkerResultSender {
    pub(crate) fn send(self, result: Result<Value>) {
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

pub(crate) trait RuntimeWorkerQueue: Send + Sync + 'static {
    fn recv_blocking(&self) -> Option<RuntimeWorkerJob>;

    fn complete_job(
        &self,
        job: RuntimeWorkerJob,
        result: Result<Value>,
        ready_jobs: Vec<RuntimeWorkerJob>,
    );
}

#[derive(Clone)]
pub(crate) struct RuntimeWorkerShutdown {
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl RuntimeWorkerShutdown {
    fn new() -> Self {
        Self {
            cancelled: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }
}

#[derive(Clone)]
pub struct RuntimeExecutor {
    inner: Arc<RuntimeExecutorInner>,
}

struct RuntimeExecutorInner {
    policy: Arc<RuntimePolicy>,
    queue: Arc<RuntimeWorkerQueueController>,
    admission: Arc<RuntimeExecutorAdmission>,
    shutdown: RuntimeWorkerShutdown,
    watchdog: WatchdogTimer,
    worker_count: usize,
    queue_capacity: usize,
    worker_handles: Mutex<Vec<JoinHandle<()>>>,
    #[cfg(test)]
    test_state: Arc<RuntimeExecutorTestState>,
}

#[cfg(test)]
pub(crate) struct RuntimeExecutorTestState {
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

    pub(crate) fn register_current_worker_runtime(&self) {
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

struct RuntimeWorkerQueueController {
    receiver: Arc<Mutex<mpsc::Receiver<RuntimeWorkerJob>>>,
    sender: Arc<Mutex<Option<mpsc::Sender<RuntimeWorkerJob>>>>,
}

impl RuntimeWorkerQueueController {
    fn new(
        receiver: Arc<Mutex<mpsc::Receiver<RuntimeWorkerJob>>>,
        sender: Arc<Mutex<Option<mpsc::Sender<RuntimeWorkerJob>>>>,
    ) -> Self {
        Self { receiver, sender }
    }

    async fn dispatch_job(&self, job: RuntimeWorkerJob) -> Result<()> {
        let dispatch_handle = job.dispatch_handle.clone();
        let sender = self
            .sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
            })?;
        sender.send(job).await.map_err(|error| {
            if let Some(dispatch_handle) = dispatch_handle {
                dispatch_handle.rollback_dispatch();
            }
            drop(error);
            NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
        })
    }

    fn dispatch_job_blocking(&self, job: RuntimeWorkerJob) -> Result<()> {
        let dispatch_handle = job.dispatch_handle.clone();
        let sender = self
            .sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .as_ref()
            .cloned()
            .ok_or_else(|| {
                NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
            })?;
        sender.blocking_send(job).map_err(|error| {
            if let Some(dispatch_handle) = dispatch_handle {
                dispatch_handle.rollback_dispatch();
            }
            drop(error);
            NeovexRuntimeError::Contract("runtime executor unexpectedly closed".to_string())
        })
    }

    fn close(&self) {
        self.sender
            .lock()
            .expect("runtime executor sender lock should not be poisoned")
            .take();
    }
}

impl RuntimeWorkerQueue for RuntimeWorkerQueueController {
    fn recv_blocking(&self) -> Option<RuntimeWorkerJob> {
        let mut receiver = self
            .receiver
            .lock()
            .expect("runtime executor receiver lock should not be poisoned");
        receiver.blocking_recv()
    }

    fn complete_job(
        &self,
        job: RuntimeWorkerJob,
        result: Result<Value>,
        ready_jobs: Vec<RuntimeWorkerJob>,
    ) {
        job.result_tx.send(result);
        for ready_job in ready_jobs {
            let dispatch_handle = ready_job.dispatch_handle.clone();
            let dispatch_sender = self
                .sender
                .lock()
                .expect("runtime executor sender lock should not be poisoned")
                .as_ref()
                .cloned();
            match dispatch_sender {
                Some(dispatch_sender) => match dispatch_sender.blocking_send(ready_job) {
                    Ok(()) => {}
                    Err(error) => {
                        let failed_job = error.0;
                        if let Some(dispatch_handle) = dispatch_handle {
                            dispatch_handle.rollback_dispatch();
                        }
                        failed_job.result_tx.send(Err(NeovexRuntimeError::Contract(
                            "runtime executor unexpectedly closed".to_string(),
                        )));
                    }
                },
                None => {
                    if let Some(dispatch_handle) = dispatch_handle {
                        dispatch_handle.rollback_dispatch();
                    }
                    ready_job.result_tx.send(Err(NeovexRuntimeError::Contract(
                        "runtime executor unexpectedly closed".to_string(),
                    )));
                }
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct RuntimeInvocationDispatchHandle {
    admission: Arc<RuntimeExecutorAdmission>,
    tenant_label: String,
    active_semaphore: Arc<tokio::sync::Semaphore>,
}

impl RuntimeInvocationDispatchHandle {
    async fn acquire_active_permit(&self) -> Result<OwnedSemaphorePermit> {
        self.admission
            .acquire_active_permit(&self.tenant_label, self.active_semaphore.clone())
            .await
    }

    fn mark_active_entered(&self) {
        self.admission.mark_active_entered(&self.tenant_label);
    }

    fn mark_active_suspended(&self) {
        self.admission.mark_active_suspended(&self.tenant_label);
    }

    fn complete_invocation(&self, was_active: bool) -> Vec<RuntimeWorkerJob> {
        self.admission
            .complete_dispatched_job(&self.tenant_label, was_active)
    }

    fn rollback_dispatch(&self) {
        self.admission.rollback_dispatched_job(&self.tenant_label);
    }
}

#[derive(Clone)]
pub(crate) struct SharedInvocationPermit {
    inner: Rc<RefCell<SharedInvocationPermitState>>,
}

struct SharedInvocationPermitState {
    policy: Arc<RuntimePolicy>,
    tenant_label: Option<String>,
    dispatch_handle: Option<RuntimeInvocationDispatchHandle>,
    bypasses_concurrency_limit: bool,
    cancellation: Option<HostCallCancellation>,
    initial_queue_started_at: Option<Instant>,
    js_permit: Option<OwnedSemaphorePermit>,
    active_permit: Option<OwnedSemaphorePermit>,
    active_entered: bool,
    invocation_started: bool,
    in_flight_host_ops: usize,
    invocation_finished: bool,
    timeout_controller: Option<RuntimeInvocationTimeoutController>,
}

impl SharedInvocationPermit {
    pub(crate) fn new(
        policy: Arc<RuntimePolicy>,
        tenant_label: Option<String>,
        dispatch_handle: Option<RuntimeInvocationDispatchHandle>,
        bypasses_concurrency_limit: bool,
        cancellation: Option<HostCallCancellation>,
    ) -> Self {
        Self {
            inner: Rc::new(RefCell::new(SharedInvocationPermitState {
                policy,
                tenant_label,
                dispatch_handle,
                bypasses_concurrency_limit,
                cancellation,
                initial_queue_started_at: None,
                js_permit: None,
                active_permit: None,
                active_entered: false,
                invocation_started: false,
                in_flight_host_ops: 0,
                invocation_finished: false,
                timeout_controller: None,
            })),
        }
    }

    pub(crate) fn set_timeout_controller(&self, controller: RuntimeInvocationTimeoutController) {
        self.inner.borrow_mut().timeout_controller = Some(controller);
    }

    pub(crate) fn clear_timeout_controller(&self) {
        self.inner.borrow_mut().timeout_controller = None;
    }

    pub(crate) async fn acquire_initial(&mut self, queue_started_at: Instant) -> Result<()> {
        self.inner.borrow_mut().initial_queue_started_at = Some(queue_started_at);
        let (policy, tenant_label, dispatch_handle, cancellation, bypasses_concurrency_limit) = {
            let state = self.inner.borrow();
            (
                state.policy.clone(),
                state.tenant_label.clone(),
                state.dispatch_handle.clone(),
                state.cancellation.clone(),
                state.bypasses_concurrency_limit,
            )
        };

        if bypasses_concurrency_limit {
            policy
                .metrics()
                .record_invocation_started_for_tenant(tenant_label.as_deref());
            policy
                .metrics()
                .increment_active_isolates_for_tenant(tenant_label.as_deref());
            let mut state = self.inner.borrow_mut();
            state.active_entered = true;
            state.invocation_started = true;
            return Ok(());
        }

        policy.metrics().increment_queued_invocations();
        let active_permit = match dispatch_handle.clone() {
            Some(dispatch_handle) => {
                let permit = dispatch_handle.acquire_active_permit().await?;
                if cancellation
                    .as_ref()
                    .is_some_and(HostCallCancellation::is_cancelled)
                {
                    drop(permit);
                    policy.metrics().decrement_queued_invocations();
                    return Err(NeovexRuntimeError::Cancelled);
                }
                Some(permit)
            }
            None => None,
        };

        let js_permit = policy
            .isolate_semaphore()
            .acquire_owned()
            .await
            .map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime isolate semaphore unexpectedly closed".to_string(),
                )
            })?;
        policy.metrics().decrement_queued_invocations();

        if let Some(dispatch_handle) = &dispatch_handle {
            dispatch_handle.mark_active_entered();
        }
        policy
            .metrics()
            .record_queue_wait_for_tenant(tenant_label.as_deref(), queue_started_at.elapsed());
        policy
            .metrics()
            .record_invocation_started_for_tenant(tenant_label.as_deref());
        policy
            .metrics()
            .increment_active_isolates_for_tenant(tenant_label.as_deref());

        let mut state = self.inner.borrow_mut();
        state.active_permit = active_permit;
        state.js_permit = Some(js_permit);
        state.active_entered = true;
        state.invocation_started = true;
        Ok(())
    }

    pub(crate) fn begin_async_host_call(&self) {
        let (policy, tenant_label, dispatch_handle, dropped_js_permit, dropped_active_permit) = {
            let mut state = self.inner.borrow_mut();
            state.in_flight_host_ops += 1;
            if state.bypasses_concurrency_limit || state.in_flight_host_ops != 1 {
                return;
            }
            let policy = state.policy.clone();
            let tenant_label = state.tenant_label.clone();
            let dispatch_handle = state.dispatch_handle.clone();
            let js_permit = state.js_permit.take();
            let active_permit = state.active_permit.take();
            if state.active_entered {
                state.active_entered = false;
            }
            (
                policy,
                tenant_label,
                dispatch_handle,
                js_permit,
                active_permit,
            )
        };

        if let Some(dispatch_handle) = dispatch_handle {
            dispatch_handle.mark_active_suspended();
        }
        policy
            .metrics()
            .decrement_active_isolates_for_tenant(tenant_label.as_deref());
        drop(dropped_js_permit);
        drop(dropped_active_permit);
    }

    pub(crate) async fn complete_async_host_call(&self) -> Result<()> {
        let (policy, tenant_label, dispatch_handle, cancellation, timeout_controller) = {
            let mut state = self.inner.borrow_mut();
            state.in_flight_host_ops = state.in_flight_host_ops.saturating_sub(1);
            if state.bypasses_concurrency_limit
                || state.invocation_finished
                || state.in_flight_host_ops != 0
            {
                return Ok(());
            }
            (
                state.policy.clone(),
                state.tenant_label.clone(),
                state.dispatch_handle.clone(),
                state.cancellation.clone(),
                state.timeout_controller.clone(),
            )
        };

        if cancellation
            .as_ref()
            .is_some_and(HostCallCancellation::is_cancelled)
        {
            return Ok(());
        }

        if let Some(timeout_controller) = timeout_controller.clone() {
            timeout_controller.pause().await;
        }

        policy.metrics().increment_queued_invocations();
        let active_permit = match dispatch_handle.clone() {
            Some(dispatch_handle) => {
                let permit = dispatch_handle.acquire_active_permit().await?;
                if cancellation
                    .as_ref()
                    .is_some_and(HostCallCancellation::is_cancelled)
                {
                    drop(permit);
                    policy.metrics().decrement_queued_invocations();
                    return Ok(());
                }
                Some(permit)
            }
            None => None,
        };
        let js_permit = policy
            .isolate_semaphore()
            .acquire_owned()
            .await
            .map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime isolate semaphore unexpectedly closed".to_string(),
                )
            })?;
        policy.metrics().decrement_queued_invocations();

        if let Some(dispatch_handle) = &dispatch_handle {
            dispatch_handle.mark_active_entered();
        }
        policy
            .metrics()
            .increment_active_isolates_for_tenant(tenant_label.as_deref());

        {
            let mut state = self.inner.borrow_mut();
            state.active_permit = active_permit;
            state.js_permit = Some(js_permit);
            state.active_entered = true;
        }

        if let Some(timeout_controller) = timeout_controller {
            timeout_controller.resume()?;
        }

        Ok(())
    }

    pub(crate) fn drop_async_host_call(&self) {
        let mut state = self.inner.borrow_mut();
        state.in_flight_host_ops = state.in_flight_host_ops.saturating_sub(1);
    }

    pub(crate) async fn finish_invocation(&self) -> Vec<RuntimeWorkerJob> {
        let (
            policy,
            tenant_label,
            dispatch_handle,
            js_permit,
            active_permit,
            was_active,
            invocation_started,
        ) = {
            let mut state = self.inner.borrow_mut();
            if state.invocation_finished {
                return Vec::new();
            }
            state.invocation_finished = true;
            (
                state.policy.clone(),
                state.tenant_label.clone(),
                state.dispatch_handle.clone(),
                state.js_permit.take(),
                state.active_permit.take(),
                std::mem::take(&mut state.active_entered),
                state.invocation_started,
            )
        };

        drop(js_permit);
        drop(active_permit);

        let ready_jobs = match dispatch_handle {
            Some(dispatch_handle) => dispatch_handle.complete_invocation(was_active),
            None => Vec::new(),
        };
        if was_active {
            policy
                .metrics()
                .decrement_active_isolates_for_tenant(tenant_label.as_deref());
        }
        if invocation_started {
            policy
                .metrics()
                .record_invocation_completed_for_tenant(tenant_label.as_deref());
        }
        ready_jobs
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

struct RuntimeExecutorTenantAdmissionState {
    active_invocations: usize,
    parked_invocations: usize,
    active_semaphore: Arc<tokio::sync::Semaphore>,
    queued_jobs: VecDeque<RuntimeWorkerJob>,
    queued_in_rotation: bool,
}

impl RuntimeExecutorTenantAdmissionState {
    fn new(max_active: usize) -> Self {
        Self {
            active_invocations: 0,
            parked_invocations: 0,
            active_semaphore: Arc::new(tokio::sync::Semaphore::new(max_active.max(1))),
            queued_jobs: VecDeque::new(),
            queued_in_rotation: false,
        }
    }

    fn total_in_flight(&self) -> usize {
        self.active_invocations + self.parked_invocations
    }
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

    fn admit_job(
        self: &Arc<Self>,
        mut job: RuntimeWorkerJob,
    ) -> Result<RuntimeExecutorAdmissionDecision> {
        let Some(tenant_label) = Self::fairness_tenant_label(&job).map(str::to_owned) else {
            return Ok(RuntimeExecutorAdmissionDecision::Dispatch(Box::new(job)));
        };

        let limits = self.policy.limits();
        let max_active = limits.max_active_top_level_invocations_per_tenant;
        let max_in_flight = limits.max_in_flight_top_level_invocations_per_tenant;
        let max_queued = limits.max_queued_top_level_invocations_per_tenant;
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        let tenant_state = state
            .tenants
            .entry(tenant_label.clone())
            .or_insert_with(|| RuntimeExecutorTenantAdmissionState::new(max_active));
        if tenant_state.total_in_flight() < max_in_flight && tenant_state.queued_jobs.is_empty() {
            tenant_state.parked_invocations += 1;
            job.dispatch_handle = Some(RuntimeInvocationDispatchHandle {
                admission: self.clone(),
                tenant_label,
                active_semaphore: tenant_state.active_semaphore.clone(),
            });
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

    async fn acquire_active_permit(
        &self,
        tenant_label: &str,
        active_semaphore: Arc<tokio::sync::Semaphore>,
    ) -> Result<OwnedSemaphorePermit> {
        active_semaphore.acquire_owned().await.map_err(|_| {
            NeovexRuntimeError::Contract(format!(
                "runtime tenant active semaphore unexpectedly closed for tenant {tenant_label}"
            ))
        })
    }

    fn mark_active_entered(&self, tenant_label: &str) {
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        if let Some(tenant_state) = state.tenants.get_mut(tenant_label) {
            tenant_state.parked_invocations = tenant_state.parked_invocations.saturating_sub(1);
            tenant_state.active_invocations += 1;
        }
    }

    fn mark_active_suspended(&self, tenant_label: &str) {
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        if let Some(tenant_state) = state.tenants.get_mut(tenant_label) {
            tenant_state.active_invocations = tenant_state.active_invocations.saturating_sub(1);
            tenant_state.parked_invocations += 1;
        }
    }

    fn complete_dispatched_job(
        self: &Arc<Self>,
        tenant_label: &str,
        was_active: bool,
    ) -> Vec<RuntimeWorkerJob> {
        let max_in_flight = self
            .policy
            .limits()
            .max_in_flight_top_level_invocations_per_tenant;
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        if let Some(tenant_state) = state.tenants.get_mut(tenant_label) {
            if was_active {
                tenant_state.active_invocations = tenant_state.active_invocations.saturating_sub(1);
            } else {
                tenant_state.parked_invocations = tenant_state.parked_invocations.saturating_sub(1);
            }
        }
        Self::cleanup_tenant_locked(&mut state, tenant_label);
        self.promote_ready_jobs_locked(&mut state, max_in_flight)
    }

    fn rollback_dispatched_job(self: &Arc<Self>, tenant_label: &str) {
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        if let Some(tenant_state) = state.tenants.get_mut(tenant_label) {
            tenant_state.parked_invocations = tenant_state.parked_invocations.saturating_sub(1);
        }
        Self::cleanup_tenant_locked(&mut state, tenant_label);
    }

    fn fairness_tenant_label(job: &RuntimeWorkerJob) -> Option<&str> {
        (!job.runtime.bypasses_concurrency_limit())
            .then_some(job.context.tenant_label.as_deref())
            .flatten()
    }

    fn promote_ready_jobs_locked(
        self: &Arc<Self>,
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
                        remove_tenant = tenant_state.total_in_flight() == 0;
                    } else if tenant_state.total_in_flight() < max_in_flight {
                        promoted_job = tenant_state.queued_jobs.pop_front().map(|mut job| {
                            tenant_state.parked_invocations += 1;
                            job.dispatch_handle = Some(RuntimeInvocationDispatchHandle {
                                admission: self.clone(),
                                tenant_label: tenant_label.clone(),
                                active_semaphore: tenant_state.active_semaphore.clone(),
                            });
                            job
                        });
                        if tenant_state.queued_jobs.is_empty() {
                            tenant_state.queued_in_rotation = false;
                            remove_tenant = tenant_state.total_in_flight() == 0;
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
            tenant_state.total_in_flight() == 0
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
        let worker_count = policy.limits().worker_threads.max(1);
        let queue_capacity = worker_count.saturating_mul(4).max(1);
        let (worker_sender, receiver) = mpsc::channel::<RuntimeWorkerJob>(queue_capacity);
        let sender = Arc::new(Mutex::new(Some(worker_sender)));
        let receiver = Arc::new(Mutex::new(receiver));
        let queue = Arc::new(RuntimeWorkerQueueController::new(receiver, sender));
        let admission = Arc::new(RuntimeExecutorAdmission::new(policy.clone()));
        let shutdown = RuntimeWorkerShutdown::new();
        let watchdog = WatchdogTimer::new();
        #[cfg(test)]
        let test_state = Arc::new(RuntimeExecutorTestState::new());
        let worker_loop_factory: Arc<dyn WorkerLoopFactory> = {
            let factory = RunToCompletionWorkerLoopFactory::new(watchdog.clone());
            #[cfg(test)]
            let factory = factory.with_test_state(test_state.clone());
            Arc::new(factory)
        };
        let mut worker_handles = Vec::with_capacity(worker_count);

        for worker_id in 0..worker_count {
            let queue: Arc<dyn RuntimeWorkerQueue> = queue.clone();
            let policy = policy.clone();
            let shutdown = shutdown.clone();
            let worker_loop_factory = worker_loop_factory.clone();
            let handle = std::thread::Builder::new()
                .name(format!("neovex-runtime-worker-{worker_id}"))
                .spawn(move || {
                    let mut worker_loop = worker_loop_factory.create(worker_id, policy);
                    worker_loop.run(queue, shutdown);
                })
                .expect("runtime executor worker thread should start");
            worker_handles.push(handle);
        }

        Self {
            inner: Arc::new(RuntimeExecutorInner {
                policy,
                queue,
                admission,
                shutdown,
                watchdog,
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
        self.inner.queue.dispatch_job(job).await
    }

    fn dispatch_admitted_job_blocking(&self, job: RuntimeWorkerJob) -> Result<()> {
        self.inner.queue.dispatch_job_blocking(job)
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
        let metrics = policy.metrics();
        let mut permit = SharedInvocationPermit::new(
            policy.clone(),
            context.tenant_label.clone(),
            None,
            runtime.bypasses_concurrency_limit(),
            cancellation.clone(),
        );
        let execution_started_at = Instant::now();
        let cancellation_for_metrics = cancellation.clone();
        let result = async {
            permit.acquire_initial(queue_started_at).await?;
            debug!(
                invocation_id = context.invocation_id,
                request_id = ?context.server_request_id,
                tenant = context.tenant_label.as_deref().unwrap_or("unknown"),
                function = %context.function_name,
                kind = context.kind,
                queued_invocations = metrics.snapshot().queued_invocations,
                "runtime invocation admitted"
            );
            runtime
                .invoke_bundle_unmanaged(
                    None,
                    RuntimeInvocationExecution {
                        watchdog: watchdog.clone(),
                        bundle: bundle.clone(),
                        request: request.clone(),
                        context: context.clone(),
                        external_cancellation: cancellation,
                        permit: permit.clone(),
                    },
                )
                .await
        }
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
        });
        let _ = permit.finish_invocation().await;
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

impl Drop for RuntimeExecutorInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
        self.queue.close();
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
        self.watchdog.shutdown();
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

    #[derive(Default)]
    struct ControlledAsyncGetHost {
        started_ids: StdMutex<Vec<String>>,
        started_notify: Arc<Notify>,
        release_slow: Arc<Notify>,
    }

    impl ControlledAsyncGetHost {
        fn started_ids(&self) -> Vec<String> {
            self.started_ids
                .lock()
                .expect("controlled async host lock should not be poisoned")
                .clone()
        }

        async fn wait_until_started(&self, document_id: &str) {
            tokio::time::timeout(Duration::from_secs(1), async {
                loop {
                    let notified = self.started_notify.notified();
                    if self
                        .started_ids()
                        .iter()
                        .any(|started| started == document_id)
                    {
                        return;
                    }
                    notified.await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("host request {document_id} should start"));
        }

        fn release_slow_jobs(&self) {
            self.release_slow.notify_waiters();
        }
    }

    impl HostBridge for ControlledAsyncGetHost {
        fn call(&self, _request: HostCallRequest) -> Result<Value> {
            Err(NeovexRuntimeError::Contract(
                "controlled async host expects async db.get path".to_string(),
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
                .expect("controlled async host lock should not be poisoned")
                .push(document_id.clone());
            self.started_notify.notify_waiters();
            let release_slow = self.release_slow.clone();
            Box::pin(async move {
                if document_id.starts_with("slow-") {
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

    struct SlowSyncQueryHost {
        delay: Duration,
        started: Arc<Notify>,
    }

    impl SlowSyncQueryHost {
        fn new(delay: Duration) -> Self {
            Self {
                delay,
                started: Arc::new(Notify::new()),
            }
        }

        async fn wait_until_started(&self) {
            tokio::time::timeout(Duration::from_secs(1), self.started.notified())
                .await
                .expect("slow sync query host should start");
        }
    }

    impl HostBridge for SlowSyncQueryHost {
        fn call(&self, request: HostCallRequest) -> Result<Value> {
            assert_eq!(request.operation, "convex.ctx.db.query.start");
            self.started.notify_waiters();
            std::thread::sleep(self.delay);
            Ok(json!({
                "status": "ok",
                "value": "builder-1",
            }))
        }

        fn call_async(
            &self,
            _request: HostCallRequest,
            _cancellation: HostCallCancellation,
        ) -> HostBridgeFuture {
            Box::pin(async move {
                Err(NeovexRuntimeError::Contract(
                    "async host bridge path should not be used for sync query builder setup"
                        .to_string(),
                ))
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

    fn write_sync_query_builder_bundle() -> (tempfile::TempDir, std::path::PathBuf) {
        let bundle_dir = tempdir().expect("tempdir should build");
        let bundle_path = bundle_dir.path().join("bundle.mjs");
        std::fs::write(
            &bundle_path,
            r#"
globalThis.__neovexInvoke = async function () {
  const ctx = globalThis.__neovexCreateContext();
  const builder = ctx.db.query("messages");
  return { builderId: builder.__builderId };
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
            worker_threads: 1,
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
                        worker_threads: 1,
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
            worker_threads: 1,
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
            worker_threads: 1,
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
    async fn permit_suspend_frees_capacity() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 2,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(ControlledAsyncGetHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);

        let slow_request = test_request("slow-1");
        let slow_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&slow_request, "tenant-a", "req-permit-slow");
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
        host.wait_until_started("slow-1").await;

        let fast_request = test_request("fast-1");
        let fast_result = tokio::time::timeout(
            Duration::from_secs(1),
            executor.invoke_on_worker(
                NeovexRuntime::with_policy(host.clone(), policy.clone()),
                bundle.clone(),
                fast_request.clone(),
                test_context_for_tenant(&fast_request, "tenant-b", "req-permit-fast"),
                None,
            ),
        )
        .await
        .expect("fast invocation should use the freed permit")
        .expect("fast invocation should succeed");

        assert_eq!(fast_result, json!({ "id": "fast-1" }));
        assert!(
            !slow_task.is_finished(),
            "slow invocation should still be parked while the second worker uses the freed permit"
        );

        host.release_slow_jobs();
        assert_eq!(
            slow_task
                .await
                .expect("slow task should join")
                .expect("slow invocation should succeed after resume"),
            json!({ "id": "slow-1" })
        );
    }

    #[tokio::test]
    async fn parked_invocation_resumes_after_host_completion() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(ControlledAsyncGetHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);
        let request = test_request("slow-1");
        let parked_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&request, "tenant-a", "req-parked-resume");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        request,
                        context,
                        None,
                    )
                    .await
            }
        });

        host.wait_until_started("slow-1").await;
        assert!(
            !parked_task.is_finished(),
            "parked invocation should remain pending until host work completes"
        );

        host.release_slow_jobs();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), parked_task)
                .await
                .expect("parked invocation should resume after host completion")
                .expect("parked task should join")
                .expect("parked invocation should succeed"),
            json!({ "id": "slow-1" })
        );
    }

    #[tokio::test]
    async fn parked_invocation_counts_toward_in_flight_limit() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            worker_threads: 2,
            max_active_top_level_invocations_per_tenant: 1,
            max_in_flight_top_level_invocations_per_tenant: 2,
            max_queued_top_level_invocations_per_tenant: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let host = Arc::new(ControlledAsyncGetHost::default());
        let bundle = RuntimeBundle::new(&bundle_path);

        let first_request = test_request("slow-1");
        let first_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&first_request, "tenant-a", "req-inflight-1");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        first_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        host.wait_until_started("slow-1").await;

        let second_request = test_request("slow-2");
        let second_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&second_request, "tenant-a", "req-inflight-2");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        second_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        host.wait_until_started("slow-2").await;

        let third_request = test_request("fast-1");
        let third_task = tokio::spawn({
            let executor = executor.clone();
            let bundle = bundle.clone();
            let host = host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&third_request, "tenant-a", "req-inflight-3");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(host, policy),
                        bundle,
                        third_request,
                        context,
                        None,
                    )
                    .await
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !host.started_ids().iter().any(|id| id == "fast-1"),
            "third invocation should remain queued while two parked invocations consume the tenant in-flight limit"
        );

        host.release_slow_jobs();
        assert_eq!(
            first_task
                .await
                .expect("first slow task should join")
                .expect("first slow invocation should succeed"),
            json!({ "id": "slow-1" })
        );
        assert_eq!(
            second_task
                .await
                .expect("second slow task should join")
                .expect("second slow invocation should succeed"),
            json!({ "id": "slow-2" })
        );
        assert_eq!(
            third_task
                .await
                .expect("third task should join")
                .expect("third invocation should succeed after queue promotion"),
            json!({ "id": "fast-1" })
        );
    }

    #[tokio::test]
    async fn timeout_excludes_permit_reacquire_wait() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_async_bundle_dir, async_bundle_path) = write_function_named_get_bundle();
        let (_sync_bundle_dir, sync_bundle_path) = write_sync_query_builder_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            execution_timeout: Duration::from_millis(120),
            max_concurrent_isolates: 1,
            worker_threads: 2,
            max_active_top_level_invocations_per_tenant: 1,
            max_in_flight_top_level_invocations_per_tenant: 1,
            ..RuntimeLimits::default()
        }));
        let executor = RuntimeExecutor::new(policy.clone());
        let parked_host = Arc::new(ControlledAsyncGetHost::default());
        let blocker_host = Arc::new(SlowSyncQueryHost::new(Duration::from_millis(80)));
        let async_bundle = RuntimeBundle::new(&async_bundle_path);
        let sync_bundle = RuntimeBundle::new(&sync_bundle_path);

        let slow_request = test_request("slow-1");
        let slow_started_at = std::time::Instant::now();
        let parked_task = tokio::spawn({
            let executor = executor.clone();
            let async_bundle = async_bundle.clone();
            let parked_host = parked_host.clone();
            let policy = policy.clone();
            let context = test_context_for_tenant(&slow_request, "tenant-a", "req-timeout-parked");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(parked_host, policy),
                        async_bundle,
                        slow_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        parked_host.wait_until_started("slow-1").await;
        tokio::time::sleep(Duration::from_millis(80)).await;

        let blocker_request = test_request("messages:list");
        let blocker_task = tokio::spawn({
            let executor = executor.clone();
            let sync_bundle = sync_bundle.clone();
            let blocker_host = blocker_host.clone();
            let policy = policy.clone();
            let context =
                test_context_for_tenant(&blocker_request, "tenant-b", "req-timeout-blocker");
            async move {
                executor
                    .invoke_on_worker(
                        NeovexRuntime::with_policy(blocker_host, policy),
                        sync_bundle,
                        blocker_request,
                        context,
                        None,
                    )
                    .await
            }
        });
        blocker_host.wait_until_started().await;
        parked_host.release_slow_jobs();

        assert_eq!(
            blocker_task
                .await
                .expect("blocker task should join")
                .expect("blocker invocation should succeed"),
            json!({ "builderId": "builder-1" })
        );
        assert_eq!(
            parked_task
                .await
                .expect("parked task should join")
                .expect("parked invocation should succeed after waiting to re-acquire the permit"),
            json!({ "id": "slow-1" })
        );
        assert!(
            slow_started_at.elapsed() >= Duration::from_millis(140),
            "parked invocation wall time should exceed the execution timeout while still succeeding because permit re-acquire wait is paused"
        );
    }

    #[tokio::test]
    async fn tenant_queue_limit_rejections_record_metrics() {
        let _test_lock = runtime_executor_test_lock().lock().await;
        let (_bundle_dir, bundle_path) = write_function_named_get_bundle();
        let policy = Arc::new(RuntimePolicy::new(RuntimeLimits {
            max_concurrent_isolates: 1,
            max_active_top_level_invocations_per_tenant: 1,
            max_in_flight_top_level_invocations_per_tenant: 1,
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
            max_active_top_level_invocations_per_tenant: 1,
            max_in_flight_top_level_invocations_per_tenant: 1,
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
