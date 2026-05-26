use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::metrics::{RuntimeMetrics, RuntimeMetricsSnapshot};

use super::{RuntimeBundleContentKind, RuntimeLimits, RuntimeTenantBudget};

#[derive(Debug)]
pub struct RuntimePolicy {
    limits: RuntimeLimits,
    runtime_instance_semaphore: Arc<Semaphore>,
    metrics: Arc<RuntimeMetrics>,
}

impl RuntimePolicy {
    pub fn new(limits: RuntimeLimits) -> Self {
        let limits = limits.normalized();
        Self {
            runtime_instance_semaphore: Arc::new(Semaphore::new(
                limits.max_concurrent_runtime_instances,
            )),
            metrics: Arc::new(RuntimeMetrics::default()),
            limits,
        }
    }

    pub fn limits(&self) -> &RuntimeLimits {
        &self.limits
    }

    pub(crate) fn validate_bundle_content_kind(
        &self,
        content_kind: RuntimeBundleContentKind,
    ) -> crate::Result<()> {
        if self.limits.bundle_content_kind == content_kind {
            return Ok(());
        }
        Err(crate::NimbusRuntimeError::Contract(format!(
            "runtime bundle content kind {:?} does not match policy content kind {:?}",
            content_kind, self.limits.bundle_content_kind
        )))
    }

    pub(crate) fn runtime_instance_semaphore(&self) -> Arc<Semaphore> {
        self.runtime_instance_semaphore.clone()
    }

    pub fn metrics(&self) -> Arc<RuntimeMetrics> {
        self.metrics.clone()
    }

    pub fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.metrics.snapshot()
    }

    pub fn tenant_budget(&self) -> RuntimeTenantBudget {
        self.limits.tenant_budget_from_normalized()
    }
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self::new(RuntimeLimits::default())
    }
}
