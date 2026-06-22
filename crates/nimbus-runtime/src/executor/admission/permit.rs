use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use tokio::sync::{OwnedSemaphorePermit, TryAcquireError};

use crate::error::{NimbusRuntimeError, Result};
use crate::host::{HostCallCancellation, HostCallOperation};
use crate::limits::RuntimePolicy;
use crate::runtime::RuntimeInvocationTimeoutController;

use super::super::queue::RuntimeWorkerJob;
use super::dispatch::RuntimeInvocationDispatchHandle;

pub(crate) enum SharedInvocationPermitAcquire {
    Acquired,
    WouldBlock,
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
    runtime_permit: Option<OwnedSemaphorePermit>,
    active_permit: Option<OwnedSemaphorePermit>,
    active_entered: bool,
    invocation_started: bool,
    in_flight_host_ops: usize,
    invocation_finished: bool,
    timeout_controller: Option<RuntimeInvocationTimeoutController>,
}

struct QueuedInvocationMetricGuard {
    policy: Arc<RuntimePolicy>,
    active: bool,
}

impl QueuedInvocationMetricGuard {
    fn enter(policy: Arc<RuntimePolicy>) -> Self {
        policy.metrics().increment_queued_invocations();
        Self {
            policy,
            active: true,
        }
    }

    fn finish(mut self) {
        self.decrement_once();
    }

    fn decrement_once(&mut self) {
        if self.active {
            self.policy.metrics().decrement_queued_invocations();
            self.active = false;
        }
    }
}

impl Drop for QueuedInvocationMetricGuard {
    fn drop(&mut self) {
        self.decrement_once();
    }
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
                .record_profile_invocation_started(policy.runtime_profile());
            policy
                .metrics()
                .increment_active_runtime_instances_for_tenant(tenant_label.as_deref());
            let mut state = self.inner.borrow_mut();
            state.active_entered = true;
            state.invocation_started = true;
            return Ok(());
        }

        let queued_metric = QueuedInvocationMetricGuard::enter(policy.clone());
        let active_permit = match dispatch_handle.clone() {
            Some(dispatch_handle) => {
                let permit = dispatch_handle.acquire_active_permit().await?;
                if cancellation
                    .as_ref()
                    .is_some_and(HostCallCancellation::is_cancelled)
                {
                    drop(permit);
                    return Err(NimbusRuntimeError::Cancelled);
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
                NimbusRuntimeError::Contract(
                    "runtime instance semaphore unexpectedly closed".to_string(),
                )
            })?;
        queued_metric.finish();

        if let Some(dispatch_handle) = &dispatch_handle {
            dispatch_handle.mark_active_entered();
        }
        let queue_wait = queue_started_at.elapsed();
        policy
            .metrics()
            .record_queue_wait_for_tenant(tenant_label.as_deref(), queue_wait);
        policy
            .metrics()
            .record_profile_queue_wait(policy.runtime_profile(), queue_wait);
        policy
            .metrics()
            .record_invocation_started_for_tenant(tenant_label.as_deref());
        policy
            .metrics()
            .record_profile_invocation_started(policy.runtime_profile());
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

    pub(crate) fn try_acquire_initial(
        &mut self,
        queue_started_at: Instant,
    ) -> Result<SharedInvocationPermitAcquire> {
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
                .record_profile_invocation_started(policy.runtime_profile());
            policy
                .metrics()
                .increment_active_runtime_instances_for_tenant(tenant_label.as_deref());
            let mut state = self.inner.borrow_mut();
            state.active_entered = true;
            state.invocation_started = true;
            return Ok(SharedInvocationPermitAcquire::Acquired);
        }

        let active_permit = match dispatch_handle.clone() {
            Some(dispatch_handle) => {
                let permit = match dispatch_handle.try_acquire_active_permit() {
                    Ok(permit) => permit,
                    Err(TryAcquireError::NoPermits) => {
                        return Ok(SharedInvocationPermitAcquire::WouldBlock);
                    }
                    Err(TryAcquireError::Closed) => {
                        return Err(NimbusRuntimeError::Contract(format!(
                            "runtime tenant active semaphore unexpectedly closed for tenant {}",
                            tenant_label.as_deref().unwrap_or("unknown")
                        )));
                    }
                };
                if cancellation
                    .as_ref()
                    .is_some_and(HostCallCancellation::is_cancelled)
                {
                    drop(permit);
                    return Err(NimbusRuntimeError::Cancelled);
                }
                Some(permit)
            }
            None => None,
        };

        let runtime_permit = match policy.runtime_instance_semaphore().try_acquire_owned() {
            Ok(permit) => permit,
            Err(TryAcquireError::NoPermits) => {
                drop(active_permit);
                return Ok(SharedInvocationPermitAcquire::WouldBlock);
            }
            Err(TryAcquireError::Closed) => {
                drop(active_permit);
                return Err(NimbusRuntimeError::Contract(
                    "runtime instance semaphore unexpectedly closed".to_string(),
                ));
            }
        };

        if let Some(dispatch_handle) = &dispatch_handle {
            dispatch_handle.mark_active_entered();
        }
        let queue_wait = queue_started_at.elapsed();
        policy
            .metrics()
            .record_queue_wait_for_tenant(tenant_label.as_deref(), queue_wait);
        policy
            .metrics()
            .record_profile_queue_wait(policy.runtime_profile(), queue_wait);
        policy
            .metrics()
            .record_invocation_started_for_tenant(tenant_label.as_deref());
        policy
            .metrics()
            .record_profile_invocation_started(policy.runtime_profile());
        policy
            .metrics()
            .increment_active_runtime_instances_for_tenant(tenant_label.as_deref());

        let mut state = self.inner.borrow_mut();
        state.active_permit = active_permit;
        state.runtime_permit = Some(runtime_permit);
        state.active_entered = true;
        state.invocation_started = true;
        Ok(SharedInvocationPermitAcquire::Acquired)
    }

    pub(crate) async fn begin_async_host_call(&self) {
        let (timeout_controller, active_suspension) = {
            let mut state = self.inner.borrow_mut();
            state.in_flight_host_ops += 1;
            if state.in_flight_host_ops != 1 {
                return;
            }
            (
                state.timeout_controller.clone(),
                if state.bypasses_concurrency_limit {
                    None
                } else {
                    let policy = state.policy.clone();
                    let tenant_label = state.tenant_label.clone();
                    let dispatch_handle = state.dispatch_handle.clone();
                    let runtime_permit = state.runtime_permit.take();
                    let active_permit = state.active_permit.take();
                    if state.active_entered {
                        state.active_entered = false;
                    }
                    Some((
                        policy,
                        tenant_label,
                        dispatch_handle,
                        runtime_permit,
                        active_permit,
                    ))
                },
            )
        };

        if let Some(timeout_controller) = timeout_controller {
            timeout_controller.pause().await;
        }

        if let Some((
            policy,
            tenant_label,
            dispatch_handle,
            dropped_runtime_permit,
            dropped_active_permit,
        )) = active_suspension
        {
            if let Some(dispatch_handle) = dispatch_handle {
                dispatch_handle.mark_active_suspended();
            }
            policy
                .metrics()
                .decrement_active_runtime_instances_for_tenant(tenant_label.as_deref());
            drop(dropped_runtime_permit);
            drop(dropped_active_permit);
        }
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

        let queued_metric = QueuedInvocationMetricGuard::enter(policy.clone());
        let active_permit = match dispatch_handle.clone() {
            Some(dispatch_handle) => {
                let permit = dispatch_handle.acquire_active_permit().await?;
                if cancellation
                    .as_ref()
                    .is_some_and(HostCallCancellation::is_cancelled)
                {
                    drop(permit);
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
                NimbusRuntimeError::Contract(
                    "runtime instance semaphore unexpectedly closed".to_string(),
                )
            })?;
        queued_metric.finish();

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

    pub(crate) fn record_host_operation_started(&self, operation: HostCallOperation) {
        let policy = self.inner.borrow().policy.clone();
        policy
            .metrics()
            .record_host_operation_started(operation.as_str());
    }

    pub(crate) fn record_host_operation_succeeded(&self, operation: HostCallOperation) {
        let policy = self.inner.borrow().policy.clone();
        policy
            .metrics()
            .record_host_operation_succeeded(operation.as_str());
    }

    pub(crate) fn record_host_operation_failed(&self, operation: HostCallOperation) {
        let policy = self.inner.borrow().policy.clone();
        policy
            .metrics()
            .record_host_operation_failed(operation.as_str());
    }

    pub(crate) fn record_host_operation_canceled_in_flight(&self, operation: HostCallOperation) {
        let policy = self.inner.borrow().policy.clone();
        policy
            .metrics()
            .record_host_operation_canceled_in_flight(operation.as_str());
    }

    pub(crate) fn record_host_bridge_call(&self, operation: HostCallOperation, duration: Duration) {
        let policy = self.inner.borrow().policy.clone();
        policy.metrics().record_host_bridge_call(duration);
        policy
            .metrics()
            .record_host_operation_duration(operation.as_str(), duration);
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
            policy
                .metrics()
                .record_profile_invocation_completed(policy.runtime_profile());
        }
        ready_jobs
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Instant;

    use tokio::sync::Semaphore;

    use super::super::RuntimeExecutorAdmission;
    use super::super::dispatch::RuntimeInvocationDispatchHandle;
    use super::*;
    use crate::limits::{RuntimeLimits, RuntimePolicy};

    fn test_policy() -> Arc<RuntimePolicy> {
        Arc::new(RuntimePolicy::new(RuntimeLimits::application_node22()))
    }

    fn closed_dispatch_handle(policy: Arc<RuntimePolicy>) -> RuntimeInvocationDispatchHandle {
        let active_semaphore = Arc::new(Semaphore::new(0));
        active_semaphore.close();
        RuntimeInvocationDispatchHandle {
            admission: Arc::new(RuntimeExecutorAdmission::new(policy)),
            tenant_label: "tenant-a".to_string(),
            active_semaphore,
        }
    }

    fn open_dispatch_handle(
        policy: Arc<RuntimePolicy>,
    ) -> (RuntimeInvocationDispatchHandle, Arc<Semaphore>) {
        let active_semaphore = Arc::new(Semaphore::new(1));
        (
            RuntimeInvocationDispatchHandle {
                admission: Arc::new(RuntimeExecutorAdmission::new(policy)),
                tenant_label: "tenant-a".to_string(),
                active_semaphore: active_semaphore.clone(),
            },
            active_semaphore,
        )
    }

    fn assert_no_queued_invocations(policy: &RuntimePolicy) {
        assert_eq!(
            policy.metrics_snapshot().queued_invocations,
            0,
            "queued invocation metric should not leak on acquire error"
        );
    }

    fn single_runtime_policy() -> Arc<RuntimePolicy> {
        let mut limits = RuntimeLimits::application_node22();
        limits.max_concurrent_runtime_instances = 1;
        limits.worker_threads = 1;
        Arc::new(RuntimePolicy::new(limits))
    }

    #[tokio::test]
    async fn try_acquire_initial_would_block_without_starting_invocation() {
        let policy = single_runtime_policy();
        let mut occupying_permit =
            SharedInvocationPermit::new(policy.clone(), None, None, false, None);
        assert!(matches!(
            occupying_permit
                .try_acquire_initial(Instant::now())
                .expect("first try acquire should succeed"),
            SharedInvocationPermitAcquire::Acquired
        ));

        let mut blocked_permit =
            SharedInvocationPermit::new(policy.clone(), None, None, false, None);
        assert!(matches!(
            blocked_permit
                .try_acquire_initial(Instant::now())
                .expect("full runtime semaphore should not error"),
            SharedInvocationPermitAcquire::WouldBlock
        ));

        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.active_runtime_instances, 1);
        assert_eq!(snapshot.started_invocations, 1);
        assert_eq!(snapshot.completed_invocations, 0);
        assert_eq!(snapshot.queued_invocations, 0);
        assert_eq!(snapshot.profiles.node_full.started_invocations, 1);
        assert_eq!(snapshot.profiles.node_full.completed_invocations, 0);
        assert_eq!(
            snapshot.profiles.node_full.queue_wait_nanos_total,
            snapshot.queue_wait_nanos_total
        );

        assert!(blocked_permit.finish_invocation().await.is_empty());
        assert!(occupying_permit.finish_invocation().await.is_empty());
        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.active_runtime_instances, 0);
        assert_eq!(snapshot.started_invocations, 1);
        assert_eq!(snapshot.completed_invocations, 1);
        assert_eq!(snapshot.profiles.node_full.completed_invocations, 1);
    }

    #[tokio::test]
    async fn try_acquire_initial_would_block_on_tenant_active_limit_without_metrics_leak() {
        let policy = single_runtime_policy();
        let dispatch_handle = RuntimeInvocationDispatchHandle {
            admission: Arc::new(RuntimeExecutorAdmission::new(policy.clone())),
            tenant_label: "tenant-a".to_string(),
            active_semaphore: Arc::new(Semaphore::new(0)),
        };
        let mut permit = SharedInvocationPermit::new(
            policy.clone(),
            Some("tenant-a".to_string()),
            Some(dispatch_handle),
            false,
            None,
        );

        assert!(matches!(
            permit
                .try_acquire_initial(Instant::now())
                .expect("full tenant active semaphore should not error"),
            SharedInvocationPermitAcquire::WouldBlock
        ));

        let snapshot = policy.metrics_snapshot();
        assert_eq!(snapshot.active_runtime_instances, 0);
        assert_eq!(snapshot.started_invocations, 0);
        assert_eq!(snapshot.completed_invocations, 0);
        assert_eq!(snapshot.queued_invocations, 0);
        assert!(permit.finish_invocation().await.is_empty());
        assert_eq!(policy.metrics_snapshot().completed_invocations, 0);
    }

    #[tokio::test]
    async fn acquire_initial_decrements_queue_when_active_permit_is_closed() {
        let policy = test_policy();
        let dispatch_handle = closed_dispatch_handle(policy.clone());
        let mut permit = SharedInvocationPermit::new(
            policy.clone(),
            Some("tenant-a".to_string()),
            Some(dispatch_handle),
            false,
            None,
        );

        let error = permit
            .acquire_initial(Instant::now())
            .await
            .expect_err("closed active semaphore should reject initial acquire");

        assert!(
            error
                .to_string()
                .contains("runtime tenant active semaphore unexpectedly closed"),
            "unexpected error: {error}"
        );
        assert_no_queued_invocations(&policy);
    }

    #[tokio::test]
    async fn acquire_initial_decrements_queue_when_runtime_semaphore_is_closed() {
        let policy = test_policy();
        policy.runtime_instance_semaphore().close();
        let mut permit = SharedInvocationPermit::new(policy.clone(), None, None, false, None);

        let error = permit
            .acquire_initial(Instant::now())
            .await
            .expect_err("closed runtime semaphore should reject initial acquire");

        assert!(
            error
                .to_string()
                .contains("runtime instance semaphore unexpectedly closed"),
            "unexpected error: {error}"
        );
        assert_no_queued_invocations(&policy);
    }

    #[tokio::test]
    async fn complete_async_host_call_decrements_queue_when_active_permit_is_closed() {
        let policy = test_policy();
        let (dispatch_handle, active_semaphore) = open_dispatch_handle(policy.clone());
        let permit = SharedInvocationPermit::new(
            policy.clone(),
            Some("tenant-a".to_string()),
            Some(dispatch_handle),
            false,
            None,
        );
        let mut acquired = permit.clone();
        acquired
            .acquire_initial(Instant::now())
            .await
            .expect("initial acquire should succeed");
        permit.begin_async_host_call().await;
        active_semaphore.close();

        let error = permit
            .complete_async_host_call()
            .await
            .expect_err("closed active semaphore should reject async-host resume");

        assert!(
            error
                .to_string()
                .contains("runtime tenant active semaphore unexpectedly closed"),
            "unexpected error: {error}"
        );
        assert_no_queued_invocations(&policy);
    }

    #[tokio::test]
    async fn complete_async_host_call_decrements_queue_when_runtime_semaphore_is_closed() {
        let policy = test_policy();
        let permit = SharedInvocationPermit::new(policy.clone(), None, None, false, None);
        let mut acquired = permit.clone();
        acquired
            .acquire_initial(Instant::now())
            .await
            .expect("initial acquire should succeed");
        permit.begin_async_host_call().await;
        policy.runtime_instance_semaphore().close();

        let error = permit
            .complete_async_host_call()
            .await
            .expect_err("closed runtime semaphore should reject async-host resume");

        assert!(
            error
                .to_string()
                .contains("runtime instance semaphore unexpectedly closed"),
            "unexpected error: {error}"
        );
        assert_no_queued_invocations(&policy);
    }
}
