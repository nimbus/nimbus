mod dispatch;
mod permit;
mod tenant_fairness;

use std::sync::{Arc, Mutex};

use tokio::sync::OwnedSemaphorePermit;

use crate::error::{NeovexRuntimeError, Result};
use crate::limits::RuntimePolicy;

use super::queue::RuntimeWorkerJob;

pub(super) use self::dispatch::RuntimeInvocationDispatchHandle;
pub(crate) use self::permit::SharedInvocationPermit;
use self::tenant_fairness::{
    RuntimeExecutorAdmissionState, RuntimeExecutorTenantAdmissionState, cleanup_tenant_locked,
    fairness_tenant_label, promote_ready_jobs_locked,
};

pub(super) struct RuntimeExecutorAdmission {
    policy: Arc<RuntimePolicy>,
    state: Mutex<RuntimeExecutorAdmissionState>,
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
        let Some(tenant_label) = fairness_tenant_label(&job).map(str::to_owned) else {
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

    pub(super) async fn acquire_active_permit(
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

    pub(super) fn mark_active_entered(&self, tenant_label: &str) {
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        if let Some(tenant_state) = state.tenants.get_mut(tenant_label) {
            tenant_state.parked_invocations = tenant_state.parked_invocations.saturating_sub(1);
            tenant_state.active_invocations += 1;
        }
    }

    pub(super) fn mark_active_suspended(&self, tenant_label: &str) {
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        if let Some(tenant_state) = state.tenants.get_mut(tenant_label) {
            tenant_state.active_invocations = tenant_state.active_invocations.saturating_sub(1);
            tenant_state.parked_invocations += 1;
        }
    }

    pub(super) fn complete_dispatched_job(
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
        cleanup_tenant_locked(&mut state, tenant_label);
        promote_ready_jobs_locked(self, &mut state, max_in_flight)
    }

    pub(super) fn rollback_dispatched_job(self: &Arc<Self>, tenant_label: &str) {
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        if let Some(tenant_state) = state.tenants.get_mut(tenant_label) {
            tenant_state.parked_invocations = tenant_state.parked_invocations.saturating_sub(1);
        }
        cleanup_tenant_locked(&mut state, tenant_label);
    }
}
