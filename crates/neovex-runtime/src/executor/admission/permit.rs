use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::OwnedSemaphorePermit;

use crate::error::{NeovexRuntimeError, Result};
use crate::host::HostCallCancellation;
use crate::limits::RuntimePolicy;
use crate::runtime::RuntimeInvocationTimeoutController;

use super::super::queue::RuntimeWorkerJob;
use super::dispatch::RuntimeInvocationDispatchHandle;

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
    runtime_permit: Option<OwnedSemaphorePermit>,
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
                runtime_permit: None,
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
                .increment_active_runtime_instances_for_tenant(tenant_label.as_deref());
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

        let runtime_permit = policy
            .runtime_instance_semaphore()
            .acquire_owned()
            .await
            .map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime instance semaphore unexpectedly closed".to_string(),
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
            .increment_active_runtime_instances_for_tenant(tenant_label.as_deref());

        let mut state = self.inner.borrow_mut();
        state.active_permit = active_permit;
        state.runtime_permit = Some(runtime_permit);
        state.active_entered = true;
        state.invocation_started = true;
        Ok(())
    }

    pub(crate) fn begin_async_host_call(&self) {
        let (policy, tenant_label, dispatch_handle, dropped_runtime_permit, dropped_active_permit) = {
            let mut state = self.inner.borrow_mut();
            state.in_flight_host_ops += 1;
            if state.bypasses_concurrency_limit || state.in_flight_host_ops != 1 {
                return;
            }
            let policy = state.policy.clone();
            let tenant_label = state.tenant_label.clone();
            let dispatch_handle = state.dispatch_handle.clone();
            let runtime_permit = state.runtime_permit.take();
            let active_permit = state.active_permit.take();
            if state.active_entered {
                state.active_entered = false;
            }
            (
                policy,
                tenant_label,
                dispatch_handle,
                runtime_permit,
                active_permit,
            )
        };

        if let Some(dispatch_handle) = dispatch_handle {
            dispatch_handle.mark_active_suspended();
        }
        policy
            .metrics()
            .decrement_active_runtime_instances_for_tenant(tenant_label.as_deref());
        drop(dropped_runtime_permit);
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
        let runtime_permit = policy
            .runtime_instance_semaphore()
            .acquire_owned()
            .await
            .map_err(|_| {
                NeovexRuntimeError::Contract(
                    "runtime instance semaphore unexpectedly closed".to_string(),
                )
            })?;
        policy.metrics().decrement_queued_invocations();

        if let Some(dispatch_handle) = &dispatch_handle {
            dispatch_handle.mark_active_entered();
        }
        policy
            .metrics()
            .increment_active_runtime_instances_for_tenant(tenant_label.as_deref());

        {
            let mut state = self.inner.borrow_mut();
            state.active_permit = active_permit;
            state.runtime_permit = Some(runtime_permit);
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
            runtime_permit,
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
                state.runtime_permit.take(),
                state.active_permit.take(),
                std::mem::take(&mut state.active_entered),
                state.invocation_started,
            )
        };

        drop(runtime_permit);
        drop(active_permit);

        let ready_jobs = match dispatch_handle {
            Some(dispatch_handle) => dispatch_handle.complete_invocation(was_active),
            None => Vec::new(),
        };
        if was_active {
            policy
                .metrics()
                .decrement_active_runtime_instances_for_tenant(tenant_label.as_deref());
        }
        if invocation_started {
            policy
                .metrics()
                .record_invocation_completed_for_tenant(tenant_label.as_deref());
        }
        ready_jobs
    }
}
