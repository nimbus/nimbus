mod dispatch;
mod permit;
mod tenant_fairness;

use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::OwnedSemaphorePermit;

use crate::error::{NimbusRuntimeError, Result};
use crate::limits::{
    RuntimeHostAdmissionAction, RuntimeHostAdmissionDecision, RuntimeHostPressureLevel,
    RuntimeHostWorkClass, RuntimePolicy,
};

use super::queue::RuntimeWorkerJob;

pub(super) use self::dispatch::RuntimeInvocationDispatchHandle;
pub(crate) use self::permit::{SharedInvocationPermit, SharedInvocationPermitAcquire};
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
        job: RuntimeWorkerJob,
    ) -> Result<RuntimeExecutorAdmissionDecision> {
        let started_at = Instant::now();
        let result = self.admit_job_inner(job);
        self.policy
            .metrics()
            .record_admission_decision(started_at.elapsed());
        result
    }

    fn admit_job_inner(
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
        let host_admission = self.host_admission_for_in_flight(state.total_in_flight(), &job);
        if matches!(host_admission.action, RuntimeHostAdmissionAction::Shed) {
            drop(state);
            self.policy
                .metrics()
                .record_rejected_invocation_for_tenant(Some(&tenant_label));
            return Err(NimbusRuntimeError::HostResourcePressureShed {
                work_class: runtime_host_work_class_for_job(&job).as_str(),
                host_pressure_level: host_admission.host_pressure_level.as_str(),
            });
        }
        let tenant_state = state
            .tenants
            .entry(tenant_label.clone())
            .or_insert_with(|| RuntimeExecutorTenantAdmissionState::new(max_active));
        if matches!(host_admission.action, RuntimeHostAdmissionAction::Admit)
            && tenant_state.total_in_flight() < max_in_flight
            && tenant_state.queued_jobs.is_empty()
        {
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
            return Err(NimbusRuntimeError::TenantQueueLimitExceeded {
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

    pub(super) fn cancel_queued_job(&self, invocation_id: u64) -> Option<RuntimeWorkerJob> {
        let mut state = self
            .state
            .lock()
            .expect("runtime executor admission lock should not be poisoned");
        remove_queued_job_locked(&mut state, invocation_id)
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
            NimbusRuntimeError::Contract(format!(
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

    pub(super) fn host_admission_action_for_in_flight(
        &self,
        current_host_in_flight: usize,
        job: &RuntimeWorkerJob,
    ) -> RuntimeHostAdmissionAction {
        self.host_admission_for_in_flight(current_host_in_flight, job)
            .action
    }

    fn host_admission_for_in_flight(
        &self,
        current_host_in_flight: usize,
        job: &RuntimeWorkerJob,
    ) -> RuntimeHostAdmissionDecision {
        if !self.policy.host_resource_governor_enabled() {
            return RuntimeHostAdmissionDecision {
                work_class: runtime_host_work_class_for_job(job),
                action: RuntimeHostAdmissionAction::Admit,
                over_capacity_action: RuntimeHostAdmissionAction::Admit,
                tenant_quota_remaining: true,
                host_pressure_level: RuntimeHostPressureLevel::Nominal,
                current_host_in_flight,
                effective_dispatch_seats: usize::MAX,
            };
        }
        self.policy
            .host_resource_decision()
            .admission_for_in_flight(
                current_host_in_flight,
                runtime_host_work_class_for_job(job),
                true,
            )
    }
}

fn runtime_host_work_class_for_job(job: &RuntimeWorkerJob) -> RuntimeHostWorkClass {
    job.execution_plan.host_work_class()
}

fn remove_queued_job_locked(
    state: &mut RuntimeExecutorAdmissionState,
    invocation_id: u64,
) -> Option<RuntimeWorkerJob> {
    let tenant_labels = state.tenants.keys().cloned().collect::<Vec<_>>();
    let mut canceled_job = None;
    let mut canceled_tenant_label = None;
    let mut removed_empty_tenant_queue = false;

    for tenant_label in tenant_labels {
        let removed_job = state
            .tenants
            .get_mut(&tenant_label)
            .and_then(|tenant_state| {
                let position = tenant_state
                    .queued_jobs
                    .iter()
                    .position(|job| job.context.invocation_id == invocation_id)?;
                let removed_job = tenant_state
                    .queued_jobs
                    .remove(position)
                    .expect("queued job position should be present");
                removed_empty_tenant_queue = tenant_state.queued_jobs.is_empty();
                if removed_empty_tenant_queue {
                    tenant_state.queued_in_rotation = false;
                }
                Some(removed_job)
            });

        if let Some(removed_job) = removed_job {
            canceled_job = Some(removed_job);
            canceled_tenant_label = Some(tenant_label);
            break;
        }
    }

    if let Some(tenant_label) = canceled_tenant_label.as_deref() {
        if removed_empty_tenant_queue {
            state
                .queued_tenants
                .retain(|queued_tenant| queued_tenant != tenant_label);
        }
        cleanup_tenant_locked(state, tenant_label);
    }
    canceled_job
}

impl RuntimeHostPressureLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Nominal => "nominal",
            Self::High => "high",
            Self::Critical => "critical",
        }
    }
}

impl RuntimeHostWorkClass {
    fn as_str(self) -> &'static str {
        match self {
            Self::Guaranteed => "guaranteed",
            Self::Burstable => "burstable",
            Self::BestEffort => "best_effort",
        }
    }
}
