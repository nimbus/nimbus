use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use super::super::queue::RuntimeWorkerJob;
use super::RuntimeExecutorAdmission;
use super::dispatch::RuntimeInvocationDispatchHandle;

#[derive(Default)]
pub(super) struct RuntimeExecutorAdmissionState {
    pub(super) tenants: BTreeMap<String, RuntimeExecutorTenantAdmissionState>,
    pub(super) queued_tenants: VecDeque<String>,
}

pub(super) struct RuntimeExecutorTenantAdmissionState {
    pub(super) active_invocations: usize,
    pub(super) parked_invocations: usize,
    pub(super) active_semaphore: Arc<tokio::sync::Semaphore>,
    pub(super) queued_jobs: VecDeque<RuntimeWorkerJob>,
    pub(super) queued_in_rotation: bool,
}

impl RuntimeExecutorTenantAdmissionState {
    pub(super) fn new(max_active: usize) -> Self {
        Self {
            active_invocations: 0,
            parked_invocations: 0,
            active_semaphore: Arc::new(tokio::sync::Semaphore::new(max_active.max(1))),
            queued_jobs: VecDeque::new(),
            queued_in_rotation: false,
        }
    }

    pub(super) fn total_in_flight(&self) -> usize {
        self.active_invocations + self.parked_invocations
    }
}

pub(super) fn fairness_tenant_label(job: &RuntimeWorkerJob) -> Option<&str> {
    (!job.runtime.bypasses_concurrency_limit())
        .then_some(job.context.tenant_label.as_deref())
        .flatten()
}

pub(super) fn promote_ready_jobs_locked(
    admission: &Arc<RuntimeExecutorAdmission>,
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
                            admission: admission.clone(),
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

pub(super) fn cleanup_tenant_locked(state: &mut RuntimeExecutorAdmissionState, tenant_label: &str) {
    let should_remove = state.tenants.get(tenant_label).is_some_and(|tenant_state| {
        tenant_state.total_in_flight() == 0
            && tenant_state.queued_jobs.is_empty()
            && !tenant_state.queued_in_rotation
    });
    if should_remove {
        state.tenants.remove(tenant_label);
    }
}
