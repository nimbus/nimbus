use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;

use super::duration_to_nanos;

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
    pub active_runtime_instances: usize,
    pub started_invocations: u64,
    pub completed_invocations: u64,
    pub rejected_invocations: u64,
    pub queued_canceled_invocations: u64,
    pub in_flight_canceled_invocations: u64,
    pub disconnect_canceled_invocations: u64,
    pub explicit_canceled_invocations: u64,
    pub queue_wait_nanos_total: u64,
    pub execution_nanos_total: u64,
    pub queue_wait_distribution: RuntimeDurationDistributionSnapshot,
    pub execution_distribution: RuntimeDurationDistributionSnapshot,
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
    active_runtime_instances: usize,
    started_invocations: u64,
    completed_invocations: u64,
    rejected_invocations: u64,
    queued_canceled_invocations: u64,
    in_flight_canceled_invocations: u64,
    disconnect_canceled_invocations: u64,
    explicit_canceled_invocations: u64,
    queue_wait_nanos_total: u64,
    execution_nanos_total: u64,
    queue_wait_distribution: RuntimeDurationDistribution,
    execution_distribution: RuntimeDurationDistribution,
}

#[derive(Debug, Default)]
pub(super) struct RuntimeTenantRegistry {
    metrics: Mutex<BTreeMap<String, RuntimeTenantMetrics>>,
}

impl RuntimeTenantRegistry {
    pub(super) fn increment_active_runtime_instances(&self, tenant_label: Option<&str>) {
        self.update(tenant_label, |metrics| {
            metrics.active_runtime_instances += 1;
        });
    }

    pub(super) fn decrement_active_runtime_instances(&self, tenant_label: Option<&str>) {
        self.update(tenant_label, |metrics| {
            metrics.active_runtime_instances = metrics.active_runtime_instances.saturating_sub(1);
        });
    }

    pub(super) fn record_invocation_started(&self, tenant_label: Option<&str>) {
        self.update(tenant_label, |metrics| {
            metrics.started_invocations += 1;
        });
    }

    pub(super) fn record_invocation_completed(&self, tenant_label: Option<&str>) {
        self.update(tenant_label, |metrics| {
            metrics.completed_invocations += 1;
        });
    }

    pub(super) fn record_rejected_invocation(&self, tenant_label: Option<&str>) {
        self.update(tenant_label, |metrics| {
            metrics.rejected_invocations += 1;
        });
    }

    pub(super) fn record_queued_canceled_invocation(&self, tenant_label: Option<&str>) {
        self.update(tenant_label, |metrics| {
            metrics.queued_canceled_invocations += 1;
        });
    }

    pub(super) fn record_in_flight_canceled_invocation(&self, tenant_label: Option<&str>) {
        self.update(tenant_label, |metrics| {
            metrics.in_flight_canceled_invocations += 1;
        });
    }

    pub(super) fn record_disconnect_canceled_invocation(&self, tenant_label: Option<&str>) {
        self.update(tenant_label, |metrics| {
            metrics.disconnect_canceled_invocations += 1;
        });
    }

    pub(super) fn record_explicit_canceled_invocation(&self, tenant_label: Option<&str>) {
        self.update(tenant_label, |metrics| {
            metrics.explicit_canceled_invocations += 1;
        });
    }

    pub(super) fn record_queue_wait(&self, tenant_label: Option<&str>, duration: Duration) {
        self.update(tenant_label, |metrics| {
            metrics.queue_wait_nanos_total += duration_to_nanos(duration);
            metrics.queue_wait_distribution.record(duration);
        });
    }

    pub(super) fn record_execution(&self, tenant_label: Option<&str>, duration: Duration) {
        self.update(tenant_label, |metrics| {
            metrics.execution_nanos_total += duration_to_nanos(duration);
            metrics.execution_distribution.record(duration);
        });
    }

    pub(super) fn snapshot(&self) -> BTreeMap<String, RuntimeTenantMetricsSnapshot> {
        self.metrics
            .lock()
            .expect("runtime tenant metrics lock should not be poisoned")
            .iter()
            .map(|(tenant, metrics)| {
                (
                    tenant.clone(),
                    RuntimeTenantMetricsSnapshot {
                        active_runtime_instances: metrics.active_runtime_instances,
                        started_invocations: metrics.started_invocations,
                        completed_invocations: metrics.completed_invocations,
                        rejected_invocations: metrics.rejected_invocations,
                        queued_canceled_invocations: metrics.queued_canceled_invocations,
                        in_flight_canceled_invocations: metrics.in_flight_canceled_invocations,
                        disconnect_canceled_invocations: metrics.disconnect_canceled_invocations,
                        explicit_canceled_invocations: metrics.explicit_canceled_invocations,
                        queue_wait_nanos_total: metrics.queue_wait_nanos_total,
                        execution_nanos_total: metrics.execution_nanos_total,
                        queue_wait_distribution: metrics.queue_wait_distribution.snapshot(),
                        execution_distribution: metrics.execution_distribution.snapshot(),
                    },
                )
            })
            .collect()
    }

    fn update(&self, tenant_label: Option<&str>, update: impl FnOnce(&mut RuntimeTenantMetrics)) {
        let Some(tenant_label) = tenant_label else {
            return;
        };
        let mut metrics = self
            .metrics
            .lock()
            .expect("runtime tenant metrics lock should not be poisoned");
        let entry = metrics.entry(tenant_label.to_string()).or_default();
        update(entry);
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
