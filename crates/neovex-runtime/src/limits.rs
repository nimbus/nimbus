use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Semaphore;

use crate::metrics::{RuntimeMetrics, RuntimeMetricsSnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeLimits {
    pub max_heap_mb: usize,
    pub initial_heap_mb: usize,
    pub execution_timeout: Duration,
    pub max_concurrent_isolates: usize,
    pub worker_threads: usize,
    pub max_active_top_level_invocations_per_tenant: usize,
    pub max_in_flight_top_level_invocations_per_tenant: usize,
    pub max_queued_top_level_invocations_per_tenant: usize,
    pub max_nested_runtime_invocations: usize,
}

impl RuntimeLimits {
    pub fn normalized(&self) -> Self {
        let max_concurrent_isolates = self.max_concurrent_isolates.max(1);
        let worker_threads = self.worker_threads.max(max_concurrent_isolates).max(1);
        let max_heap_mb = self.max_heap_mb.max(1);
        let initial_heap_mb = self.initial_heap_mb.max(1).min(max_heap_mb);
        let max_active_top_level_invocations_per_tenant = self
            .max_active_top_level_invocations_per_tenant
            .max(1)
            .min(max_concurrent_isolates);
        let max_in_flight_top_level_invocations_per_tenant = self
            .max_in_flight_top_level_invocations_per_tenant
            .max(max_active_top_level_invocations_per_tenant)
            .min(worker_threads);
        Self {
            max_heap_mb,
            initial_heap_mb,
            execution_timeout: self.execution_timeout,
            max_concurrent_isolates,
            worker_threads,
            max_active_top_level_invocations_per_tenant,
            max_in_flight_top_level_invocations_per_tenant,
            max_queued_top_level_invocations_per_tenant: self
                .max_queued_top_level_invocations_per_tenant,
            max_nested_runtime_invocations: self.max_nested_runtime_invocations,
        }
    }
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        let max_concurrent_isolates = std::thread::available_parallelism()
            .unwrap_or(NonZeroUsize::MIN)
            .get();
        let worker_threads = max_concurrent_isolates.saturating_mul(2).max(1);
        let max_active_top_level_invocations_per_tenant =
            max_concurrent_isolates.saturating_sub(1).max(1);
        let max_in_flight_top_level_invocations_per_tenant =
            max_active_top_level_invocations_per_tenant
                .saturating_mul(2)
                .min(worker_threads)
                .max(max_active_top_level_invocations_per_tenant);
        Self {
            max_heap_mb: 128,
            initial_heap_mb: 8,
            execution_timeout: Duration::from_secs(30),
            max_concurrent_isolates,
            worker_threads,
            max_active_top_level_invocations_per_tenant,
            max_in_flight_top_level_invocations_per_tenant,
            max_queued_top_level_invocations_per_tenant:
                max_in_flight_top_level_invocations_per_tenant,
            max_nested_runtime_invocations: 64,
        }
    }
}

#[derive(Debug)]
pub struct RuntimePolicy {
    limits: RuntimeLimits,
    isolate_semaphore: Arc<Semaphore>,
    metrics: Arc<RuntimeMetrics>,
}

impl RuntimePolicy {
    pub fn new(limits: RuntimeLimits) -> Self {
        let limits = limits.normalized();
        Self {
            isolate_semaphore: Arc::new(Semaphore::new(limits.max_concurrent_isolates)),
            metrics: Arc::new(RuntimeMetrics::default()),
            limits,
        }
    }

    pub fn limits(&self) -> &RuntimeLimits {
        &self.limits
    }

    pub(crate) fn isolate_semaphore(&self) -> Arc<Semaphore> {
        self.isolate_semaphore.clone()
    }

    pub fn metrics(&self) -> Arc<RuntimeMetrics> {
        self.metrics.clone()
    }

    pub fn metrics_snapshot(&self) -> RuntimeMetricsSnapshot {
        self.metrics.snapshot()
    }
}

impl Default for RuntimePolicy {
    fn default() -> Self {
        Self::new(RuntimeLimits::default())
    }
}
