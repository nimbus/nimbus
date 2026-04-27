mod correlations;
mod global;
mod host_operations;
mod tenants;

use std::sync::atomic::Ordering;
use std::time::Duration;

use serde::Serialize;

use crate::context::RuntimeInvocationContext;
use crate::host::HostCallCancellationCause;

pub use self::correlations::RuntimeRequestCorrelationSnapshot;
use self::global::RuntimeGlobalCounters;
pub use self::host_operations::RuntimeHostOperationMetricsSnapshot;
use self::host_operations::RuntimeHostOperationRegistry;
use self::tenants::RuntimeTenantRegistry;
pub use self::tenants::{RuntimeDurationDistributionSnapshot, RuntimeTenantMetricsSnapshot};

// These atomics back diagnostics-only snapshots and counters. They do not
// participate in runtime correctness or cancellation safety, so relaxed
// ordering is sufficient and avoids paying global-ordering costs.
const DIAGNOSTIC_COUNTER_ORDERING: Ordering = Ordering::Relaxed;

#[derive(Debug, Default)]
pub struct RuntimeMetrics {
    global: RuntimeGlobalCounters,
    host_operations: RuntimeHostOperationRegistry,
    tenants: RuntimeTenantRegistry,
    recent_request_correlations: correlations::RuntimeRequestCorrelationLog,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeMetricsSnapshot {
    pub active_runtime_instances: usize,
    pub queued_invocations: usize,
    pub worker_dispatched_invocations: u64,
    pub worker_affinity_routed_invocations: u64,
    pub worker_least_loaded_routed_invocations: u64,
    pub worker_affinity_cache_entries: usize,
    pub worker_affinity_cache_evictions: u64,
    pub retained_runtime_pool_entries: usize,
    pub retained_runtime_pool_evictions: u64,
    pub retained_runtime_pool_retirements: u64,
    pub bundle_loads: u64,
    pub bundle_load_nanos_total: u64,
    pub bundle_module_loads: u64,
    pub bundle_module_load_nanos_total: u64,
    pub bundle_evaluations: u64,
    pub bundle_evaluation_nanos_total: u64,
    pub runtime_pool_hits: u64,
    pub runtime_pool_misses: u64,
    pub runtime_pool_replacements: u64,
    pub started_invocations: u64,
    pub completed_invocations: u64,
    pub queue_wait_nanos_total: u64,
    pub execution_nanos_total: u64,
    pub timed_out_invocations: u64,
    pub canceled_invocations: u64,
    pub rejected_invocations: u64,
    pub queued_canceled_invocations: u64,
    pub in_flight_canceled_invocations: u64,
    pub disconnect_canceled_invocations: u64,
    pub explicit_canceled_invocations: u64,
    pub canceled_host_ops: u64,
    pub precanceled_host_ops: u64,
    pub in_flight_canceled_host_ops: u64,
    pub nested_local_dispatches: u64,
    pub fallback_cross_runtime_dispatches: u64,
    pub warm_pool_hits: u64,
    pub warm_pool_misses: u64,
    pub warm_pool_retirements: u64,
    pub warm_pool_discard_unquiesced: u64,
    pub host_operations: std::collections::BTreeMap<String, RuntimeHostOperationMetricsSnapshot>,
    pub tenants: std::collections::BTreeMap<String, RuntimeTenantMetricsSnapshot>,
    pub recent_request_correlations: Vec<RuntimeRequestCorrelationSnapshot>,
}

impl RuntimeMetrics {
    pub fn increment_queued_invocations(&self) {
        self.global.increment_queued_invocations();
    }

    pub fn decrement_queued_invocations(&self) {
        self.global.decrement_queued_invocations();
    }

    pub fn increment_active_runtime_instances(&self) {
        self.increment_active_runtime_instances_for_tenant(None);
    }

    pub fn increment_active_runtime_instances_for_tenant(&self, tenant_label: Option<&str>) {
        self.global.increment_active_runtime_instances();
        self.tenants
            .increment_active_runtime_instances(tenant_label);
    }

    pub fn record_invocation_started(&self) {
        self.record_invocation_started_for_tenant(None);
    }

    pub fn record_invocation_started_for_tenant(&self, tenant_label: Option<&str>) {
        self.global.record_invocation_started();
        self.tenants.record_invocation_started(tenant_label);
    }

    pub fn record_worker_dispatch(&self) {
        self.global.record_worker_dispatch();
    }

    pub fn record_worker_affinity_route(&self) {
        self.global.record_worker_affinity_route();
    }

    pub fn record_worker_least_loaded_route(&self) {
        self.global.record_worker_least_loaded_route();
    }

    pub fn update_worker_affinity_cache_entries(&self, entries: usize) {
        self.global.update_worker_affinity_cache_entries(entries);
    }

    pub fn record_worker_affinity_cache_eviction(&self) {
        self.global.record_worker_affinity_cache_eviction();
    }

    pub fn increment_retained_runtime_pool_entries(&self) {
        self.global.increment_retained_runtime_pool_entries();
    }

    pub fn decrement_retained_runtime_pool_entries(&self) {
        self.global.decrement_retained_runtime_pool_entries();
    }

    pub fn record_retained_runtime_pool_eviction(&self) {
        self.global.record_retained_runtime_pool_eviction();
    }

    pub fn record_retained_runtime_pool_retirement(&self) {
        self.global.record_retained_runtime_pool_retirement();
    }

    pub fn record_warm_pool_hit(&self) {
        self.global.record_warm_pool_hit();
    }

    pub fn record_warm_pool_miss(&self) {
        self.global.record_warm_pool_miss();
    }

    pub fn record_warm_pool_retirement(&self) {
        self.global.record_warm_pool_retirement();
    }

    pub fn record_warm_pool_discard_unquiesced(&self) {
        self.global.record_warm_pool_discard_unquiesced();
    }

    pub fn record_bundle_load(&self, duration: Duration) {
        self.global.record_bundle_load(duration);
    }

    pub fn record_bundle_module_load(&self, duration: Duration) {
        self.global.record_bundle_module_load(duration);
    }

    pub fn record_bundle_evaluation(&self, duration: Duration) {
        self.global.record_bundle_evaluation(duration);
    }

    pub fn record_runtime_pool_hit(&self) {
        self.global.record_runtime_pool_hit();
    }

    pub fn record_runtime_pool_miss(&self) {
        self.global.record_runtime_pool_miss();
    }

    pub fn record_runtime_pool_replacement(&self) {
        self.global.record_runtime_pool_replacement();
    }

    pub fn decrement_active_runtime_instances(&self) {
        self.decrement_active_runtime_instances_for_tenant(None);
    }

    pub fn decrement_active_runtime_instances_for_tenant(&self, tenant_label: Option<&str>) {
        self.global.decrement_active_runtime_instances();
        self.tenants
            .decrement_active_runtime_instances(tenant_label);
    }

    pub fn record_invocation_completed(&self) {
        self.record_invocation_completed_for_tenant(None);
    }

    pub fn record_invocation_completed_for_tenant(&self, tenant_label: Option<&str>) {
        self.global.record_invocation_completed();
        self.tenants.record_invocation_completed(tenant_label);
    }

    pub fn record_queue_wait(&self, duration: Duration) {
        self.record_queue_wait_for_tenant(None, duration);
    }

    pub fn record_queue_wait_for_tenant(&self, tenant_label: Option<&str>, duration: Duration) {
        self.global.record_queue_wait(duration);
        self.tenants.record_queue_wait(tenant_label, duration);
    }

    pub fn record_execution(&self, duration: Duration) {
        self.record_execution_for_tenant(None, duration);
    }

    pub fn record_execution_for_tenant(&self, tenant_label: Option<&str>, duration: Duration) {
        self.global.record_execution(duration);
        self.tenants.record_execution(tenant_label, duration);
    }

    pub fn record_timeout(&self) {
        self.global.record_timeout();
    }

    pub fn record_canceled_invocation(&self) {
        self.global.record_canceled_invocation();
    }

    pub fn record_rejected_invocation_for_tenant(&self, tenant_label: Option<&str>) {
        self.global.record_rejected_invocation();
        self.tenants.record_rejected_invocation(tenant_label);
    }

    pub fn record_queued_canceled_invocation(&self) {
        self.record_queued_canceled_invocation_for_tenant(None, None);
    }

    pub fn record_queued_canceled_invocation_for_tenant(
        &self,
        tenant_label: Option<&str>,
        cause: Option<HostCallCancellationCause>,
    ) {
        self.global.record_queued_canceled_invocation();
        self.record_canceled_invocation_cause(tenant_label, cause);
        self.tenants.record_queued_canceled_invocation(tenant_label);
    }

    pub fn record_in_flight_canceled_invocation(&self) {
        self.record_in_flight_canceled_invocation_for_tenant(None, None);
    }

    pub fn record_in_flight_canceled_invocation_for_tenant(
        &self,
        tenant_label: Option<&str>,
        cause: Option<HostCallCancellationCause>,
    ) {
        self.global.record_in_flight_canceled_invocation();
        self.record_canceled_invocation_cause(tenant_label, cause);
        self.tenants
            .record_in_flight_canceled_invocation(tenant_label);
    }

    pub fn record_canceled_host_op(&self) {
        self.global.record_canceled_host_op();
    }

    pub fn record_precanceled_host_op(&self) {
        self.global.record_precanceled_host_op();
    }

    pub fn record_in_flight_canceled_host_op(&self) {
        self.global.record_in_flight_canceled_host_op();
    }

    pub fn record_host_operation_started(&self, operation: &str) {
        self.host_operations.record_started(operation);
    }

    pub fn record_host_operation_succeeded(&self, operation: &str) {
        self.host_operations.record_succeeded(operation);
    }

    pub fn record_host_operation_failed(&self, operation: &str) {
        self.host_operations.record_failed(operation);
    }

    pub fn record_host_operation_canceled_before_start(&self, operation: &str) {
        self.record_precanceled_host_op();
        self.host_operations.record_canceled_before_start(operation);
    }

    pub fn record_host_operation_canceled_in_flight(&self, operation: &str) {
        self.record_in_flight_canceled_host_op();
        self.host_operations.record_canceled_in_flight(operation);
    }

    pub fn record_nested_local_dispatch(&self) {
        self.global.record_nested_local_dispatch();
    }

    pub fn record_fallback_cross_runtime_dispatch(&self) {
        self.global.record_fallback_cross_runtime_dispatch();
    }

    pub fn record_request_correlation(&self, context: &RuntimeInvocationContext) {
        self.recent_request_correlations.record(context);
    }

    pub fn snapshot(&self) -> RuntimeMetricsSnapshot {
        let global = self.global.snapshot();
        RuntimeMetricsSnapshot {
            active_runtime_instances: global.active_runtime_instances,
            queued_invocations: global.queued_invocations,
            worker_dispatched_invocations: global.worker_dispatched_invocations,
            worker_affinity_routed_invocations: global.worker_affinity_routed_invocations,
            worker_least_loaded_routed_invocations: global.worker_least_loaded_routed_invocations,
            worker_affinity_cache_entries: global.worker_affinity_cache_entries,
            worker_affinity_cache_evictions: global.worker_affinity_cache_evictions,
            retained_runtime_pool_entries: global.retained_runtime_pool_entries,
            retained_runtime_pool_evictions: global.retained_runtime_pool_evictions,
            retained_runtime_pool_retirements: global.retained_runtime_pool_retirements,
            bundle_loads: global.bundle_loads,
            bundle_load_nanos_total: global.bundle_load_nanos_total,
            bundle_module_loads: global.bundle_module_loads,
            bundle_module_load_nanos_total: global.bundle_module_load_nanos_total,
            bundle_evaluations: global.bundle_evaluations,
            bundle_evaluation_nanos_total: global.bundle_evaluation_nanos_total,
            runtime_pool_hits: global.runtime_pool_hits,
            runtime_pool_misses: global.runtime_pool_misses,
            runtime_pool_replacements: global.runtime_pool_replacements,
            started_invocations: global.started_invocations,
            completed_invocations: global.completed_invocations,
            queue_wait_nanos_total: global.queue_wait_nanos_total,
            execution_nanos_total: global.execution_nanos_total,
            timed_out_invocations: global.timed_out_invocations,
            canceled_invocations: global.canceled_invocations,
            rejected_invocations: global.rejected_invocations,
            queued_canceled_invocations: global.queued_canceled_invocations,
            in_flight_canceled_invocations: global.in_flight_canceled_invocations,
            disconnect_canceled_invocations: global.disconnect_canceled_invocations,
            explicit_canceled_invocations: global.explicit_canceled_invocations,
            canceled_host_ops: global.canceled_host_ops,
            precanceled_host_ops: global.precanceled_host_ops,
            in_flight_canceled_host_ops: global.in_flight_canceled_host_ops,
            nested_local_dispatches: global.nested_local_dispatches,
            fallback_cross_runtime_dispatches: global.fallback_cross_runtime_dispatches,
            warm_pool_hits: global.warm_pool_hits,
            warm_pool_misses: global.warm_pool_misses,
            warm_pool_retirements: global.warm_pool_retirements,
            warm_pool_discard_unquiesced: global.warm_pool_discard_unquiesced,
            host_operations: self.host_operations.snapshot(),
            tenants: self.tenants.snapshot(),
            recent_request_correlations: self.recent_request_correlations.snapshot(),
        }
    }

    fn record_canceled_invocation_cause(
        &self,
        tenant_label: Option<&str>,
        cause: Option<HostCallCancellationCause>,
    ) {
        match cause {
            Some(HostCallCancellationCause::Disconnect) => {
                self.global.record_disconnect_canceled_invocation();
                self.tenants
                    .record_disconnect_canceled_invocation(tenant_label);
            }
            Some(HostCallCancellationCause::Explicit) => {
                self.global.record_explicit_canceled_invocation();
                self.tenants
                    .record_explicit_canceled_invocation(tenant_label);
            }
            None => {}
        }
    }
}

pub(super) fn duration_to_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    #[test]
    fn tenant_metrics_snapshot_tracks_distributions_and_cancellations() {
        let metrics = RuntimeMetrics::default();

        metrics.record_invocation_started_for_tenant(Some("demo"));
        metrics.increment_active_runtime_instances_for_tenant(Some("demo"));
        metrics.record_queue_wait_for_tenant(Some("demo"), Duration::from_micros(500));
        metrics.record_execution_for_tenant(Some("demo"), Duration::from_millis(7));
        metrics.record_queued_canceled_invocation_for_tenant(
            Some("demo"),
            Some(HostCallCancellationCause::Disconnect),
        );
        metrics.record_in_flight_canceled_invocation_for_tenant(
            Some("demo"),
            Some(HostCallCancellationCause::Explicit),
        );
        metrics.record_request_correlation(&RuntimeInvocationContext {
            invocation_id: 7,
            function_name: "messages:list".to_string(),
            kind: "query",
            is_top_level: true,
            bypasses_concurrency_limit: false,
            tenant_label: Some("demo".to_string()),
            server_request_id: Some("req-7".to_string()),
        });
        metrics.decrement_active_runtime_instances_for_tenant(Some("demo"));
        metrics.record_invocation_completed_for_tenant(Some("demo"));

        let snapshot = metrics.snapshot();
        assert_eq!(
            snapshot
                .tenants
                .get("demo")
                .expect("tenant metrics should be present"),
            &RuntimeTenantMetricsSnapshot {
                active_runtime_instances: 0,
                started_invocations: 1,
                completed_invocations: 1,
                rejected_invocations: 0,
                queued_canceled_invocations: 1,
                in_flight_canceled_invocations: 1,
                disconnect_canceled_invocations: 1,
                explicit_canceled_invocations: 1,
                queue_wait_nanos_total: 500_000,
                execution_nanos_total: 7_000_000,
                queue_wait_distribution: RuntimeDurationDistributionSnapshot {
                    samples: 1,
                    under_1ms: 1,
                    ..RuntimeDurationDistributionSnapshot::default()
                },
                execution_distribution: RuntimeDurationDistributionSnapshot {
                    samples: 1,
                    ms_5_to_25: 1,
                    ..RuntimeDurationDistributionSnapshot::default()
                },
            }
        );
        assert_eq!(
            snapshot.recent_request_correlations,
            vec![RuntimeRequestCorrelationSnapshot {
                invocation_id: 7,
                server_request_id: "req-7".to_string(),
                tenant_label: Some("demo".to_string()),
                function_name: "messages:list".to_string(),
                kind: "query".to_string(),
            }]
        );
    }

    #[test]
    fn unattributed_metrics_do_not_create_tenant_entries() {
        let metrics = RuntimeMetrics::default();

        metrics.record_invocation_started();
        metrics.increment_active_runtime_instances();
        metrics.increment_queued_invocations();
        metrics.record_queue_wait(Duration::from_millis(1));
        metrics.record_execution(Duration::from_millis(2));
        metrics.record_worker_dispatch();
        metrics.record_worker_affinity_route();
        metrics.record_worker_least_loaded_route();
        metrics.update_worker_affinity_cache_entries(1);
        metrics.record_worker_affinity_cache_eviction();
        metrics.increment_retained_runtime_pool_entries();
        metrics.record_retained_runtime_pool_eviction();
        metrics.record_retained_runtime_pool_retirement();
        metrics.record_bundle_load(Duration::from_millis(5));
        metrics.record_bundle_module_load(Duration::from_millis(6));
        metrics.record_bundle_evaluation(Duration::from_millis(7));
        metrics.decrement_retained_runtime_pool_entries();
        metrics.record_runtime_pool_miss();
        metrics.record_runtime_pool_hit();
        metrics.record_runtime_pool_replacement();
        metrics.record_timeout();
        metrics.record_rejected_invocation_for_tenant(None);
        metrics.record_queued_canceled_invocation();
        metrics.record_precanceled_host_op();
        metrics.record_in_flight_canceled_host_op();
        metrics.record_nested_local_dispatch();
        metrics.record_fallback_cross_runtime_dispatch();
        metrics.decrement_queued_invocations();
        metrics.decrement_active_runtime_instances();
        metrics.record_invocation_completed();

        let snapshot = metrics.snapshot();
        assert_eq!(
            snapshot,
            RuntimeMetricsSnapshot {
                active_runtime_instances: 0,
                queued_invocations: 0,
                worker_dispatched_invocations: 1,
                worker_affinity_routed_invocations: 1,
                worker_least_loaded_routed_invocations: 1,
                worker_affinity_cache_entries: 1,
                worker_affinity_cache_evictions: 1,
                retained_runtime_pool_entries: 0,
                retained_runtime_pool_evictions: 1,
                retained_runtime_pool_retirements: 1,
                bundle_loads: 1,
                bundle_load_nanos_total: 5_000_000,
                bundle_module_loads: 1,
                bundle_module_load_nanos_total: 6_000_000,
                bundle_evaluations: 1,
                bundle_evaluation_nanos_total: 7_000_000,
                runtime_pool_hits: 1,
                runtime_pool_misses: 1,
                runtime_pool_replacements: 1,
                started_invocations: 1,
                completed_invocations: 1,
                queue_wait_nanos_total: 1_000_000,
                execution_nanos_total: 2_000_000,
                timed_out_invocations: 1,
                canceled_invocations: 1,
                rejected_invocations: 1,
                queued_canceled_invocations: 1,
                in_flight_canceled_invocations: 0,
                disconnect_canceled_invocations: 0,
                explicit_canceled_invocations: 0,
                canceled_host_ops: 2,
                precanceled_host_ops: 1,
                in_flight_canceled_host_ops: 1,
                nested_local_dispatches: 1,
                fallback_cross_runtime_dispatches: 1,
                warm_pool_hits: 0,
                warm_pool_misses: 0,
                warm_pool_retirements: 0,
                warm_pool_discard_unquiesced: 0,
                host_operations: BTreeMap::new(),
                tenants: BTreeMap::new(),
                recent_request_correlations: Vec::new(),
            }
        );
    }
}
