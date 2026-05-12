use std::sync::Arc;

use tokio::sync::OwnedSemaphorePermit;

use crate::error::Result;

use super::super::queue::RuntimeWorkerJob;
use super::RuntimeExecutorAdmission;

#[derive(Clone)]
pub(crate) struct RuntimeInvocationDispatchHandle {
    pub(in crate::executor::admission) admission: Arc<RuntimeExecutorAdmission>,
    pub(in crate::executor::admission) tenant_label: String,
    pub(in crate::executor::admission) active_semaphore: Arc<tokio::sync::Semaphore>,
}

impl RuntimeInvocationDispatchHandle {
    pub(in crate::executor) async fn acquire_active_permit(&self) -> Result<OwnedSemaphorePermit> {
        self.admission
            .acquire_active_permit(&self.tenant_label, self.active_semaphore.clone())
            .await
    }

    pub(in crate::executor) fn mark_active_entered(&self) {
        self.admission.mark_active_entered(&self.tenant_label);
    }

    pub(in crate::executor) fn mark_active_suspended(&self) {
        self.admission.mark_active_suspended(&self.tenant_label);
    }

    pub(in crate::executor) fn complete_invocation(
        &self,
        was_active: bool,
    ) -> Vec<RuntimeWorkerJob> {
        self.admission
            .complete_dispatched_job(&self.tenant_label, was_active)
    }

    pub(in crate::executor) fn rollback_dispatch(&self) {
        self.admission.rollback_dispatched_job(&self.tenant_label);
    }
}
