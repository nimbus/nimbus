use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use serde::Serialize;

use crate::context::RuntimeInvocationContext;
use crate::host::HostCallCancellationCause;

#[derive(Debug, Default)]
pub struct RuntimeMetrics {
    active_isolates: AtomicUsize,
    queued_invocations: AtomicUsize,
    worker_dispatched_invocations: AtomicU64,
    started_invocations: AtomicU64,
    completed_invocations: AtomicU64,
    queue_wait_nanos_total: AtomicU64,
    execution_nanos_total: AtomicU64,
    timed_out_invocations: AtomicU64,
    canceled_invocations: AtomicU64,
    queued_canceled_invocations: AtomicU64,
    in_flight_canceled_invocations: AtomicU64,
    disconnect_canceled_invocations: AtomicU64,
    explicit_canceled_invocations: AtomicU64,
    canceled_host_ops: AtomicU64,
    precanceled_host_ops: AtomicU64,
    in_flight_canceled_host_ops: AtomicU64,
    nested_local_dispatches: AtomicU64,
    fallback_cross_isolate_dispatches: AtomicU64,
    host_operations: Mutex<BTreeMap<String, RuntimeHostOperationMetrics>>,
    tenant_metrics: Mutex<BTreeMap<String, RuntimeTenantMetrics>>,
    recent_request_correlations: Mutex<VecDeque<RuntimeRequestCorrelation>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeMetricsSnapshot {
    pub active_isolates: usize,
    pub queued_invocations: usize,
    pub worker_dispatched_invocations: u64,
    pub started_invocations: u64,
    pub completed_invocations: u64,
    pub queue_wait_nanos_total: u64,
    pub execution_nanos_total: u64,
    pub timed_out_invocations: u64,
    pub canceled_invocations: u64,
    pub queued_canceled_invocations: u64,
    pub in_flight_canceled_invocations: u64,
    pub disconnect_canceled_invocations: u64,
    pub explicit_canceled_invocations: u64,
    pub canceled_host_ops: u64,
    pub precanceled_host_ops: u64,
    pub in_flight_canceled_host_ops: u64,
    pub nested_local_dispatches: u64,
    pub fallback_cross_isolate_dispatches: u64,
    pub host_operations: BTreeMap<String, RuntimeHostOperationMetricsSnapshot>,
    pub tenants: BTreeMap<String, RuntimeTenantMetricsSnapshot>,
    pub recent_request_correlations: Vec<RuntimeRequestCorrelationSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct RuntimeHostOperationMetricsSnapshot {
    pub started: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub canceled_before_start: u64,
    pub canceled_in_flight: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct RuntimeDurationDistributionSnapshot {
    pub samples: u64,
    pub under_1ms: u64,
    pub ms_1_to_5: u64,
    pub ms_5_to_25: u64,
    pub ms_25_to_100: u64,
    pub ms_100_to_500: u64,
    pub at_least_500ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct RuntimeTenantMetricsSnapshot {
    pub active_isolates: usize,
    pub started_invocations: u64,
    pub completed_invocations: u64,
    pub queued_canceled_invocations: u64,
    pub in_flight_canceled_invocations: u64,
    pub disconnect_canceled_invocations: u64,
    pub explicit_canceled_invocations: u64,
    pub queue_wait_nanos_total: u64,
    pub execution_nanos_total: u64,
    pub queue_wait_distribution: RuntimeDurationDistributionSnapshot,
    pub execution_distribution: RuntimeDurationDistributionSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeRequestCorrelationSnapshot {
    pub invocation_id: u64,
    pub server_request_id: String,
    pub tenant_label: Option<String>,
    pub function_name: String,
    pub kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct RuntimeHostOperationMetrics {
    started: u64,
    succeeded: u64,
    failed: u64,
    canceled_before_start: u64,
    canceled_in_flight: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct RuntimeDurationDistribution {
    samples: u64,
    under_1ms: u64,
    ms_1_to_5: u64,
    ms_5_to_25: u64,
    ms_25_to_100: u64,
    ms_100_to_500: u64,
    at_least_500ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct RuntimeTenantMetrics {
    active_isolates: usize,
    started_invocations: u64,
    completed_invocations: u64,
    queued_canceled_invocations: u64,
    in_flight_canceled_invocations: u64,
    disconnect_canceled_invocations: u64,
    explicit_canceled_invocations: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    queue_wait_distribution: RuntimeDurationDistribution,
    execution_distribution: RuntimeDurationDistribution,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeRequestCorrelation {
    invocation_id: u64,
    server_request_id: String,
    tenant_label: Option<String>,
    function_name: String,
    kind: &'static str,
}

impl RuntimeMetrics {
    pub fn increment_queued_invocations(&self) {
        self.queued_invocations.fetch_add(1, Ordering::SeqCst);
    }

    pub fn decrement_queued_invocations(&self) {
        self.queued_invocations.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn increment_active_isolates(&self) {
        self.increment_active_isolates_for_tenant(None);
    }

    pub fn increment_active_isolates_for_tenant(&self, tenant_label: Option<&str>) {
        self.active_isolates.fetch_add(1, Ordering::SeqCst);
        self.started_invocations.fetch_add(1, Ordering::SeqCst);
        self.update_tenant_metrics(tenant_label, |metrics| {
            metrics.active_isolates += 1;
            metrics.started_invocations += 1;
        });
    }

    pub fn record_worker_dispatch(&self) {
        self.worker_dispatched_invocations
            .fetch_add(1, Ordering::SeqCst);
    }

    pub fn decrement_active_isolates(&self) {
        self.decrement_active_isolates_for_tenant(None);
    }

    pub fn decrement_active_isolates_for_tenant(&self, tenant_label: Option<&str>) {
        self.active_isolates.fetch_sub(1, Ordering::SeqCst);
        self.completed_invocations.fetch_add(1, Ordering::SeqCst);
        self.update_tenant_metrics(tenant_label, |metrics| {
            metrics.active_isolates = metrics.active_isolates.saturating_sub(1);
            metrics.completed_invocations += 1;
        });
    }

    pub fn record_queue_wait(&self, duration: Duration) {
        self.record_queue_wait_for_tenant(None, duration);
    }

    pub fn record_queue_wait_for_tenant(&self, tenant_label: Option<&str>, duration: Duration) {
        self.queue_wait_nanos_total
            .fetch_add(duration_to_nanos(duration), Ordering::SeqCst);
        self.update_tenant_metrics(tenant_label, |metrics| {
            metrics.queue_wait_nanos_total += duration_to_nanos(duration);
            metrics.queue_wait_distribution.record(duration);
        });
    }

    pub fn record_execution(&self, duration: Duration) {
        self.record_execution_for_tenant(None, duration);
    }

    pub fn record_execution_for_tenant(&self, tenant_label: Option<&str>, duration: Duration) {
        self.execution_nanos_total
            .fetch_add(duration_to_nanos(duration), Ordering::SeqCst);
        self.update_tenant_metrics(tenant_label, |metrics| {
            metrics.execution_nanos_total += duration_to_nanos(duration);
            metrics.execution_distribution.record(duration);
        });
    }

    pub fn record_timeout(&self) {
        self.timed_out_invocations.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_canceled_invocation(&self) {
        self.canceled_invocations.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_queued_canceled_invocation(&self) {
        self.record_queued_canceled_invocation_for_tenant(None, None);
    }

    pub fn record_queued_canceled_invocation_for_tenant(
        &self,
        tenant_label: Option<&str>,
        cause: Option<HostCallCancellationCause>,
    ) {
        self.queued_canceled_invocations
            .fetch_add(1, Ordering::SeqCst);
        self.record_canceled_invocation();
        self.record_canceled_invocation_cause(tenant_label, cause);
        self.update_tenant_metrics(tenant_label, |metrics| {
            metrics.queued_canceled_invocations += 1;
        });
    }

    pub fn record_in_flight_canceled_invocation(&self) {
        self.record_in_flight_canceled_invocation_for_tenant(None, None);
    }

    pub fn record_in_flight_canceled_invocation_for_tenant(
        &self,
        tenant_label: Option<&str>,
        cause: Option<HostCallCancellationCause>,
    ) {
        self.in_flight_canceled_invocations
            .fetch_add(1, Ordering::SeqCst);
        self.record_canceled_invocation();
        self.record_canceled_invocation_cause(tenant_label, cause);
        self.update_tenant_metrics(tenant_label, |metrics| {
            metrics.in_flight_canceled_invocations += 1;
        });
    }

    pub fn record_canceled_host_op(&self) {
        self.canceled_host_ops.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_precanceled_host_op(&self) {
        self.precanceled_host_ops.fetch_add(1, Ordering::SeqCst);
        self.record_canceled_host_op();
    }

    pub fn record_in_flight_canceled_host_op(&self) {
        self.in_flight_canceled_host_ops
            .fetch_add(1, Ordering::SeqCst);
        self.record_canceled_host_op();
    }

    pub fn record_host_operation_started(&self, operation: &str) {
        self.update_host_operation_metrics(operation, |metrics| metrics.started += 1);
    }

    pub fn record_host_operation_succeeded(&self, operation: &str) {
        self.update_host_operation_metrics(operation, |metrics| metrics.succeeded += 1);
    }

    pub fn record_host_operation_failed(&self, operation: &str) {
        self.update_host_operation_metrics(operation, |metrics| metrics.failed += 1);
    }

    pub fn record_host_operation_canceled_before_start(&self, operation: &str) {
        self.record_precanceled_host_op();
        self.update_host_operation_metrics(operation, |metrics| {
            metrics.canceled_before_start += 1;
        });
    }

    pub fn record_host_operation_canceled_in_flight(&self, operation: &str) {
        self.record_in_flight_canceled_host_op();
        self.update_host_operation_metrics(operation, |metrics| {
            metrics.canceled_in_flight += 1;
        });
    }

    pub fn record_nested_local_dispatch(&self) {
        self.nested_local_dispatches.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_fallback_cross_isolate_dispatch(&self) {
        self.fallback_cross_isolate_dispatches
            .fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_request_correlation(&self, context: &RuntimeInvocationContext) {
        let Some(server_request_id) = context.server_request_id.clone() else {
            return;
        };
        const MAX_RECENT_REQUEST_CORRELATIONS: usize = 128;

        let mut recent_request_correlations = self
            .recent_request_correlations
            .lock()
            .expect("runtime request correlations lock should not be poisoned");
        if recent_request_correlations.len() == MAX_RECENT_REQUEST_CORRELATIONS {
            recent_request_correlations.pop_front();
        }
        recent_request_correlations.push_back(RuntimeRequestCorrelation {
            invocation_id: context.invocation_id,
            server_request_id,
            tenant_label: context.tenant_label.clone(),
            function_name: context.function_name.clone(),
            kind: context.kind,
        });
    }

    pub fn snapshot(&self) -> RuntimeMetricsSnapshot {
        RuntimeMetricsSnapshot {
            active_isolates: self.active_isolates.load(Ordering::SeqCst),
            queued_invocations: self.queued_invocations.load(Ordering::SeqCst),
            worker_dispatched_invocations: self
                .worker_dispatched_invocations
                .load(Ordering::SeqCst),
            started_invocations: self.started_invocations.load(Ordering::SeqCst),
            completed_invocations: self.completed_invocations.load(Ordering::SeqCst),
            queue_wait_nanos_total: self.queue_wait_nanos_total.load(Ordering::SeqCst),
            execution_nanos_total: self.execution_nanos_total.load(Ordering::SeqCst),
            timed_out_invocations: self.timed_out_invocations.load(Ordering::SeqCst),
            canceled_invocations: self.canceled_invocations.load(Ordering::SeqCst),
            queued_canceled_invocations: self.queued_canceled_invocations.load(Ordering::SeqCst),
            in_flight_canceled_invocations: self
                .in_flight_canceled_invocations
                .load(Ordering::SeqCst),
            disconnect_canceled_invocations: self
                .disconnect_canceled_invocations
                .load(Ordering::SeqCst),
            explicit_canceled_invocations: self
                .explicit_canceled_invocations
                .load(Ordering::SeqCst),
            canceled_host_ops: self.canceled_host_ops.load(Ordering::SeqCst),
            precanceled_host_ops: self.precanceled_host_ops.load(Ordering::SeqCst),
            in_flight_canceled_host_ops: self.in_flight_canceled_host_ops.load(Ordering::SeqCst),
            nested_local_dispatches: self.nested_local_dispatches.load(Ordering::SeqCst),
            fallback_cross_isolate_dispatches: self
                .fallback_cross_isolate_dispatches
                .load(Ordering::SeqCst),
            host_operations: self
                .host_operations
                .lock()
                .expect("runtime host operation metrics lock should not be poisoned")
                .iter()
                .map(|(operation, metrics)| {
                    (
                        operation.clone(),
                        RuntimeHostOperationMetricsSnapshot {
                            started: metrics.started,
                            succeeded: metrics.succeeded,
                            failed: metrics.failed,
                            canceled_before_start: metrics.canceled_before_start,
                            canceled_in_flight: metrics.canceled_in_flight,
                        },
                    )
                })
                .collect(),
            tenants: self
                .tenant_metrics
                .lock()
                .expect("runtime tenant metrics lock should not be poisoned")
                .iter()
                .map(|(tenant, metrics)| {
                    (
                        tenant.clone(),
                        RuntimeTenantMetricsSnapshot {
                            active_isolates: metrics.active_isolates,
                            started_invocations: metrics.started_invocations,
                            completed_invocations: metrics.completed_invocations,
                            queued_canceled_invocations: metrics.queued_canceled_invocations,
                            in_flight_canceled_invocations: metrics.in_flight_canceled_invocations,
                            disconnect_canceled_invocations: metrics
                                .disconnect_canceled_invocations,
                            explicit_canceled_invocations: metrics.explicit_canceled_invocations,
                            queue_wait_nanos_total: metrics.queue_wait_nanos_total,
                            execution_nanos_total: metrics.execution_nanos_total,
                            queue_wait_distribution: metrics.queue_wait_distribution.snapshot(),
                            execution_distribution: metrics.execution_distribution.snapshot(),
                        },
                    )
                })
                .collect(),
            recent_request_correlations: self
                .recent_request_correlations
                .lock()
                .expect("runtime request correlations lock should not be poisoned")
                .iter()
                .map(|correlation| RuntimeRequestCorrelationSnapshot {
                    invocation_id: correlation.invocation_id,
                    server_request_id: correlation.server_request_id.clone(),
                    tenant_label: correlation.tenant_label.clone(),
                    function_name: correlation.function_name.clone(),
                    kind: correlation.kind.to_string(),
                })
                .collect(),
        }
    }

    fn update_host_operation_metrics(
        &self,
        operation: &str,
        update: impl FnOnce(&mut RuntimeHostOperationMetrics),
    ) {
        let mut host_operations = self
            .host_operations
            .lock()
            .expect("runtime host operation metrics lock should not be poisoned");
        let metrics = host_operations.entry(operation.to_string()).or_default();
        update(metrics);
    }

    fn update_tenant_metrics(
        &self,
        tenant_label: Option<&str>,
        update: impl FnOnce(&mut RuntimeTenantMetrics),
    ) {
        let Some(tenant_label) = tenant_label else {
            return;
        };
        let mut tenant_metrics = self
            .tenant_metrics
            .lock()
            .expect("runtime tenant metrics lock should not be poisoned");
        let metrics = tenant_metrics.entry(tenant_label.to_string()).or_default();
        update(metrics);
    }

    fn record_canceled_invocation_cause(
        &self,
        tenant_label: Option<&str>,
        cause: Option<HostCallCancellationCause>,
    ) {
        match cause {
            Some(HostCallCancellationCause::Disconnect) => {
                self.disconnect_canceled_invocations
                    .fetch_add(1, Ordering::SeqCst);
                self.update_tenant_metrics(tenant_label, |metrics| {
                    metrics.disconnect_canceled_invocations += 1;
                });
            }
            Some(HostCallCancellationCause::Explicit) => {
                self.explicit_canceled_invocations
                    .fetch_add(1, Ordering::SeqCst);
                self.update_tenant_metrics(tenant_label, |metrics| {
                    metrics.explicit_canceled_invocations += 1;
                });
            }
            None => {}
        }
    }
}

impl RuntimeDurationDistribution {
    fn record(&mut self, duration: Duration) {
        let nanos = duration_to_nanos(duration);
        self.samples += 1;
        if nanos < 1_000_000 {
            self.under_1ms += 1;
        } else if nanos < 5_000_000 {
            self.ms_1_to_5 += 1;
        } else if nanos < 25_000_000 {
            self.ms_5_to_25 += 1;
        } else if nanos < 100_000_000 {
            self.ms_25_to_100 += 1;
        } else if nanos < 500_000_000 {
            self.ms_100_to_500 += 1;
        } else {
            self.at_least_500ms += 1;
        }
    }

    fn snapshot(&self) -> RuntimeDurationDistributionSnapshot {
        RuntimeDurationDistributionSnapshot {
            samples: self.samples,
            under_1ms: self.under_1ms,
            ms_1_to_5: self.ms_1_to_5,
            ms_5_to_25: self.ms_5_to_25,
            ms_25_to_100: self.ms_25_to_100,
            ms_100_to_500: self.ms_100_to_500,
            at_least_500ms: self.at_least_500ms,
        }
    }
}

fn duration_to_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tenant_metrics_snapshot_tracks_distributions_and_cancellations() {
        let metrics = RuntimeMetrics::default();

        metrics.increment_active_isolates_for_tenant(Some("demo"));
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
            tenant_label: Some("demo".to_string()),
            server_request_id: Some("req-7".to_string()),
        });
        metrics.decrement_active_isolates_for_tenant(Some("demo"));

        let snapshot = metrics.snapshot();
        assert_eq!(
            snapshot
                .tenants
                .get("demo")
                .expect("tenant metrics should be present"),
            &RuntimeTenantMetricsSnapshot {
                active_isolates: 0,
                started_invocations: 1,
                completed_invocations: 1,
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

        metrics.increment_active_isolates();
        metrics.record_queue_wait(Duration::from_millis(1));
        metrics.record_execution(Duration::from_millis(2));
        metrics.record_queued_canceled_invocation();
        metrics.decrement_active_isolates();

        assert!(metrics.snapshot().tenants.is_empty());
        assert!(metrics.snapshot().recent_request_correlations.is_empty());
    }
}
