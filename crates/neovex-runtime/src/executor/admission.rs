use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::OwnedSemaphorePermit;

use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::RuntimeInvocationTimeoutController;

use super::queue::RuntimeWorkerJob;

#[derive(Clone)]
pub(crate) struct RuntimeInvocationDispatchHandle {
    admission: Arc<RuntimeExecutorAdmission>,
    tenant_label: String,
    active_semaphore: Arc<tokio::sync::Semaphore>,
}

impl RuntimeInvocationDispatchHandle {
    pub(crate) async fn acquire_active_permit(&self) -> Result<OwnedSemaphorePermit> {
        self.admission
            .acquire_active_permit(&self.tenant_label, self.active_semaphore.clone())
            .await
    }

    pub(crate) fn mark_active_entered(&self) {
        self.admission.mark_active_entered(&self.tenant_label);
    }

    pub(crate) fn mark_active_suspended(&self) {
        self.admission.mark_active_suspended(&self.tenant_label);
    }

    pub(crate) fn complete_invocation(&self, was_active: bool) -> Vec<RuntimeWorkerJob> {
        self.admission
            .complete_dispatched_job(&self.tenant_label, was_active)
    }

    pub(crate) fn rollback_dispatch(&self) {
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

pub(super) struct RuntimeExecutorAdmission {
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

pub(super) enum RuntimeExecutorAdmissionDecision {
    Dispatch(Box<RuntimeWorkerJob>),
    Queued,
}

impl RuntimeExecutorAdmission {
    pub(super) fn new(policy: Arc<RuntimePolicy>) -> Self {
        Self {
            policy,
            state: Mutex::new(RuntimeExecutorAdmissionState::default()),
        }
    }

    pub(super) fn admit_job(
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

    pub(super) fn drain_queued_jobs(&self) -> Vec<RuntimeWorkerJob> {
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
