use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
pub struct RuntimeHostOperationMetricsSnapshot {
    pub started: u64,
    pub succeeded: u64,
    pub failed: u64,
    pub canceled_before_start: u64,
    pub canceled_in_flight: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct RuntimeHostOperationMetrics {
    started: u64,
    succeeded: u64,
    failed: u64,
    canceled_before_start: u64,
    canceled_in_flight: u64,
}

#[derive(Debug, Default)]
pub(super) struct RuntimeHostOperationRegistry {
    metrics: Mutex<BTreeMap<String, RuntimeHostOperationMetrics>>,
}

impl RuntimeHostOperationRegistry {
    pub(super) fn record_started(&self, operation: &str) {
        self.update(operation, |metrics| metrics.started += 1);
    }

    pub(super) fn record_succeeded(&self, operation: &str) {
        self.update(operation, |metrics| metrics.succeeded += 1);
    }

    pub(super) fn record_failed(&self, operation: &str) {
        self.update(operation, |metrics| metrics.failed += 1);
    }

    pub(super) fn record_canceled_before_start(&self, operation: &str) {
        self.update(operation, |metrics| metrics.canceled_before_start += 1);
    }

    pub(super) fn record_canceled_in_flight(&self, operation: &str) {
        self.update(operation, |metrics| metrics.canceled_in_flight += 1);
    }

    pub(super) fn snapshot(&self) -> BTreeMap<String, RuntimeHostOperationMetricsSnapshot> {
        self.metrics
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
            .collect()
    }

    fn update(&self, operation: &str, update: impl FnOnce(&mut RuntimeHostOperationMetrics)) {
        let mut metrics = self
            .metrics
            .lock()
            .expect("runtime host operation metrics lock should not be poisoned");
        let entry = metrics.entry(operation.to_string()).or_default();
        update(entry);
    }
}
