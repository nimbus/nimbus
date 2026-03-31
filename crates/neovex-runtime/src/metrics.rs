use std::collections::BTreeMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use serde::Serialize;

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
    canceled_host_ops: AtomicU64,
    precanceled_host_ops: AtomicU64,
    in_flight_canceled_host_ops: AtomicU64,
    nested_local_dispatches: AtomicU64,
    fallback_cross_isolate_dispatches: AtomicU64,
    host_operations: Mutex<BTreeMap<String, RuntimeHostOperationMetrics>>,
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
    pub canceled_host_ops: u64,
    pub precanceled_host_ops: u64,
    pub in_flight_canceled_host_ops: u64,
    pub nested_local_dispatches: u64,
    pub fallback_cross_isolate_dispatches: u64,
    pub host_operations: BTreeMap<String, RuntimeHostOperationMetricsSnapshot>,
}

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

impl RuntimeMetrics {
    pub fn increment_queued_invocations(&self) {
        self.queued_invocations.fetch_add(1, Ordering::SeqCst);
    }

    pub fn decrement_queued_invocations(&self) {
        self.queued_invocations.fetch_sub(1, Ordering::SeqCst);
    }

    pub fn increment_active_isolates(&self) {
        self.active_isolates.fetch_add(1, Ordering::SeqCst);
        self.started_invocations.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_worker_dispatch(&self) {
        self.worker_dispatched_invocations
            .fetch_add(1, Ordering::SeqCst);
    }

    pub fn decrement_active_isolates(&self) {
        self.active_isolates.fetch_sub(1, Ordering::SeqCst);
        self.completed_invocations.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_queue_wait(&self, duration: Duration) {
        self.queue_wait_nanos_total
            .fetch_add(duration_to_nanos(duration), Ordering::SeqCst);
    }

    pub fn record_execution(&self, duration: Duration) {
        self.execution_nanos_total
            .fetch_add(duration_to_nanos(duration), Ordering::SeqCst);
    }

    pub fn record_timeout(&self) {
        self.timed_out_invocations.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_canceled_invocation(&self) {
        self.canceled_invocations.fetch_add(1, Ordering::SeqCst);
    }

    pub fn record_queued_canceled_invocation(&self) {
        self.queued_canceled_invocations
            .fetch_add(1, Ordering::SeqCst);
        self.record_canceled_invocation();
    }

    pub fn record_in_flight_canceled_invocation(&self) {
        self.in_flight_canceled_invocations
            .fetch_add(1, Ordering::SeqCst);
        self.record_canceled_invocation();
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
}

fn duration_to_nanos(duration: Duration) -> u64 {
    duration.as_nanos().min(u128::from(u64::MAX)) as u64
}
