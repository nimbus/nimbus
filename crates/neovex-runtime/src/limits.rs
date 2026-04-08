use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Semaphore;

use crate::metrics::{RuntimeMetrics, RuntimeMetricsSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBackendKind {
    DenoCore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeExecutionModel {
    RunToCompletion,
    CooperativeLocker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRoutingAffinity {
    None,
    Tenant,
    Function,
    Script,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePoolKind {
    /// Reuse the worker-local bootstrap snapshot, then build a fresh JsRuntime
    /// for every invocation.
    ///
    /// This preserves the freshest execution boundary and is currently the
    /// default low-latency mode.
    StartupSnapshotCache,
    /// Retain whole JsRuntime instances with evaluated modules alive across
    /// invocations. No realm reset, no module reload — only surgical
    /// per-request state cleanup via `reset_request_state()`.
    ///
    /// Requires `CooperativeLocker` execution model. Fails fast with
    /// `RunToCompletion`.
    WarmModulePool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeModuleStateSemantics {
    FreshPerInvocation,
    /// Modules persist across invocations by contract. Module-level side
    /// effects (e.g. `let counter = 0`) accumulate across requests.
    WarmPerBundle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RuntimeResetCapabilities {
    pub op_state_per_invocation: bool,
    pub bootstrap_state_per_invocation: bool,
    pub user_module_state_per_invocation: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeLimits {
    pub backend_kind: RuntimeBackendKind,
    pub execution_model: RuntimeExecutionModel,
    pub runtime_pool_kind: RuntimePoolKind,
    pub routing_affinity: RuntimeRoutingAffinity,
    pub routing_affinity_max_entries: usize,
    pub max_warm_module_pool_entries_per_worker: usize,
    pub max_warm_module_reuses: usize,
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
    pub fn module_state_semantics(&self) -> RuntimeModuleStateSemantics {
        match self.runtime_pool_kind {
            RuntimePoolKind::WarmModulePool => RuntimeModuleStateSemantics::WarmPerBundle,
            _ => RuntimeModuleStateSemantics::FreshPerInvocation,
        }
    }

    pub fn reset_capabilities(&self) -> RuntimeResetCapabilities {
        match self.runtime_pool_kind {
            RuntimePoolKind::WarmModulePool => RuntimeResetCapabilities {
                op_state_per_invocation: true,
                bootstrap_state_per_invocation: true,
                user_module_state_per_invocation: false,
            },
            RuntimePoolKind::StartupSnapshotCache => RuntimeResetCapabilities {
                op_state_per_invocation: true,
                bootstrap_state_per_invocation: true,
                user_module_state_per_invocation: true,
            },
        }
    }

    pub fn normalized(&self) -> Self {
        // WarmModulePool requires CooperativeLocker — fail fast.
        if matches!(self.runtime_pool_kind, RuntimePoolKind::WarmModulePool)
            && !matches!(
                self.execution_model,
                RuntimeExecutionModel::CooperativeLocker
            )
        {
            panic!(
                "WarmModulePool requires CooperativeLocker execution model, \
                 got {:?}",
                self.execution_model
            );
        }

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
            backend_kind: self.backend_kind,
            execution_model: self.execution_model,
            runtime_pool_kind: self.runtime_pool_kind,
            routing_affinity: self.routing_affinity,
            routing_affinity_max_entries: self.routing_affinity_max_entries.max(1),
            max_warm_module_pool_entries_per_worker: self
                .max_warm_module_pool_entries_per_worker
                .max(1),
            max_warm_module_reuses: self.max_warm_module_reuses.max(1),
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
        let routing_affinity_max_entries = worker_threads.saturating_mul(256).max(1024);
        Self {
            backend_kind: RuntimeBackendKind::DenoCore,
            execution_model: RuntimeExecutionModel::CooperativeLocker,
            runtime_pool_kind: RuntimePoolKind::WarmModulePool,
            routing_affinity: RuntimeRoutingAffinity::Tenant,
            routing_affinity_max_entries,
            max_warm_module_pool_entries_per_worker: 4,
            max_warm_module_reuses: 10_000,
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
